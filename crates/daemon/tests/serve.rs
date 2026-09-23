use std::time::{Duration, SystemTime, UNIX_EPOCH};

use congmiao_core::{
    dispatch_host, AppConfig, DaemonClient, DaemonPaths, Endpoint, EngineKind, HostRequest,
};
use congmiao_daemon::serve_with;
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio::time::sleep;

#[tokio::test]
async fn extension_and_desktop_share_one_daemon() {
    let _guard = test_lock().await;
    let dir = std::env::temp_dir().join(format!(
        "congmiao-daemon-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let paths = DaemonPaths::new(&dir);
    let mut config = AppConfig::default();
    for engine in &mut config.engines {
        engine.enabled = engine.kind == EngineKind::Echo;
    }
    config.save_to(&paths.config()).unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (shutdown, rx) = watch::channel(false);
    let task = tokio::spawn(serve_with(paths.clone(), listener, rx));
    let endpoint = wait_for_endpoint(&paths).await;
    let client = DaemonClient::new(&endpoint).unwrap();

    let first = client.translate("auto", "zh", "hello").await.unwrap();
    assert_eq!(first.text, "hello");
    assert!(!first.cache_hit);
    let second = client.translate("auto", "zh", "hello").await.unwrap();
    assert!(second.cache_hit);

    let response = dispatch_host(
        HostRequest::Translate {
            id: "ext-1".into(),
            text: "hello".into(),
            source: None,
            target: "zh".into(),
        },
        &client,
    )
    .await;
    match response {
        congmiao_core::HostResponse::TranslateResult { id, cache_hit, .. } => {
            assert_eq!(id, "ext-1");
            assert!(cache_hit);
        }
        other => panic!("unexpected host response: {other:?}"),
    }

    let rejected = DaemonClient::new(&Endpoint {
        port: endpoint.port,
        token: "not-the-token".into(),
    })
    .unwrap();
    assert!(rejected.health().await.is_err());

    shutdown.send(true).unwrap();
    task.await.unwrap().unwrap();
    assert!(!paths.endpoint().exists());
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn file_job_echoes_a_text_file() {
    let _guard = test_lock().await;
    let dir = std::env::temp_dir().join(format!(
        "congmiao-job-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let paths = DaemonPaths::new(&dir);
    let mut config = AppConfig::default();
    for engine in &mut config.engines {
        engine.enabled = engine.kind == EngineKind::Echo;
    }
    config.save_to(&paths.config()).unwrap();
    let input = dir.join("note.txt");
    std::fs::write(&input, "Hello\n\nWorld").unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (shutdown, rx) = watch::channel(false);
    let task = tokio::spawn(serve_with(paths.clone(), listener, rx));
    let endpoint = wait_for_endpoint(&paths).await;
    let client = DaemonClient::new(&endpoint).unwrap();
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
        .unwrap();
    let id = started["id"].as_str().unwrap().to_string();
    let mut status = String::new();
    let mut output = String::new();
    for _ in 0..50 {
        let body = client.job_status(&id).await.unwrap();
        status = body["job"]["status"].as_str().unwrap_or("").to_string();
        output = body["job"]["output_path"]
            .as_str()
            .unwrap_or("")
            .to_string();
        if status == "done" || status == "failed" {
            break;
        }
        sleep(Duration::from_millis(40)).await;
    }
    assert_eq!(status, "done", "{output}");
    let written = std::fs::read_to_string(&output).unwrap();
    assert!(written.contains("Hello"));
    assert!(written.contains("World"));
    shutdown.send(true).unwrap();
    task.await.unwrap().unwrap();
    std::fs::remove_dir_all(dir).ok();
}

async fn test_lock() -> tokio::sync::MutexGuard<'static, ()> {
    static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    LOCK.lock().await
}

async fn wait_for_endpoint(paths: &DaemonPaths) -> Endpoint {
    for _ in 0..50 {
        if let Ok(endpoint) = Endpoint::load(&paths.endpoint()) {
            return endpoint;
        }
        sleep(Duration::from_millis(20)).await;
    }
    panic!("守护进程没有写下端口文件");
}
