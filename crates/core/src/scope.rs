/// 选定方案 B：跨平台 Rust 核心 + Tauri 薄界面 + 浏览器扩展。
pub const ARCHITECTURE: &str = "B";

/// 桌面端覆盖这三个平台。
pub const V1_PLATFORMS: &[&str] = &["macos", "windows", "linux"];

/// v1 先打通的入口。OCR 截图和系统划词目前只在 macOS 上有交互。
pub const V1_ENTRIES: &[&str] = &[
    "in_app",
    "browser_extension",
    "ocr_screenshot",
    "system_selection",
];

/// 接口已留出，还没有交互。
pub const DEFERRED_ENTRIES: &[&str] = &[];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locks_the_chosen_architecture_and_v1_scope() {
        assert_eq!(ARCHITECTURE, "B");
        assert_eq!(V1_PLATFORMS, ["macos", "windows", "linux"]);
        assert_eq!(
            V1_ENTRIES,
            [
                "in_app",
                "browser_extension",
                "ocr_screenshot",
                "system_selection"
            ]
        );
        assert!(DEFERRED_ENTRIES.is_empty());
    }
}
