use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use hmac::{Hmac, Mac};
use md5::Md5;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::config::{EngineConfig, EngineKind};
use crate::error::{Error, Result};
use crate::provider::{openai_translator, EchoTranslator, TranslateRequest, Translator};

type HmacSha256 = Hmac<Sha256>;

pub struct FailingTranslator {
    id: String,
    message: String,
}

#[async_trait]
impl Translator for FailingTranslator {
    fn id(&self) -> &str {
        &self.id
    }

    async fn translate(
        &self,
        _request: &TranslateRequest,
    ) -> Result<crate::provider::RawTranslation> {
        Err(Error::Provider {
            status: None,
            message: self.message.clone(),
        })
    }
}

pub struct BoundEngine {
    pub kind: EngineKind,
    pub translator: Arc<dyn Translator>,
}

pub fn build_engines(configs: &[EngineConfig], proxy: &str) -> Vec<BoundEngine> {
    configs
        .iter()
        .filter(|config| config.enabled)
        .map(|config| BoundEngine {
            kind: config.kind,
            translator: build_one(config, proxy),
        })
        .collect()
}

fn build_one(config: &EngineConfig, proxy: &str) -> Arc<dyn Translator> {
    match try_build(config, proxy) {
        Ok(translator) => translator,
        Err(err) => Arc::new(FailingTranslator {
            id: config.kind.as_str().into(),
            message: err.to_string(),
        }),
    }
}

fn try_build(config: &EngineConfig, proxy: &str) -> Result<Arc<dyn Translator>> {
    match config.kind {
        EngineKind::Echo => Ok(Arc::new(EchoTranslator)),
        EngineKind::Openai => openai_translator(config, proxy),
        EngineKind::Google
        | EngineKind::Bing
        | EngineKind::DeeplFree
        | EngineKind::YoudaoWeb
        | EngineKind::Deepl
        | EngineKind::Baidu
        | EngineKind::Tencent
        | EngineKind::Alibaba
        | EngineKind::Youdao
        | EngineKind::Azure
        | EngineKind::Gemini => Ok(Arc::new(WebTranslator::new(config, proxy)?)),
    }
}

struct WebTranslator {
    kind: EngineKind,
    config: EngineConfig,
    client: reqwest::Client,
}

impl WebTranslator {
    fn new(config: &EngineConfig, proxy: &str) -> Result<Self> {
        if config.kind.needs_key()
            && config.api_key.trim().is_empty()
            && config.secret.trim().is_empty()
        {
            return Err(Error::MissingApiKey);
        }
        let client = crate::provider::http_client(Duration::from_secs(20), proxy)?;
        Ok(Self {
            kind: config.kind,
            config: config.clone(),
            client,
        })
    }
}

#[async_trait]
impl Translator for WebTranslator {
    fn id(&self) -> &str {
        self.kind.as_str()
    }

    fn streams(&self) -> bool {
        self.kind == EngineKind::Gemini
    }

    async fn translate_with_deltas(
        &self,
        request: &TranslateRequest,
        on_update: std::sync::Arc<dyn crate::provider::DeltaSink>,
    ) -> Result<crate::provider::RawTranslation> {
        if self.kind != EngineKind::Gemini {
            return self.translate(request).await;
        }
        let response = self.gemini_request(request, true).await?;
        let text = crate::provider::consume_sse(response, parse_gemini_delta, on_update).await?;
        Ok(crate::provider::RawTranslation { text })
    }

    async fn translate(
        &self,
        request: &TranslateRequest,
    ) -> Result<crate::provider::RawTranslation> {
        let text = match self.kind {
            EngineKind::Google => self.google(request).await?,
            EngineKind::Bing => self.bing(request).await?,
            EngineKind::DeeplFree => self.deepl_free(request).await?,
            EngineKind::YoudaoWeb => self.youdao_web(request).await?,
            EngineKind::Deepl => self.deepl_api(request).await?,
            EngineKind::Baidu => self.baidu(request).await?,
            EngineKind::Tencent => self.tencent(request).await?,
            EngineKind::Alibaba => self.alibaba(request).await?,
            EngineKind::Youdao => self.youdao_api(request).await?,
            EngineKind::Azure => self.azure(request).await?,
            EngineKind::Gemini => self.gemini(request).await?,
            _ => {
                return Err(Error::Provider {
                    status: None,
                    message: "这个引擎没有网页或 API 实现".into(),
                })
            }
        };
        Ok(crate::provider::RawTranslation { text })
    }
}

