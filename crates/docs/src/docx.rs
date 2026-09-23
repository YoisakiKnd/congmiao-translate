use std::io::{Cursor, Read, Write};

use regex::Regex;

use crate::textfmt::OutputMode;

pub fn extract(bytes: &[u8]) -> Result<Vec<String>, String> {
    let xml = document_xml(bytes)?;
    Ok(paragraph_texts(&xml)
        .into_iter()
        .filter(|text| !text.trim().is_empty())
        .collect())
}

pub fn rebuild(bytes: &[u8], translated: &[String], mode: OutputMode) -> Result<Vec<u8>, String> {
    let xml = document_xml(bytes)?;
    let paragraphs = paragraph_texts(&xml);
    let useful: Vec<&String> = paragraphs
        .iter()
        .filter(|text| !text.trim().is_empty())
        .collect();
    if useful.len() != translated.len() {
        return Err("译文数量和 Word 段落不一致".into());
    }
    let mut next = 0;
    let para = Regex::new(r"(?s)<w:p[ >].*?</w:p>").map_err(|err| err.to_string())?;
    let text =
        Regex::new(r"(?s)(<w:t(?:\s[^>]*)?>)(.*?)(</w:t>)").map_err(|err| err.to_string())?;
    let mut out = String::new();
    let mut last = 0;
    for found in para.find_iter(&xml) {
        out.push_str(&xml[last..found.start()]);
        let body = found.as_str();
        let runs: Vec<_> = text.find_iter(body).collect();
        if runs.is_empty() || paragraph_plain(body).trim().is_empty() {
            out.push_str(body);
        } else {
            let source = paragraph_plain(body);
            let target = &translated[next];
            next += 1;
            let replacement = match mode {
                OutputMode::TranslatedOnly => target.clone(),
                OutputMode::Bilingual => format!("{source}\n{target}"),
            };
            let mut rewritten = String::new();
            let mut cursor = 0;
            for (index, run) in runs.iter().enumerate() {
                rewritten.push_str(&body[cursor..run.start()]);
                let caps = text.captures(run.as_str()).ok_or("无法读取 Word 文本")?;
                rewritten.push_str(&caps[1]);
                if index == 0 {
                    rewritten.push_str(&xml_escape(&replacement));
                }
                rewritten.push_str(&caps[3]);
                cursor = run.end();
            }
            rewritten.push_str(&body[cursor..]);
            out.push_str(&rewritten);
        }
        last = found.end();
    }
    out.push_str(&xml[last..]);
    if next != translated.len() {
        return Err("Word 回填没有用完译文".into());
    }
    replace_entry(bytes, "word/document.xml", out.as_bytes())
}

fn paragraph_texts(xml: &str) -> Vec<String> {
    let Ok(para) = Regex::new(r"(?s)<w:p[ >].*?</w:p>") else {
        return Vec::new();
    };
    para.find_iter(xml)
        .map(|found| paragraph_plain(found.as_str()))
        .collect()
}

fn paragraph_plain(body: &str) -> String {
    let Ok(text) = Regex::new(r"(?s)<w:t(?:\s[^>]*)?>(.*?)</w:t>") else {
        return String::new();
    };
    text.captures_iter(body)
        .map(|caps| xml_unescape(&caps[1]))
        .collect::<Vec<_>>()
        .join("")
}

fn document_xml(bytes: &[u8]) -> Result<String, String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|err| err.to_string())?;
    let mut file = archive
        .by_name("word/document.xml")
        .map_err(|_| "这不是可以读取正文的 docx".to_string())?;
    let mut xml = String::new();
    file.read_to_string(&mut xml)
        .map_err(|err| err.to_string())?;
    Ok(xml)
}

fn replace_entry(bytes: &[u8], name: &str, contents: &[u8]) -> Result<Vec<u8>, String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|err| err.to_string())?;
    let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).map_err(|err| err.to_string())?;
        let entry_name = file.name().to_string();
        output
            .start_file(entry_name.clone(), options)
            .map_err(|err| err.to_string())?;
        if entry_name == name {
            output.write_all(contents).map_err(|err| err.to_string())?;
        } else {
            std::io::copy(&mut file, &mut output).map_err(|err| err.to_string())?;
        }
    }
    output
        .finish()
        .map(|cursor| cursor.into_inner())
        .map_err(|err| err.to_string())
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn xml_unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::{extract, rebuild};
    use crate::textfmt::OutputMode;
    use std::io::{Cursor, Write};

    fn sample() -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        writer.start_file("word/document.xml", options).unwrap();
        writer
            .write_all(
                br#"<?xml version="1.0"?><w:document><w:body><w:p><w:r><w:t>Hello</w:t></w:r><w:r><w:t> world</w:t></w:r></w:p></w:body></w:document>"#,
            )
            .unwrap();
        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn rewrites_the_first_text_run_and_keeps_the_zip() {
        let bytes = sample();
        assert_eq!(extract(&bytes).unwrap(), vec!["Hello world".to_string()]);
        let rebuilt = rebuild(&bytes, &["你好 世界".into()], OutputMode::TranslatedOnly).unwrap();
        assert_eq!(extract(&rebuilt).unwrap(), vec!["你好 世界".to_string()]);
    }
}
