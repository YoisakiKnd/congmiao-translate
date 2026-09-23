//! 从喵翻译的唯一翻译实现。
//!
//! 桌面窗口和浏览器扩展都不直接调用翻译接口，而是把请求交给本机守护进程，
//! 由这里的引擎完成校验、术语表、缓存和供应商调用。

#![forbid(unsafe_code)]

mod cache;
mod client;
mod config;
mod dict;
mod endpoint;
mod engine;
mod engines;
mod error;
mod glossary;
mod language;
mod paths;
mod platform;
mod protocol;
mod provider;
mod scope;
mod store;
mod tts;

pub use client::DaemonClient;
pub use config::{config_stamp, AppConfig, EngineConfig, EngineKind, ShortcutConfig};
pub use dict::{looks_like_word, lookup as lookup_dict, DictEntry};
pub use endpoint::{new_token, tokens_equal, Endpoint};
pub use engine::{CompareResponse, Engine, EngineOutput, Prepared, TranslateResponse};
pub use engines::build_engines;
pub use error::Error;
pub use glossary::{parse_lines as parse_glossary, GlossaryEntry};
pub use language::{language_name, LANGUAGES, MAX_TEXT_CHARS};
pub use paths::{data_dir, DaemonPaths};
pub use platform::{
    linux_missing_command, ocr_backend, on_wayland, selection_backend, wayland_selection_hint,
    wayland_shortcut_hint, OcrBackend, SelectionBackend,
};
pub use protocol::{
    decode_frame, dispatch_host, encode_frame, parse_request, write_frame, HostRequest,
    HostResponse, MAX_NATIVE_BYTES,
};
pub use provider::openai_translator;
pub use scope::{ARCHITECTURE, DEFERRED_ENTRIES, V1_ENTRIES, V1_PLATFORMS};
pub use store::{HistoryItem, HistoryResult, JobRecord, JobSegment, Store, VocabItem};
pub use tts::{google_voice_url, youdao_voice_url};

pub const DAEMON_PORT: u16 = 47321;
pub const NATIVE_HOST_NAME: &str = "app.congmiao.translate";
pub const FIREFOX_EXTENSION_ID: &str = "translate@congmiao.app";
