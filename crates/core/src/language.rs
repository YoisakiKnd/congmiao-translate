use crate::error::{Error, Result};

pub const MAX_TEXT_CHARS: usize = 8_000;

pub struct Language {
    pub code: &'static str,
    pub name: &'static str,
}

pub const LANGUAGES: &[Language] = &[
    Language {
        code: "auto",
        name: "自动检测",
    },
    Language {
        code: "zh",
        name: "中文",
    },
    Language {
        code: "en",
        name: "英语",
    },
    Language {
        code: "ja",
        name: "日语",
    },
    Language {
        code: "ko",
        name: "韩语",
    },
    Language {
        code: "fr",
        name: "法语",
    },
    Language {
        code: "de",
        name: "德语",
    },
    Language {
        code: "es",
        name: "西班牙语",
    },
    Language {
        code: "ru",
        name: "俄语",
    },
];

pub fn language_name(code: &str) -> Option<&'static str> {
    LANGUAGES
        .iter()
        .find(|language| language.code == code)
        .map(|language| language.name)
}

pub fn validate_pair(source: &str, target: &str) -> Result<()> {
    if language_name(source).is_none() {
        return Err(Error::InvalidLanguage(source.to_string()));
    }
    if target == "auto" || language_name(target).is_none() {
        return Err(Error::InvalidLanguage(target.to_string()));
    }
    if source == target {
        return Err(Error::SameLanguage);
    }
    Ok(())
}

pub fn detect_code(text: &str) -> Option<String> {
    let info = whatlang::detect(text)?;
    let code = match info.lang() {
        whatlang::Lang::Cmn => "zh",
        whatlang::Lang::Eng => "en",
        whatlang::Lang::Jpn => "ja",
        whatlang::Lang::Kor => "ko",
        whatlang::Lang::Fra => "fr",
        whatlang::Lang::Deu => "de",
        whatlang::Lang::Spa => "es",
        whatlang::Lang::Rus => "ru",
        _ => return None,
    };
    language_name(code).map(|_| code.to_string())
}

pub fn resolve_pair(
    source: &str,
    target: &str,
    text: &str,
    swap_when_same: bool,
) -> (String, String, Option<String>) {
    let detected = if source == "auto" {
        detect_code(text)
    } else {
        None
    };
    if swap_when_same {
        if let Some(found) = &detected {
            if found == target {
                let flipped = if target == "zh" { "en" } else { "zh" };
                return (found.clone(), flipped.to_string(), detected);
            }
        }
    }
    (source.to_string(), target.to_string(), detected)
}

pub fn validate_text(text: &str) -> Result<()> {
    if text.trim().is_empty() {
        return Err(Error::EmptyText);
    }
    let len = text.chars().count();
    if len > MAX_TEXT_CHARS {
        return Err(Error::TextTooLong {
            len,
            max: MAX_TEXT_CHARS,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_same_language_and_overlong_text() {
        assert!(matches!(validate_text("  \n"), Err(Error::EmptyText)));
        assert!(matches!(
            validate_pair("zh", "zh"),
            Err(Error::SameLanguage)
        ));
        let long = "字".repeat(MAX_TEXT_CHARS + 1);
        assert!(matches!(
            validate_text(&long),
            Err(Error::TextTooLong { .. })
        ));
    }

    #[test]
    fn swaps_to_english_when_the_detected_language_is_the_target() {
        let (source, target, detected) =
            resolve_pair("auto", "zh", "今天天气不错，适合出门走走。", true);
        assert_eq!(source, "zh");
        assert_eq!(target, "en");
        assert_eq!(detected.as_deref(), Some("zh"));
        let (source, target, _) = resolve_pair("auto", "zh", "hello", true);
        assert_eq!(source, "auto");
        assert_eq!(target, "zh");
    }
}
