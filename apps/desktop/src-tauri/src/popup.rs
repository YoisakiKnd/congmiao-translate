use serde_json::json;
use tauri::{AppHandle, Emitter, LogicalPosition, Manager, PhysicalPosition, WebviewWindow};

pub fn open(app: &AppHandle, x: f64, y: f64, text: &str, kind: &str) {
    let Some(window) = app.get_webview_window("popup") else {
        return;
    };
    place(&window, x, y);
    let _ = window.show();
    if kind == "input" {
        let _ = window.set_focus();
    }
    let _ = window.emit("popup-open", json!({ "text": text, "kind": kind }));
}

pub fn hide(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("popup") {
        let _ = window.hide();
    }
}

fn place(window: &WebviewWindow, x: f64, y: f64) {
    let Some(monitor) = window
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| window.primary_monitor().ok().flatten())
    else {
        let _ = window.set_position(LogicalPosition::new(x.max(8.0), 48.0));
        return;
    };
    let scale = monitor.scale_factor();
    let width = monitor.size().width as f64 / scale;
    let height = monitor.size().height as f64 / scale;
    let origin_x = monitor.position().x as f64 / scale;
    let origin_y = monitor.position().y as f64 / scale;
    let popup_w = 420.0;
    let popup_h = 360.0;
    let mut left = x;
    let mut top = height - y;
    if left + popup_w > origin_x + width {
        left = origin_x + width - popup_w - 12.0;
    }
    if top + popup_h > origin_y + height {
        top = origin_y + height - popup_h - 12.0;
    }
    left = left.max(origin_x + 8.0);
    top = top.max(origin_y + 8.0);
    let _ = window.set_position(PhysicalPosition::new(
        (left * scale) as i32,
        (top * scale) as i32,
    ));
}

pub fn open_at_cursor(app: &AppHandle, text: &str, kind: &str) {
    let (x, y) = cursor();
    open(app, x, y, text, kind);
}

fn cursor() -> (f64, f64) {
    #[cfg(target_os = "macos")]
    {
        macos_cursor()
    }
    #[cfg(not(target_os = "macos"))]
    {
        (80.0, 80.0)
    }
}

#[cfg(target_os = "macos")]
fn macos_cursor() -> (f64, f64) {
    use crate::objc_compat::{class, msg_send, NSPoint};
    unsafe {
        let point: NSPoint = msg_send![class!(NSEvent), mouseLocation];
        (point.x, point.y)
    }
}
