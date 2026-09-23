use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::dict::DictEntry;
use crate::endpoint::Endpoint;
use crate::engine::{CompareResponse, EngineOutput, TranslateResponse};
use crate::error::{Error, Result};
use crate::store::{HistoryItem, VocabItem};

#[derive(Debug, Serialize)]
struct TranslateBody<'a> {
    text: &'a str,
    source: &'a str,
    target: &'a str,
}

#[derive(Debug, Deserialize)]
struct ErrorBody {
    message: String,
}

pub struct DaemonClient {
    base: String,
    token: String,
    http: reqwest::Client,
}

impl DaemonClient {
    pub fn new(endpoint: &Endpoint) -> Result<Self> {
        if endpoint.port == 0 || endpoint.token.is_empty() {
            return Err(Error::DaemonOffline);
        }
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(50))
            .build()
            .map_err(|err| Error::Io(err.to_string()))?;
        Ok(Self {
            base: format!("http://127.0.0.1:{}", endpoint.port),
            token: endpoint.token.clone(),
            http,
        })
    }

    pub async fn health(&self) -> Result<()> {
        let response = self
            .http
            .get(format!("{}/v1/health", self.base))
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(|_| Error::DaemonOffline)?;
        if response.status().as_u16() == 401 {
            return Err(Error::Unauthorized);
        }
        if !response.status().is_success() {
            return Err(Error::DaemonOffline);
        }
        Ok(())
    }

    pub async fn translate(
        &self,
        source: &str,
        target: &str,
        text: &str,
    ) -> Result<TranslateResponse> {
        let response = self
            .http
            .post(format!("{}/v1/translate", self.base))
            .bearer_auth(&self.token)
            .json(&TranslateBody {
                text,
                source,
                target,
            })
            .send()
            .await
            .map_err(|_| Error::DaemonOffline)?;
        let status = response.status();
        if status.as_u16() == 401 {
            return Err(Error::Unauthorized);
        }
        if !status.is_success() {
            let message = response
                .json::<ErrorBody>()
                .await
                .map(|body| body.message)
                .unwrap_or_else(|_| format!("守护进程返回 {}", status.as_u16()));
            return Err(Error::Provider {
                status: Some(status.as_u16()),
                message,
            });
        }
        response
            .json()
            .await
            .map_err(|err| Error::Json(err.to_string()))
    }

    pub async fn compare(&self, source: &str, target: &str, text: &str) -> Result<CompareResponse> {
        self.post_json(
            "/v1/translate/compare",
            &TranslateBody {
                text,
                source,
                target,
            },
        )
        .await
    }

    pub async fn stream_compare<F>(
        &self,
        source: &str,
        target: &str,
        text: &str,
        mut on_result: F,
    ) -> Result<CompareResponse>
    where
        F: FnMut(EngineOutput),
    {
        let response = self
            .http
            .post(format!("{}/v1/translate/stream", self.base))
            .bearer_auth(&self.token)
            .json(&TranslateBody {
                text,
                source,
                target,
            })
            .send()
            .await
            .map_err(|_| Error::DaemonOffline)?;
        let status = response.status();
        if status.as_u16() == 401 {
            return Err(Error::Unauthorized);
        }
        if !status.is_success() {
            let message = response
                .text()
                .await
                .unwrap_or_else(|_| format!("守护进程返回 {}", status.as_u16()));
            return Err(Error::Provider {
                status: Some(status.as_u16()),
                message,
            });
        }
        let mut stream = response.bytes_stream();
        let mut pending = String::new();
        let mut results = Vec::new();
        use futures::StreamExt;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|err| Error::Provider {
                status: None,
                message: err.to_string(),
            })?;
            pending.push_str(&String::from_utf8_lossy(&chunk));
            while let Some(index) = pending.find('\n') {
                let line: String = pending.drain(..=index).collect();
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                let output: EngineOutput =
                    serde_json::from_str(line).map_err(|err| Error::Json(err.to_string()))?;
                on_result(output.clone());
                fold_stream_output(&mut results, output);
            }
        }
        Ok(CompareResponse {
            source: source.to_string(),
            target: target.to_string(),
            detected_source: None,
            results,
        })
    }

    pub async fn test_engine(&self, kind: &str) -> Result<EngineOutput> {
        self.post_json("/v1/engines/test", &serde_json::json!({ "kind": kind }))
            .await
    }

    pub async fn dict(&self, text: &str) -> Result<DictEntry> {
        self.post_json("/v1/dict", &serde_json::json!({ "text": text }))
            .await
    }

    pub async fn history(&self, query: &str) -> Result<Vec<HistoryItem>> {
        self.post_json("/v1/history/list", &serde_json::json!({ "query": query }))
            .await
    }

    pub async fn delete_history(&self, id: i64) -> Result<()> {
        self.post_empty("/v1/history/delete", &serde_json::json!({ "id": id }))
            .await
    }

    pub async fn clear_history(&self) -> Result<()> {
        self.post_empty("/v1/history/clear", &serde_json::json!({}))
            .await
    }

    pub async fn vocabulary(&self) -> Result<Vec<VocabItem>> {
        self.post_json("/v1/vocabulary/list", &serde_json::json!({}))
            .await
    }

    pub async fn add_vocabulary(
        &self,
        word: &str,
        translation: &str,
        phonetic: &str,
    ) -> Result<i64> {
        #[derive(serde::Deserialize)]
        struct Created {
            id: i64,
        }
        let created: Created = self
            .post_json(
                "/v1/vocabulary/add",
                &serde_json::json!({
                    "word": word,
                    "translation": translation,
                    "phonetic": phonetic
                }),
            )
            .await?;
        Ok(created.id)
    }

    pub async fn delete_vocabulary(&self, id: i64) -> Result<()> {
        self.post_empty("/v1/vocabulary/delete", &serde_json::json!({ "id": id }))
            .await
    }

    pub async fn export_vocabulary(&self, format: &str) -> Result<String> {
        #[derive(serde::Deserialize)]
        struct Exported {
            text: String,
        }
        let exported: Exported = self
            .post_json(
                "/v1/vocabulary/export",
                &serde_json::json!({ "format": format }),
            )
            .await?;
        Ok(exported.text)
    }

    pub async fn clear_cache(&self) -> Result<()> {
        self.post_empty("/v1/cache/clear", &serde_json::json!({}))
            .await
    }

    pub async fn job_start(&self, body: &impl serde::Serialize) -> Result<serde_json::Value> {
        self.post_json("/v1/jobs/start", body).await
    }

    pub async fn job_status(&self, id: &str) -> Result<serde_json::Value> {
        self.post_json("/v1/jobs/status", &serde_json::json!({ "id": id }))
            .await
    }

    pub async fn job_cancel(&self, id: &str) -> Result<serde_json::Value> {
        self.post_json("/v1/jobs/cancel", &serde_json::json!({ "id": id }))
            .await
    }

    pub async fn job_continue(&self, id: &str) -> Result<serde_json::Value> {
        self.post_json("/v1/jobs/continue", &serde_json::json!({ "id": id }))
            .await
    }

    async fn post_json<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &impl serde::Serialize,
    ) -> Result<T> {
        let response = self.authorized_post(path, body).await?;
        response
            .json()
            .await
            .map_err(|err| Error::Json(err.to_string()))
    }

    async fn post_empty(&self, path: &str, body: &impl serde::Serialize) -> Result<()> {
        let _ = self.authorized_post(path, body).await?;
        Ok(())
    }

    async fn authorized_post(
        &self,
        path: &str,
        body: &impl serde::Serialize,
    ) -> Result<reqwest::Response> {
        let response = self
            .http
            .post(format!("{}{path}", self.base))
            .bearer_auth(&self.token)
            .json(body)
            .send()
            .await
            .map_err(|_| Error::DaemonOffline)?;
        let status = response.status();
        if status.as_u16() == 401 {
            return Err(Error::Unauthorized);
        }
        if !status.is_success() {
            let message = response
                .json::<ErrorBody>()
                .await
                .map(|body| body.message)
                .unwrap_or_else(|_| format!("守护进程返回 {}", status.as_u16()));
            return Err(Error::Provider {
                status: Some(status.as_u16()),
                message,
            });
        }
        Ok(response)
    }
}

fn fold_stream_output(results: &mut Vec<EngineOutput>, output: EngineOutput) {
    if let Some(existing) = results.iter_mut().find(|item| item.engine == output.engine) {
        *existing = output;
    } else {
        results.push(output);
    }
}

#[cfg(test)]
mod tests {
    use super::fold_stream_output;
    use crate::engine::EngineOutput;

    fn sample(engine: &str, text: &str, partial: bool) -> EngineOutput {
        EngineOutput {
            engine: engine.into(),
            label: engine.into(),
            unofficial: false,
            text: text.into(),
            error: None,
            cache_hit: false,
            partial,
        }
    }

    #[test]
    fn partial_updates_collapse_to_one_line_per_engine() {
        let mut results = Vec::new();
        fold_stream_output(&mut results, sample("openai", "你", true));
        fold_stream_output(&mut results, sample("openai", "你好", true));
        fold_stream_output(&mut results, sample("google", "你好", false));
        fold_stream_output(&mut results, sample("openai", "你好", false));
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].text, "你好");
        assert!(!results[0].partial);
        assert_eq!(results[1].engine, "google");
    }
}
