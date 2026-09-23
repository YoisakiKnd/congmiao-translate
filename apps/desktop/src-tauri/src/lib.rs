use std::time::Duration;

use congmiao_core::{
    data_dir, language_name, parse_glossary, AppConfig, DaemonClient, DaemonPaths, Endpoint,
    EngineConfig, EngineKind,
};
use congmiao_daemon::serve;
use serde::{Deserialize, Serialize};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, WindowEvent};
use tauri_plugin_deep_link::DeepLinkExt;
use tauri_plugin_global_shortcut::ShortcutState;
use tokio::sync::watch;

mod capture;
mod clipboard;
mod commands;
mod deeplink;
#[cfg(target_os = "macos")]
mod objc_compat;
mod popup;
mod portal;
mod selection;
mod smoke;
mod speech;

struct DaemonControl {
    shutdown: watch::Sender<bool>,
}

#[derive(Debug, Serialize)]
struct SettingsView {
    engines: Vec<EngineConfig>,
    default_source: String,
    default_target: String,
    glossary_text: String,
    data_dir: String,
    swap_when_same: bool,
    shortcuts: congmiao_core::ShortcutConfig,
    watch_clipboard: bool,
    auto_copy: bool,
    launch_at_login: bool,
    ui_language: String,
    show_tray: bool,
    onboarded: bool,
    prompt: String,
    proxy: String,
    job_concurrency: u8,
    auto_translate: bool,
    wayland_hint: String,
}

#[derive(Debug, Deserialize)]
struct SettingsInput {
    engines: Vec<EngineConfig>,
    default_source: String,
    default_target: String,
    glossary_text: String,
    swap_when_same: bool,
    shortcuts: congmiao_core::ShortcutConfig,
    watch_clipboard: bool,
    auto_copy: bool,
    launch_at_login: bool,
    ui_language: String,
    show_tray: bool,
    onboarded: bool,
    prompt: String,
    proxy: String,
    job_concurrency: u8,
    auto_translate: bool,
}

#[derive(Debug, Serialize)]
struct DaemonStatus {
    running: bool,
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            deeplink::open_from_args(app, &args);
        }))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if event.state != ShortcutState::Pressed {
                        return;
                    }
                    commands::handle_shortcut(app, shortcut);
                })
                .build(),
        )
        .setup(|app| {
            let smoke = smoke::report_path();
            if smoke.is_some() && std::env::var_os("CONGMIAO_DATA_DIR").is_none() {
                eprintln!("CONGMIAO_SMOKE 需要同时设置 CONGMIAO_DATA_DIR，避免改写日常配置");
                std::process::exit(1);
            }
            if smoke.is_some() {
                if let Ok(dir) = data_dir() {
                    std::fs::create_dir_all(&dir).ok();
                    smoke::write_echo_config(&dir);
                }
            }
            let (shutdown, rx) = watch::channel(false);
            app.manage(DaemonControl { shutdown });
            tauri::async_runtime::spawn(async move {
                if daemon_is_running().await {
                    return;
                }
                if let Err(err) = serve(rx).await {
                    eprintln!("{err}");
                }
            });
            if let Ok(dir) = data_dir() {
                congmiao_daemon::init_log(&DaemonPaths::new(dir).logs());
            }
            if smoke.is_none() {
                commands::register_shortcuts(app.handle());
                clipboard::start(app.handle().clone());
            }
            #[cfg(any(target_os = "windows", target_os = "linux"))]
            {
                let _ = app.deep_link().register_all();
            }
            let handle = app.handle().clone();
            app.deep_link().on_open_url(move |event| {
                deeplink::open_from_urls(&handle, &event.urls());
            });
            if let Ok(Some(urls)) = app.deep_link().get_current() {
                deeplink::open_from_urls(app.handle(), &urls);
            }
            if let Ok(paths) = paths() {
                if let Ok(config) = AppConfig::load_from(&paths.config()) {
                    clipboard::set_watch(config.watch_clipboard);
                }
            }
            setup_tray(app)?;
            if let Some(path) = smoke {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let report = smoke::run(&handle).await;
                    let ok = report
                        .get("ok")
                        .and_then(|value| value.as_bool())
                        .unwrap_or(false);
                    if let Some(parent) = std::path::Path::new(&path).parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    let _ = std::fs::write(
                        &path,
                        serde_json::to_string_pretty(&report).unwrap_or_default(),
                    );
                    handle.exit(if ok { 0 } else { 1 });
                });
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            translate,
            commands::translate_compare,
            commands::test_engine,
            commands::lookup_dict,
            commands::history_action,
            commands::vocabulary_action,
            commands::clear_cache,
            commands::speak,
            commands::app_action,
            commands::job_action,
            get_settings,
            save_settings,
            daemon_status
        ])
        .run(tauri::generate_context!())
        .expect("启动从喵翻译失败");
}

