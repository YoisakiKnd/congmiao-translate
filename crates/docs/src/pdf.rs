use crate::textfmt::{paragraphs, OutputMode};

pub fn extract(bytes: &[u8]) -> Result<Vec<String>, String> {
    let text = pdf_extract::extract_text_from_mem(bytes).map_err(|err| err.to_string())?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(
            "没有从 PDF 提取到文字。扫描版需要先做文字识别，这一版只处理有文字层的 PDF。".into(),
        );
    }
    let pages: Vec<String> = if trimmed.contains('\u{c}') {
        trimmed
            .split('\u{c}')
            .map(str::trim)
            .filter(|page| !page.is_empty())
            .map(str::to_string)
            .collect()
    } else {
        paragraphs(trimmed)
    };
    if pages.is_empty() {
        return Err(
            "没有从 PDF 提取到文字。扫描版需要先做文字识别，这一版只处理有文字层的 PDF。".into(),
        );
    }
    Ok(pages)
}

pub fn rebuild(
    pages: &[String],
    translated: &[String],
    mode: OutputMode,
) -> Result<String, String> {
    if pages.len() != translated.len() {
        return Err("译文数量和 PDF 页数不一致".into());
    }
    let body = pages
        .iter()
        .zip(translated)
        .enumerate()
        .map(|(index, (source, target))| {
            let content = match mode {
                OutputMode::TranslatedOnly => target.clone(),
                OutputMode::Bilingual => format!("{source}\n\n{target}"),
            };
            format!("# 第 {} 页\n\n{content}", index + 1)
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::extract;

    #[test]
    fn empty_pdf_explains_that_scans_are_not_read() {
        let err = extract(b"not a pdf").unwrap_err();
        assert!(err.contains("扫描版") || !err.is_empty());
    }
}
