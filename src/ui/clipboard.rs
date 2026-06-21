use hudhook::imgui::ClipboardBackend;
use hudhook::windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL};
use hudhook::windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData,
};
use hudhook::windows::Win32::System::Memory::{
    GMEM_MOVEABLE, GMEM_ZEROINIT, GlobalAlloc, GlobalLock, GlobalUnlock,
};
use hudhook::windows::Win32::System::Ole::CF_UNICODETEXT;

pub struct WindowsClipboard;

struct OpenClipboardGuard;

impl OpenClipboardGuard {
    fn open() -> Option<Self> {
        unsafe { OpenClipboard(None) }.ok()?;
        Some(Self)
    }
}

impl Drop for OpenClipboardGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseClipboard();
        }
    }
}

impl ClipboardBackend for WindowsClipboard {
    fn get(&mut self) -> Option<String> {
        let _clipboard = OpenClipboardGuard::open()?;
        let handle = unsafe { GetClipboardData(CF_UNICODETEXT.0 as u32) }.ok()?;

        let ptr = unsafe { GlobalLock(HGLOBAL(handle.0)) } as *const u16;
        if ptr.is_null() {
            return None;
        }

        let mut len = 0;
        unsafe {
            while *ptr.add(len) != 0 {
                len += 1;
            }
            let text = String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len));
            let _ = GlobalUnlock(HGLOBAL(handle.0));
            Some(text)
        }
    }

    fn set(&mut self, value: &str) {
        let Some(_clipboard) = OpenClipboardGuard::open() else {
            return;
        };
        if unsafe { EmptyClipboard() }.is_err() {
            return;
        }

        let wide: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
        let bytes = wide.len() * std::mem::size_of::<u16>();
        let Ok(handle) = (unsafe { GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, bytes) }) else {
            return;
        };

        let ptr = unsafe { GlobalLock(handle) } as *mut u16;
        if ptr.is_null() {
            unsafe {
                let _ = GlobalFree(Some(handle));
            }
            return;
        }

        unsafe {
            std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
            let _ = GlobalUnlock(handle);
            if SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(handle.0))).is_err() {
                let _ = GlobalFree(Some(handle));
            }
        }
    }
}
