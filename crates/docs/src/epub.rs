use std::io::{Cursor, Read, Write};

use regex::Regex;

use crate::textfmt::OutputMode;

pub fn extract(bytes: &[u8]) -> Result<Vec<String>, String> {
    let mut texts = Vec::new();
    if let Some((_, title)) = book_title(bytes)? {
        texts.push(title);
    }
    for href in spine(bytes)? {
        let xml = read_entry(bytes, &href)?;
        texts.extend(text_nodes(&xml));
    }
    Ok(texts)
}

pub fn rebuild(bytes: &[u8], translated: &[String], mode: OutputMode) -> Result<Vec<u8>, String> {
    let hrefs = spine(bytes)?;
    let mut next = 0;
    let mut replaced = Vec::new();
    if let Some((opf_name, title)) = book_title(bytes)? {
        let target = translated.get(next).ok_or("译文数量和电子书段落不一致")?;
        next += 1;
        let opf = read_entry(bytes, &opf_name)?;
        replaced.push((opf_name, replace_once(&opf, &title, &xml_escape(target))));
    }
    for href in &hrefs {
        let xml = read_entry(bytes, href)?;
        let nodes = text_nodes(&xml);
        let mut updated = xml;
        for source in nodes {
            let target = translated.get(next).ok_or("译文数量和电子书段落不一致")?;
            next += 1;
            let replacement = match mode {
                OutputMode::TranslatedOnly => target.clone(),
                OutputMode::Bilingual => format!("{source}<br/>{target}"),
            };
            updated = replace_once(&updated, &source, &xml_escape(&replacement));
        }
        replaced.push((href.clone(), updated));
    }
    if next != translated.len() {
        return Err("电子书回填没有用完译文".into());
    }
    rewrite_zip(bytes, &replaced)
}

fn spine(bytes: &[u8]) -> Result<Vec<String>, String> {
    let opf_name = zip_names(bytes)?
        .into_iter()
        .find(|name| name.ends_with(".opf"))
        .ok_or("epub 里没有 content.opf")?;
    let opf = read_entry(bytes, &opf_name)?;
    let base = opf_name
        .rsplit_once('/')
        .map(|(dir, _)| format!("{dir}/"))
        .unwrap_or_default();
    let id = Regex::new(r#"id="([^"]+)""#).map_err(|err| err.to_string())?;
    let href = Regex::new(r#"href="([^"]+)""#).map_err(|err| err.to_string())?;
    let mut manifest = Vec::new();
    for item in Regex::new(r"(?s)<item\b[^>]*>")
        .map_err(|err| err.to_string())?
        .find_iter(&opf)
    {
        let body = item.as_str();
        let Some(id) = id.captures(body).map(|caps| caps[1].to_string()) else {
            continue;
        };
        let Some(href) = href.captures(body).map(|caps| caps[1].to_string()) else {
            continue;
        };
        manifest.push((id, href));
    }
    let mut ordered = Vec::new();
    for item in Regex::new(r#"idref="([^"]+)""#)
        .map_err(|err| err.to_string())?
        .captures_iter(&opf)
    {
        if let Some((_, href)) = manifest.iter().find(|(id, _)| id == &item[1]) {
            ordered.push(format!("{base}{href}"));
        }
    }
    if ordered.is_empty() {
        return Err("epub 目录是空的".into());
    }
    Ok(ordered)
}

fn book_title(bytes: &[u8]) -> Result<Option<(String, String)>, String> {
    let Some(opf_name) = zip_names(bytes)?
        .into_iter()
        .find(|name| name.ends_with(".opf"))
    else {
        return Ok(None);
    };
    let opf = read_entry(bytes, &opf_name)?;
    let Some(caps) = Regex::new(r"<dc:title[^>]*>([^<]+)</dc:title>")
        .map_err(|err| err.to_string())?
        .captures(&opf)
    else {
        return Ok(None);
    };
    let title = xml_unescape(caps[1].trim());
    if title.is_empty() {
        Ok(None)
    } else {
        Ok(Some((opf_name, title)))
    }
}

fn text_nodes(xml: &str) -> Vec<String> {
    let mut skipped = xml.to_string();
    for tag in ["script", "style", "code", "pre"] {
        let pattern = format!(r"(?s)<{tag}\b[^>]*>.*?</{tag}>");
        skipped = Regex::new(&pattern)
            .expect("标签")
            .replace_all(&skipped, "")
            .into_owned();
    }
    Regex::new(r">([^<]+)<")
        .expect("文本")
        .captures_iter(&skipped)
        .map(|caps| xml_unescape(caps[1].trim()))
        .filter(|text| !text.is_empty())
        .collect()
}

fn replace_once(xml: &str, source: &str, replacement: &str) -> String {
    let escaped = xml_escape(source);
    if let Some(index) = xml.find(&escaped) {
        let mut out = String::new();
        out.push_str(&xml[..index]);
        out.push_str(replacement);
        out.push_str(&xml[index + escaped.len()..]);
        return out;
    }
    xml.to_string()
}

fn zip_names(bytes: &[u8]) -> Result<Vec<String>, String> {
    let archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|err| err.to_string())?;
    Ok((0..archive.len())
        .filter_map(|index| archive.name_for_index(index).map(str::to_string))
        .collect())
}

fn read_entry(bytes: &[u8], name: &str) -> Result<String, String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|err| err.to_string())?;
    let mut file = archive
        .by_name(name)
        .map_err(|_| format!("epub 缺少 {name}"))?;
    let mut text = String::new();
    file.read_to_string(&mut text)
        .map_err(|err| err.to_string())?;
    Ok(text)
}

