use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, Result};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DictEntry {
    pub word: String,
    pub phonetic: String,
    pub meanings: Vec<String>,
    pub examples: Vec<String>,
    pub audio_url: String,
}

pub fn looks_like_word(text: &str) -> bool {
    let trimmed = text.trim();
    !trimmed.is_empty()
        && !trimmed.contains('\n')
        && trimmed.chars().count() <= 40
        && trimmed.split_whitespace().count() <= 3
}

pub async fn lookup(text: &str) -> Result<DictEntry> {
    let word = text.trim();
    if !looks_like_word(word) {
        return Err(Error::Provider {
            status: None,
            message: "这段文字不像一个词或短语".into(),
        });
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(12))
        .build()
        .map_err(|err| Error::Io(err.to_string()))?;
    let youdao_url = format!(
        "https://dict.youdao.com/jsonapi?q={}&le=en",
        urlencoding::encode(word)
    );
    if let Ok(body) = client.get(youdao_url).send().await {
        if let Ok(text) = body.text().await {
            if let Ok(entry) = parse_youdao_dict(&text) {
                return Ok(entry);
            }
        }
    }
    let free_url = format!(
        "https://api.dictionaryapi.dev/api/v2/entries/en/{}",
        urlencoding::encode(word)
    );
    let response = client
        .get(free_url)
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
            message: crate::engines::snippet(&body),
        });
    }
    parse_free_dictionary(&body)
}

pub fn parse_free_dictionary(body: &str) -> Result<DictEntry> {
    let value: Value = serde_json::from_str(body).map_err(|err| Error::Json(err.to_string()))?;
    let entry = value
        .as_array()
        .and_then(|items| items.first())
        .ok_or_else(|| Error::Provider {
            status: None,
            message: crate::engines::snippet(body),
        })?;
    let word = entry
        .get("word")
        .and_then(|item| item.as_str())
        .unwrap_or("")
        .to_string();
    let phonetic = entry
        .get("phonetic")
        .and_then(|item| item.as_str())
        .unwrap_or("")
        .to_string();
    let mut meanings = Vec::new();
    let mut examples = Vec::new();
    if let Some(items) = entry.get("meanings").and_then(|item| item.as_array()) {
        for meaning in items {
            let part = meaning
                .get("partOfSpeech")
                .and_then(|item| item.as_str())
                .unwrap_or("");
            if let Some(defs) = meaning.get("definitions").and_then(|item| item.as_array()) {
                for definition in defs {
                    if let Some(text) = definition.get("definition").and_then(|item| item.as_str())
                    {
                        if part.is_empty() {
                            meanings.push(text.to_string());
                        } else {
                            meanings.push(format!("{part}. {text}"));
                        }
                    }
                    if let Some(example) = definition.get("example").and_then(|item| item.as_str())
                    {
                        examples.push(example.to_string());
                    }
                }
            }
        }
    }
    if word.is_empty() || meanings.is_empty() {
        return Err(Error::Provider {
            status: None,
            message: "词典没有返回释义".into(),
        });
    }
    Ok(DictEntry {
        audio_url: crate::tts::youdao_voice_url(&word),
        word,
        phonetic,
        meanings,
        examples,
    })
}

pub fn parse_youdao_dict(body: &str) -> Result<DictEntry> {
    let value: Value = serde_json::from_str(body).map_err(|err| Error::Json(err.to_string()))?;
    let word_node = value.pointer("/ec/word/0");
    let word = word_node
        .and_then(|item| item.get("return-phrase"))
        .and_then(|item| item.get("l"))
        .and_then(|item| item.get("i"))
        .and_then(|item| item.as_str())
        .or_else(|| value.get("input").and_then(|item| item.as_str()))
        .unwrap_or("")
        .to_string();
    let phonetic = word_node
        .and_then(|item| item.get("ukphone").or_else(|| item.get("phone")))
        .and_then(|item| item.as_str())
        .unwrap_or("")
        .to_string();
    let mut meanings = Vec::new();
    if let Some(trs) = word_node
        .and_then(|item| item.get("trs"))
        .and_then(|item| item.as_array())
    {
        for item in trs {
            if let Some(text) = item.pointer("/tr/0/l/i/0").and_then(|item| item.as_str()) {
                meanings.push(text.to_string());
            }
        }
    }
    if word.is_empty() || meanings.is_empty() {
        return Err(Error::Provider {
            status: None,
            message: "有道词典没有返回释义".into(),
        });
    }
    Ok(DictEntry {
        audio_url: crate::tts::youdao_voice_url(&word),
        word,
        phonetic,
        meanings,
        examples: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_dictionary_fixtures() {
        let free = r#"[{"word":"hello","phonetic":"/həˈləʊ/","meanings":[{"partOfSpeech":"noun","definitions":[{"definition":"a greeting","example":"say hello"}]}]}]"#;
        let entry = parse_free_dictionary(free).unwrap();
        assert_eq!(entry.word, "hello");
        assert_eq!(entry.meanings[0], "noun. a greeting");
        assert_eq!(entry.examples[0], "say hello");
        let youdao = r#"{"input":"hello","ec":{"word":[{"ukphone":"həˈləʊ","return-phrase":{"l":{"i":"hello"}},"trs":[{"tr":[{"l":{"i":["n. 你好"]}}]}]}]}}"#;
        let entry = parse_youdao_dict(youdao).unwrap();
        assert_eq!(entry.phonetic, "həˈləʊ");
        assert_eq!(entry.meanings[0], "n. 你好");
        assert!(looks_like_word("hello"));
        assert!(!looks_like_word("one two three four"));
    }
}
