use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use crate::config::EngineConfig;
use crate::error::{Error, Result};
use crate::glossary::GlossaryEntry;

#[derive(Debug, Clone)]
pub struct TranslateRequest {
    pub text: String,
    pub source: String,
    pub target: String,
    pub glossary: Vec<GlossaryEntry>,
    pub style: String,
}

#[derive(Debug, Clone)]
pub struct RawTranslation {
    pub text: String,
}

pub trait DeltaSink: Send + Sync {
    fn push(&self, text: &str);
}

impl<F> DeltaSink for F
where
    F: Fn(&str) + Send + Sync,
{
    fn push(&self, text: &str) {
        self(text);
    }
}

#[async_trait]
pub trait Translator: Send + Sync {
    fn id(&self) -> &str;
    fn streams(&self) -> bool {
        false
    }
    async fn translate(&self, request: &TranslateRequest) -> Result<RawTranslation>;
    async fn translate_with_deltas(
        &self,
        request: &TranslateRequest,
        on_update: Arc<dyn DeltaSink>,
    ) -> Result<RawTranslation> {
        let _ = on_update;
        self.translate(request).await
    }
}

pub fn openai_translator(config: &EngineConfig, proxy: &str) -> Result<Arc<dyn Translator>> {
    Ok(Arc::new(HttpTranslator::new(config, proxy)?))
}

pub fn http_client(timeout: Duration, proxy: &str) -> Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder().timeout(timeout);
    if !proxy.trim().is_empty() {
        let proxy = reqwest::Proxy::all(proxy.trim()).map_err(|err| Error::Provider {
            status: None,
            message: format!("代理地址无效：{err}"),
        })?;
        builder = builder.proxy(proxy);
    }
    builder.build().map_err(|err| Error::Provider {
        status: None,
        message: err.to_string(),
    })
}

pub struct EchoTranslator;

#[async_trait]
impl Translator for EchoTranslator {
    fn id(&self) -> &str {
        "echo"
    }

    async fn translate(&self, request: &TranslateRequest) -> Result<RawTranslation> {
        Ok(RawTranslation {
            text: request.text.clone(),
        })
    }
}

pub struct HttpTranslator {
    id: String,
    url: String,
    api_key: String,
    model: String,
    client: reqwest::Client,
}

impl HttpTranslator {
    pub fn new(config: &EngineConfig, proxy: &str) -> Result<Self> {
        if config.api_key.trim().is_empty() {
            return Err(Error::MissingApiKey);
        }
        if config.model.trim().is_empty() {
            return Err(Error::MissingModel);
        }
        let url = chat_url(&config.base_url)?;
        let client = http_client(Duration::from_secs(45), proxy)?;
        Ok(Self {
            id: format!("openai:{}:{}", config.base_url.trim(), config.model.trim()),
            url,
            api_key: config.api_key.clone(),
            model: config.model.trim().to_string(),
            client,
        })
    }
}

pub fn chat_url(base_url: &str) -> Result<String> {
    let base = base_url.trim().trim_end_matches('/');
    if !(base.starts_with("https://") || base.starts_with("http://")) || base.contains(' ') {
        return Err(Error::InvalidBaseUrl(base_url.to_string()));
    }
    Ok(format!("{base}/chat/completions"))
}

pub fn system_prompt(request: &TranslateRequest) -> Result<String> {
    let target = crate::language::language_name(&request.target)
        .ok_or_else(|| Error::InvalidLanguage(request.target.clone()))?;
    let mut prompt = format!(
        "你是翻译引擎。把用户给出的文本翻译成{target}。只输出译文，不要解释，不要加引号。保留原文的换行、数字和专有格式。"
    );
    if request.source != "auto" {
        if let Some(source) = crate::language::language_name(&request.source) {
            prompt.push_str(&format!("源语言是{source}。"));
        }
    }
    if !request.glossary.is_empty() {
        prompt.push_str("必须遵守术语表：");
        for entry in &request.glossary {
            prompt.push_str(&format!("{} => {}；", entry.source, entry.target));
        }
    }
    if !request.style.trim().is_empty() {
        prompt.push_str("额外要求：");
        prompt.push_str(request.style.trim());
    }
    Ok(prompt)
}

pub fn parse_chat_completion(body: &str) -> Result<String> {
    let parsed: ChatResponse = serde_json::from_str(body).map_err(|err| Error::Provider {
        status: None,
        message: format!("无法解析响应：{err}"),
    })?;
    if let Some(error) = parsed.error.and_then(|item| item.message) {
        if parsed
            .choices
            .as_ref()
            .and_then(|choices| choices.first())
            .is_none()
        {
            return Err(Error::Provider {
                status: None,
                message: error,
            });
        }
    }
    let text = parsed
        .choices
        .and_then(|choices| choices.into_iter().next())
        .and_then(|choice| choice.message)
        .and_then(|message| message.content)
        .map(|content| content_to_text(&content))
        .unwrap_or_default();
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err(Error::Provider {
            status: None,
            message: "翻译接口没有返回文本".into(),
        });
    }
    Ok(text)
}

fn content_to_text(content: &Content) -> String {
    match content {
        Content::Text(text) => text.clone(),
        Content::Parts(parts) => parts
            .iter()
            .filter_map(|part| part.text.clone())
            .collect::<Vec<_>>()
            .join(""),
    }
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Option<Vec<Choice>>,
    error: Option<ApiErrorBody>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: Option<ChoiceMessage>,
}

