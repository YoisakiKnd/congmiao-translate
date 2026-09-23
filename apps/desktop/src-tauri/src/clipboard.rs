use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::AppHandle;

pub static WATCH: AtomicBool = AtomicBool::new(false);
static IGNORE_UNTIL: AtomicU64 = AtomicU64::new(0);

pub fn set_watch(enabled: bool) {
    WATCH.store(enabled, Ordering::Relaxed);
}

pub fn ignore_briefly() {
    IGNORE_UNTIL.store(now_ms() + 1500, Ordering::Relaxed);
}

pub fn start(app: AppHandle) {
    std::thread::spawn(move || {
        let mut last = String::new();
        loop {
            std::thread::sleep(std::time::Duration::from_millis(700));
            if !WATCH.load(Ordering::Relaxed) || now_ms() < IGNORE_UNTIL.load(Ordering::Relaxed) {
                continue;
            }
            let Some(text) = read_text() else {
                continue;
            };
            let trimmed = text.trim();
            if trimmed.is_empty() || trimmed == last {
                continue;
            }
            last = trimmed.to_string();
            crate::popup::open_at_cursor(&app, trimmed, "translate");
        }
    });
}

pub(crate) fn read_for_replace() -> Option<String> {
    read_text()
}

pub fn write_text(text: &str) {
    ignore_briefly();
    #[cfg(target_os = "macos")]
    macos_write(text);
    #[cfg(target_os = "windows")]
    windows_write(text);
    #[cfg(target_os = "linux")]
    linux_write(text);
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    let _ = text;
}

pub fn roundtrip_clipboard(sample: &str) -> bool {
    write_text(sample);
    std::thread::sleep(std::time::Duration::from_millis(150));
    read_text()
        .map(|text| text.trim() == sample)
        .unwrap_or(false)
}

fn read_text() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        return macos_read();
    }
    #[cfg(target_os = "windows")]
    {
        return windows_read();
    }
    #[cfg(target_os = "linux")]
    {
        return linux_read();
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    None
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(target_os = "macos")]
fn macos_read() -> Option<String> {
    use crate::objc_compat::{class, id, msg_send};
    unsafe {
        let pasteboard: id = msg_send![class!(NSPasteboard), generalPasteboard];
        let value: id =
            msg_send![pasteboard, stringForType: ns(pasteboard, "public.utf8-plain-text")];
        if value.is_null() {
            return None;
        }
        Some(ns_to_string(value))
    }
}

#[cfg(target_os = "macos")]
fn macos_write(text: &str) {
    use crate::objc_compat::{class, id, msg_send, BOOL};
    unsafe {
        let pasteboard: id = msg_send![class!(NSPasteboard), generalPasteboard];
        let _: () = msg_send![pasteboard, clearContents];
        let _: BOOL = msg_send![pasteboard, setString: ns(pasteboard, text), forType: ns(pasteboard, "public.utf8-plain-text")];
    }
}

#[cfg(target_os = "macos")]
fn ns(_anchor: crate::objc_compat::id, text: &str) -> crate::objc_compat::id {
    crate::objc_compat::ns_string(text)
}

#[cfg(target_os = "macos")]
fn ns_to_string(text: crate::objc_compat::id) -> String {
    use std::ffi::CStr;
    unsafe {
        let utf8: *const i8 = msg_send_utf8(text);
        if utf8.is_null() {
            return String::new();
        }
        CStr::from_ptr(utf8).to_string_lossy().into_owned()
    }
}

#[cfg(target_os = "macos")]
fn msg_send_utf8(text: crate::objc_compat::id) -> *const i8 {
    use crate::objc_compat::msg_send;
    unsafe { msg_send![text, UTF8String] }
}

#[cfg(target_os = "windows")]
fn windows_read() -> Option<String> {
    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", "Get-Clipboard -Raw"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

#[cfg(target_os = "windows")]
fn windows_write(text: &str) {
    let _ = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "Set-Clipboard -Value $env:CONGMIAO_TEXT",
        ])
        .env("CONGMIAO_TEXT", text)
        .status();
}

#[cfg(target_os = "linux")]
fn linux_read() -> Option<String> {
    command_stdout("xclip", &["-o", "-selection", "clipboard"])
        .or_else(|| command_stdout("wl-paste", &[]))
}

#[cfg(target_os = "linux")]
fn command_stdout(program: &str, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new(program)
        .args(args)
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

#[cfg(target_os = "linux")]
fn linux_write(text: &str) {
    if std::process::Command::new("wl-copy")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(stdin) = child.stdin.as_mut() {
                stdin.write_all(text.as_bytes())?;
            }
            child.wait()
        })
        .is_ok()
    {
        return;
    }
    let _ = std::process::Command::new("xclip")
        .args(["-selection", "clipboard"])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(stdin) = child.stdin.as_mut() {
                stdin.write_all(text.as_bytes())?;
            }
            child.wait()
        });
}
