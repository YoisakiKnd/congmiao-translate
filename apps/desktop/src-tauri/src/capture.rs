use std::sync::atomic::{AtomicBool, Ordering};

use tauri::AppHandle;

#[cfg(target_os = "linux")]
#[path = "capture_linux.rs"]
mod linux;
#[cfg(target_os = "macos")]
#[allow(unexpected_cfgs)]
#[path = "capture_macos.rs"]
mod macos;
#[cfg(target_os = "windows")]
#[path = "capture_windows.rs"]
mod windows;

static SILENT: AtomicBool = AtomicBool::new(false);

pub fn begin(app: &AppHandle) {
    dispatch(app);
}

pub fn begin_silent(app: &AppHandle) {
    SILENT.store(true, Ordering::SeqCst);
    dispatch(app);
}

pub(crate) fn take_silent() -> bool {
    SILENT.swap(false, Ordering::SeqCst)
}

fn dispatch(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    macos::begin(app);
    #[cfg(target_os = "windows")]
    windows::begin(app);
    #[cfg(target_os = "linux")]
    linux::begin(app);
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        let _ = app;
        crate::popup::open_at_cursor(app, "当前平台还没有 OCR 截图翻译", "notice");
    }
}

pub fn show_translation(app: &AppHandle, x: f64, y: f64, text: &str) {
    crate::popup::open(app, x, y, text, "translate");
}

pub fn show_notice(app: &AppHandle, x: f64, y: f64, message: &str) {
    crate::popup::open(app, x, y, message, "notice");
}

pub fn recognize_fixture(bytes: &[u8]) -> Result<String, String> {
    #[cfg(target_os = "windows")]
    {
        return windows::recognize_png(bytes);
    }
    #[cfg(target_os = "linux")]
    {
        return linux::recognize_bytes(bytes);
    }
    #[cfg(target_os = "macos")]
    {
        let _ = bytes;
        Ok("delegated".into())
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        let _ = bytes;
        Err("当前平台没有文字识别".into())
    }
}

pub fn screen_recording_granted() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::screen_recording_granted()
    }
    #[cfg(not(target_os = "macos"))]
    true
}

pub fn request_screen_recording() {
    #[cfg(target_os = "macos")]
    macos::request_screen_recording();
}

pub(crate) fn drag_bounds(x0: i32, y0: i32, x1: i32, y1: i32) -> Option<(i32, i32, i32, i32)> {
    let left = x0.min(x1);
    let top = y0.min(y1);
    let width = (x1 - x0).unsigned_abs() as i32;
    let height = (y1 - y0).unsigned_abs() as i32;
    if width < 8 || height < 8 {
        None
    } else {
        Some((left, top, width, height))
    }
}

#[cfg(test)]
mod tests {
    use super::drag_bounds;

    #[test]
    fn drag_keeps_a_region_at_least_eight_pixels() {
        assert_eq!(drag_bounds(10, 20, 40, 50), Some((10, 20, 30, 30)));
        assert_eq!(drag_bounds(40, 50, 10, 20), Some((10, 20, 30, 30)));
        assert_eq!(drag_bounds(0, 0, 4, 40), None);
    }
}