#[derive(Debug, Deserialize)]
struct ChoiceMessage {
    content: Option<Content>,
}

#[derive(Debug, Deserialize)]
struct ApiErrorBody {
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Content {
    Text(String),
    Parts(Vec<ContentPart>),
}

#[derive(Debug, Deserialize)]
struct ContentPart {
    text: Option<String>,
}

#[async_trait]
impl Translator for HttpTranslator {
    fn id(&self) -> &str {
        &self.id
    }

    fn streams(&self) -> bool {
        true
    }

    async fn translate_with_deltas(
        &self,
        request: &TranslateRequest,
        on_update: Arc<dyn DeltaSink>,
    ) -> Result<RawTranslation> {
        let prompt = system_prompt(request)?;
        let response = self
            .client
            .post(&self.url)
            .bearer_auth(&self.api_key)
            .header("user-agent", "congmiao-translate/0.1")
            .json(&json!({
                "model": self.model,
                "temperature": 0,
                "stream": true,
                "messages": [
                    { "role": "system", "content": prompt },
                    { "role": "user", "content": request.text }
                ]
            }))
            .send()
            .await
            .map_err(|err| Error::Provider {
                status: err.status().map(|status| status.as_u16()),
                message: err.to_string(),
            })?;
        let text = consume_sse(response, parse_openai_delta, on_update).await?;
        Ok(RawTranslation { text })
    }

    async fn translate(&self, request: &TranslateRequest) -> Result<RawTranslation> {
        let prompt = system_prompt(request)?;
        let response = self
            .client
            .post(&self.url)
            .bearer_auth(&self.api_key)
            .header("user-agent", "congmiao-translate/0.1")
            .json(&json!({
                "model": self.model,
                "temperature": 0,
                "messages": [
                    { "role": "system", "content": prompt },
                    { "role": "user", "content": request.text }
                ]
            }))
            .send()
            .await
            .map_err(|err| Error::Provider {
                status: err.status().map(|status| status.as_u16()),
                message: err.to_string(),
            })?;
        let status = response.status();
        let body = response.text().await.map_err(|err| Error::Provider {
            status: Some(status.as_u16()),
            message: err.to_string(),
        })?;
        if !status.is_success() {
            return Err(Error::Provider {
                status: Some(status.as_u16()),
                message: error_message(&body),
            });
        }
        Ok(RawTranslation {
            text: parse_chat_completion(&body)?,
        })
    }
}

fn error_message(body: &str) -> String {
    if let Ok(parsed) = serde_json::from_str::<ChatResponse>(body) {
        if let Some(message) = parsed.error.and_then(|item| item.message) {
            return message;
        }
    }
    truncate(body)
}

pub fn parse_openai_delta(line: &str) -> Option<String> {
    let data = line.trim().strip_prefix("data:")?.trim();
    if data == "[DONE]" {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(data).ok()?;
    value
        .pointer("/choices/0/delta/content")
        .and_then(|item| item.as_str())
        .map(str::to_string)
}

pub async fn consume_sse<F>(
    response: reqwest::Response,
    mut pick: F,
    on_update: Arc<dyn DeltaSink>,
) -> Result<String>
where
    F: FnMut(&str) -> Option<String>,
{
    use futures::StreamExt;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(Error::Provider {
            status: Some(status.as_u16()),
            message: error_message(&body),
        });
    }
    let mut stream = response.bytes_stream();
    let mut pending = String::new();
    let mut full = String::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|err| Error::Provider {
            status: None,
            message: err.to_string(),
        })?;
        pending.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(index) = pending.find('\n') {
            let line: String = pending.drain(..=index).collect();
            if let Some(piece) = pick(line.trim()) {
                full.push_str(&piece);
                on_update.push(&full);
            }
        }
    }
    if let Some(piece) = pick(pending.trim()) {
        full.push_str(&piece);
        on_update.push(&full);
    }
    let text = full.trim().to_string();
    if text.is_empty() {
        return Err(Error::Provider {
            status: None,
            message: "翻译接口没有返回文本".into(),
        });
    }
    Ok(text)
}

fn truncate(body: &str) -> String {
    let max = 300.min(body.len());
    let mut cut = max;
    while cut > 0 && !body.is_char_boundary(cut) {
        cut -= 1;
    }
    body[..cut].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_string_and_part_content() {
        let raw = r#"{"choices":[{"message":{"content":"  你好  "}}]}"#;
        assert_eq!(parse_chat_completion(raw).unwrap(), "你好");
        let parts = r#"{"choices":[{"message":{"content":[{"type":"text","text":"你"},{"type":"text","text":"好"}]}}]}"#;
        assert_eq!(parse_chat_completion(parts).unwrap(), "你好");
    }

    #[test]
    fn surfaces_api_error_objects() {
        let raw = r#"{"error":{"message":"invalid api key"}}"#;
        let err = parse_chat_completion(raw).unwrap_err();
        assert!(err.to_string().contains("invalid api key"));
    }

    #[test]
    fn rejects_missing_key_and_bad_url() {
        let mut config = EngineConfig::new(crate::config::EngineKind::Openai);
        config.base_url = "ftp://example.com".into();
        config.api_key.clear();
        assert!(matches!(
            HttpTranslator::new(&config, ""),
            Err(Error::MissingApiKey)
        ));
        config.api_key = "key".into();
        assert!(matches!(
            HttpTranslator::new(&config, ""),
            Err(Error::InvalidBaseUrl(_))
        ));
    }
}
