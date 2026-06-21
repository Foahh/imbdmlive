use std::ffi::c_void;
use std::mem;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Direct3D9::{
    D3D_SDK_VERSION, D3DADAPTER_DEFAULT, D3DCREATE_SOFTWARE_VERTEXPROCESSING, D3DDEVTYPE_HAL,
    D3DDISPLAYMODE, D3DFORMAT, D3DPRESENT_PARAMETERS, D3DSWAPEFFECT_DISCARD, Direct3DCreate9,
    IDirect3DDevice9, IDirect3DSwapChain9,
};
use windows::Win32::Graphics::Gdi::RGNDATA;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassExW,
    UnregisterClassW, WNDCLASSEXW, WS_EX_OVERLAPPEDWINDOW, WS_OVERLAPPEDWINDOW,
};
use windows::core::{BOOL, Error, HRESULT, Interface, Result, w};

/// `IDirect3DSwapChain9::Present` hook target. The host presents through the
/// swapchain rather than the device directly.
pub type SwapChainPresentFn = unsafe extern "system" fn(
    this: IDirect3DSwapChain9,
    psourcerect: *const RECT,
    pdestrect: *const RECT,
    hdestwindowoverride: HWND,
    pdirtyregion: *const RGNDATA,
    dwflags: u32,
) -> HRESULT;

/// `IDirect3DDevice9::Reset` hook target. Used to release device-pool
/// resources on non-Ex resets; Ex hosts keep resources across resets.
pub type Dx9ResetFn =
    unsafe extern "system" fn(this: IDirect3DDevice9, *const D3DPRESENT_PARAMETERS) -> HRESULT;

/// Helper for fallible `windows` APIs with an optional pointer out-param.
pub fn try_out_ptr<T, F, E, O>(mut f: F) -> std::result::Result<T, E>
where
    F: FnMut(&mut Option<T>) -> std::result::Result<O, E>,
{
    let mut t: Option<T> = None;
    match f(&mut t) {
        Ok(_) => Ok(t.unwrap()),
        Err(e) => Err(e),
    }
}

/// RAII wrapper for a temporary window used only to create a throwaway D3D9 device.
struct DummyHwnd(HWND, WNDCLASSEXW);

impl DummyHwnd {
    fn new() -> Self {
        unsafe extern "system" fn wnd_proc(
            hwnd: HWND,
            msg: u32,
            wparam: WPARAM,
            lparam: LPARAM,
        ) -> LRESULT {
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }

        let wndclass = WNDCLASSEXW {
            cbSize: mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            hInstance: unsafe { GetModuleHandleW(None).unwrap().into() },
            lpszClassName: w!("IMBDMLIVE"),
            ..Default::default()
        };
        unsafe { RegisterClassExW(&wndclass) };

        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_OVERLAPPEDWINDOW,
                wndclass.lpszClassName,
                w!("IMBDMLIVE"),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                100,
                100,
                None,
                None,
                Some(wndclass.hInstance),
                None,
            )
        }
        .expect("CreateWindowExW");

        Self(hwnd, wndclass)
    }

    fn hwnd(&self) -> HWND {
        self.0
    }
}

impl Drop for DummyHwnd {
    fn drop(&mut self) {
        unsafe {
            if let Err(e) = DestroyWindow(self.0) {
                log::error!("DestroyWindow: {e}");
            }
            if let Err(e) = UnregisterClassW(self.1.lpszClassName, Some(self.1.hInstance)) {
                log::error!("UnregisterClass: {e}");
            }
        }
    }
}

/// Resolve the vtable addresses of `IDirect3DSwapChain9::Present` and
/// `IDirect3DDevice9::Reset` by creating a throwaway NULLREF device and reading
/// its implicit swapchain. These addresses are shared across all objects of the
/// same implementation, so hooking them affects the host's real swapchain and device.
pub fn get_targets() -> Result<(SwapChainPresentFn, Dx9ResetFn)> {
    let d9 = unsafe { Direct3DCreate9(D3D_SDK_VERSION) }
        .ok_or_else(|| Error::from_hresult(HRESULT(-1)))?;

    fn ctx(step: &'static str) -> impl Fn(Error) -> Error {
        move |e: Error| Error::new(e.code(), format!("{step}: {e}"))
    }

    let mut display_mode =
        D3DDISPLAYMODE { Width: 0, Height: 0, RefreshRate: 0, Format: D3DFORMAT(0) };
    unsafe { d9.GetAdapterDisplayMode(D3DADAPTER_DEFAULT, &mut display_mode) }
        .map_err(ctx("GetAdapterDisplayMode"))?;

    let mut present_params = D3DPRESENT_PARAMETERS {
        Windowed: BOOL(1),
        SwapEffect: D3DSWAPEFFECT_DISCARD,
        BackBufferFormat: display_mode.Format,
        ..Default::default()
    };

    let dummy_hwnd = DummyHwnd::new();
    let device: IDirect3DDevice9 = try_out_ptr(|v| unsafe {
        d9.CreateDevice(
            D3DADAPTER_DEFAULT,
            D3DDEVTYPE_HAL,
            dummy_hwnd.hwnd(),
            D3DCREATE_SOFTWARE_VERTEXPROCESSING as u32,
            &mut present_params,
            v,
        )
    })
    .map_err(ctx("CreateDevice"))?;

    let swapchain: IDirect3DSwapChain9 =
        unsafe { device.GetSwapChain(0) }.map_err(ctx("GetSwapChain"))?;

    let present_ptr = swapchain.vtable().Present;
    let reset_ptr = device.vtable().Reset;

    Ok(unsafe {
        (
            mem::transmute::<
                unsafe extern "system" fn(
                    *mut c_void,
                    *const RECT,
                    *const RECT,
                    HWND,
                    *const RGNDATA,
                    u32,
                ) -> HRESULT,
                SwapChainPresentFn,
            >(present_ptr),
            mem::transmute::<
                unsafe extern "system" fn(*mut c_void, *mut D3DPRESENT_PARAMETERS) -> HRESULT,
                Dx9ResetFn,
            >(reset_ptr),
        )
    })
}

/// Create a detour for `target -> detour` via MinHook and return the trampoline
/// (a pointer to the original function).
///
/// # Safety
/// `target` must point at a hookable function and `detour` must have a
/// compatible ABI/signature.
pub unsafe fn create_hook(target: *mut c_void, detour: *mut c_void) -> Result<*mut c_void> {
    unsafe {
        minhook::MinHook::create_hook(target, detour)
            .map_err(|e| Error::new(HRESULT(-1), format!("MH_CreateHook: {e:?}")))
    }
}

/// Enable all created hooks.
///
/// # Safety
/// All target functions must remain valid for the lifetime of the process.
pub unsafe fn enable_all_hooks() -> Result<()> {
    unsafe {
        minhook::MinHook::enable_all_hooks()
            .map_err(|e| Error::new(HRESULT(-1), format!("MH_EnableHook: {e:?}")))
    }
}
