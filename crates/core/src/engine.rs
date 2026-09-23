use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;

use crate::cache::{CacheKey, MemoryCache};
use crate::config::EngineKind;
use crate::engines::{build_engines, BoundEngine};
use crate::error::{Error, Result};
use crate::glossary::{self, GlossaryEntry};
use crate::language::{self, language_name};
use crate::provider::{TranslateRequest, Translator};
use crate::store::{HistoryResult, Store};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranslateResponse {
    pub text: String,
    pub source: String,
    pub target: String,
    pub detected_source: Option<String>,
    pub cache_hit: bool,
    pub provider: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineOutput {
    pub engine: String,
    pub label: String,
    pub unofficial: bool,
    pub text: String,
    pub error: Option<String>,
    pub cache_hit: bool,
    #[serde(default)]
    pub partial: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompareResponse {
    pub source: String,
    pub target: String,
    pub detected_source: Option<String>,
    pub results: Vec<EngineOutput>,
}

#[derive(Clone)]
pub struct Engine {
    inner: Arc<EngineInner>,
}

struct EngineInner {
    engines: Vec<BoundEngine>,
    glossary: Vec<GlossaryEntry>,
    cache: Mutex<MemoryCache>,
    store: Option<Arc<Store>>,
    swap_when_same: bool,
    style: String,
}

#[derive(Clone)]
pub struct Prepared {
    request_source: String,
    request_target: String,
    resolved_source: String,
    resolved_target: String,
    detected_source: Option<String>,
    pub text: String,
    pub glossary_hit: Option<String>,
}

impl Engine {
    pub fn new(translator: Arc<dyn Translator>, glossary: Vec<GlossaryEntry>) -> Self {
        Self {
            inner: Arc::new(EngineInner {
                engines: vec![BoundEngine {
                    kind: EngineKind::Echo,
                    translator,
                }],
                glossary,
                cache: Mutex::new(MemoryCache::new(256)),
                store: None,
                swap_when_same: false,
                style: String::new(),
            }),
        }
    }

    pub fn from_config(config: &crate::config::AppConfig, store: Option<Arc<Store>>) -> Self {
        Self {
            inner: Arc::new(EngineInner {
                engines: build_engines(&config.engines, &config.proxy),
                glossary: config.glossary.clone(),
                cache: Mutex::new(MemoryCache::new(64)),
                store,
                swap_when_same: config.swap_when_same,
                style: config.prompt.clone(),
            }),
        }
    }

    pub fn provider_id(&self) -> &str {
        self.inner
            .engines
            .first()
            .map(|engine| engine.translator.id())
            .unwrap_or("none")
    }

    pub fn store(&self) -> Option<Arc<Store>> {
        self.inner.store.clone()
    }

    pub fn len(&self) -> usize {
        self.inner.engines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.engines.is_empty()
    }

    pub async fn translate(
        &self,
        source: &str,
        target: &str,
        text: &str,
    ) -> Result<TranslateResponse> {
        let prepared = self.prepare(source, target, text)?;
        if let Some(translated) = &prepared.glossary_hit {
            return Ok(self.single(&prepared, translated.clone(), false, "glossary"));
        }
        let Some(first) = self.inner.engines.first() else {
            return Err(Error::Provider {
                status: None,
                message: "没有启用的翻译引擎".into(),
            });
        };
        let output = self.run_bound(&prepared, first).await;
        if let Some(message) = output.error {
            return Err(Error::Provider {
                status: None,
                message,
            });
        }
        Ok(self.single(&prepared, output.text, output.cache_hit, &output.engine))
    }

    pub async fn compare(&self, source: &str, target: &str, text: &str) -> Result<CompareResponse> {
        let prepared = self.prepare(source, target, text)?;
        let results = if let Some(translated) = &prepared.glossary_hit {
            vec![glossary_output(translated)]
        } else if self.inner.engines.is_empty() {
            return Err(Error::Provider {
                status: None,
                message: "没有启用的翻译引擎".into(),
            });
        } else {
            let mut tasks = Vec::new();
            for engine in &self.inner.engines {
                let prepared = prepared.clone();
                let engine_kind = engine.kind;
                let translator = Arc::clone(&engine.translator);
                let cache = self.clone();
                tasks.push(async move {
                    cache
                        .run_translator(&prepared, engine_kind, translator)
                        .await
                });
            }
            futures::future::join_all(tasks).await
        };
        self.remember(&prepared, &results);
        Ok(CompareResponse {
            source: prepared.resolved_source,
            target: prepared.resolved_target,
            detected_source: prepared.detected_source,
            results,
        })
    }

    pub async fn run_index(&self, prepared: &Prepared, index: usize) -> Option<EngineOutput> {
        if let Some(translated) = &prepared.glossary_hit {
            return Some(glossary_output(translated));
        }
        let engine = self.inner.engines.get(index)?;
        Some(self.run_bound(prepared, engine).await)
    }

    pub async fn run_index_stream(
        &self,
        prepared: &Prepared,
        index: usize,
        emit: &tokio::sync::mpsc::UnboundedSender<EngineOutput>,
    ) -> Option<EngineOutput> {
        if let Some(translated) = &prepared.glossary_hit {
            let output = glossary_output(translated);
            let _ = emit.send(output.clone());
            return Some(output);
        }
        let engine = self.inner.engines.get(index)?;
        Some(self.run_bound_stream(prepared, engine, emit).await)
    }

    async fn run_bound_stream(
        &self,
        prepared: &Prepared,
        engine: &BoundEngine,
        emit: &tokio::sync::mpsc::UnboundedSender<EngineOutput>,
    ) -> EngineOutput {
        let request = TranslateRequest {
            text: prepared.text.clone(),
            source: prepared.resolved_source.clone(),
            target: prepared.resolved_target.clone(),
            glossary: self.inner.glossary.clone(),
            style: self.inner.style.clone(),
        };
        let key = cache_key(engine.translator.id(), &request);
        if let Some(text) = self.read_cache(&key).await {
            let output = output(engine.kind, text, None, true);
            let _ = emit.send(output.clone());
            return output;
        }
        if engine.translator.streams() {
            let updates = emit.clone();
            let kind = engine.kind;
            let on_update: Arc<dyn crate::provider::DeltaSink> = Arc::new(move |so_far: &str| {
                let _ = updates.send(partial_output(kind, so_far.to_string()));
            });
            let streamed = engine
                .translator
                .translate_with_deltas(&request, on_update)
                .await;
            let output = match streamed {
                Ok(translated) => {
                    self.write_cache(&key, &translated.text).await;
                    output(engine.kind, translated.text, None, false)
                }
                Err(err) => output(engine.kind, String::new(), Some(err.to_string()), false),
            };
            let _ = emit.send(output.clone());
            return output;
        }
        let output = self
            .run_translator(prepared, engine.kind, Arc::clone(&engine.translator))
            .await;
        let _ = emit.send(output.clone());
        output
    }

    pub async fn test_kind(&self, kind: &str) -> Result<EngineOutput> {
        let Some(engine) = self
            .inner
            .engines
            .iter()
            .find(|engine| engine.kind.as_str() == kind)
        else {
            return Err(Error::Provider {
                status: None,
                message: "这个引擎没有启用".into(),
            });
        };
        let prepared = Prepared {
            request_source: "auto".into(),
            request_target: "zh".into(),
            resolved_source: "auto".into(),
            resolved_target: "zh".into(),
            detected_source: None,
            text: "hello".into(),
            glossary_hit: None,
        };
        Ok(self.run_bound(&prepared, engine).await)
    }

    pub fn prepare(&self, source: &str, target: &str, text: &str) -> Result<Prepared> {
        language::validate_text(text)?;
        language::validate_pair(source, target)?;
        let (resolved_source, resolved_target, detected_source) =
            language::resolve_pair(source, target, text, self.inner.swap_when_same);
        if resolved_source == resolved_target {
            return Err(Error::SameLanguage);
        }
        Ok(Prepared {
            request_source: source.to_string(),
            request_target: target.to_string(),
            resolved_source,
            resolved_target,
            detected_source,
            text: text.to_string(),
            glossary_hit: glossary::exact_match(text, &self.inner.glossary).map(str::to_string),
        })
    }

    pub fn remember(&self, prepared: &Prepared, results: &[EngineOutput]) {
        let Some(store) = &self.inner.store else {
            return;
        };
        let stored = results
            .iter()
            .map(|result| HistoryResult {
                engine: result.engine.clone(),
                translated: result.text.clone(),
                error: result.error.clone(),
            })
            .collect::<Vec<_>>();
        store.add_history(
            &prepared.resolved_source,
            &prepared.resolved_target,
            &prepared.text,
            &stored,
        );
    }

    async fn run_bound(&self, prepared: &Prepared, engine: &BoundEngine) -> EngineOutput {
        self.run_translator(prepared, engine.kind, Arc::clone(&engine.translator))
            .await
    }

    async fn run_translator(
        &self,
        prepared: &Prepared,
        kind: EngineKind,
        translator: Arc<dyn Translator>,
    ) -> EngineOutput {
        let request = TranslateRequest {
            text: prepared.text.clone(),
            source: prepared.resolved_source.clone(),
            target: prepared.resolved_target.clone(),
            glossary: self.inner.glossary.clone(),
            style: self.inner.style.clone(),
        };
        let key = cache_key(translator.id(), &request);
        if let Some(text) = self.read_cache(&key).await {
            return output(kind, text, None, true);
        }
        match translator.translate(&request).await {
            Ok(translated) => {
                self.write_cache(&key, &translated.text).await;
                output(kind, translated.text, None, false)
            }
            Err(err) => output(kind, String::new(), Some(err.to_string()), false),
        }
    }

    async fn read_cache(&self, key: &CacheKey) -> Option<String> {
        if let Some(store) = &self.inner.store {
            if let Some(text) = store.cache_get(&cache_id(key)) {
                return Some(text);
            }
        }
        self.inner.cache.lock().await.get(key)
    }

    async fn write_cache(&self, key: &CacheKey, text: &str) {
        if let Some(store) = &self.inner.store {
            store.cache_put(&cache_id(key), text);
        }
        self.inner
            .cache
            .lock()
            .await
            .insert(key.clone(), text.to_string());
    }

    fn single(
        &self,
        prepared: &Prepared,
        text: String,
        cache_hit: bool,
        provider: &str,
    ) -> TranslateResponse {
        let detected_source = prepared.detected_source.clone().or_else(|| {
            if prepared.request_source == "auto" {
                None
            } else {
                language_name(&prepared.request_source).map(|_| prepared.request_source.clone())
            }
        });
        TranslateResponse {
            text,
            source: prepared.request_source.clone(),
            target: prepared.request_target.clone(),
            detected_source,
            cache_hit,
            provider: provider.to_string(),
        }
    }
}

fn glossary_output(text: &str) -> EngineOutput {
    EngineOutput {
        engine: "glossary".into(),
        label: "术语表".into(),
        unofficial: false,
        text: text.to_string(),
        error: None,
        cache_hit: false,
        partial: false,
    }
}

fn output(kind: EngineKind, text: String, error: Option<String>, cache_hit: bool) -> EngineOutput {
    EngineOutput {
        engine: kind.as_str().into(),
        label: kind.label().into(),
        unofficial: kind.unofficial(),
        text,
        error,
        cache_hit,
        partial: false,
    }
}

fn partial_output(kind: EngineKind, text: String) -> EngineOutput {
    let mut output = output(kind, text, None, false);
    output.partial = true;
    output
}

fn cache_key(provider: &str, request: &TranslateRequest) -> CacheKey {
    CacheKey {
        provider: provider.to_string(),
        source: request.source.clone(),
        target: request.target.clone(),
        text: request.text.clone(),
        glossary: mix_style(glossary::fingerprint(&request.glossary), &request.style),
    }
}

fn mix_style(glossary: u64, style: &str) -> u64 {
    if style.is_empty() {
        return glossary;
    }
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    style.hash(&mut hasher);
    glossary ^ hasher.finish()
}

fn cache_id(key: &CacheKey) -> String {
    let raw = format!(
        "{}|{}|{}|{}|{}",
        key.provider, key.source, key.target, key.glossary, key.text
    );
    format!("{:x}", Sha256::digest(raw.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AppConfig, EngineKind};
    use crate::provider::EchoTranslator;

    fn engine(glossary: Vec<GlossaryEntry>) -> Engine {
        Engine::new(Arc::new(EchoTranslator), glossary)
    }

    #[tokio::test]
    async fn second_call_hits_memory_cache() {
        let engine = engine(Vec::new());
        let first = engine.translate("auto", "zh", "hello").await.unwrap();
        let second = engine.translate("auto", "zh", "hello").await.unwrap();
        assert!(!first.cache_hit);
        assert!(second.cache_hit);
        assert_eq!(second.text, "hello");
        assert_eq!(second.provider, "echo");
    }

    #[tokio::test]
    async fn glossary_short_circuits_before_the_provider() {
        let engine = engine(vec![GlossaryEntry {
            source: "hello".into(),
            target: "你好".into(),
        }]);
        let translated = engine.translate("en", "zh", " hello ").await.unwrap();
        assert_eq!(translated.text, "你好");
        assert!(!translated.cache_hit);
        assert_eq!(translated.provider, "glossary");
        assert_eq!(translated.detected_source.as_deref(), Some("en"));
    }

    #[tokio::test]
    async fn rejects_invalid_requests() {
        let engine = engine(Vec::new());
        assert!(matches!(
            engine.translate("auto", "zh", " ").await,
            Err(Error::EmptyText)
        ));
        assert!(matches!(
            engine.translate("en", "en", "hello").await,
            Err(Error::SameLanguage)
        ));
    }

    #[tokio::test]
    async fn one_failing_engine_does_not_hide_the_other() {
        let mut config = AppConfig::default();
        for engine in &mut config.engines {
            engine.enabled = matches!(engine.kind, EngineKind::Echo | EngineKind::Baidu);
        }
        let engine = Engine::from_config(&config, None);
        let compared = engine.compare("en", "zh", "hello").await.unwrap();
        assert_eq!(compared.results.len(), 2);
        let echo = compared
            .results
            .iter()
            .find(|item| item.engine == "echo")
            .unwrap();
        let baidu = compared
            .results
            .iter()
            .find(|item| item.engine == "baidu")
            .unwrap();
        assert_eq!(echo.text, "hello");
        assert!(echo.error.is_none());
        assert!(baidu.error.as_ref().unwrap().contains("API Key"));
    }

    #[tokio::test]
    async fn sqlite_cache_survives_a_new_engine() {
        let dir = std::env::temp_dir().join(format!(
            "congmiao-engine-cache-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = Arc::new(Store::open(&dir.join("store.db")).unwrap());
        let mut config = AppConfig::default();
        for engine in &mut config.engines {
            engine.enabled = engine.kind == EngineKind::Echo;
        }
        let first = Engine::from_config(&config, Some(store.clone()));
        let translated = first.translate("en", "zh", "cache-me").await.unwrap();
        assert!(!translated.cache_hit);
        let second = Engine::from_config(&config, Some(store));
        let again = second.translate("en", "zh", "cache-me").await.unwrap();
        assert!(again.cache_hit);
        let started = std::time::Instant::now();
        let timed = second.translate("en", "zh", "cache-me").await.unwrap();
        assert!(timed.cache_hit);
        assert!(started.elapsed() < std::time::Duration::from_millis(300));
        std::fs::remove_dir_all(dir).ok();
    }
}
