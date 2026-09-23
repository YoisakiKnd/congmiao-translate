use std::path::PathBuf;

use crate::error::{Error, Result};

#[derive(Debug, Clone)]
pub struct DaemonPaths {
    pub data_dir: PathBuf,
}

impl DaemonPaths {
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
        }
    }

    pub fn config(&self) -> PathBuf {
        self.data_dir.join("config.json")
    }

    pub fn endpoint(&self) -> PathBuf {
        self.data_dir.join("endpoint.json")
    }

    pub fn store(&self) -> PathBuf {
        self.data_dir.join("store.db")
    }

    pub fn logs(&self) -> PathBuf {
        self.data_dir.join("logs")
    }
}

pub fn data_dir() -> Result<PathBuf> {
    if let Ok(override_dir) = std::env::var("CONGMIAO_DATA_DIR") {
        if !override_dir.is_empty() {
            return Ok(PathBuf::from(override_dir));
        }
    }
    let home = dirs::home_dir().ok_or_else(|| Error::Io("无法定位用户目录".into()))?;
    if cfg!(target_os = "macos") {
        Ok(home.join("Library/Application Support/congmiao-translate"))
    } else if cfg!(target_os = "windows") {
        Ok(dirs::data_dir()
            .unwrap_or_else(|| home.join("AppData").join("Roaming"))
            .join("congmiao-translate"))
    } else {
        Ok(dirs::data_dir()
            .unwrap_or_else(|| home.join(".local").join("share"))
            .join("congmiao-translate"))
    }
}
