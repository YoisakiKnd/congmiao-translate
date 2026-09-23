use tauri::AppHandle;

#[cfg(target_os = "linux")]
#[path = "selection_linux.rs"]
mod linux;
#[cfg(target_os = "macos")]
#[allow(unexpected_cfgs)]
#[path = "selection_macos.rs"]
mod macos;
#[cfg(target_os = "windows")]
#[path = "selection_windows.rs"]
mod windows;

pub fn begin(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    macos::begin(app);
    #[cfg(target_os = "windows")]
    windows::begin(app);
    #[cfg(target_os = "linux")]
    linux::begin(app);
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        crate::popup::open_at_cursor(app, "当前平台还没有系统划词翻译", "notice");
    }
}

pub fn replace(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    macos::replace(app);
    #[cfg(target_os = "windows")]
    windows::replace(app);
    #[cfg(target_os = "linux")]
    linux::replace(app);
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        crate::popup::open_at_cursor(app, "当前平台还没有替换翻译", "notice");
    }
}

#[cfg(target_os = "linux")]
pub fn read_linux() -> Result<Option<String>, String> {
    linux::read_selection()
}

pub fn accessibility_granted() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::accessibility_granted()
    }
    #[cfg(not(target_os = "macos"))]
    true
}

pub fn request_accessibility() {
    #[cfg(target_os = "macos")]
    macos::request_accessibility();
}
