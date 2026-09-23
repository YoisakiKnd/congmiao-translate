use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GlossaryEntry {
    pub source: String,
    pub target: String,
}

pub fn exact_match<'a>(text: &str, glossary: &'a [GlossaryEntry]) -> Option<&'a str> {
    let trimmed = text.trim();
    glossary
        .iter()
        .find(|entry| entry.source == trimmed)
        .map(|entry| entry.target.as_str())
}

pub fn fingerprint(glossary: &[GlossaryEntry]) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    for entry in glossary {
        entry.source.hash(&mut hasher);
        entry.target.hash(&mut hasher);
    }
    hasher.finish()
}

pub fn parse_lines(text: &str) -> Result<Vec<GlossaryEntry>, String> {
    let mut entries = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Some((source, target)) = line.split_once('=') else {
            return Err(format!("第 {} 行缺少等号", index + 1));
        };
        let source = source.trim();
        let target = target.trim();
        if source.is_empty() || target.is_empty() {
            return Err(format!("第 {} 行的术语或译文为空", index + 1));
        }
        entries.push(GlossaryEntry {
            source: source.to_string(),
            target: target.to_string(),
        });
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_whole_trimmed_string_only() {
        let glossary = vec![GlossaryEntry {
            source: "hello".into(),
            target: "你好".into(),
        }];
        assert_eq!(exact_match("  hello ", &glossary), Some("你好"));
        assert_eq!(exact_match("hello world", &glossary), None);
    }
}
