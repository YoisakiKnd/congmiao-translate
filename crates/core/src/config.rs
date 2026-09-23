use std::fs;
use std::path::Path;

use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::glossary::GlossaryEntry;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineKind {
    Echo,
    Openai,
    Google,
    Bing,
    DeeplFree,
    YoudaoWeb,
    Deepl,
    Baidu,
    Tencent,
    Alibaba,
    Youdao,
    Azure,
    Gemini,
}

impl EngineKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Echo => "echo",
            Self::Openai => "openai",
            Self::Google => "google",
            Self::Bing => "bing",
            Self::DeeplFree => "deepl_free",
            Self::YoudaoWeb => "youdao_web",
            Self::Deepl => "deepl",
            Self::Baidu => "baidu",
            Self::Tencent => "tencent",
            Self::Alibaba => "alibaba",
            Self::Youdao => "youdao",
            Self::Azure => "azure",
            Self::Gemini => "gemini",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Echo => "回声",
            Self::Openai => "OpenAI 兼容",
            Self::Google => "Google",
            Self::Bing => "Bing",
            Self::DeeplFree => "DeepL",
            Self::YoudaoWeb => "有道",
            Self::Deepl => "DeepL API",
            Self::Baidu => "百度翻译",
            Self::Tencent => "腾讯翻译",
            Self::Alibaba => "阿里翻译",
            Self::Youdao => "有道智云",
            Self::Azure => "Azure 翻译",
            Self::Gemini => "Gemini",
        }
    }

    pub fn unofficial(self) -> bool {
        matches!(
            self,
            Self::Google | Self::Bing | Self::DeeplFree | Self::YoudaoWeb
        )
    }

    pub fn needs_key(self) -> bool {
        !matches!(
            self,
            Self::Echo | Self::Google | Self::Bing | Self::DeeplFree | Self::YoudaoWeb
        )
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "echo" => Self::Echo,
            "openai" => Self::Openai,
            "google" => Self::Google,
            "bing" => Self::Bing,
            "deepl_free" => Self::DeeplFree,
            "youdao_web" => Self::YoudaoWeb,
            "deepl" => Self::Deepl,
            "baidu" => Self::Baidu,
            "tencent" => Self::Tencent,
            "alibaba" => Self::Alibaba,
            "youdao" => Self::Youdao,
            "azure" => Self::Azure,
            "gemini" => Self::Gemini,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EngineConfig {
    pub kind: EngineKind,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub app_id: String,
    #[serde(default)]
    pub secret: String,
    #[serde(default)]
    pub region: String,
}

fn default_true() -> bool {
    true
}

impl EngineConfig {
    pub fn new(kind: EngineKind) -> Self {
        let (base_url, model) = match kind {
            EngineKind::Openai => ("https://api.openai.com/v1".into(), "gpt-4o-mini".into()),
            EngineKind::Gemini => (
                "https://generativelanguage.googleapis.com/v1beta".into(),
                "gemini-2.0-flash".into(),
            ),
            EngineKind::Deepl => ("https://api-free.deepl.com".into(), String::new()),
            _ => (String::new(), String::new()),
        };
        Self {
            kind,
            enabled: !kind.needs_key() && kind != EngineKind::Echo,
            base_url,
            api_key: String::new(),
            model,
            app_id: String::new(),
            secret: String::new(),
            region: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ShortcutConfig {
    pub screenshot: String,
    pub selection: String,
    pub input: String,
    pub replace: String,
    pub silent_ocr: String,
}

impl Default for ShortcutConfig {
    fn default() -> Self {
        Self {
            screenshot: "ctrl+alt+shift+t".into(),
            selection: "ctrl+alt+shift+s".into(),
            input: "ctrl+alt+shift+a".into(),
            replace: "ctrl+alt+shift+r".into(),
            silent_ocr: "ctrl+alt+shift+o".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppConfig {
    pub engines: Vec<EngineConfig>,
    pub default_source: String,
    pub default_target: String,
    pub glossary: Vec<GlossaryEntry>,
    #[serde(default = "default_true")]
    pub swap_when_same: bool,
    #[serde(default)]
    pub shortcuts: ShortcutConfig,
    #[serde(default)]
    pub watch_clipboard: bool,
    #[serde(default)]
    pub auto_copy: bool,
    #[serde(default)]
    pub launch_at_login: bool,
    #[serde(default = "default_ui_language")]
    pub ui_language: String,
    #[serde(default = "default_true")]
    pub show_tray: bool,
    #[serde(default)]
    pub onboarded: bool,
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub proxy: String,
    #[serde(default = "default_job_concurrency")]
    pub job_concurrency: u8,
    #[serde(default)]
    pub auto_translate: bool,
}

fn default_job_concurrency() -> u8 {
    3
}

fn default_ui_language() -> String {
    "zh".into()
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            engines: vec![
                EngineConfig::new(EngineKind::Google),
                EngineConfig::new(EngineKind::Bing),
                EngineConfig::new(EngineKind::YoudaoWeb),
                EngineConfig::new(EngineKind::DeeplFree),
                EngineConfig::new(EngineKind::Echo),
                EngineConfig::new(EngineKind::Openai),
                EngineConfig::new(EngineKind::Deepl),
                EngineConfig::new(EngineKind::Baidu),
                EngineConfig::new(EngineKind::Tencent),
                EngineConfig::new(EngineKind::Alibaba),
                EngineConfig::new(EngineKind::Youdao),
                EngineConfig::new(EngineKind::Azure),
                EngineConfig::new(EngineKind::Gemini),
            ],
            default_source: "auto".into(),
            default_target: "zh".into(),
            glossary: Vec::new(),
            swap_when_same: true,
            shortcuts: ShortcutConfig::default(),
            watch_clipboard: false,
            auto_copy: false,
            launch_at_login: false,
            ui_language: "zh".into(),
            show_tray: true,
            onboarded: false,
            prompt: String::new(),
            proxy: String::new(),
            job_concurrency: 3,
            auto_translate: false,
        }
    }
}

#[derive(Debug, Deserialize)]
struct LegacyConfig {
    provider: LegacyProvider,
    #[serde(default = "default_source")]
    default_source: String,
    #[serde(default = "default_target")]
    default_target: String,
    #[serde(default)]
    glossary: Vec<GlossaryEntry>,
}

fn default_source() -> String {
    "auto".into()
}

fn default_target() -> String {
    "zh".into()
}

#[derive(Debug, Deserialize)]
struct LegacyProvider {
    kind: String,
    #[serde(default)]
    base_url: String,
    #[serde(default)]
    api_key: String,
    #[serde(default)]
    model: String,
}

impl AppConfig {
    pub fn load_from(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = fs::read_to_string(path).map_err(io_error)?;
        let value: serde_json::Value =
            serde_json::from_str(&raw).map_err(|err| Error::Json(err.to_string()))?;
        if value.get("engines").is_none() {
            let legacy: LegacyConfig =
                serde_json::from_value(value).map_err(|err| Error::Json(err.to_string()))?;
            return Ok(migrate_legacy(legacy));
        }
        serde_json::from_value(value).map_err(|err| Error::Json(err.to_string()))
    }

    pub fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(io_error)?;
        }
        let raw = serde_json::to_string_pretty(self).map_err(|err| Error::Json(err.to_string()))?;
        write_private(path, raw.as_bytes())
    }

    pub fn enabled_engines(&self) -> Vec<&EngineConfig> {
        self.engines
            .iter()
            .filter(|engine| engine.enabled)
            .collect()
    }
}

fn migrate_legacy(legacy: LegacyConfig) -> AppConfig {
    let mut config = AppConfig {
        default_source: legacy.default_source,
        default_target: legacy.default_target,
        glossary: legacy.glossary,
        onboarded: true,
        ..AppConfig::default()
    };
    config.engines.iter_mut().for_each(|engine| {
        engine.enabled = false;
    });
    let kind = EngineKind::parse(&legacy.provider.kind).unwrap_or(EngineKind::Openai);
    if let Some(engine) = config.engines.iter_mut().find(|engine| engine.kind == kind) {
        engine.enabled = true;
        if !legacy.provider.base_url.is_empty() {
            engine.base_url = legacy.provider.base_url;
        }
        engine.api_key = legacy.provider.api_key;
        if !legacy.provider.model.is_empty() {
            engine.model = legacy.provider.model;
        }
    }
    config
}

pub fn config_stamp(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|meta| meta.modified()).ok()
}

pub(crate) fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let raw = fs::read_to_string(path).map_err(io_error)?;
    serde_json::from_str(&raw).map_err(|err| Error::Json(err.to_string()))
}

pub(crate) fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(io_error)?;
    }
    let mut file = fs::File::create(path).map_err(io_error)?;
    file.write_all(bytes).map_err(io_error)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(io_error)?;
    }
    Ok(())
}

