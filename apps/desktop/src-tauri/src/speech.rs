use tauri::AppHandle;

pub fn speak(app: &AppHandle, text: String) -> Result<(), String> {
    if text.trim().is_empty() {
        return Err("没有可朗读的文字".into());
    }
    #[cfg(target_os = "macos")]
    {
        let text = text.clone();
        app.run_on_main_thread(move || macos_speak(&text))
            .map_err(|err| err.to_string())?;
        return Ok(());
    }
    #[cfg(target_os = "windows")]
    {
        let _ = app;
        return windows_speak(&text);
    }
    #[cfg(target_os = "linux")]
    {
        let _ = app;
        return linux_speak(&text);
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        let _ = (app, text);
        Err("当前平台没有系统朗读".into())
    }
}

#[cfg(target_os = "macos")]
fn macos_speak(text: &str) {
    use crate::objc_compat::{class, id, msg_send, nil, ns_string, BOOL};
    use std::cell::RefCell;
    thread_local! {
        static SPEAKER: RefCell<id> = const { RefCell::new(nil) };
    }
    unsafe {
        SPEAKER.with(|slot| {
            let current = *slot.borrow();
            if !current.is_null() {
                let _: () = msg_send![current, stopSpeaking];
            }
            let speaker: id = msg_send![class!(NSSpeechSynthesizer), alloc];
            let speaker: id = msg_send![speaker, initWithVoice: nil];
            let value = ns_string(text);
            let _: BOOL = msg_send![speaker, startSpeakingString: value];
            *slot.borrow_mut() = speaker;
        });
    }
}

#[cfg(target_os = "windows")]
fn windows_speak(text: &str) -> Result<(), String> {
    std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "Add-Type -AssemblyName System.Speech; (New-Object System.Speech.Synthesis.SpeechSynthesizer).Speak($env:CONGMIAO_TEXT)",
        ])
        .env("CONGMIAO_TEXT", text)
        .spawn()
        .map(|_| ())
        .map_err(|err| err.to_string())
}

#[cfg(target_os = "linux")]
fn linux_speak(text: &str) -> Result<(), String> {
    std::process::Command::new("spd-say")
        .arg(text)
        .spawn()
        .map(|_| ())
        .map_err(|err| format!("没有找到 speech-dispatcher：{err}"))
}
