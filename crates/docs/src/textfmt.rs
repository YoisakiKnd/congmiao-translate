use crate::batch::{split_long, MAX_PACK_CHARS};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    TranslatedOnly,
    Bilingual,
}

impl OutputMode {
    pub fn parse(value: &str) -> Self {
        if value == "bilingual" {
            Self::Bilingual
        } else {
            Self::TranslatedOnly
        }
    }
}

pub fn paragraphs(text: &str) -> Vec<String> {
    text.split("\n\n")
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .flat_map(|item| split_long(item, MAX_PACK_CHARS))
        .collect()
}

pub fn join_paragraphs(original: &[String], translated: &[String], mode: OutputMode) -> String {
    original
        .iter()
        .zip(translated)
        .map(|(source, target)| match mode {
            OutputMode::TranslatedOnly => target.clone(),
            OutputMode::Bilingual => format!("{source}\n\n{target}"),
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub fn sibling_output(
    input: &std::path::Path,
    target_lang: &str,
    extension: &str,
) -> std::path::PathBuf {
    let stem = input
        .file_stem()
        .and_then(|item| item.to_str())
        .unwrap_or("translated");
    let name = format!("{stem}.{target_lang}.{extension}");
    match input.parent() {
        Some(parent) => parent.join(name),
        None => std::path::PathBuf::from(name),
    }
}