impl WebTranslator {
    async fn google(&self, request: &TranslateRequest) -> Result<String> {
        let url = format!(
            "https://translate.googleapis.com/translate_a/single?client=gtx&sl={}&tl={}&dt=t&q={}",
            map_lang(self.kind, &request.source),
            map_lang(self.kind, &request.target),
            urlencoding::encode(&request.text)
        );
        let body = send(&self.client, self.client.get(url)).await?;
        parse_google(&body)
    }

    async fn bing(&self, request: &TranslateRequest) -> Result<String> {
        let token = send(
            &self.client,
            self.client.get("https://edge.microsoft.com/translate/auth"),
        )
        .await?;
        let to = map_lang(self.kind, &request.target);
        let mut url = format!(
            "https://api-edge.cognitive.microsofttranslator.com/translate?api-version=3.0&to={to}"
        );
        if request.source != "auto" {
            url.push_str(&format!("&from={}", map_lang(self.kind, &request.source)));
        }
        let body = send(
            &self.client,
            self.client
                .post(url)
                .bearer_auth(token.trim())
                .json(&json!([{ "Text": request.text }])),
        )
        .await?;
        parse_bing(&body)
    }

    async fn deepl_free(&self, request: &TranslateRequest) -> Result<String> {
        let body = deepl_free_body(&request.text, &request.source, &request.target);
        let response = send(
            &self.client,
            self.client
                .post("https://www2.deepl.com/jsonrpc")
                .header("content-type", "application/json")
                .body(body),
        )
        .await?;
        parse_deepl(&response)
    }

    async fn youdao_web(&self, request: &TranslateRequest) -> Result<String> {
        let body = send(
            &self.client,
            self.client
                .post("https://fanyi.youdao.com/translate?doctype=json")
                .form(&[
                    ("i", request.text.as_str()),
                    ("from", map_lang(self.kind, &request.source).as_str()),
                    ("to", map_lang(self.kind, &request.target).as_str()),
                    ("doctype", "json"),
                ]),
        )
        .await?;
        parse_youdao_web(&body)
    }

    async fn deepl_api(&self, request: &TranslateRequest) -> Result<String> {
        let base = if self.config.base_url.trim().is_empty() {
            "https://api-free.deepl.com"
        } else {
            self.config.base_url.trim().trim_end_matches('/')
        };
        let mut fields = vec![
            ("text".to_string(), request.text.clone()),
            (
                "target_lang".to_string(),
                map_lang(self.kind, &request.target),
            ),
        ];
        if request.source != "auto" {
            fields.push(("source_lang".into(), map_lang(self.kind, &request.source)));
        }
        let body = send(
            &self.client,
            self.client
                .post(format!("{base}/v2/translate"))
                .header(
                    "authorization",
                    format!("DeepL-Auth-Key {}", self.config.api_key.trim()),
                )
                .form(&fields),
        )
        .await?;
        parse_deepl_api(&body)
    }

    async fn baidu(&self, request: &TranslateRequest) -> Result<String> {
        let salt = now_millis().to_string();
        let sign = baidu_sign(
            self.config.app_id.trim(),
            &request.text,
            &salt,
            self.config.secret.trim(),
        );
        let body = send(
            &self.client,
            self.client
                .post("https://fanyi-api.baidu.com/api/trans/vip/translate")
                .form(&[
                    ("q", request.text.as_str()),
                    ("from", map_lang(self.kind, &request.source).as_str()),
                    ("to", map_lang(self.kind, &request.target).as_str()),
                    ("appid", self.config.app_id.trim()),
                    ("salt", salt.as_str()),
                    ("sign", sign.as_str()),
                ]),
        )
        .await?;
        parse_baidu(&body)
    }

