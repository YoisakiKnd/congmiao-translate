use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};
use zip::write::SimpleFileOptions;

use crate::snbt::{self, SnbtValue};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub text: String,
    pub meta: String,
    pub skip: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceSummary {
    pub kind: String,
    pub path: String,
    pub entries: usize,
    pub translated: usize,
}

#[derive(Debug, Clone)]
pub struct Scan {
    pub sources: Vec<SourceSummary>,
    pub segments: Vec<Segment>,
    pub terms: Vec<String>,
}

pub fn builtin_terms() -> Vec<(String, String)> {
    [
        ("The End", "末地"),
        ("Overworld", "主世界"),
        ("Nether", "下界"),
        ("Ender", "末影"),
        ("Quest", "任务"),
        ("Chapter", "章节"),
    ]
    .into_iter()
    .map(|(source, target)| (source.to_string(), target.to_string()))
    .collect()
}

pub fn scan_instance(root: &Path) -> Result<Scan, String> {
    let mut sources = Vec::new();
    let mut segments = Vec::new();
    let installed = installed_chinese(root);
    scan_mods(root, &mut sources, &mut segments, &installed)?;
    scan_ftb(root, &mut sources, &mut segments)?;
    scan_patchouli_tree(root, &mut sources, &mut segments)?;
    let terms = frequent_terms(&segments);
    Ok(Scan {
        sources,
        segments,
        terms,
    })
}

pub fn apply_instance(
    root: &Path,
    segments: &[Segment],
    translations: &[String],
    pack_format: u32,
) -> Result<PathBuf, String> {
    if segments.len() != translations.len() {
        return Err("译文数量和模组条目不一致".into());
    }
    let mut lang: std::collections::BTreeMap<String, Map<String, Value>> =
        std::collections::BTreeMap::new();
    let mut patchouli: Vec<(String, String, Value)> = Vec::new();
    let mut ftb: std::collections::BTreeMap<String, Vec<(String, SnbtValue)>> =
        std::collections::BTreeMap::new();
    for (segment, translation) in segments.iter().zip(translations) {
        if segment.skip {
            continue;
        }
        let meta: Value = serde_json::from_str(&segment.meta).map_err(|err| err.to_string())?;
        match meta.get("op").and_then(Value::as_str).unwrap_or("") {
            "lang" => {
                let namespace = meta["namespace"].as_str().unwrap_or("minecraft");
                let key = meta["key"].as_str().unwrap_or("");
                lang.entry(namespace.to_string())
                    .or_default()
                    .insert(key.to_string(), Value::String(translation.clone()));
            }
            "patchouli" => {
                let path = meta["path"].as_str().unwrap_or("").to_string();
                let pointer = meta["pointer"].as_str().unwrap_or("");
                if let Some((_, _, value)) = patchouli.iter_mut().find(|(item, _, _)| item == &path)
                {
                    set_pointer(value, pointer, translation);
                } else if let Some(bytes) = read_maybe(root, &path) {
                    let mut value: Value =
                        serde_json::from_slice(&bytes).unwrap_or(Value::Object(Map::new()));
                    set_pointer(&mut value, pointer, translation);
                    patchouli.push((path, meta["namespace"].as_str().unwrap_or("").into(), value));
                }
            }
            "ftb" => {
                let file = meta["file"].as_str().unwrap_or("").to_string();
                let key = meta["key"].as_str().unwrap_or("").to_string();
                let entry = ftb.entry(file).or_default();
                if let Some(index) = meta.get("index").and_then(Value::as_u64) {
                    if let Some((_, SnbtValue::List(items))) =
                        entry.iter_mut().find(|(item, _)| item == &key)
                    {
                        if let Some(slot) = items.get_mut(index as usize) {
                            *slot = translation.clone();
                        }
                    } else {
                        let mut items = Vec::new();
                        items.resize(index as usize + 1, String::new());
                        items[index as usize] = translation.clone();
                        entry.push((key, SnbtValue::List(items)));
                    }
                } else if let Some((_, SnbtValue::String(text))) =
                    entry.iter_mut().find(|(item, _)| item == &key)
                {
                    *text = translation.clone();
                } else {
                    entry.push((key, SnbtValue::String(translation.clone())));
                }
            }
            _ => {}
        }
    }
    write_ftb(root, &ftb)?;
    write_resource_pack(root, &lang, &patchouli, pack_format)
}