fn io_error(err: std::io::Error) -> Error {
    Error::Io(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_engine_secrets_with_private_permissions() {
        let dir = std::env::temp_dir().join(format!("congmiao-config-{}", std::process::id()));
        let path = dir.join("config.json");
        let mut config = AppConfig::default();
        config.engines[5].api_key = "secret".into();
        config.save_to(&path).unwrap();
        let loaded = AppConfig::load_from(&path).unwrap();
        assert_eq!(loaded.engines[5].api_key, "secret");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn migrates_the_single_provider_field() {
        let raw = r#"{
            "provider": {"kind": "echo", "base_url": "http://127.0.0.1", "api_key": "", "model": "echo"},
            "default_source": "auto",
            "default_target": "zh",
            "glossary": [{"source": "hello", "target": "你好"}]
        }"#;
        let legacy: serde_json::Value = serde_json::from_str(raw).unwrap();
        let parsed: LegacyConfig = serde_json::from_value(legacy).unwrap();
        let config = migrate_legacy(parsed);
        assert!(config.onboarded);
        assert_eq!(config.glossary[0].target, "你好");
        let echo = config
            .engines
            .iter()
            .find(|engine| engine.kind == EngineKind::Echo)
            .unwrap();
        assert!(echo.enabled);
        assert!(config
            .engines
            .iter()
            .filter(|engine| engine.kind != EngineKind::Echo)
            .all(|engine| !engine.enabled));
    }
}
