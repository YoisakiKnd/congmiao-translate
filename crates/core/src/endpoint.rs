use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::config::{read_json, write_private};
use crate::error::{Error, Result};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Endpoint {
    pub port: u16,
    pub token: String,
}

impl Endpoint {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Err(Error::DaemonOffline);
        }
        read_json(path)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let raw = serde_json::to_vec(self).map_err(|err| Error::Json(err.to_string()))?;
        write_private(path, &raw)
    }

    pub fn remove(path: &Path) {
        let _ = std::fs::remove_file(path);
    }
}

pub fn new_token() -> Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes).map_err(|err| Error::Io(err.to_string()))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

pub fn tokens_equal(left: &str, right: &str) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in left.bytes().zip(right.bytes()) {
        diff |= a ^ b;
    }
    diff == 0
}
