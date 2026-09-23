use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::SystemTime;

use axum::extract::{DefaultBodyLimit, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use congmiao_core::{
    config_stamp, looks_like_word, lookup_dict, tokens_equal, AppConfig, DaemonPaths, Endpoint,
    Engine, Error, Store, DAEMON_PORT,
};
use serde::Deserialize;
use tokio::net::TcpListener;
use tokio::sync::{watch, Mutex};

struct Runtime {
    stamp: Option<SystemTime>,
    engine: Option<Engine>,
}

pub(crate) struct AppState {
    pub(crate) paths: DaemonPaths,
    pub(crate) token: String,
    pub(crate) store: Option<std::sync::Arc<Store>>,
    runtime: Mutex<Runtime>,
    pub(crate) cancels: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

struct ApiError {
    status: StatusCode,
    message: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = Json(serde_json::json!({ "message": self.message }));
        (self.status, body).into_response()
    }
}

impl From<Error> for ApiError {
    fn from(err: Error) -> Self {
        let status = match &err {
            Error::Unauthorized => StatusCode::UNAUTHORIZED,
            Error::EmptyText
            | Error::TextTooLong { .. }
            | Error::SameLanguage
            | Error::InvalidLanguage(_)
            | Error::MissingApiKey
            | Error::MissingModel
            | Error::InvalidBaseUrl(_)
            | Error::Json(_) => StatusCode::BAD_REQUEST,
            Error::DaemonOffline => StatusCode::SERVICE_UNAVAILABLE,
            _ => StatusCode::BAD_GATEWAY,
        };
        Self {
            status,
            message: err.to_string(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct TranslateInput {
    text: String,
    source: String,
    target: String,
}

pub async fn serve(shutdown: watch::Receiver<bool>) -> Result<(), Error> {
    let paths = DaemonPaths::new(congmiao_core::data_dir()?);
    let listener = TcpListener::bind(("127.0.0.1", DAEMON_PORT))
        .await
        .map_err(|_| {
            Error::Io(format!(
                "端口 {DAEMON_PORT} 已被占用。如果从喵翻译已经在运行，直接使用即可"
            ))
        })?;
    serve_with(paths, listener, shutdown).await
}

pub async fn serve_with(
    paths: DaemonPaths,
    listener: TcpListener,
    mut shutdown: watch::Receiver<bool>,
) -> Result<(), Error> {
    let addr = listener
        .local_addr()
        .map_err(|err| Error::Io(err.to_string()))?;
    let token = congmiao_core::new_token()?;
    let endpoint = Endpoint {
        port: addr.port(),
        token: token.clone(),
    };
    tracing::info!("从喵翻译正在监听 {}", addr);
    endpoint.save(&paths.endpoint())?;
    let store = Store::open(&paths.store()).ok().map(std::sync::Arc::new);
    let state = Arc::new(AppState {
        paths: paths.clone(),
        token,
        store,
        runtime: Mutex::new(Runtime {
            stamp: None,
            engine: None,
        }),
        cancels: Mutex::new(HashMap::new()),
    });
    let app = Router::new()
        .route("/v1/health", get(health))
        .route("/v1/translate", post(translate))
        .route("/v1/translate/compare", post(compare))
        .route("/v1/translate/stream", post(stream_translate))
        .route("/v1/engines/test", post(test_engine))
        .route("/v1/dict", post(dict))
        .route("/v1/history/list", post(history_list))
        .route("/v1/history/delete", post(history_delete))
        .route("/v1/history/clear", post(history_clear))
        .route("/v1/vocabulary/list", post(vocabulary_list))
        .route("/v1/vocabulary/add", post(vocabulary_add))
        .route("/v1/vocabulary/delete", post(vocabulary_delete))
        .route("/v1/vocabulary/export", post(vocabulary_export))
        .route("/v1/cache/clear", post(cache_clear))
        .route("/v1/jobs/start", post(job_start))
        .route("/v1/jobs/status", post(job_status))
        .route("/v1/jobs/cancel", post(job_cancel))
        .route("/v1/jobs/continue", post(job_continue))
        .layer(DefaultBodyLimit::max(64 * 1024))
        .with_state(state);
    let result = axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            let _ = shutdown.changed().await;
        })
        .await
        .map_err(|err| Error::Io(err.to_string()));
    Endpoint::remove(&paths.endpoint());
    result
}

fn authorize(headers: &HeaderMap, token: &str) -> Result<(), ApiError> {
    let value = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let Some(presented) = value.strip_prefix("Bearer ") else {
        return Err(ApiError {
            status: StatusCode::UNAUTHORIZED,
            message: Error::Unauthorized.to_string(),
        });
    };
    if !tokens_equal(presented, token) {
        return Err(ApiError {
            status: StatusCode::UNAUTHORIZED,
            message: Error::Unauthorized.to_string(),
        });
    }
    Ok(())
}

async fn health(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    authorize(&headers, &state.token)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn translate(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(input): Json<TranslateInput>,
) -> Result<Json<congmiao_core::TranslateResponse>, ApiError> {
    authorize(&headers, &state.token)?;
    let engine = current_engine(&state).await?;
    let response = engine
        .translate(&input.source, &input.target, &input.text)
        .await?;
    Ok(Json(response))
}

async fn compare(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(input): Json<TranslateInput>,
) -> Result<Json<congmiao_core::CompareResponse>, ApiError> {
    authorize(&headers, &state.token)?;
    let engine = current_engine(&state).await?;
    Ok(Json(
        engine
            .compare(&input.source, &input.target, &input.text)
            .await?,
    ))
}

async fn stream_translate(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(input): Json<TranslateInput>,
) -> Result<Response, ApiError> {
    authorize(&headers, &state.token)?;
    let engine = current_engine(&state).await?;
    let prepared = engine.prepare(&input.source, &input.target, &input.text)?;
    let (tx, rx) = tokio::sync::mpsc::channel::<String>(16);
    let (out_tx, mut out_rx) =
        tokio::sync::mpsc::unbounded_channel::<congmiao_core::EngineOutput>();
    let remember_engine = engine.clone();
    let remember_prepared = prepared.clone();
    let body_tx = tx.clone();
    tokio::spawn(async move {
        let mut finals: Vec<congmiao_core::EngineOutput> = Vec::new();
        while let Some(output) = out_rx.recv().await {
            let line = format!("{}\n", serde_json::to_string(&output).unwrap_or_default());
            if body_tx.send(line).await.is_err() {
                break;
            }
            if !output.partial {
                if let Some(slot) = finals.iter_mut().find(|item| item.engine == output.engine) {
                    *slot = output;
                } else {
                    finals.push(output);
                }
            }
        }
        remember_engine.remember(&remember_prepared, &finals);
    });
    if prepared.glossary_hit.is_some() || engine.is_empty() {
        let _ = engine.run_index_stream(&prepared, 0, &out_tx).await;
    } else {
        for index in 0..engine.len() {
            let engine = engine.clone();
            let prepared = prepared.clone();
            let out_tx = out_tx.clone();
            tokio::spawn(async move {
                let _ = engine.run_index_stream(&prepared, index, &out_tx).await;
            });
        }
    }
    drop(out_tx);
    drop(tx);
    let stream = futures::stream::unfold(rx, |mut rx| async move {
        let line = rx.recv().await?;
        Some((Ok::<String, std::convert::Infallible>(line), rx))
    });
    Ok(axum::body::Body::from_stream(stream).into_response())
}

async fn test_engine(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(input): Json<KindInput>,
) -> Result<Json<congmiao_core::EngineOutput>, ApiError> {
    authorize(&headers, &state.token)?;
    let engine = current_engine(&state).await?;
    Ok(Json(engine.test_kind(&input.kind).await?))
}

async fn dict(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(input): Json<TextInput>,
) -> Result<Json<congmiao_core::DictEntry>, ApiError> {
    authorize(&headers, &state.token)?;
    if !looks_like_word(&input.text) {
        return Err(Error::Provider {
            status: None,
            message: "这段文字不像一个词或短语".into(),
        }
        .into());
    }
    Ok(Json(lookup_dict(&input.text).await?))
}

async fn history_list(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(input): Json<QueryInput>,
) -> Result<Json<Vec<congmiao_core::HistoryItem>>, ApiError> {
    authorize(&headers, &state.token)?;
    let store = require_store(&state)?;
    Ok(Json(store.list_history(&input.query, 200)?))
}

async fn history_delete(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(input): Json<IdInput>,
) -> Result<StatusCode, ApiError> {
    authorize(&headers, &state.token)?;
    require_store(&state)?.delete_history(input.id)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn history_clear(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    authorize(&headers, &state.token)?;
    require_store(&state)?.clear_history()?;
    Ok(StatusCode::NO_CONTENT)
}

async fn vocabulary_list(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<congmiao_core::VocabItem>>, ApiError> {
    authorize(&headers, &state.token)?;
    Ok(Json(require_store(&state)?.list_words()?))
}

async fn vocabulary_add(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(input): Json<VocabInput>,
) -> Result<Json<serde_json::Value>, ApiError> {
    authorize(&headers, &state.token)?;
    let id = require_store(&state)?.add_word(
        input.word.trim(),
        &input.translation,
        &input.phonetic,
        "",
    )?;
    Ok(Json(serde_json::json!({ "id": id })))
}

async fn vocabulary_delete(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(input): Json<IdInput>,
) -> Result<StatusCode, ApiError> {
    authorize(&headers, &state.token)?;
    require_store(&state)?.delete_word(input.id)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn vocabulary_export(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(input): Json<FormatInput>,
) -> Result<Json<serde_json::Value>, ApiError> {
    authorize(&headers, &state.token)?;
    let store = require_store(&state)?;
    let text = if input.format == "anki" {
        store.export_anki()?
    } else {
        store.export_csv()?
    };
    Ok(Json(serde_json::json!({ "text": text })))
}

async fn cache_clear(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    authorize(&headers, &state.token)?;
    if let Some(store) = &state.store {
        store.cache_clear()?;
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn job_start(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(input): Json<crate::jobs::JobStart>,
) -> Result<Json<serde_json::Value>, ApiError> {
    authorize(&headers, &state.token)?;
    Ok(Json(crate::jobs::start(&state, input).await?))
}

async fn job_status(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(input): Json<JobIdInput>,
) -> Result<Json<serde_json::Value>, ApiError> {
    authorize(&headers, &state.token)?;
    Ok(Json(crate::jobs::status(&state, &input.id).await?))
}

async fn job_cancel(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(input): Json<JobIdInput>,
) -> Result<Json<serde_json::Value>, ApiError> {
    authorize(&headers, &state.token)?;
    Ok(Json(crate::jobs::cancel(&state, &input.id).await?))
}

async fn job_continue(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(input): Json<JobIdInput>,
) -> Result<Json<serde_json::Value>, ApiError> {
    authorize(&headers, &state.token)?;
    Ok(Json(crate::jobs::resume(&state, &input.id).await?))
}

fn require_store(state: &AppState) -> Result<&Store, Error> {
    state
        .store
        .as_deref()
        .ok_or_else(|| Error::Io("本地数据库没有打开".into()))
}

async fn current_engine(state: &AppState) -> Result<Engine, Error> {
    let stamp = config_stamp(&state.paths.config());
    let mut runtime = state.runtime.lock().await;
    if runtime.stamp != stamp || runtime.engine.is_none() {
        let config = AppConfig::load_from(&state.paths.config())?;
        runtime.engine = Some(Engine::from_config(&config, state.store.clone()));
        runtime.stamp = stamp;
    }
    runtime.engine.clone().ok_or_else(|| Error::Provider {
        status: None,
        message: "没有启用的翻译引擎".into(),
    })
}

#[derive(Debug, Deserialize)]
struct KindInput {
    kind: String,
}

#[derive(Debug, Deserialize)]
struct TextInput {
    text: String,
}

#[derive(Debug, Deserialize)]
struct QueryInput {
    #[serde(default)]
    query: String,
}

#[derive(Debug, Deserialize)]
struct IdInput {
    id: i64,
}

#[derive(Debug, Deserialize)]
struct JobIdInput {
    id: String,
}

#[derive(Debug, Deserialize)]
struct VocabInput {
    word: String,
    #[serde(default)]
    translation: String,
    #[serde(default)]
    phonetic: String,
}

#[derive(Debug, Deserialize)]
struct FormatInput {
    #[serde(default)]
    format: String,
}
