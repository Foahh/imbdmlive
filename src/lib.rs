use std::ffi::c_void;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use windows::Win32::Foundation::HINSTANCE;
use windows::Win32::System::SystemServices::DLL_PROCESS_ATTACH;

pub mod config;
pub mod danmaku;
pub mod logger;
mod overlay;
pub mod state;
pub mod ui;

/// Keeps the danmaku connection's threads alive for the DLL's lifetime.
static DANMAKU: OnceLock<danmaku::DanmakuHandle> = OnceLock::new();
/// Guards against bootstrapping more than once.
static STARTED: AtomicBool = AtomicBool::new(false);

/// Standard DLL entry.
///
/// # Safety
/// Called by the Windows loader with the conventional `DllMain` contract; not
/// meant to be invoked directly.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllMain(
    _hmodule: HINSTANCE,
    reason: u32,
    _reserved: *mut c_void,
) -> i32 {
    if reason == DLL_PROCESS_ATTACH {
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
    thread::Builder::new().name("bdmlive-bootstrap".into()).spawn(bootstrap).ok();
}

fn bootstrap() {
    let cfg = config::Config::load();
    logger::set_level(cfg.log_level_filter());
    log::info!("Config loaded");

    let state = state::OverlayState::shared(cfg.room_id.clone());

    // Start the live danmaku connection once and keep it running.
    let handle = danmaku::start(&cfg, std::sync::Arc::clone(&state));
    let _ = DANMAKU.set(handle);

    // Install the overlay.
    let overlay = ui::OverlayUi::new(state, cfg);
    match overlay::install(overlay) {
        Ok(()) => log::info!("Overlay hooks installed"),
        Err(e) => log::warn!("{e}"),
    }
}
