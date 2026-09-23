use congmiao_core::{youdao_voice_url, AppConfig, ShortcutConfig};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

use crate::{capture, clipboard, selection, speech};

pub fn handle_shortcut(app: &AppHandle, shortcut: &Shortcut) {
    let Ok(config) = load_config() else {
        return;
    };
    if same(&config.shortcuts.selection, shortcut) {
        selection::begin(app);
    } else if same(&config.shortcuts.screenshot, shortcut) {
        capture::begin(app);
    } else if same(&config.shortcuts.input, shortcut) {
        crate::popup::open_at_cursor(app, "", "input");
    } else if same(&config.shortcuts.replace, shortcut) {
        selection::replace(app);
    } else if same(&config.shortcuts.silent_ocr, shortcut) {
        capture::begin_silent(app);
    }
}

pub fn register_shortcuts(app: &AppHandle) {
    let Ok(config) = load_config() else {
        return;
    };
    let _ = app.global_shortcut().unregister_all();
    for raw in [
        config.shortcuts.screenshot.as_str(),
        config.shortcuts.selection.as_str(),
        config.shortcuts.input.as_str(),
        config.shortcuts.replace.as_str(),
        config.shortcuts.silent_ocr.as_str(),
    ] {
        if let Ok(shortcut) = raw.parse::<Shortcut>() {
            if let Err(err) = app.global_shortcut().register(shortcut) {
                eprintln!("快捷键 {raw} 没有注册成功：{err}");
            }
        }
    }
}

pub fn validate_shortcuts(shortcuts: &ShortcutConfig) -> Result<(), String> {
    let values = [
        shortcuts.screenshot.as_str(),
        shortcuts.selection.as_str(),
        shortcuts.input.as_str(),
        shortcuts.replace.as_str(),
        shortcuts.silent_ocr.as_str(),
    ];
    let mut parsed = Vec::new();
    for raw in values {
        let shortcut = raw
            .parse::<Shortcut>()
            .map_err(|_| format!("无法识别快捷键：{raw}"))?;
        if parsed.contains(&shortcut) {
            return Err(format!("快捷键冲突：{raw}"));
        }
        parsed.push(shortcut);
    }
    Ok(())
}

fn same(raw: &str, shortcut: &Shortcut) -> bool {
    raw.parse::<Shortcut>().ok().as_ref() == Some(shortcut)
}

fn load_config() -> Result<AppConfig, String> {
    let dir = congmiao_core::data_dir().map_err(|err| err.to_string())?;
    AppConfig::load_from(&congmiao_core::DaemonPaths::new(dir).config())
        .map_err(|err| err.to_string())
}

#[tauri::command]
pub async fn translate_compare(
    app: AppHandle,
    text: String,
    source: String,
    target: String,
) -> Result<congmiao_core::CompareResponse, String> {
    let client = crate::client_after_wait().await?;
    let app_for_events = app.clone();
    let response = client
        .stream_compare(&source, &target, &text, |output| {
            let _ = app_for_events.emit("engine-result", &output);
        })
        .await
        .map_err(|err| err.to_string())?;
    if let Ok(config) = load_config() {
        if config.auto_copy {
            if let Some(text) = response
                .results
                .iter()
                .find(|item| item.error.is_none() && !item.text.is_empty())
                .map(|item| item.text.clone())
            {
                clipboard::write_text(&text);
            }
        }
    }
    Ok(response)
}

#[tauri::command]
pub async fn test_engine(kind: String) -> Result<congmiao_core::EngineOutput, String> {
    let client = crate::client_after_wait().await?;
    client
        .test_engine(&kind)
        .await
        .map_err(|err| err.to_string())
}

#[tauri::command]
pub async fn lookup_dict(text: String) -> Result<congmiao_core::DictEntry, String> {
    let client = crate::client_after_wait().await?;
    client.dict(&text).await.map_err(|err| err.to_string())
}

#[derive(Debug, Deserialize)]
pub struct HistoryAction {
    op: String,
    #[serde(default)]
    id: i64,
    #[serde(default)]
    query: String,
}

#[tauri::command]
pub async fn history_action(action: HistoryAction) -> Result<serde_json::Value, String> {
    let client = crate::client_after_wait().await?;
    match action.op.as_str() {
        "list" => client
            .history(&action.query)
            .await
            .map(|items| serde_json::json!(items))
            .map_err(|err| err.to_string()),
        "delete" => client
            .delete_history(action.id)
            .await
            .map(|()| serde_json::json!({ "ok": true }))
            .map_err(|err| err.to_string()),
        "clear" => client
            .clear_history()
            .await
            .map(|()| serde_json::json!({ "ok": true }))
            .map_err(|err| err.to_string()),
        _ => Err("未知的历史操作".into()),
    }
}

#[derive(Debug, Deserialize)]
pub struct VocabularyAction {
    op: String,
    #[serde(default)]
    id: i64,
    #[serde(default)]
    word: String,
    #[serde(default)]
    translation: String,
    #[serde(default)]
    phonetic: String,
    #[serde(default)]
    format: String,
}