fn installed_chinese(
    root: &Path,
) -> std::collections::HashMap<String, std::collections::HashMap<String, String>> {
    let mut map =
        std::collections::HashMap::<String, std::collections::HashMap<String, String>>::new();
    let packs = root.join("resourcepacks");
    if !packs.is_dir() {
        return map;
    }
    let Ok(reader) = fs::read_dir(&packs) else {
        return map;
    };
    for entry in reader.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_lang_tree(&path, &mut map);
        } else if path.extension().and_then(|item| item.to_str()) == Some("zip") {
            if let Ok(bytes) = fs::read(&path) {
                collect_lang_zip(&bytes, &mut map);
            }
        }
    }
    map
}

fn collect_lang_tree(
    dir: &Path,
    map: &mut std::collections::HashMap<String, std::collections::HashMap<String, String>>,
) {
    let mut pending = vec![dir.to_path_buf()];
    while let Some(current) = pending.pop() {
        let Ok(reader) = fs::read_dir(&current) else {
            continue;
        };
        for entry in reader.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            let text = path.to_string_lossy().replace('\\', "/");
            if let Some(namespace) = lang_namespace(&text, "zh_cn.json") {
                if let Ok(bytes) = fs::read(&path) {
                    merge_lang(&namespace, &bytes, map);
                }
            }
        }
    }
}

fn collect_lang_zip(
    bytes: &[u8],
    map: &mut std::collections::HashMap<String, std::collections::HashMap<String, String>>,
) {
    let Ok(mut archive) = zip::ZipArchive::new(Cursor::new(bytes)) else {
        return;
    };
    let names: Vec<String> = (0..archive.len())
        .filter_map(|index| archive.name_for_index(index).map(str::to_string))
        .collect();
    for name in names {
        let Some(namespace) = lang_namespace(&name, "zh_cn.json") else {
            continue;
        };
        let Ok(mut file) = archive.by_name(&name) else {
            continue;
        };
        let mut contents = Vec::new();
        if file.read_to_end(&mut contents).is_err() {
            continue;
        }
        merge_lang(&namespace, &contents, map);
    }
}

fn merge_lang(
    namespace: &str,
    bytes: &[u8],
    map: &mut std::collections::HashMap<String, std::collections::HashMap<String, String>>,
) {
    let Ok(Value::Object(object)) = serde_json::from_slice::<Value>(bytes) else {
        return;
    };
    let slot = map.entry(namespace.to_string()).or_default();
    for (key, value) in object {
        if let Some(text) = value.as_str() {
            if !text.trim().is_empty() {
                slot.insert(key, text.to_string());
            }
        }
    }
}

fn scan_mods(
    root: &Path,
    sources: &mut Vec<SourceSummary>,
    segments: &mut Vec<Segment>,
    installed: &std::collections::HashMap<String, std::collections::HashMap<String, String>>,
) -> Result<(), String> {
    let mods = root.join("mods");
    if !mods.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(&mods).map_err(|err| err.to_string())? {
        let entry = entry.map_err(|err| err.to_string())?;
        let path = entry.path();
        if path.extension().and_then(|item| item.to_str()) != Some("jar") {
            continue;
        }
        let before = segments.len();
        if let Ok(archive) = fs::read(&path) {
            scan_jar(&archive, segments, installed)?;
        }
        push_summary(sources, "mod_lang", &path, &segments[before..]);
    }
    Ok(())
}

