use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::AppHandle;

pub fn begin(app: &AppHandle) {
    let silent = crate::capture::take_silent();
    let app = app.clone();
    std::thread::spawn(move || {
        let text = recognize().unwrap_or_else(|err| err);
        if text.trim().is_empty() {
            crate::popup::open_at_cursor(&app, "没有识别到文字", "notice");
            return;
        }
        if silent {
            crate::clipboard::write_text(text.trim());
            crate::popup::open_at_cursor(&app, "已复制识别到的文字", "notice");
            return;
        }
        crate::popup::open_at_cursor(&app, text.trim(), "ocr");
    });
}

fn recognize() -> Result<String, String> {
    let path = std::env::temp_dir().join(format!(
        "congmiao-ocr-{}.png",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or(0)
    ));
    let path_text = path.display().to_string();
    if portal_screenshot(&path_text).is_err() && gnome_screenshot(&path_text).is_err() {
        return Err("没有完成截图。Linux 需要 xdg-desktop-portal 或 gnome-screenshot。".into());
    }
    let output = Command::new("tesseract")
        .args([&path_text, "stdout", "-l", "chi_sim+eng"])
        .output()
        .map_err(|_| congmiao_core::linux_missing_command("tesseract"))?;
    let _ = std::fs::remove_file(&path);
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn portal_screenshot(path: &str) -> Result<(), String> {
    use std::io::{BufRead, BufReader};
    use std::time::Duration;
    let token = format!(
        "congmiao{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or(0)
    );
    let mut monitor = Command::new("dbus-monitor")
        .args([
            "--session",
            "type='signal',interface='org.freedesktop.portal.Request',member='Response'",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                congmiao_core::linux_missing_command("dbus-monitor")
            } else {
                err.to_string()
            }
        })?;
    let stdout = monitor.stdout.take().ok_or("无法读取 portal 响应")?;
    std::thread::sleep(Duration::from_millis(200));
    let options = format!("{{'interactive': <true>, 'handle_token': <'{token}'>}}");
    let started = Command::new("gdbus")
        .args([
            "call",
            "--session",
            "--dest",
            "org.freedesktop.portal.Desktop",
            "--object-path",
            "/org/freedesktop/portal/desktop",
            "--method",
            "org.freedesktop.portal.Screenshot.Screenshot",
            "",
            &options,
        ])
        .output();
    if let Err(err) = &started {
        let _ = monitor.kill();
        if err.kind() == std::io::ErrorKind::NotFound {
            return Err(congmiao_core::linux_missing_command("gdbus"));
        }
        return Err(err.to_string());
    }
    if started
        .as_ref()
        .map(|output| !output.status.success())
        .unwrap_or(true)
    {
        let _ = monitor.kill();
        return Err("portal 截图没有启动".into());
    }
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut all = String::new();
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => {
                    let _ = tx.send(all);
                    return;
                }
                Ok(_) => {
                    all.push_str(&line);
                    if all.contains("file://") || all.contains("uint32 1") {
                        let _ = tx.send(all);
                        return;
                    }
                }
            }
        }
    });
    let text = rx
        .recv_timeout(Duration::from_secs(60))
        .map_err(|_| "portal 没有在时限内返回截图".to_string());
    let _ = monitor.kill();
    let source = crate::portal::file_from_monitor(&text?).ok_or("portal 没有返回截图")?;
    std::fs::copy(source, path).map_err(|err| err.to_string())?;
    Ok(())
}

fn gnome_screenshot(path: &str) -> Result<(), String> {
    let status = Command::new("gnome-screenshot")
        .args(["-a", "-f", path])
        .status()
        .map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                congmiao_core::linux_missing_command("gnome-screenshot")
            } else {
                err.to_string()
            }
        })?;
    if status.success() {
        Ok(())
    } else {
        Err("gnome-screenshot 没有完成".into())
    }
}

pub fn recognize_bytes(bytes: &[u8]) -> Result<String, String> {
    let path = std::env::temp_dir().join(format!(
        "congmiao-fixture-{}.png",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or(0)
    ));
    std::fs::write(&path, bytes).map_err(|err| err.to_string())?;
    let output = Command::new("tesseract")
        .args([path.to_str().unwrap_or(""), "stdout", "-l", "eng"])
        .output()
        .map_err(|_| congmiao_core::linux_missing_command("tesseract"))?;
    let _ = std::fs::remove_file(&path);
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
