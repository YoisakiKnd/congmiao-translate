use tauri::AppHandle;

pub fn begin(app: &AppHandle) {
    match selected_text() {
        Ok(Some(text)) => crate::popup::open_at_cursor(app, text.trim(), "translate"),
        Ok(None) => crate::popup::open_at_cursor(app, "没有选中文字", "notice"),
        Err(message) => crate::popup::open_at_cursor(app, &message, "notice"),
    }
}

pub fn replace(app: &AppHandle) {
    let text = match selected_text() {
        Ok(Some(text)) => text,
        Ok(None) => {
            crate::popup::open_at_cursor(app, "没有选中文字", "notice");
            return;
        }
        Err(message) => {
            crate::popup::open_at_cursor(app, &message, "notice");
            return;
        }
    };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let translated = translate(&text).await;
        crate::clipboard::write_text(&translated);
        paste_ctrl_v();
        crate::popup::open_at_cursor(&app, &translated, "notice");
    });
}

fn selected_text() -> Result<Option<String>, String> {
    if let Some(text) = uia_selection() {
        return Ok(Some(text));
    }
    copy_with_ctrl_c()
}

fn uia_selection() -> Option<String> {
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Accessibility::{
        CUIAutomation, IUIAutomation, IUIAutomationTextPattern, UIA_TextPatternId,
    };
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let automation: IUIAutomation =
            CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()?;
        let focused = automation.GetFocusedElement().ok()?;
        let text: IUIAutomationTextPattern = focused.GetCurrentPatternAs(UIA_TextPatternId).ok()?;
        let ranges = text.GetSelection().ok()?;
        let count = ranges.Length().ok()?;
        if count == 0 {
            return None;
        }
        let range = ranges.GetElement(0).ok()?;
        let value = range.GetText(-1).ok()?;
        let owned = value.to_string();
        if owned.trim().is_empty() {
            None
        } else {
            Some(owned)
        }
    }
}

fn copy_with_ctrl_c() -> Result<Option<String>, String> {
    send_ctrl(0x43);
    std::thread::sleep(std::time::Duration::from_millis(120));
    Ok(crate::clipboard::read_for_replace())
}

fn paste_ctrl_v() {
    send_ctrl(0x56);
}

fn send_ctrl(key: u16) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{SendInput, INPUT, VK_CONTROL};
    unsafe {
        let mut down = [key_input(VK_CONTROL.0, false), key_input(key, false)];
        SendInput(&down, std::mem::size_of::<INPUT>() as i32);
        down[1] = key_input(key, true);
        down[0] = key_input(VK_CONTROL.0, true);
        SendInput(&down, std::mem::size_of::<INPUT>() as i32);
    }
}

fn key_input(key: u16, up: bool) -> windows::Win32::UI::Input::KeyboardAndMouse::INPUT {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY,
    };
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(key),
                wScan: 0,
                dwFlags: if up {
                    KEYEVENTF_KEYUP
                } else {
                    windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS(0)
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

async fn translate(text: &str) -> String {
    let Ok(dir) = congmiao_core::data_dir() else {
        return text.to_string();
    };
    let paths = congmiao_core::DaemonPaths::new(dir);
    let target = congmiao_core::AppConfig::load_from(&paths.config())
        .map(|config| config.default_target)
        .unwrap_or_else(|_| "zh".into());
    let Ok(endpoint) = congmiao_core::Endpoint::load(&paths.endpoint()) else {
        return text.to_string();
    };
    let Ok(client) = congmiao_core::DaemonClient::new(&endpoint) else {
        return text.to_string();
    };
    client
        .translate("auto", &target, text)
        .await
        .map(|response| response.text)
        .unwrap_or_else(|_| text.to_string())
}