    async fn youdao_api(&self, request: &TranslateRequest) -> Result<String> {
        let salt = uuid_salt();
        let curtime = now_secs().to_string();
        let sign = youdao_sign(
            self.config.app_id.trim(),
            &request.text,
            &salt,
            &curtime,
            self.config.secret.trim(),
        );
        let body = send(
            &self.client,
            self.client.post("https://openapi.youdao.com/api").form(&[
                ("q", request.text.as_str()),
                ("from", map_lang(self.kind, &request.source).as_str()),
                ("to", map_lang(self.kind, &request.target).as_str()),
                ("appKey", self.config.app_id.trim()),
                ("salt", salt.as_str()),
                ("sign", sign.as_str()),
                ("signType", "v3"),
                ("curtime", curtime.as_str()),
            ]),
        )
        .await?;
        parse_youdao_api(&body)
    }

    async fn azure(&self, request: &TranslateRequest) -> Result<String> {
        let mut url = format!(
            "https://api.cognitive.microsofttranslator.com/translate?api-version=3.0&to={}",
            map_lang(self.kind, &request.target)
        );
        if request.source != "auto" {
            url.push_str(&format!("&from={}", map_lang(self.kind, &request.source)));
        }
        let mut call = self
            .client
            .post(url)
            .header("Ocp-Apim-Subscription-Key", self.config.api_key.trim())
            .json(&json!([{ "Text": request.text }]));
        if !self.config.region.trim().is_empty() {
            call = call.header("Ocp-Apim-Subscription-Region", self.config.region.trim());
        }
        let body = send(&self.client, call).await?;
        parse_bing(&body)
    }

    async fn gemini(&self, request: &TranslateRequest) -> Result<String> {
        let response = self.gemini_request(request, false).await?;
        let status = response.status();
        let body = response.text().await.map_err(|err| Error::Provider {
            status: Some(status.as_u16()),
            message: err.to_string(),
        })?;
        if !status.is_success() {
            return Err(Error::Provider {
                status: Some(status.as_u16()),
                message: snippet(&body),
            });
        }
        parse_gemini(&body)
    }

    async fn gemini_request(
        &self,
        request: &TranslateRequest,
        stream: bool,
    ) -> Result<reqwest::Response> {
        let base = if self.config.base_url.trim().is_empty() {
            "https://generativelanguage.googleapis.com/v1beta".to_string()
        } else {
            self.config
                .base_url
                .trim()
                .trim_end_matches('/')
                .to_string()
        };
        let model = if self.config.model.trim().is_empty() {
            "gemini-2.0-flash"
        } else {
            self.config.model.trim()
        };
        let prompt = format!(
            "把下面的文本翻译成{}。只输出译文。\n{}",
            map_lang(self.kind, &request.target),
            request.text
        );
        let method = if stream {
            "streamGenerateContent?alt=sse&"
        } else {
            "generateContent?"
        };
        let url = format!(
            "{base}/models/{model}:{method}key={}",
            urlencoding::encode(self.config.api_key.trim())
        );
        self.client
            .post(url)
            .json(&json!({
                "contents": [{ "parts": [{ "text": prompt }] }]
            }))
            .send()
            .await
            .map_err(|err| Error::Provider {
                status: err.status().map(|status| status.as_u16()),
                message: err.to_string(),
            })
    }

    async fn tencent(&self, request: &TranslateRequest) -> Result<String> {
        let payload = json!({
            "SourceText": request.text,
            "Source": map_lang(self.kind, &request.source),
            "Target": map_lang(self.kind, &request.target),
            "ProjectId": 0
        })
        .to_string();
        let timestamp = now_secs();
        let region = if self.config.region.trim().is_empty() {
            "ap-guangzhou"
        } else {
            self.config.region.trim()
        };
        let authorization = tencent_authorization(
            self.config.app_id.trim(),
            self.config.secret.trim(),
            &payload,
            timestamp,
        );
        let body = send(
            &self.client,
            self.client
                .post("https://tmt.tencentcloudapi.com")
                .header("authorization", authorization)
                .header("content-type", "application/json; charset=utf-8")
                .header("host", "tmt.tencentcloudapi.com")
                .header("x-tc-action", "TextTranslate")
                .header("x-tc-version", "2018-03-21")
                .header("x-tc-timestamp", timestamp.to_string())
                .header("x-tc-region", region)
                .body(payload),
        )
        .await?;
        parse_tencent(&body)
    }

