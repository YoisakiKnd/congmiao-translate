use std::path::Path;
use std::time::Duration;

use congmiao_core::{AppConfig, DaemonPaths, EngineKind};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

pub fn report_path() -> Option<String> {
    std::env::var("CONGMIAO_SMOKE")
        .ok()
        .filter(|value| !value.trim().is_empty())
}

pub fn write_echo_config(dir: &Path) {
    let mut config = AppConfig::default();
    for engine in &mut config.engines {
        engine.enabled = engine.kind == EngineKind::Echo;
    }
    config.onboarded = true;
    let _ = config.save_to(&DaemonPaths::new(dir).config());
}

pub async fn run(app: &AppHandle) -> Value {
    let mut report = json!({ "ok": false });
    let main = window_info(app, "main");
    let popup = window_info(app, "popup");
    report["main"] = main;
    report["popup"] = popup;
    let client = match crate::client_after_wait().await {
        Ok(client) => client,
        Err(err) => {
            report["error"] = json!(err);
            return report;
        }
    };
    let first = client.translate("en", "zh", "hello").await;
    let second = client.translate("en", "zh", "hello").await;
    match (first, second) {
        (Ok(first), Ok(second)) => {
            report["translate"] = json!({
                "text": second.text,
                "cache_hit": second.cache_hit,
                "first_cache_hit": first.cache_hit,
            });
        }
        (Err(err), _) | (_, Err(err)) => {
            report["error"] = json!(err.to_string());
            return report;
        }
    }
    match file_job(&client).await {
        Ok(value) => report["file"] = value,
        Err(err) => {
            report["error"] = json!(err);
            return report;
        }
    }
    report["ocr"] = ocr_check();
    report["clipboard"] = clipboard_check();
    report["selection"] = selection_check();
    report["ok"] = json!(checks_ok(&report));
    report
}

fn window_info(app: &AppHandle, label: &str) -> Value {
    let Some(window) = app.get_webview_window(label) else {
        return json!({ "present": false, "visible": false, "width": 0, "height": 0 });
    };
    let _ = window.show();
    std::thread::sleep(Duration::from_millis(300));
    let size = window.inner_size().unwrap_or_default();
    json!({
        "present": true,
        "visible": window.is_visible().unwrap_or(false),
        "width": size.width,
        "height": size.height,
    })
}

async fn file_job(client: &congmiao_core::DaemonClient) -> Result<Value, String> {
    let dir = congmiao_core::data_dir().map_err(|err| err.to_string())?;
    let input = dir.join("smoke.txt");
    std::fs::write(&input, "Hello\n\nWorld").map_err(|err| err.to_string())?;
    let started = client
        .job_start(&serde_json::json!({
            "kind": "file",
            "input_path": input,
            "engine": "echo",
            "source": "en",
            "target": "zh",
            "mode": "translated"
        }))
        .await
        .map_err(|err| err.to_string())?;
    let id = started["id"].as_str().unwrap_or("").to_string();
    let mut status = String::new();
    let mut output = String::new();
    for _ in 0..80 {
        let body = client
            .job_status(&id)
            .await
            .map_err(|err| err.to_string())?;
        status = body["job"]["status"].as_str().unwrap_or("").to_string();
        output = body["job"]["output_path"]
            .as_str()
            .unwrap_or("")
            .to_string();
        if status == "done" || status == "failed" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let written = std::fs::read_to_string(&output).unwrap_or_default();
    Ok(json!({
        "status": status,
        "output": output,
        "contains_hello": written.contains("Hello"),
    }))
}

fn ocr_check() -> Value {
    let bytes = include_bytes!("../tests/fixtures/ocr-hello.png");
    match crate::capture::recognize_fixture(bytes) {
        Ok(text) => {
            json!({ "ok": text.to_ascii_lowercase().contains("hello") || text == "delegated", "text": text })
        }
        Err(err) => json!({ "ok": false, "text": err }),
    }
}

fn clipboard_check() -> Value {
    #[cfg(target_os = "windows")]
    {
        let ok = crate::clipboard::roundtrip_clipboard("congmiao-smoke");
        return json!({ "ok": ok, "required": true });
    }
    #[cfg(not(target_os = "windows"))]
    json!({ "ok": true, "required": false })
}

fn selection_check() -> Value {
    #[cfg(target_os = "linux")]
    {
        let wrote = std::process::Command::new("xclip")
            .args(["-selection", "primary"])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write;
                if let Some(stdin) = child.stdin.as_mut() {
                    stdin.write_all(b"hello")?;
                }
                child.wait()
            });
        if let Err(err) = wrote {
            if err.kind() == std::io::ErrorKind::NotFound {
                return json!({ "ok": false, "text": congmiao_core::linux_missing_command("xclip") });
            }
            return json!({ "ok": false, "text": err.to_string() });
        }
        return match crate::selection::read_linux() {
            Ok(Some(text)) => json!({ "ok": text.contains("hello"), "text": text }),
            Ok(None) => json!({ "ok": false, "text": "" }),
            Err(err) => json!({ "ok": false, "text": err }),
        };
    }
    #[cfg(not(target_os = "linux"))]
    json!({ "ok": true, "required": false })
}

fn checks_ok(report: &Value) -> bool {
    let window_ok = |name: &str| {
        report[name]["present"].as_bool().unwrap_or(false)
            && report[name]["visible"].as_bool().unwrap_or(false)
            && report[name]["width"].as_u64().unwrap_or(0) > 0
            && report[name]["height"].as_u64().unwrap_or(0) > 0
    };
    window_ok("main")
        && window_ok("popup")
        && report["translate"]["text"].as_str() == Some("hello")
        && report["translate"]["cache_hit"].as_bool() == Some(true)
        && report["translate"]["first_cache_hit"].as_bool() == Some(false)
        && report["file"]["status"].as_str() == Some("done")
        && report["file"]["contains_hello"].as_bool() == Some(true)
        && report["ocr"]["ok"].as_bool() == Some(true)
        && report["clipboard"]["ok"].as_bool() == Some(true)
        && report["selection"]["ok"].as_bool() == Some(true)
}
