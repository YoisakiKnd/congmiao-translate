use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use congmiao_core::{parse_glossary, AppConfig, Engine, Error, JobRecord, JobSegment, Store};
use congmiao_docs::{
    apply_instance, builtin_terms, extract_path, numbered, pack_format_for, packs, parse_numbered,
    protect, rebuild_path, restore, scan_instance, McSegment, OutputMode, MAX_PACK_CHARS,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::server::AppState;

#[derive(Debug, Deserialize)]
pub struct JobStart {
    pub kind: String,
    pub input_path: String,
    #[serde(default)]
    pub engine: String,
    #[serde(default = "default_source")]
    pub source: String,
    #[serde(default = "default_target")]
    pub target: String,
    #[serde(default = "default_mode")]
    pub mode: String,
    #[serde(default)]
    pub mc_version: String,
    #[serde(default)]
    pub glossary: String,
}

fn default_source() -> String {
    "auto".into()
}

fn default_target() -> String {
    "zh".into()
}

fn default_mode() -> String {
    "translated".into()
}

pub async fn start(state: &Arc<AppState>, mut input: JobStart) -> Result<Value, Error> {
    if input.kind == "minecraft" {
        input.glossary =
            remember_glossary(&state.paths.data_dir, &input.input_path, &input.glossary);
    }
    if input.kind == "minecraft" && input.mode == "preview" {
        let scan = scan_instance(std::path::Path::new(&input.input_path)).map_err(Error::Io)?;
        let sources: Vec<Value> = scan
            .sources
            .iter()
            .map(|item| {
                json!({
                    "kind": item.kind,
                    "path": item.path,
                    "entries": item.entries,
                    "translated": item.translated,
                })
            })
            .collect();
        return Ok(json!({
            "preview": true,
            "sources": sources,
            "terms": scan.terms,
            "entries": scan.segments.len(),
            "already": scan.segments.iter().filter(|item| item.skip).count(),
            "glossary": input.glossary,
        }));
    }
    let store = require(state)?;
    let config = AppConfig::load_from(&state.paths.config())?;
    let engine = if input.engine.trim().is_empty() {
        config
            .enabled_engines()
            .first()
            .map(|engine| engine.kind.as_str().to_string())
            .unwrap_or_else(|| "echo".into())
    } else {
        input.engine.clone()
    };
    if congmiao_core::EngineKind::parse(&engine).is_none() {
        return Err(Error::Provider {
            status: None,
            message: "未知的翻译引擎".into(),
        });
    }
    let (kind, context, segments) = prepare(&input, &engine)?;
    let id = format!(
        "{:x}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|item| item.as_nanos())
            .unwrap_or(0)
    );
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|item| item.as_secs() as i64)
        .unwrap_or(0);
    let skipped = segments
        .iter()
        .filter(|item| item.status == "skipped")
        .count() as i64;
    let job = JobRecord {
        id: id.clone(),
        kind,
        status: "queued".into(),
        engine,
        source: input.source,
        target: input.target,
        mode: input.mode,
        input_path: input.input_path,
        output_path: String::new(),
        total: segments.len() as i64,
        done_count: 0,
        failed_count: 0,
        skipped_count: skipped,
        error: String::new(),
        context,
        created_at: now,
        updated_at: now,
    };
    store.job_insert(&job, &segments)?;
    spawn(state.clone(), id.clone());
    Ok(json!({ "id": id }))
}

pub async fn status(state: &AppState, id: &str) -> Result<Value, Error> {
    let store = require(state)?;
    let job = store
        .job_get(id)?
        .ok_or_else(|| Error::Io("找不到这个任务".into()))?;
    let failures: Vec<Value> = store
        .job_segments(id)?
        .into_iter()
        .filter(|item| item.status == "failed")
        .take(20)
        .map(|item| json!({ "idx": item.idx, "original": item.original, "error": item.error }))
        .collect();
    Ok(json!({ "job": job, "failures": failures }))
}

