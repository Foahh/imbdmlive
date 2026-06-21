use std::sync::{Arc, mpsc};
use std::thread;

use hudhook::Hudhook;
use hudhook::hooks::dx9::ImguiDx9Hooks;
use hudhook::windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use hudhook::windows::Win32::Graphics::Direct3D9::*;
use hudhook::windows::Win32::System::LibraryLoader::GetModuleHandleW;
use hudhook::windows::Win32::UI::WindowsAndMessaging::*;

use imbdmlive::config::Config;
use imbdmlive::state::OverlayState;
use imbdmlive::ui::OverlayUi;

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0u16)).collect()
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_DESTROY {
        unsafe { PostQuitMessage(0) };
        return LRESULT(0);
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

fn main() {
    imbdmlive::logger::init();

    unsafe {
        let hmodule = GetModuleHandleW(None).expect("GetModuleHandleW");
        let hinstance = HINSTANCE(hmodule.0);

        let class_name = to_wide("BDMLive");
        let window_title = to_wide("BDMLive");

        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            hInstance: hinstance,
            lpszClassName: hudhook::windows::core::PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };
        assert!(RegisterClassExW(&wc) != 0, "RegisterClassExW failed");

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            hudhook::windows::core::PCWSTR(class_name.as_ptr()),
            hudhook::windows::core::PCWSTR(window_title.as_ptr()),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            100,
            100,
            1024,
            768,
            None,
            None,
            Some(hinstance),
            None,
        )
        .expect("CreateWindowExW failed");

        // D3D9 device
        let d3d = Direct3DCreate9(D3D_SDK_VERSION).expect("D3D9 unavailable");

        let mut pp = D3DPRESENT_PARAMETERS {
            Windowed: true.into(),
            SwapEffect: D3DSWAPEFFECT_DISCARD,
            BackBufferFormat: D3DFMT_UNKNOWN,
            hDeviceWindow: hwnd,
            ..Default::default()
        };

        let mut device: Option<IDirect3DDevice9> = None;
        d3d.CreateDevice(
            D3DADAPTER_DEFAULT,
            D3DDEVTYPE_HAL,
            hwnd,
            D3DCREATE_SOFTWARE_VERTEXPROCESSING as u32,
            &mut pp,
            &mut device,
        )
        .expect("CreateDevice failed");
        let device = device.unwrap();

        let cfg = Config::load();
        let state = OverlayState::shared(cfg.max_lines, cfg.room_id.clone());
        let (reconnect_tx, reconnect_rx) = mpsc::channel::<Config>();

        {
            let state = Arc::clone(&state);
            let initial = cfg.clone();
            thread::Builder::new()
                .name("bdmlive-supervisor".into())
                .spawn(move || {
                    let mut handle = imbdmlive::danmaku::start(&initial, Arc::clone(&state));
                    while let Ok(new_cfg) = reconnect_rx.recv() {
                        drop(handle);
                        handle = imbdmlive::danmaku::start(&new_cfg, Arc::clone(&state));
                    }
                    drop(handle);
                })
                .ok();
        }

        let overlay = OverlayUi::new(state, reconnect_tx, cfg);

        // Hook D3D9 Present (must be called after the device exists)
        Hudhook::builder()
            .with::<ImguiDx9Hooks>(overlay)
            .build()
            .apply()
            .expect("Hudhook install failed");

        // Message + render loop
        let mut msg = MSG::default();
        loop {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT {
                    return;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }

            device
                .Clear(
                    0,
                    std::ptr::null(),
                    D3DCLEAR_TARGET as u32,
                    0xFF_1A_1A_2E,
                    1.0,
                    0,
                )
                .ok();
            device.BeginScene().ok();
            device.EndScene().ok();
            device
                .Present(
                    std::ptr::null(),
                    std::ptr::null(),
                    HWND(std::ptr::null_mut()),
                    std::ptr::null(),
                )
                .ok();
        }
    }
}
