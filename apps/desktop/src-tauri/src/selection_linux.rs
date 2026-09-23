use std::process::Command;

use tauri::AppHandle;

pub fn begin(app: &AppHandle) {
    match read_selection() {
        Ok(Some(text)) if !text.trim().is_empty() => {
            crate::popup::open_at_cursor(app, text.trim(), "translate");
        }
        Ok(_) => {
            crate::popup::open_at_cursor(app, congmiao_core::wayland_selection_hint(), "notice")
        }
        Err(message) => crate::popup::open_at_cursor(app, &message, "notice"),
    }
}

pub fn replace(app: &AppHandle) {
    let text = match read_selection() {
        Ok(Some(text)) if !text.trim().is_empty() => text,
        Ok(_) => {
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
        if let Err(message) = paste_translation() {
            crate::popup::open_at_cursor(&app, &message, "notice");
            return;
        }
        crate::popup::open_at_cursor(&app, &translated, "notice");
    });
}

pub fn read_selection() -> Result<Option<String>, String> {
    match Command::new("xclip")
        .args(["-o", "-selection", "primary"])
        .output()
    {
        Ok(output) if output.status.success() => {
            let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !text.is_empty() {
                return Ok(Some(text));
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound && !congmiao_core::on_wayland() => {
            return Err(congmiao_core::linux_missing_command("xclip"));
        }
        _ => {}
    }
    match Command::new("wl-paste").arg("--primary").output() {
        Ok(output) if output.status.success() => {
            let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if text.is_empty() {
                Ok(None)
            } else {
                Ok(Some(text))
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound && congmiao_core::on_wayland() => {
            Err(congmiao_core::linux_missing_command("wl-paste"))
        }
        _ => Ok(None),
    }
}

fn paste_translation() -> Result<(), String> {
    Command::new("xdotool")
        .args(["key", "ctrl+v"])
        .status()
        .map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                congmiao_core::linux_missing_command("xdotool")
            } else {
                err.to_string()
            }
        })?;
    Ok(())
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