fn scan_jar(
    bytes: &[u8],
    segments: &mut Vec<Segment>,
    installed: &std::collections::HashMap<String, std::collections::HashMap<String, String>>,
) -> Result<(), String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|err| err.to_string())?;
    let names: Vec<String> = (0..archive.len())
        .filter_map(|index| archive.name_for_index(index).map(str::to_string))
        .collect();
    for name in names {
        if let Some(namespace) = lang_namespace(&name, "en_us.json") {
            let contents = {
                let mut file = archive.by_name(&name).map_err(|err| err.to_string())?;
                let mut contents = Vec::new();
                file.read_to_end(&mut contents)
                    .map_err(|err| err.to_string())?;
                contents
            };
            let zh_name = name.replace("en_us.json", "zh_cn.json");
            let existing = read_zip_strings(&mut archive, &zh_name);
            let empty = std::collections::HashMap::new();
            let extra = installed.get(&namespace).unwrap_or(&empty);
            push_json_lang(&contents, &namespace, &existing, extra, segments);
        } else if let Some(namespace) = lang_namespace(&name, "en_us.lang") {
            let mut file = archive.by_name(&name).map_err(|err| err.to_string())?;
            let mut text = String::new();
            file.read_to_string(&mut text)
                .map_err(|err| err.to_string())?;
            for line in text.lines() {
                let Some((key, value)) = line.split_once('=') else {
                    continue;
                };
                if value.trim().is_empty() {
                    continue;
                }
                let extra = installed.get(&namespace);
                let skip = extra.is_some_and(|map| {
                    map.get(key.trim())
                        .is_some_and(|text| !text.trim().is_empty())
                });
                segments.push(Segment {
                    text: value.trim().to_string(),
                    meta: serde_json::json!({ "op": "lang", "namespace": namespace, "key": key.trim() }).to_string(),
                    skip,
                });
            }
        }
    }
    Ok(())
}

fn scan_ftb(
    root: &Path,
    sources: &mut Vec<SourceSummary>,
    segments: &mut Vec<Segment>,
) -> Result<(), String> {
    let lang = root.join("config/ftbquests/quests/lang");
    if !lang.is_dir() {
        return Ok(());
    }
    let single = lang.join("en_us.snbt");
    if single.is_file() {
        let before = segments.len();
        push_snbt(
            &single,
            &fs::read_to_string(&single).map_err(|err| err.to_string())?,
            segments,
        )?;
        push_summary(sources, "ftb", &single, &segments[before..]);
    }
    let dir = lang.join("en_us");
    if dir.is_dir() {
        for entry in fs::read_dir(&dir).map_err(|err| err.to_string())? {
            let path = entry.map_err(|err| err.to_string())?.path();
            if path.extension().and_then(|item| item.to_str()) != Some("snbt") {
                continue;
            }
            let before = segments.len();
            push_snbt(
                &path,
                &fs::read_to_string(&path).map_err(|err| err.to_string())?,
                segments,
            )?;
            push_summary(sources, "ftb", &path, &segments[before..]);
        }
    }
    Ok(())
}

fn push_snbt(path: &Path, text: &str, segments: &mut Vec<Segment>) -> Result<(), String> {
    let file = path.display().to_string();
    for (key, value) in snbt::parse(text)? {
        match value {
            SnbtValue::String(text) => segments.push(Segment {
                text,
                meta: serde_json::json!({ "op": "ftb", "file": file, "key": key }).to_string(),
                skip: false,
            }),
            SnbtValue::List(items) => {
                for (index, text) in items.into_iter().enumerate() {
                    segments.push(Segment {
                        text,
                        meta: serde_json::json!({ "op": "ftb", "file": file, "key": key, "index": index }).to_string(),
                        skip: false,
                    });
                }
            }
        }
    }
    Ok(())
}

