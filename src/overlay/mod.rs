mod hook;
mod renderer;

use std::ffi::c_void;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use imgui::Context;
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Direct3D9::{
    D3DBACKBUFFER_TYPE_MONO, D3DPRESENT_PARAMETERS, D3DSURFACE_DESC, IDirect3DDevice9,
    IDirect3DSurface9, IDirect3DSwapChain9,
};
use windows::Win32::Graphics::Gdi::RGNDATA;
use windows::core::{HRESULT, Result};

use hook::{Dx9ResetFn, SwapChainPresentFn};
use renderer::D3D9RenderEngine;

use crate::ui::OverlayUi;

static PRESENT_TRAMPOLINE: OnceLock<SwapChainPresentFn> = OnceLock::new();
static RESET_TRAMPOLINE: OnceLock<Dx9ResetFn> = OnceLock::new();
static STATE: OnceLock<Shared> = OnceLock::new();

/// All mutable overlay state. The D3D9 device, imgui [`Context`], and the
/// rendering engine are only ever touched on the host's render thread (inside
/// the Present hook), guarded by the `pipeline` mutex.
struct Shared {
    /// `None` until the first Present, where we have a live device to build it.
    pipeline: Mutex<Option<Pipeline>>,
    /// The UI, held here between `install` and first-frame construction.
    pending_ui: Mutex<Option<OverlayUi>>,
}

// SAFETY: the contained COM/imgui objects are confined to the render thread;
// access is serialized through the mutexes and the OnceLock handoff.
unsafe impl Send for Shared {}
unsafe impl Sync for Shared {}

struct Pipeline {
    device: IDirect3DDevice9,
    ctx: Context,
    engine: D3D9RenderEngine,
    ui: OverlayUi,
    last_frame: Instant,
}

impl Pipeline {
    fn new(device: &IDirect3DDevice9, mut ui: OverlayUi) -> Result<Self> {
        let mut ctx = Context::create();
        let mut engine = D3D9RenderEngine::new(device, &mut ctx)?;
        ui.initialize(&mut ctx);
        engine.setup_fonts(&mut ctx)?;
        Ok(Self { device: device.clone(), ctx, engine, ui, last_frame: Instant::now() })
    }

    fn render(&mut self, surface: IDirect3DSurface9) -> Result<()> {
        let mut desc = D3DSURFACE_DESC::default();
        unsafe { surface.GetDesc(&mut desc)? };
        let display = [desc.Width as f32, desc.Height as f32];
        if display[0] <= 0.0 || display[1] <= 0.0 {
            return Ok(());
        }

        let io = self.ctx.io_mut();
        io.display_size = display;
        let now = Instant::now();
        io.update_delta_time(now.saturating_duration_since(self.last_frame));
        self.last_frame = now;

        let ui = self.ctx.frame();
        self.ui.render(ui, display);
        let draw_data = self.ctx.render();

        unsafe { self.device.BeginScene()? };
        let result = self.engine.render(draw_data, surface);
        unsafe { self.device.EndScene()? };
        result
    }
}

/// Install the overlay hooks. Takes ownership of `ui`, which is constructed into
/// a full pipeline on the first present (when a live device exists).
pub fn install(ui: OverlayUi) -> std::result::Result<(), String> {
    let (present_addr, reset_addr) =
        hook::get_targets().map_err(|e| format!("resolve D3D9 vtables: {e}"))?;

    STATE
        .set(Shared { pipeline: Mutex::new(None), pending_ui: Mutex::new(Some(ui)) })
        .map_err(|_| "overlay already installed".to_string())?;

    unsafe {
        let present_tramp =
            hook::create_hook(present_addr as *mut c_void, present_detour as *mut c_void)
                .map_err(|e| format!("hook swapchain Present: {e}"))?;
        let reset_tramp =
            hook::create_hook(reset_addr as *mut c_void, reset_detour as *mut c_void)
                .map_err(|e| format!("hook Reset: {e}"))?;

        let _ = PRESENT_TRAMPOLINE
            .set(std::mem::transmute::<*mut c_void, SwapChainPresentFn>(present_tramp));
        let _ = RESET_TRAMPOLINE.set(std::mem::transmute::<*mut c_void, Dx9ResetFn>(reset_tramp));

        hook::enable_all_hooks().map_err(|e| format!("enable hooks: {e}"))?;
    }

    Ok(())
}

unsafe extern "system" fn present_detour(
    swapchain: IDirect3DSwapChain9,
    psourcerect: *const RECT,
    pdestrect: *const RECT,
    hdestwindowoverride: HWND,
    pdirtyregion: *const RGNDATA,
    dwflags: u32,
) -> HRESULT {
    if let Some(state) = STATE.get()
        && let Ok(mut guard) = state.pipeline.try_lock()
    {
        if guard.is_none()
            && let Ok(device) = unsafe { swapchain.GetDevice() }
            && let Some(ui) = state.pending_ui.lock().ok().and_then(|mut u| u.take())
        {
            match Pipeline::new(&device, ui) {
                Ok(p) => {
                    log::info!("Overlay pipeline initialized");
                    *guard = Some(p);
                },
                Err(e) => log::error!("Pipeline init failed: {e}"),
            }
        }
        if let Some(pipeline) = guard.as_mut() {
            match unsafe { swapchain.GetBackBuffer(0, D3DBACKBUFFER_TYPE_MONO) } {
                Ok(surface) => {
                    if let Err(e) = pipeline.render(surface) {
                        log::error!("Render error: {e}");
                    }
                },
                Err(e) => log::error!("GetBackBuffer failed: {e}"),
            }
        }
    }

    let trampoline = PRESENT_TRAMPOLINE.get().expect("Present trampoline uninitialized");
    unsafe {
        trampoline(swapchain, psourcerect, pdestrect, hdestwindowoverride, pdirtyregion, dwflags)
    }
}

unsafe extern "system" fn reset_detour(
    device: IDirect3DDevice9,
    present_params: *const D3DPRESENT_PARAMETERS,
) -> HRESULT {
    // D3DPOOL_DEFAULT resources are lost on Reset; drop the pipeline so it is
    // rebuilt on the next present. Ex hosts use ResetEx, which is not hooked.
    if let Some(state) = STATE.get()
        && let Ok(mut guard) = state.pipeline.try_lock()
    {
        *guard = None;
    }

    let trampoline = RESET_TRAMPOLINE.get().expect("Reset trampoline uninitialized");
    unsafe { trampoline(device, present_params) }
}
