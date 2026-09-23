use std::fmt;

#[derive(Debug)]
pub enum Error {
    EmptyText,
    TextTooLong {
        len: usize,
        max: usize,
    },
    SameLanguage,
    InvalidLanguage(String),
    InvalidBaseUrl(String),
    MissingApiKey,
    MissingModel,
    Provider {
        status: Option<u16>,
        message: String,
    },
    DaemonOffline,
    Unauthorized,
    Io(String),
    Json(String),
    PlatformUnavailable(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::EmptyText => write!(f, "没有可翻译的文本"),
            Error::TextTooLong { len, max } => {
                write!(f, "文本长度为 {len}，超过上限 {max}")
            }
            Error::SameLanguage => write!(f, "源语言和目标语言相同"),
            Error::InvalidLanguage(code) => write!(f, "不支持的语言代码：{code}"),
            Error::InvalidBaseUrl(url) => write!(f, "翻译接口地址无效：{url}"),
            Error::MissingApiKey => write!(f, "还没有填写 API Key"),
            Error::MissingModel => write!(f, "还没有填写模型名称"),
            Error::Provider { status, message } => match status {
                Some(code) => write!(f, "翻译接口返回 {code}：{message}"),
                None => write!(f, "翻译接口调用失败：{message}"),
            },
            Error::DaemonOffline => write!(f, "从喵翻译没有在运行"),
            Error::Unauthorized => write!(f, "本机守护进程拒绝了这次请求"),
            Error::Io(message) => write!(f, "{message}"),
            Error::Json(message) => write!(f, "数据格式错误：{message}"),
            Error::PlatformUnavailable(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;