    async fn alibaba(&self, request: &TranslateRequest) -> Result<String> {
        let nonce = uuid_salt();
        let timestamp = alibaba_timestamp();
        let mut params = vec![
            ("FormatType", "text".to_string()),
            ("SourceLanguage", map_lang(self.kind, &request.source)),
            ("TargetLanguage", map_lang(self.kind, &request.target)),
            ("SourceText", request.text.clone()),
            ("Scene", "general".into()),
            ("Action", "TranslateGeneral".into()),
            ("Version", "2018-10-12".into()),
            ("Format", "JSON".into()),
            ("AccessKeyId", self.config.app_id.trim().to_string()),
            ("SignatureMethod", "HMAC-SHA1".into()),
            ("SignatureVersion", "1.0".into()),
            ("SignatureNonce", nonce),
            ("Timestamp", timestamp),
        ];
        let signature = alibaba_signature(self.config.secret.trim(), &params);
        params.push(("Signature", signature));
        let body = send(
            &self.client,
            self.client.post("https://mt.aliyuncs.com/").form(&params),
        )
        .await?;
        parse_alibaba(&body)
    }
}

async fn send(_client: &reqwest::Client, request: reqwest::RequestBuilder) -> Result<String> {
    let response = request.send().await.map_err(|err| Error::Provider {
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
            message: snippet(&body),
        });
    }
    Ok(body)
}

pub fn snippet(body: &str) -> String {
    let trimmed = body.trim();
    let max = 500.min(trimmed.len());
    let mut cut = max;
    while cut > 0 && !trimmed.is_char_boundary(cut) {
        cut -= 1;
    }
    trimmed[..cut].to_string()
}

pub fn map_lang(kind: EngineKind, code: &str) -> String {
    match kind {
        EngineKind::Google | EngineKind::YoudaoWeb => match code {
            "zh" => "zh-CN".into(),
            "auto" => "auto".into(),
            other => other.into(),
        },
        EngineKind::Deepl | EngineKind::DeeplFree => match code {
            "zh" => "ZH".into(),
            "en" => "EN".into(),
            "ja" => "JA".into(),
            "ko" => "KO".into(),
            "fr" => "FR".into(),
            "de" => "DE".into(),
            "es" => "ES".into(),
            "ru" => "RU".into(),
            "auto" => "auto".into(),
            other => other.to_uppercase(),
        },
        EngineKind::Bing | EngineKind::Azure => match code {
            "zh" => "zh-Hans".into(),
            "auto" => "auto".into(),
            other => other.into(),
        },
        EngineKind::Tencent | EngineKind::Alibaba => match code {
            "zh" => "zh".into(),
            "auto" => "auto".into(),
            other => other.into(),
        },
        _ => code.into(),
    }
}

pub fn parse_google(body: &str) -> Result<String> {
    let value: Value = serde_json::from_str(body).map_err(|err| Error::Provider {
        status: None,
        message: format!("{}：{err}", snippet(body)),
    })?;
    let mut out = String::new();
    if let Some(rows) = value
        .as_array()
        .and_then(|items| items.first())
        .and_then(|row| row.as_array())
    {
        for row in rows {
            if let Some(text) = row.get(0).and_then(|item| item.as_str()) {
                out.push_str(text);
            }
        }
    }
    nonempty(out, body)
}

pub fn parse_bing(body: &str) -> Result<String> {
    let value: Value = serde_json::from_str(body).map_err(|err| Error::Provider {
        status: None,
        message: format!("{}：{err}", snippet(body)),
    })?;
    if let Some(message) = value
        .get("error")
        .and_then(|item| item.get("message"))
        .and_then(|item| item.as_str())
    {
        return Err(Error::Provider {
            status: None,
            message: message.to_string(),
        });
    }
    let text = value
        .as_array()
        .and_then(|items| items.first())
        .and_then(|item| item.get("translations"))
        .and_then(|items| items.as_array())
        .and_then(|items| items.first())
        .and_then(|item| item.get("text"))
        .and_then(|item| item.as_str())
        .unwrap_or("")
        .to_string();
    nonempty(text, body)
}

pub fn parse_deepl(body: &str) -> Result<String> {
    let value: Value = serde_json::from_str(body).map_err(|err| Error::Provider {
        status: None,
        message: format!("{}：{err}", snippet(body)),
    })?;
    let text = value
        .pointer("/result/texts/0/text")
        .and_then(|item| item.as_str())
        .unwrap_or("")
        .to_string();
    nonempty(text, body)
}