fn scan_patchouli_tree(
    root: &Path,
    sources: &mut Vec<SourceSummary>,
    segments: &mut Vec<Segment>,
) -> Result<(), String> {
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(reader) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in reader.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            let text = path.to_string_lossy().replace('\\', "/");
            if text.contains("patchouli_books")
                && text.contains("/en_us/")
                && text.ends_with(".json")
            {
                let before = segments.len();
                if let Ok(bytes) = fs::read(&path) {
                    if let Ok(value) = serde_json::from_slice::<Value>(&bytes) {
                        let relative = path
                            .strip_prefix(root)
                            .unwrap_or(&path)
                            .to_string_lossy()
                            .replace('\\', "/");
                        walk_patchouli(&relative, &value, "", segments);
                    }
                }
                push_summary(sources, "patchouli", &path, &segments[before..]);
            }
        }
    }
    Ok(())
}

fn walk_patchouli(path: &str, value: &Value, pointer: &str, segments: &mut Vec<Segment>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                let next = format!("{pointer}/{key}");
                if matches!(key.as_str(), "name" | "description" | "title" | "text") {
                    push_patchouli_text(path, &next, child, segments);
                } else if key != "clickEvent" && key != "hoverEvent" {
                    walk_patchouli(path, child, &next, segments);
                }
            }
        }
        Value::Array(items) => {
            for (index, child) in items.iter().enumerate() {
                walk_patchouli(path, child, &format!("{pointer}/{index}"), segments);
            }
        }
        _ => {}
    }
}

fn push_patchouli_text(path: &str, pointer: &str, value: &Value, segments: &mut Vec<Segment>) {
    match value {
        Value::String(text) if !text.trim().is_empty() => segments.push(Segment {
            text: text.clone(),
            meta: serde_json::json!({ "op": "patchouli", "path": path, "pointer": pointer })
                .to_string(),
            skip: false,
        }),
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                if let Some(text) = item.as_str() {
                    segments.push(Segment {
                        text: text.to_string(),
                        meta: serde_json::json!({ "op": "patchouli", "path": path, "pointer": format!("{pointer}/{index}") }).to_string(),
                        skip: false,
                    });
                }
            }
        }
        _ => {}
    }
}

fn push_json_lang(
    bytes: &[u8],
    namespace: &str,
    existing: &std::collections::HashMap<String, String>,
    installed: &std::collections::HashMap<String, String>,
    segments: &mut Vec<Segment>,
) {
    let Ok(value) = serde_json::from_slice::<Value>(bytes) else {
        return;
    };
    let Some(map) = value.as_object() else {
        return;
    };
    for (key, item) in map {
        let Some(text) = item.as_str() else {
            continue;
        };
        if text.trim().is_empty() {
            continue;
        }
        let skip = existing
            .get(key)
            .is_some_and(|item| !item.trim().is_empty())
            || installed
                .get(key)
                .is_some_and(|item| !item.trim().is_empty());
        segments.push(Segment {
            text: text.to_string(),
            meta: serde_json::json!({ "op": "lang", "namespace": namespace, "key": key })
                .to_string(),
            skip,
        });
    }
}

fn lang_namespace(path: &str, suffix: &str) -> Option<String> {
    let marker = "assets/";
    let index = path.find(marker)?;
    let rest = &path[index + marker.len()..];
    let (namespace, tail) = rest.split_once('/')?;
    if tail == format!("lang/{suffix}") {
        Some(namespace.to_string())
    } else {
        None
    }
}

fn read_zip_strings(
    archive: &mut zip::ZipArchive<Cursor<&[u8]>>,
    name: &str,
) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    let Ok(mut file) = archive.by_name(name) else {
        return map;
    };
    let mut bytes = Vec::new();
    if file.read_to_end(&mut bytes).is_err() {
        return map;
    }
    if let Ok(Value::Object(object)) = serde_json::from_slice(&bytes) {
        for (key, value) in object {
            if let Some(text) = value.as_str() {
                map.insert(key, text.to_string());
            }
        }
    }
    map
}