pub async fn cancel(state: &Arc<AppState>, id: &str) -> Result<Value, Error> {
    if let Some(flag) = state.cancels.lock().await.get(id) {
        flag.store(true, Ordering::SeqCst);
    }
    require(state)?.job_set_status(id, "cancelled", "已取消", "")?;
    Ok(json!({ "id": id, "status": "cancelled" }))
}

pub async fn resume(state: &Arc<AppState>, id: &str) -> Result<Value, Error> {
    let store = require(state)?;
    let job = store
        .job_get(id)?
        .ok_or_else(|| Error::Io("找不到这个任务".into()))?;
    if state.cancels.lock().await.contains_key(id) && job.status == "running" {
        return Ok(json!({ "id": id, "status": "running" }));
    }
    store.job_reset_failed(id)?;
    store.job_set_status(id, "queued", "", "")?;
    spawn(state.clone(), id.to_string());
    Ok(json!({ "id": id, "status": "queued" }))
}

fn spawn(state: Arc<AppState>, id: String) {
    tokio::spawn(async move {
        let cancel = Arc::new(AtomicBool::new(false));
        state
            .cancels
            .lock()
            .await
            .insert(id.clone(), Arc::clone(&cancel));
        run(state.clone(), id.clone(), cancel).await;
        state.cancels.lock().await.remove(&id);
    });
}

async fn run(state: Arc<AppState>, id: String, cancel: Arc<AtomicBool>) {
    let Some(store) = state.store.clone() else {
        return;
    };
    let _ = store.job_set_status(&id, "running", "", "");
    let Ok(Some(job)) = store.job_get(&id) else {
        return;
    };
    let config = AppConfig::load_from(&state.paths.config()).unwrap_or_default();
    let engine = scoped_engine(&config, &job, state.store.clone());
    let glossary = glossary_pairs(&config, &job);
    let pending = store
        .job_segments(&id)
        .unwrap_or_default()
        .into_iter()
        .filter(|item| item.status == "pending")
        .collect::<Vec<_>>();
    if matches!(job.engine.as_str(), "openai" | "gemini") {
        translate_packed(&engine, &store, &job, &pending, &glossary, &cancel).await;
    } else {
        let limit = config.job_concurrency.max(1) as usize;
        translate_concurrent(&engine, &store, &job, &pending, &glossary, &cancel, limit).await;
    }
    if cancel.load(Ordering::SeqCst) {
        let _ = store.job_set_status(&id, "cancelled", "已取消", "");
        return;
    }
    finish(store, &job).await;
}

fn scoped_engine(config: &AppConfig, job: &JobRecord, store: Option<Arc<Store>>) -> Engine {
    let mut scoped = config.clone();
    for engine in &mut scoped.engines {
        engine.enabled = engine.kind.as_str() == job.engine;
    }
    Engine::from_config(&scoped, store)
}