fn setup_tray(app: &mut tauri::App) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "打开", true, None::<&str>)?;
    let capture = MenuItem::with_id(app, "capture", "截图翻译", true, None::<&str>)?;
    let selection = MenuItem::with_id(app, "selection", "划词翻译", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &capture, &selection, &quit])?;
    let mut tray = TrayIconBuilder::new()
        .menu(&menu)
        .tooltip("从喵翻译")
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main(app),
            "capture" => capture::begin(app),
            "selection" => selection::begin(app),
            "quit" => {
                if let Some(control) = app.try_state::<DaemonControl>() {
                    let _ = control.shutdown.send(true);
                }
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

pub(crate) fn show_main(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

#[tauri::command]
async fn translate(
    text: String,
    source: String,
    target: String,
) -> Result<congmiao_core::TranslateResponse, String> {
    let client = client_after_wait().await?;
    client
        .translate(&source, &target, &text)
        .await
        .map_err(|err| err.to_string())
}

#[tauri::command]
fn get_settings() -> Result<SettingsView, String> {
    let paths = paths()?;
    let config = AppConfig::load_from(&paths.config()).map_err(|err| err.to_string())?;
    Ok(SettingsView {
        engines: config.engines,
        default_source: config.default_source,
        default_target: config.default_target,
        glossary_text: config
            .glossary
            .iter()
            .map(|entry| format!("{}={}", entry.source, entry.target))
            .collect::<Vec<_>>()
            .join("\n"),
        data_dir: paths.data_dir.display().to_string(),
        swap_when_same: config.swap_when_same,
        shortcuts: config.shortcuts,
        watch_clipboard: config.watch_clipboard,
        auto_copy: config.auto_copy,
        launch_at_login: config.launch_at_login,
        ui_language: config.ui_language,
        show_tray: config.show_tray,
        onboarded: config.onboarded,
        prompt: config.prompt,
        proxy: config.proxy,
        job_concurrency: config.job_concurrency,
        auto_translate: config.auto_translate,
        wayland_hint: if congmiao_core::on_wayland() {
            congmiao_core::wayland_shortcut_hint().into()
        } else {
            String::new()
        },
    })
}

#[tauri::command]
fn save_settings(app: AppHandle, input: SettingsInput) -> Result<(), String> {
    if language_name(&input.default_source).is_none()
        || language_name(&input.default_target).is_none()
    {
        return Err("语言设置无效".into());
    }
    if input.default_target == "auto" {
        return Err("目标语言不能是自动检测".into());
    }
    let glossary = parse_glossary(&input.glossary_text)?;
    for engine in &input.engines {
        if EngineKind::parse(engine.kind.as_str()).is_none() {
            return Err("未知的翻译引擎".into());
        }
    }
    let config = AppConfig {
        engines: input.engines,
        default_source: input.default_source,
        default_target: input.default_target,
        glossary,
        swap_when_same: input.swap_when_same,
        shortcuts: input.shortcuts,
        watch_clipboard: input.watch_clipboard,
        auto_copy: input.auto_copy,
        launch_at_login: input.launch_at_login,
        ui_language: input.ui_language,
        show_tray: input.show_tray,
        onboarded: input.onboarded,
        prompt: input.prompt,
        proxy: input.proxy,
        job_concurrency: input.job_concurrency.max(1),
        auto_translate: input.auto_translate,
    };
    commands::validate_shortcuts(&config.shortcuts)?;
    let paths = paths()?;
    config
        .save_to(&paths.config())
        .map_err(|err| err.to_string())?;
    clipboard::set_watch(config.watch_clipboard);
    commands::register_shortcuts(&app);
    apply_login(&app, config.launch_at_login);
    Ok(())
}

fn apply_login(app: &AppHandle, enabled: bool) {
    use tauri_plugin_autostart::ManagerExt;
    let autolaunch = app.autolaunch();
    let _ = if enabled {
        autolaunch.enable()
    } else {
        autolaunch.disable()
    };
}

#[tauri::command]
async fn daemon_status() -> DaemonStatus {
    DaemonStatus {
        running: daemon_is_running().await,
    }
}

fn paths() -> Result<DaemonPaths, String> {
    Ok(DaemonPaths::new(data_dir().map_err(|err| err.to_string())?))
}

pub(crate) async fn client_after_wait() -> Result<DaemonClient, String> {
    for _ in 0..80 {
        if let Some(client) = running_client().await {
            return Ok(client);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Err("从喵翻译的本机服务还没有就绪".into())
}

async fn daemon_is_running() -> bool {
    running_client().await.is_some()
}

async fn running_client() -> Option<DaemonClient> {
    let paths = DaemonPaths::new(data_dir().ok()?);
    let endpoint = Endpoint::load(&paths.endpoint()).ok()?;
    let client = DaemonClient::new(&endpoint).ok()?;
    client.health().await.ok()?;
    Some(client)
}
