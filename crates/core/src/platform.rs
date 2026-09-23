use crate::error::{Error, Result};

pub trait OcrBackend: Send + Sync {
    fn recognize_png(&self, png: &[u8]) -> Result<String>;
}

pub trait SelectionBackend: Send + Sync {
    fn selected_text(&self) -> Result<Option<String>>;
}

struct Deferred {
    message: &'static str,
}

impl OcrBackend for Deferred {
    fn recognize_png(&self, _png: &[u8]) -> Result<String> {
        Err(Error::PlatformUnavailable(self.message.into()))
    }
}

impl SelectionBackend for Deferred {
    fn selected_text(&self) -> Result<Option<String>> {
        Err(Error::PlatformUnavailable(self.message.into()))
    }
}

pub fn ocr_backend() -> Box<dyn OcrBackend> {
    let message = if cfg!(target_os = "macos") {
        "macOS 截图识别由桌面端调用 Vision"
    } else if cfg!(target_os = "windows") {
        "Windows 截图识别由桌面端调用 Windows.Media.Ocr"
    } else {
        "Linux 截图识别由桌面端调用 xdg-desktop-portal 和 Tesseract"
    };
    Box::new(Deferred { message })
}

pub fn selection_backend() -> Box<dyn SelectionBackend> {
    let message = if cfg!(target_os = "macos") {
        "系统划词由桌面端读取 AXSelectedText"
    } else if cfg!(target_os = "windows") {
        "Windows 划词由桌面端读取 UI Automation TextPattern"
    } else {
        "Linux 划词在 X11 上读取 PRIMARY selection"
    };
    Box::new(Deferred { message })
}

pub fn wayland_selection_hint() -> &'static str {
    "没有读到选中的文字。Wayland 上请先复制，X11 会读取 PRIMARY 选区。"
}

pub fn wayland_shortcut_hint() -> &'static str {
    "Wayland 下全局快捷键可能无效。请改用托盘菜单，或把系统快捷键指向 congmiao://translate。"
}

pub fn on_wayland() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
}

pub fn linux_missing_command(program: &str) -> String {
    let package = match program {
        "xclip" => "xclip",
        "xdotool" => "xdotool",
        "tesseract" => "tesseract-ocr tesseract-ocr-chi-sim",
        "gdbus" => "libglib2.0-bin",
        "dbus-monitor" => "dbus",
        "gnome-screenshot" => "gnome-screenshot",
        "wl-paste" | "wl-copy" => "wl-clipboard",
        other => other,
    };
    format!("没有找到 {program}。请安装：sudo apt install {package}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deferred_backends_name_the_platform_api() {
        let err = ocr_backend().recognize_png(&[]).unwrap_err();
        let text = err.to_string();
        if cfg!(target_os = "macos") {
            assert!(text.contains("Vision"));
        }
        let selection = selection_backend().selected_text().unwrap_err().to_string();
        if cfg!(target_os = "macos") {
            assert!(selection.contains("AXSelectedText"));
        }
        assert!(wayland_selection_hint().contains("Wayland"));
        assert!(wayland_shortcut_hint().contains("congmiao://translate"));
        assert!(linux_missing_command("tesseract").contains("tesseract-ocr"));
        assert!(linux_missing_command("xclip").contains("sudo apt install xclip"));
    }
}
