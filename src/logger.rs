use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use hudhook::windows::Win32::System::SystemInformation::GetLocalTime;

struct Logger {
    file: Mutex<Option<File>>,
}

static LOGGER: Logger = Logger {
    file: Mutex::new(None),
};

fn redact_message(target: &str, message: String) -> String {
    if target == "blivedm::client::auth" && message.contains("response") {
        if let Some((prefix, _)) = message.split_once(':') {
            return format!("{prefix}: <redacted>");
        }
        return "<redacted response>".to_string();
    }
    message
}

/// Log file path: next to the host executable.
pub fn log_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("imbdmlive.log")
}

/// Install the logger.
pub fn init() {
    if log::set_logger(&LOGGER).is_err() {
        return; // already initialized
    }
    log::set_max_level(log::LevelFilter::Info);

    if let Ok(f) = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(log_path())
    {
        if let Ok(mut guard) = LOGGER.file.lock() {
            *guard = Some(f);
        }
    }

    log::info!("logger initialized -> {}", log_path().display());
}

impl log::Log for Logger {
    fn enabled(&self, _meta: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        let t = unsafe { GetLocalTime() };
        let message = redact_message(record.target(), record.args().to_string());
        let line = format!(
            "[{:04}/{:02}/{:02} {:02}:{:02}:{:02}.{:03}] {:<5} {}: {}\n",
            t.wYear,
            t.wMonth,
            t.wDay,
            t.wHour,
            t.wMinute,
            t.wSecond,
            t.wMilliseconds,
            record.level(),
            record.target(),
            message,
        );

        let mut out = std::io::stdout();
        let _ = out.write_all(line.as_bytes());
        let _ = out.flush();

        if let Ok(mut guard) = self.file.lock() {
            if let Some(f) = guard.as_mut() {
                let _ = f.write_all(line.as_bytes());
                let _ = f.flush();
            }
        }
    }

    fn flush(&self) {
        if let Ok(mut guard) = self.file.lock() {
            if let Some(f) = guard.as_mut() {
                let _ = f.flush();
            }
        }
    }
}