#[tauri::command]
pub async fn vocabulary_action(action: VocabularyAction) -> Result<serde_json::Value, String> {
    let client = crate::client_after_wait().await?;
    match action.op.as_str() {
        "list" => client
            .vocabulary()
            .await
            .map(|items| serde_json::json!(items))
            .map_err(|err| err.to_string()),
        "add" => client
            .add_vocabulary(&action.word, &action.translation, &action.phonetic)
            .await
            .map(|id| serde_json::json!({ "id": id }))
            .map_err(|err| err.to_string()),
        "delete" => client
            .delete_vocabulary(action.id)
            .await
            .map(|()| serde_json::json!({ "ok": true }))
            .map_err(|err| err.to_string()),
        "export" => client
            .export_vocabulary(if action.format == "anki" {
                "anki"
            } else {
                "csv"
            })
            .await
            .map(|text| serde_json::json!({ "text": text }))
            .map_err(|err| err.to_string()),
        _ => Err("未知的生词本操作".into()),
    }
}

#[tauri::command]
pub async fn clear_cache() -> Result<(), String> {
    let client = crate::client_after_wait().await?;
    client.clear_cache().await.map_err(|err| err.to_string())
}

#[derive(Debug, Serialize)]
pub struct SpeakResult {
    audio_url: String,
}

#[tauri::command]
pub fn speak(app: AppHandle, text: String, lang: String) -> Result<SpeakResult, String> {
    let _ = lang;
    speech::speak(&app, text.clone())?;
    Ok(SpeakResult {
        audio_url: youdao_voice_url(&text),
    })
}

#[derive(Debug, Deserialize)]
pub struct AppAction {
    op: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    pinned: bool,
}

#[derive(Debug, Serialize)]
pub struct AppInfo {
    version: String,
    accessibility: bool,
    screen_recording: bool,
    log_dir: String,
    data_dir: String,
}

#[tauri::command]
pub fn app_action(app: AppHandle, action: AppAction) -> Result<AppInfo, String> {
    match action.op.as_str() {
        "info" => {}
        "popup-hide" => {
            if !action.pinned {
                crate::popup::hide(&app);
            }
        }
        "input" => crate::popup::open_at_cursor(&app, "", "input"),
        "selection" => selection::begin(&app),
        "capture" => capture::begin(&app),
        "silent" => capture::begin_silent(&app),
        "replace" => selection::replace(&app),
        "logs" => open_dir(&log_dir()?),
        "request-accessibility" => {
            selection::request_accessibility();
            open_privacy("Privacy_Accessibility");
        }
        "request-screen" => {
            capture::request_screen_recording();
            open_privacy("Privacy_ScreenCapture");
        }
        "open-data" => open_dir(&data_dir_text()?),
        "update" => {
            let handle = app.clone();
            tauri::async_runtime::spawn(async move {
                use tauri_plugin_updater::UpdaterExt;
                if let Ok(updater) = handle.updater() {
                    if let Ok(Some(update)) = updater.check().await {
                        let _ = update.download_and_install(|_, _| {}, || {}).await;
                    }
                }
            });
        }
        _ => return Err("未知操作".into()),
    }
    Ok(current_info())
}

fn current_info() -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION").into(),
        accessibility: selection::accessibility_granted(),
        screen_recording: capture::screen_recording_granted(),
        log_dir: log_dir().unwrap_or_default(),
        data_dir: data_dir_text().unwrap_or_default(),
    }
}

fn log_dir() -> Result<String, String> {
    let dir = congmiao_core::data_dir().map_err(|err| err.to_string())?;
    Ok(congmiao_core::DaemonPaths::new(dir)
        .logs()
        .display()
        .to_string())
}

fn data_dir_text() -> Result<String, String> {
    congmiao_core::data_dir()
        .map(|dir| dir.display().to_string())
        .map_err(|err| err.to_string())
}

#[derive(Debug, Deserialize)]
pub struct JobAction {
    pub op: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub input_path: String,
    #[serde(default)]
    pub engine: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub mc_version: String,
    #[serde(default)]
    pub glossary: String,
    #[serde(default)]
    pub id: String,
}

#[tauri::command]
pub async fn job_action(action: JobAction) -> Result<serde_json::Value, String> {
    if action.op == "reveal" {
        let path = std::path::Path::new(&action.input_path);
        let folder = if path.is_dir() {
            path.to_path_buf()
        } else {
            path.parent().unwrap_or(path).to_path_buf()
        };
        open_dir(&folder.display().to_string());
        return Ok(serde_json::json!({ "ok": true }));
    }
    let client = crate::client_after_wait().await?;
    let mode = if action.op == "preview" {
        "preview"
    } else if action.mode.is_empty() {
        "translated"
    } else {
        action.mode.as_str()
    };
    let value = match action.op.as_str() {
        "start" | "preview" => client
            .job_start(&serde_json::json!({
                "kind": action.kind,
                "input_path": action.input_path,
                "engine": action.engine,
                "source": if action.source.is_empty() { "auto" } else { action.source.as_str() },
                "target": if action.target.is_empty() { "zh" } else { action.target.as_str() },
                "mode": mode,
                "mc_version": action.mc_version,
                "glossary": action.glossary,
            }))
            .await,
        "status" => client.job_status(&action.id).await,
        "cancel" => client.job_cancel(&action.id).await,
        "continue" => client.job_continue(&action.id).await,
        _ => return Err("未知的任务操作".into()),
    };
    value.map_err(|err| err.to_string())
}

fn open_dir(path: &str) {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(path).spawn();
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("explorer").arg(path).spawn();
    #[cfg(target_os = "linux")]
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    let _ = path;
}

fn open_privacy(anchor: &str) {
    #[cfg(target_os = "macos")]
    {
        let url = format!("x-apple.systempreferences:com.apple.preference.security?{anchor}");
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
    #[cfg(not(target_os = "macos"))]
    let _ = anchor;
}