fn rewrite_zip(bytes: &[u8], replaced: &[(String, String)]) -> Result<Vec<u8>, String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|err| err.to_string())?;
    let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).map_err(|err| err.to_string())?;
        let name = file.name().to_string();
        output
            .start_file(name.clone(), options)
            .map_err(|err| err.to_string())?;
        if let Some((_, contents)) = replaced.iter().find(|(href, _)| href == &name) {
            output
                .write_all(contents.as_bytes())
                .map_err(|err| err.to_string())?;
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

    #[test]
    fn follows_the_spine_and_skips_code() {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        writer.start_file("OEBPS/content.opf", options).unwrap();
        writer
            .write_all(
                br#"<package><manifest><item id="c1" href="chap.xhtml"/></manifest><spine><itemref idref="c1"/></spine></package>"#,
            )
            .unwrap();
        writer.start_file("OEBPS/chap.xhtml", options).unwrap();
        writer
            .write_all(br#"<html><body><p>Hello</p><code>skip()</code></body></html>"#)
            .unwrap();
        let bytes = writer.finish().unwrap().into_inner();
        assert_eq!(extract(&bytes).unwrap(), vec!["Hello".to_string()]);
        let rebuilt = rebuild(&bytes, &["你好".into()], OutputMode::TranslatedOnly).unwrap();
        assert_eq!(extract(&rebuilt).unwrap(), vec!["你好".to_string()]);
    }

    #[test]
    fn updates_the_opf_title() {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        writer.start_file("OEBPS/content.opf", options).unwrap();
        writer
            .write_all(
                br#"<package><metadata><dc:title>Guide</dc:title></metadata><manifest><item id="c1" href="chap.xhtml"/></manifest><spine><itemref idref="c1"/></spine></package>"#,
            )
            .unwrap();
        writer.start_file("OEBPS/chap.xhtml", options).unwrap();
        writer
            .write_all(br#"<html><body><p>Hello</p></body></html>"#)
            .unwrap();
        let bytes = writer.finish().unwrap().into_inner();
        assert_eq!(
            extract(&bytes).unwrap(),
            vec!["Guide".to_string(), "Hello".to_string()]
        );
        let rebuilt = rebuild(
            &bytes,
            &["指南".into(), "你好".into()],
            OutputMode::TranslatedOnly,
        )
        .unwrap();
        let opf = String::from_utf8({
            let mut archive = zip::ZipArchive::new(Cursor::new(rebuilt)).unwrap();
            let mut file = archive.by_name("OEBPS/content.opf").unwrap();
            let mut contents = Vec::new();
            std::io::Read::read_to_end(&mut file, &mut contents).unwrap();
            contents
        })
        .unwrap();
        assert!(opf.contains("<dc:title>指南</dc:title>"));
    }
}
