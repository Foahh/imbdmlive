use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::sync::mpsc;
use std::thread;

use hudhook::windows::Win32::Foundation::HINSTANCE;
use hudhook::windows::Win32::System::SystemServices::DLL_PROCESS_ATTACH;

mod backend;
pub mod config;
mod danmaku;
pub mod logger;
pub mod state;
pub mod ui;

use backend::RenderBackend;

/// Raw `HINSTANCE` of this DLL, captured in `DllMain`.
static MODULE: OnceLock<usize> = OnceLock::new();
/// Guards against bootstrapping more than once.
static STARTED: AtomicBool = AtomicBool::new(false);

/// Standard DLL entry.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllMain(
    hmodule: HINSTANCE,
    reason: u32,
    _reserved: *mut c_void,
) -> i32 {
    if reason == DLL_PROCESS_ATTACH {
        let _ = MODULE.set(hmodule.0 as usize);
        logger::init();
        log::info!("imbdmlive attached");
        start_once();
    }
    1
}

fn start_once() {
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    thread::Builder::new()
        .name("bdmlive-bootstrap".into())
        .spawn(bootstrap)
        .ok();
}

fn bootstrap() {
    let cfg = config::Config::load();
    log::info!("Config loaded (room {})", cfg.room_id);

    let state = state::OverlayState::shared(cfg.max_lines, cfg.room_id.clone());
    let (reconnect_tx, reconnect_rx) = mpsc::channel::<config::Config>();

    // Live danmaku connection supervisor
    {
        let state = Arc::clone(&state);
        let initial = cfg.clone();
        thread::Builder::new()
            .name("bdmlive-supervisor".into())
            .spawn(move || {
                let mut handle = danmaku::start(&initial, Arc::clone(&state));
                while let Ok(new_cfg) = reconnect_rx.recv() {
                    drop(handle); // stop the old connection's threads
                    handle = danmaku::start(&new_cfg, Arc::clone(&state));
                }
                drop(handle);
            })
            .ok();
    }

    // Install the overlay via the D3D9 backend.
    let hmodule = MODULE.get().map(|raw| HINSTANCE(*raw as *mut c_void));
    let overlay = ui::OverlayUi::new(state, reconnect_tx, cfg);
    match backend::Dx9Backend.install(overlay, hmodule) {
        Ok(()) => log::info!("Overlay hooks installed"),
        Err(e) => log::warn!("{e}"),
    }
}