pub fn parse_deepl_api(body: &str) -> Result<String> {
    let value: Value = serde_json::from_str(body).map_err(|err| Error::Provider {
        status: None,
        message: format!("{}：{err}", snippet(body)),
    })?;
    if let Some(message) = value.get("message").and_then(|item| item.as_str()) {
        if value.pointer("/translations/0/text").is_none() {
            return Err(Error::Provider {
                status: None,
                message: message.to_string(),
            });
        }
    }
    let text = value
        .pointer("/translations/0/text")
        .and_then(|item| item.as_str())
        .unwrap_or("")
        .to_string();
    nonempty(text, body)
}

pub fn parse_youdao_web(body: &str) -> Result<String> {
    let value: Value = serde_json::from_str(body).map_err(|err| Error::Provider {
        status: None,
        message: format!("{}：{err}", snippet(body)),
    })?;
    let mut out = String::new();
    if let Some(rows) = value
        .get("translateResult")
        .and_then(|item| item.as_array())
    {
        for row in rows {
            if let Some(items) = row.as_array() {
                for item in items {
                    if let Some(text) = item.get("tgt").and_then(|item| item.as_str()) {
                        out.push_str(text);
                    }
                }
            }
        }
    }
    nonempty(out, body)
}

pub fn parse_baidu(body: &str) -> Result<String> {
    let value: Value = serde_json::from_str(body).map_err(|err| Error::Provider {
        status: None,
        message: format!("{}：{err}", snippet(body)),
    })?;
    if let Some(message) = value.get("error_msg").and_then(|item| item.as_str()) {
        return Err(Error::Provider {
            status: None,
            message: message.to_string(),
        });
    }
    let text = value
        .pointer("/trans_result/0/dst")
        .and_then(|item| item.as_str())
        .unwrap_or("")
        .to_string();
    nonempty(text, body)
}

pub fn parse_youdao_api(body: &str) -> Result<String> {
    let value: Value = serde_json::from_str(body).map_err(|err| Error::Provider {
        status: None,
        message: format!("{}：{err}", snippet(body)),
    })?;
    let code = value
        .get("errorCode")
        .and_then(|item| item.as_str())
        .unwrap_or("0");
    if code != "0" {
        return Err(Error::Provider {
            status: None,
            message: snippet(body),
        });
    }
    let text = value
        .get("translation")
        .and_then(|item| item.as_array())
        .and_then(|items| items.first())
        .and_then(|item| item.as_str())
        .unwrap_or("")
        .to_string();
    nonempty(text, body)
}

