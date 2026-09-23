use std::io::{Read, Write};

use serde::{Deserialize, Serialize};

use crate::client::DaemonClient;
use crate::error::{Error, Result};

pub const MAX_NATIVE_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HostRequest {
    Ping {
        id: String,
    },
    Translate {
        id: String,
        text: String,
        source: Option<String>,
        target: String,
    },
    Compare {
        id: String,
        text: String,
        source: Option<String>,
        target: String,
    },
    Dict {
        id: String,
        text: String,
    },
    Batch {
        id: String,
        texts: Vec<String>,
        source: Option<String>,
        target: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HostResponse {
    Pong {
        id: String,
    },
    TranslateResult {
        id: String,
        text: String,
        detected_source: Option<String>,
        cache_hit: bool,
    },
    CompareResult {
        id: String,
        detected_source: Option<String>,
        results: Vec<crate::engine::EngineOutput>,
    },
    DictResult {
        id: String,
        entry: crate::dict::DictEntry,
    },
    BatchResult {
        id: String,
        texts: Vec<String>,
    },
    Error {
        id: String,
        message: String,
    },
}

pub fn encode_frame(payload: &[u8]) -> Result<Vec<u8>> {
    if payload.len() > MAX_NATIVE_BYTES {
        return Err(Error::TextTooLong {
            len: payload.len(),
            max: MAX_NATIVE_BYTES,
        });
    }
    let mut frame = Vec::with_capacity(4 + payload.len());
    frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    frame.extend_from_slice(payload);
    Ok(frame)
}

pub fn decode_frame(reader: &mut impl Read) -> Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    reader
        .read_exact(&mut len_buf)
        .map_err(|err| Error::Io(err.to_string()))?;
    let len = u32::from_le_bytes(len_buf) as usize;
    if len > MAX_NATIVE_BYTES {
        return Err(Error::TextTooLong {
            len,
            max: MAX_NATIVE_BYTES,
        });
    }
    let mut payload = vec![0u8; len];
    reader
        .read_exact(&mut payload)
        .map_err(|err| Error::Io(err.to_string()))?;
    Ok(payload)
}

pub fn write_frame(writer: &mut impl Write, response: &HostResponse) -> Result<()> {
    let payload = serde_json::to_vec(response).map_err(|err| Error::Json(err.to_string()))?;
    let frame = encode_frame(&payload)?;
    writer
        .write_all(&frame)
        .map_err(|err| Error::Io(err.to_string()))?;
    writer.flush().map_err(|err| Error::Io(err.to_string()))?;
    Ok(())
}

pub fn parse_request(payload: &[u8]) -> Result<HostRequest> {
    serde_json::from_slice(payload).map_err(|err| Error::Json(err.to_string()))
}

pub async fn dispatch_host(request: HostRequest, client: &DaemonClient) -> HostResponse {
    match request {
        HostRequest::Ping { id } => match client.health().await {
            Ok(()) => HostResponse::Pong { id },
            Err(err) => HostResponse::Error {
                id,
                message: err.to_string(),
            },
        },
        HostRequest::Translate {
            id,
            text,
            source,
            target,
        } => match client
            .translate(source.as_deref().unwrap_or("auto"), &target, &text)
            .await
        {
            Ok(response) => HostResponse::TranslateResult {
                id,
                text: response.text,
                detected_source: response.detected_source,
                cache_hit: response.cache_hit,
            },
            Err(err) => HostResponse::Error {
                id,
                message: err.to_string(),
            },
        },
        HostRequest::Compare {
            id,
            text,
            source,
            target,
        } => match client
            .compare(source.as_deref().unwrap_or("auto"), &target, &text)
            .await
        {
            Ok(response) => HostResponse::CompareResult {
                id,
                detected_source: response.detected_source,
                results: response.results,
            },
            Err(err) => HostResponse::Error {
                id,
                message: err.to_string(),
            },
        },
        HostRequest::Dict { id, text } => match client.dict(&text).await {
            Ok(entry) => HostResponse::DictResult { id, entry },
            Err(err) => HostResponse::Error {
                id,
                message: err.to_string(),
            },
        },
        HostRequest::Batch {
            id,
            texts,
            source,
            target,
        } => {
            let mut translated = Vec::new();
            for text in texts {
                match client
                    .translate(source.as_deref().unwrap_or("auto"), &target, &text)
                    .await
                {
                    Ok(response) => translated.push(response.text),
                    Err(err) => {
                        return HostResponse::Error {
                            id,
                            message: err.to_string(),
                        };
                    }
                }
            }
            HostResponse::BatchResult {
                id,
                texts: translated,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_unicode_and_rejects_oversized_payloads() {
        let payload = "从喵翻译".as_bytes();
        let frame = encode_frame(payload).unwrap();
        let decoded = decode_frame(&mut frame.as_slice()).unwrap();
        assert_eq!(decoded, payload);
        let err = encode_frame(&vec![0u8; MAX_NATIVE_BYTES + 1]).unwrap_err();
        assert!(matches!(err, Error::TextTooLong { .. }));
    }
}
