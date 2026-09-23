use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use congmiao_core::FIREFOX_EXTENSION_ID;
use congmiao_daemon::{install_host, serve, InstallRequest};
use tokio::sync::watch;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("install-host") => install(args.collect()),
        Some("serve") | None => run_serve(),
        Some(other) => {
            eprintln!("未知命令 {other}。可用命令：serve、install-host");
            ExitCode::from(2)
        }
    }
}

fn run_serve() -> ExitCode {
    if let Ok(dir) = congmiao_core::data_dir() {
        congmiao_daemon::init_log(&congmiao_core::DaemonPaths::new(dir).logs());
    }
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::from(1);
        }
    };
    let (tx, rx) = watch::channel(false);
    runtime.spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        let _ = tx.send(true);
    });
    match runtime.block_on(serve(rx)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::from(1)
        }
    }
}

fn install(args: Vec<String>) -> ExitCode {
    let mut chrome_extension_id = None;
    let mut firefox_extension_id = None;
    let mut firefox = false;
    let mut dry_run = false;
    let mut host_path = None;
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--dry-run" => dry_run = true,
            "--firefox" => firefox = true,
            "--extension-id" => chrome_extension_id = iter.next(),
            "--firefox-id" => firefox_extension_id = iter.next(),
            "--host" => host_path = iter.next(),
            other => {
                eprintln!("无法识别的参数 {other}");
                return ExitCode::from(2);
            }
        }
    }
    if firefox && firefox_extension_id.is_none() {
        firefox_extension_id = Some(FIREFOX_EXTENSION_ID.to_string());
    }
    if chrome_extension_id.is_none() && firefox_extension_id.is_none() {
        eprintln!(
            "用法：congmiao-daemon install-host --firefox [--firefox-id <id>] [--extension-id <chrome-id>] [--host <path>] [--dry-run]"
        );
        return ExitCode::from(2);
    }
    let host_path = host_path
        .map(PathBuf::from)
        .unwrap_or_else(default_host_path);
    match install_host(&InstallRequest {
        chrome_extension_id,
        firefox_extension_id,
        host_path,
        dry_run,
    }) {
        Ok(paths) => {
            for path in paths {
                println!("{}", path.display());
            }
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("{err}");
            ExitCode::from(1)
        }
    }
}

fn default_host_path() -> PathBuf {
    env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|dir| dir.join(host_name())))
        .unwrap_or_else(|| PathBuf::from(host_name()))
}

fn host_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "congmiao-host.exe"
    } else {
        "congmiao-host"
    }
}