fn push_summary(sources: &mut Vec<SourceSummary>, kind: &str, path: &Path, segments: &[Segment]) {
    if segments.is_empty() {
        return;
    }
    sources.push(SourceSummary {
        kind: kind.into(),
        path: path.display().to_string(),
        entries: segments.len(),
        translated: segments.iter().filter(|item| item.skip).count(),
    });
}

fn frequent_terms(segments: &[Segment]) -> Vec<String> {
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    for segment in segments {
        for word in segment.text.split(|ch: char| !ch.is_ascii_alphabetic()) {
            if word.len() >= 4 && word.chars().next().is_some_and(|ch| ch.is_uppercase()) {
                *counts.entry(word.to_string()).or_default() += 1;
            }
        }
    }
    let mut terms: Vec<_> = counts
        .into_iter()
        .filter(|(_, count)| *count >= 3)
        .map(|(word, _)| word)
        .collect();
    terms.sort();
    terms
}

fn write_ftb(
    _root: &Path,
    files: &std::collections::BTreeMap<String, Vec<(String, SnbtValue)>>,
) -> Result<(), String> {
    for (source, entries) in files {
        let source_path = PathBuf::from(source);
        let target = if source_path.ends_with("en_us.snbt") {
            source_path.with_file_name("zh_cn.snbt")
        } else {
            let text = source_path.to_string_lossy().replace("/en_us/", "/zh_cn/");
            PathBuf::from(text)
        };
        if target.exists() {
            let backup = target.with_extension("snbt.bak");
            let _ = fs::copy(&target, backup);
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        let mut merged = if target.exists() {
            snbt::parse(&fs::read_to_string(&target).unwrap_or_default()).unwrap_or_default()
        } else {
            Vec::new()
        };
        for (key, value) in entries {
            if let Some(slot) = merged.iter_mut().find(|(item, _)| item == key) {
                slot.1 = value.clone();
            } else {
                merged.push((key.clone(), value.clone()));
            }
        }
        fs::write(&target, snbt::write(&merged)).map_err(|err| err.to_string())?;
    }
    Ok(())
}

fn write_resource_pack(
    root: &Path,
    lang: &std::collections::BTreeMap<String, Map<String, Value>>,
    patchouli: &[(String, String, Value)],
    pack_format: u32,
) -> Result<PathBuf, String> {
    let path = root.join("resourcepacks/congmiao-zh_cn.zip");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let mut writer =
        zip::ZipWriter::new(std::fs::File::create(&path).map_err(|err| err.to_string())?);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    writer
        .start_file("pack.mcmeta", options)
        .map_err(|err| err.to_string())?;
    let meta = serde_json::json!({
        "pack": { "pack_format": pack_format, "description": "从喵翻译生成的中文资源包" }
    });
    writer
        .write_all(
            serde_json::to_string_pretty(&meta)
                .unwrap_or_default()
                .as_bytes(),
        )
        .map_err(|err| err.to_string())?;
    for (namespace, map) in lang {
        writer
            .start_file(format!("assets/{namespace}/lang/zh_cn.json"), options)
            .map_err(|err| err.to_string())?;
        writer
            .write_all(
                serde_json::to_string_pretty(map)
                    .unwrap_or_default()
                    .as_bytes(),
            )
            .map_err(|err| err.to_string())?;
    }
    for (relative, _, value) in patchouli {
        let target = relative.replace("/en_us/", "/zh_cn/");
        writer
            .start_file(target, options)
            .map_err(|err| err.to_string())?;
        writer
            .write_all(
                serde_json::to_string_pretty(value)
                    .unwrap_or_default()
                    .as_bytes(),
            )
            .map_err(|err| err.to_string())?;
    }
    writer.finish().map_err(|err| err.to_string())?;
    Ok(path)
}

fn set_pointer(value: &mut Value, pointer: &str, text: &str) {
    let mut current = value;
    let parts: Vec<&str> = pointer.split('/').filter(|item| !item.is_empty()).collect();
    for (index, part) in parts.iter().enumerate() {
        let last = index + 1 == parts.len();
        if let Ok(number) = part.parse::<usize>() {
            let Some(array) = current.as_array_mut() else {
                return;
            };
            let Some(child) = array.get_mut(number) else {
                return;
            };
            if last {
                *child = Value::String(text.to_string());
                return;
            }
            current = child;
        } else {
            let Some(object) = current.as_object_mut() else {
                return;
            };
            if last {
                object.insert((*part).to_string(), Value::String(text.to_string()));
                return;
            }
            if !object.contains_key(*part) {
                return;
            }
            current = object.get_mut(*part).expect("键刚刚确认存在");
        }
    }
}

fn read_maybe(root: &Path, relative: &str) -> Option<Vec<u8>> {
    fs::read(root.join(relative)).ok()
}

#[cfg(test)]
mod tests {
    use super::{apply_instance, scan_instance};
    use std::io::Write;

    #[test]
    fn scans_a_jar_ftb_file_and_patchouli_page() {
        let root = std::env::temp_dir().join(format!("congmiao-mc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("mods")).unwrap();
        std::fs::create_dir_all(root.join("config/ftbquests/quests/lang")).unwrap();
        std::fs::create_dir_all(
            root.join("resourcepacks/book/assets/demo/patchouli_books/guide/en_us/entries"),
        )
        .unwrap();
        let mut jar =
            zip::ZipWriter::new(std::fs::File::create(root.join("mods/demo.jar")).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        jar.start_file("assets/demo/lang/en_us.json", options)
            .unwrap();
        jar.write_all(
            r#"{"item.demo.apple":"§aApple %s","item.demo.done":"Done","item.demo.pack":"Packed"}"#
                .as_bytes(),
        )
        .unwrap();
        jar.start_file("assets/demo/lang/zh_cn.json", options)
            .unwrap();
        jar.write_all(r#"{"item.demo.done":"完成"}"#.as_bytes())
            .unwrap();
        jar.finish().unwrap();
        std::fs::create_dir_all(root.join("resourcepacks/extra/assets/demo/lang")).unwrap();
        std::fs::write(
            root.join("resourcepacks/extra/assets/demo/lang/zh_cn.json"),
            r#"{"item.demo.pack":"已打包"}"#,
        )
        .unwrap();
        std::fs::write(
            root.join("config/ftbquests/quests/lang/en_us.snbt"),
            "{\n\t\"quest.title\": \"Visit the Nether\"\n}\n",
        )
        .unwrap();
        std::fs::write(
            root.join(
                "resourcepacks/book/assets/demo/patchouli_books/guide/en_us/entries/apple.json",
            ),
            r#"{"name":"Apple","pages":[{"text":"Eat $(item)"}],"clickEvent":{"action":"open"}}"#,
        )
        .unwrap();
        let scan = scan_instance(&root).unwrap();
        assert!(scan.sources.iter().any(|item| item.kind == "mod_lang"));
        assert!(scan.sources.iter().any(|item| item.kind == "ftb"));
        assert!(scan.sources.iter().any(|item| item.kind == "patchouli"));
        let apple = scan
            .segments
            .iter()
            .find(|item| item.text.contains("Apple"))
            .unwrap();
        assert!(!apple.skip);
        let done = scan
            .segments
            .iter()
            .find(|item| item.text == "Done")
            .unwrap();
        assert!(done.skip);
        let packed = scan
            .segments
            .iter()
            .find(|item| item.text == "Packed")
            .unwrap();
        assert!(packed.skip);
        let translations: Vec<_> = scan
            .segments
            .iter()
            .map(|item| {
                if item.skip {
                    item.text.clone()
                } else {
                    format!("译:{}", item.text)
                }
            })
            .collect();
        let pack = apply_instance(&root, &scan.segments, &translations, 34).unwrap();
        assert!(pack.ends_with("congmiao-zh_cn.zip"));
        let zh =
            std::fs::read_to_string(root.join("config/ftbquests/quests/lang/zh_cn.snbt")).unwrap();
        assert!(zh.contains("译:Visit the Nether"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