fn glossary_pairs(config: &AppConfig, job: &JobRecord) -> Vec<(String, String)> {
    let mut pairs: Vec<(String, String)> = config
        .glossary
        .iter()
        .map(|entry| (entry.source.clone(), entry.target.clone()))
        .collect();
    if job.kind == "minecraft" {
        pairs.extend(builtin_terms());
    }
    if let Ok(extra) = parse_glossary(
        &serde_json::from_str::<Value>(&job.context)
            .ok()
            .and_then(|value| {
                value
                    .get("glossary")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .unwrap_or_default(),
    ) {
        pairs.extend(extra.into_iter().map(|entry| (entry.source, entry.target)));
    }
    pairs
}

async fn translate_concurrent(
    engine: &Engine,
    store: &Store,
    job: &JobRecord,
    pending: &[JobSegment],
    glossary: &[(String, String)],
    cancel: &AtomicBool,
    limit: usize,
) {
    let mut index = 0;
    while index < pending.len() {
        if cancel.load(Ordering::SeqCst) {
            return;
        }
        let end = (index + limit).min(pending.len());
        let mut tasks = Vec::new();
        for segment in &pending[index..end] {
            let engine = engine.clone();
            let segment = segment.clone();
            let source = job.source.clone();
            let target = job.target.clone();
            let glossary = glossary.to_vec();
            tasks.push(async move {
                let result =
                    translate_segment(&engine, &source, &target, &segment.original, &glossary)
                        .await;
                (segment.idx, result)
            });
        }
        for (idx, result) in futures::future::join_all(tasks).await {
            record(store, &job.id, idx, result);
        }
        index = end;
    }
}

async fn translate_packed(
    engine: &Engine,
    store: &Store,
    job: &JobRecord,
    pending: &[JobSegment],
    glossary: &[(String, String)],
    cancel: &AtomicBool,
) {
    let masked: Vec<_> = pending
        .iter()
        .map(|segment| protect(&segment.original, &borrow_pairs(glossary)))
        .collect();
    let texts: Vec<String> = masked.iter().map(|item| item.text.clone()).collect();
    for group in packs(&texts, MAX_PACK_CHARS) {
        if cancel.load(Ordering::SeqCst) {
            return;
        }
        let items: Vec<&str> = group.iter().map(|index| texts[*index].as_str()).collect();
        let blob = numbered(&items);
        let parsed = if blob.chars().count() > MAX_PACK_CHARS {
            None
        } else {
            match engine.translate(&job.source, &job.target, &blob).await {
                Ok(response) => parse_numbered(&response.text, items.len()),
                Err(_) => None,
            }
        };
        if let Some(lines) = parsed {
            for (offset, line) in lines.iter().enumerate() {
                let index = group[offset];
                let result = restore(line, &masked[index].slots).map_err(|err| err.to_string());
                record(store, &job.id, pending[index].idx, result);
            }
        } else {
            for index in group {
                let result = translate_segment(
                    engine,
                    &job.source,
                    &job.target,
                    &pending[index].original,
                    glossary,
                )
                .await;
                record(store, &job.id, pending[index].idx, result);
            }
        }
    }
}

async fn translate_segment(
    engine: &Engine,
    source: &str,
    target: &str,
    original: &str,
    glossary: &[(String, String)],
) -> Result<String, String> {
    let masked = protect(original, &borrow_pairs(glossary));
    let mut joined = String::new();
    for part in congmiao_docs::split_long(&masked.text, MAX_PACK_CHARS) {
        let response = engine
            .translate(source, target, &part)
            .await
            .map_err(|err| err.to_string())?;
        joined.push_str(&response.text);
    }
    restore(&joined, &masked.slots)
}

fn finish(store: Arc<Store>, job: &JobRecord) -> impl std::future::Future<Output = ()> + Send {
    let job = job.clone();
    async move {
        let segments = store.job_segments(&job.id).unwrap_or_default();
        if segments.iter().any(|item| item.status == "pending") {
            return;
        }
        if segments.iter().any(|item| item.status == "failed") {
            let _ = store.job_set_status(&job.id, "failed", "有片段没有翻译成功", "");
            return;
        }
        let translated: Vec<String> = segments
            .iter()
            .map(|item| {
                if item.translated.is_empty() {
                    item.original.clone()
                } else {
                    item.translated.clone()
                }
            })
            .collect();
        let outcome = if job.kind == "minecraft" {
            let mc: Vec<McSegment> = segments
                .iter()
                .map(|item| McSegment {
                    text: item.original.clone(),
                    meta: item.meta.clone(),
                    skip: item.status == "skipped",
                })
                .collect();
            let format = serde_json::from_str::<Value>(&job.context)
                .ok()
                .and_then(|value| value.get("pack_format").and_then(Value::as_u64))
                .unwrap_or(34) as u32;
            apply_instance(
                std::path::Path::new(&job.input_path),
                &mc,
                &translated,
                format,
            )
            .map(|path| path.display().to_string())
        } else {
            let format = serde_json::from_str::<Value>(&job.context)
                .ok()
                .and_then(|value| {
                    value
                        .get("format")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .unwrap_or_else(|| "txt".into());
            rebuild_path(
                std::path::Path::new(&job.input_path),
                &format,
                &translated,
                OutputMode::parse(&job.mode),
                &job.target,
            )
            .map(|path| path.display().to_string())
        };
        match outcome {
            Ok(path) => {
                let _ = store.job_set_status(&job.id, "done", "", &path);
            }
            Err(err) => {
                let _ = store.job_set_status(&job.id, "failed", &err, "");
            }
        }
    }
}

fn record(store: &Store, id: &str, idx: i64, result: Result<String, String>) {
    match result {
        Ok(text) => {
            let _ = store.job_finish_segment(id, idx, &text, "done", "");
        }
        Err(err) => {
            let _ = store.job_finish_segment(id, idx, "", "failed", &err);
        }
    }
}

fn borrow_pairs(glossary: &[(String, String)]) -> Vec<(&str, &str)> {
    glossary
        .iter()
        .map(|(source, target)| (source.as_str(), target.as_str()))
        .collect()
}

fn prepare(input: &JobStart, _engine: &str) -> Result<(String, String, Vec<JobSegment>), Error> {
    let path = PathBuf::from(&input.input_path);
    if !path.exists() {
        return Err(Error::Io("找不到要翻译的文件或目录".into()));
    }
    if input.kind == "minecraft" {
        let scan = scan_instance(&path).map_err(Error::Io)?;
        if scan.segments.is_empty() {
            return Err(Error::Io(
                "这个目录里没有可汉化的模组语言、任务或手册".into(),
            ));
        }
        let segments = scan
            .segments
            .into_iter()
            .enumerate()
            .map(|(idx, item)| {
                let skipped = item.skip;
                JobSegment {
                    idx: idx as i64,
                    original: item.text.clone(),
                    translated: if skipped { item.text } else { String::new() },
                    status: if skipped {
                        "skipped".into()
                    } else {
                        "pending".into()
                    },
                    error: String::new(),
                    meta: item.meta,
                }
            })
            .collect();
        let context = json!({
            "pack_format": pack_format_for(if input.mc_version.is_empty() { "1.21" } else { &input.mc_version }),
            "glossary": input.glossary,
            "terms": scan.terms,
        })
        .to_string();
        return Ok(("minecraft".into(), context, segments));
    }
    let (format, pieces) = extract_path(&path).map_err(Error::Io)?;
    if pieces.is_empty() {
        return Err(Error::Io("没有提取到可翻译的文字".into()));
    }
    let segments = pieces
        .into_iter()
        .enumerate()
        .map(|(idx, item)| JobSegment {
            idx: idx as i64,
            original: item.text,
            translated: String::new(),
            status: "pending".into(),
            error: String::new(),
            meta: String::new(),
        })
        .collect();
    let context = json!({ "format": format, "glossary": input.glossary }).to_string();
    Ok(("file".into(), context, segments))
}

fn require(state: &AppState) -> Result<Arc<Store>, Error> {
    state
        .store
        .clone()
        .ok_or_else(|| Error::Io("本地数据库没有打开".into()))
}

fn remember_glossary(data_dir: &std::path::Path, input: &str, glossary: &str) -> String {
    let path = data_dir
        .join("minecraft")
        .join(instance_name(input))
        .join("glossary");
    if glossary.trim().is_empty() {
        return std::fs::read_to_string(path).unwrap_or_default();
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, glossary);
    glossary.to_string()
}

fn instance_name(input: &str) -> String {
    let raw = std::path::Path::new(input)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("instance");
    let name: String = raw
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect();
    if name.is_empty() {
        "instance".into()
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use super::remember_glossary;

    #[test]
    fn instance_glossary_is_reloaded_when_the_request_is_empty() {
        let dir = std::env::temp_dir().join(format!("congmiao-glossary-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let saved = remember_glossary(&dir, "/packs/My Pack", "Nether=下界\n");
        assert_eq!(saved, "Nether=下界\n");
        let loaded = remember_glossary(&dir, "/packs/My Pack", "  ");
        assert_eq!(loaded, "Nether=下界\n");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