pub fn parse_gemini(body: &str) -> Result<String> {
    let value: Value = serde_json::from_str(body).map_err(|err| Error::Provider {
        status: None,
        message: format!("{}：{err}", snippet(body)),
    })?;
    if let Some(message) = value
        .pointer("/error/message")
        .and_then(|item| item.as_str())
    {
        return Err(Error::Provider {
            status: None,
            message: message.to_string(),
        });
    }
    let text = value
        .pointer("/candidates/0/content/parts/0/text")
        .and_then(|item| item.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    nonempty(text, body)
}

pub fn parse_tencent(body: &str) -> Result<String> {
    let value: Value = serde_json::from_str(body).map_err(|err| Error::Provider {
        status: None,
        message: format!("{}：{err}", snippet(body)),
    })?;
    if let Some(message) = value
        .pointer("/Response/Error/Message")
        .and_then(|item| item.as_str())
    {
        return Err(Error::Provider {
            status: None,
            message: message.to_string(),
        });
    }
    let text = value
        .pointer("/Response/TargetText")
        .and_then(|item| item.as_str())
        .unwrap_or("")
        .to_string();
    nonempty(text, body)
}

pub fn parse_alibaba(body: &str) -> Result<String> {
    let value: Value = serde_json::from_str(body).map_err(|err| Error::Provider {
        status: None,
        message: format!("{}：{err}", snippet(body)),
    })?;
    if let Some(message) = value.get("Message").and_then(|item| item.as_str()) {
        if value.pointer("/Data/Translated").is_none() {
            return Err(Error::Provider {
                status: None,
                message: message.to_string(),
            });
        }
    }
    let text = value
        .pointer("/Data/Translated")
        .and_then(|item| item.as_str())
        .unwrap_or("")
        .to_string();
    nonempty(text, body)
}

pub fn parse_gemini_delta(line: &str) -> Option<String> {
    let data = line.trim().strip_prefix("data:").unwrap_or(line).trim();
    if data.is_empty() || data == "[DONE]" {
        return None;
    }
    let value: Value = serde_json::from_str(data).ok()?;
    value
        .pointer("/candidates/0/content/parts/0/text")
        .and_then(|item| item.as_str())
        .map(str::to_string)
}

fn nonempty(text: String, body: &str) -> Result<String> {
    let text = text.trim().to_string();
    if text.is_empty() {
        Err(Error::Provider {
            status: None,
            message: snippet(body),
        })
    } else {
        Ok(text)
    }
}

pub fn deepl_free_body(text: &str, source: &str, target: &str) -> String {
    let mut id = 100_000i64;
    if (id + 5) % 29 == 0 || (id + 3) % 13 == 0 {
        id += 1;
    }
    let source_lang = if source == "auto" {
        Value::Null
    } else {
        Value::String(map_lang(EngineKind::DeeplFree, source))
    };
    let body = json!({
        "jsonrpc": "2.0",
        "method": "LMT_handle_texts",
        "id": id,
        "params": {
            "texts": [{ "text": text, "requestAlternatives": 0 }],
            "splitting": "newlines",
            "lang": {
                "source_lang_user_selected": source_lang,
                "target_lang": map_lang(EngineKind::DeeplFree, target)
            },
            "timestamp": now_millis()
        }
    });
    let raw = body.to_string();
    if id % 2 == 0 {
        raw.replace("\"method\":\"", "\"method\" : \"")
    } else {
        raw.replace("\"method\":\"", "\"method\": \"")
    }
}

pub fn baidu_sign(app_id: &str, query: &str, salt: &str, secret: &str) -> String {
    let raw = format!("{app_id}{query}{salt}{secret}");
    format!("{:x}", Md5::digest(raw.as_bytes()))
}

pub fn youdao_input(query: &str) -> String {
    let chars: Vec<char> = query.chars().collect();
    if chars.len() <= 20 {
        return query.to_string();
    }
    let head: String = chars[..10].iter().collect();
    let tail: String = chars[chars.len() - 10..].iter().collect();
    format!("{head}{}{tail}", chars.len())
}

pub fn youdao_sign(app_key: &str, query: &str, salt: &str, curtime: &str, secret: &str) -> String {
    let raw = format!("{app_key}{}{salt}{curtime}{secret}", youdao_input(query));
    format!("{:x}", Sha256::digest(raw.as_bytes()))
}

pub fn tencent_authorization(
    secret_id: &str,
    secret: &str,
    payload: &str,
    timestamp: u64,
) -> String {
    let date = utc_date(timestamp);
    let hashed_payload = format!("{:x}", Sha256::digest(payload.as_bytes()));
    let canonical = format!(
        "POST\n/\n\ncontent-type:application/json; charset=utf-8\nhost:tmt.tencentcloudapi.com\n\ncontent-type;host\n{hashed_payload}"
    );
    let scope = format!("{date}/tmt/tc3_request");
    let hashed_canonical = format!("{:x}", Sha256::digest(canonical.as_bytes()));
    let string_to_sign = format!("TC3-HMAC-SHA256\n{timestamp}\n{scope}\n{hashed_canonical}");
    let secret_date = hmac_sha256(format!("TC3{secret}").as_bytes(), date.as_bytes());
    let secret_service = hmac_sha256(&secret_date, b"tmt");
    let secret_signing = hmac_sha256(&secret_service, b"tc3_request");
    let signature = hex::encode(hmac_sha256(&secret_signing, string_to_sign.as_bytes()));
    format!(
        "TC3-HMAC-SHA256 Credential={secret_id}/{scope}, SignedHeaders=content-type;host, Signature={signature}"
    )
}

pub fn alibaba_signature(secret: &str, params: &[(&str, String)]) -> String {
    let mut pairs: Vec<(String, String)> = params
        .iter()
        .map(|(key, value)| ((*key).to_string(), value.clone()))
        .collect();
    pairs.sort_by(|left, right| left.0.cmp(&right.0));
    let canonical = pairs
        .iter()
        .map(|(key, value)| format!("{}={}", percent(key), percent(value)))
        .collect::<Vec<_>>()
        .join("&");
    let string_to_sign = format!("POST&{}&{}", percent("/"), percent(&canonical));
    let key = format!("{secret}&");
    let mut mac = Hmac::<sha1::Sha1>::new_from_slice(key.as_bytes()).expect("hmac key");
    mac.update(string_to_sign.as_bytes());
    B64.encode(mac.finalize().into_bytes())
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("hmac key");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn percent(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn uuid_salt() -> String {
    format!("{:x}", now_millis())
}

fn utc_date(timestamp: u64) -> String {
    let days = timestamp / 86_400;
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };
    format!("{year:04}-{month:02}-{day:02}")
}

fn alibaba_timestamp() -> String {
    format!("{}T00:00:00Z", utc_date(now_secs()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_recorded_engine_payloads() {
        assert_eq!(
            parse_google(r#"[[["你好","hello",null,null,1]],null,"en"]"#).unwrap(),
            "你好"
        );
        assert_eq!(
            parse_bing(r#"[{"translations":[{"text":"你好","to":"zh-Hans"}]}]"#).unwrap(),
            "你好"
        );
        assert_eq!(
            parse_deepl(r#"{"result":{"texts":[{"text":"你好"}]}}"#).unwrap(),
            "你好"
        );
        assert_eq!(
            parse_deepl_api(r#"{"translations":[{"text":"你好"}]}"#).unwrap(),
            "你好"
        );
        assert_eq!(
            parse_youdao_web(r#"{"translateResult":[[{"tgt":"你好","src":"hello"}]]}"#).unwrap(),
            "你好"
        );
        assert_eq!(
            parse_baidu(r#"{"trans_result":[{"dst":"你好","src":"hello"}]}"#).unwrap(),
            "你好"
        );
        assert!(
            parse_baidu(r#"{"error_code":"52003","error_msg":"UNAUTHORIZED USER"}"#)
                .unwrap_err()
                .to_string()
                .contains("UNAUTHORIZED")
        );
        assert_eq!(
            parse_youdao_api(r#"{"errorCode":"0","translation":["你好"]}"#).unwrap(),
            "你好"
        );
        assert_eq!(
            parse_gemini(r#"{"candidates":[{"content":{"parts":[{"text":"你好"}]}}]}"#).unwrap(),
            "你好"
        );
        assert_eq!(
            parse_tencent(r#"{"Response":{"TargetText":"你好","Source":"en","Target":"zh"}}"#)
                .unwrap(),
            "你好"
        );
        assert_eq!(
            parse_alibaba(r#"{"Data":{"Translated":"你好"}}"#).unwrap(),
            "你好"
        );
        assert_eq!(
            crate::provider::parse_openai_delta(
                r#"data: {"choices":[{"delta":{"content":"你"}}]}"#
            )
            .as_deref(),
            Some("你")
        );
        assert_eq!(
            parse_gemini_delta(r#"data: {"candidates":[{"content":{"parts":[{"text":"你"}]}}]}"#)
                .as_deref(),
            Some("你")
        );
        assert_eq!(parse_gemini_delta("data: [DONE]"), None);
    }

    #[test]
    fn signs_baidu_youdao_and_keeps_deepl_spacing() {
        assert_eq!(baidu_sign("app", "hello", "1", "secret").len(), 32);
        assert_eq!(youdao_input("hello"), "hello");
        assert!(youdao_input("abcdefghijklmnopqrstuvwxyz").contains("26"));
        assert_eq!(youdao_sign("app", "hello", "1", "2", "secret").len(), 64);
        let body = deepl_free_body("hello", "auto", "zh");
        assert!(body.contains("LMT_handle_texts"));
        assert!(body.contains("\"method\""));
        let auth = tencent_authorization("id", "secret", "{}", 1_700_000_000);
        assert!(auth.starts_with("TC3-HMAC-SHA256"));
        let signature = alibaba_signature("secret", &[("Action", "TranslateGeneral".into())]);
        assert!(!signature.is_empty());
    }

    #[test]
    fn missing_key_becomes_a_failing_engine_instead_of_dropping_it() {
        let mut config = EngineConfig::new(EngineKind::Baidu);
        config.enabled = true;
        let engines = build_engines(&[config], "");
        assert_eq!(engines.len(), 1);
        assert_eq!(engines[0].kind, EngineKind::Baidu);
    }
}
