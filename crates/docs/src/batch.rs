pub const MAX_PACK_CHARS: usize = 8_000;

pub fn packs(texts: &[String], max_chars: usize) -> Vec<Vec<usize>> {
    let mut packs = Vec::new();
    let mut current = Vec::new();
    let mut size = 0usize;
    for (index, text) in texts.iter().enumerate() {
        let len = text.chars().count().max(1);
        if !current.is_empty() && size + len > max_chars {
            packs.push(current);
            current = Vec::new();
            size = 0;
        }
        current.push(index);
        size += len;
    }
    if !current.is_empty() {
        packs.push(current);
    }
    packs
}

pub fn numbered(items: &[&str]) -> String {
    items
        .iter()
        .enumerate()
        .map(|(index, text)| format!("{}. {}", index + 1, text.replace('\n', " ")))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn parse_numbered(text: &str, count: usize) -> Option<Vec<String>> {
    let mut lines = vec![None; count];
    for line in text.lines() {
        let line = line.trim();
        let Some((number, rest)) = line.split_once('.') else {
            continue;
        };
        let Ok(index) = number.trim().parse::<usize>() else {
            continue;
        };
        if (1..=count).contains(&index) {
            lines[index - 1] = Some(rest.trim().to_string());
        }
    }
    if lines.iter().any(|item| item.is_none()) {
        return None;
    }
    Some(lines.into_iter().flatten().collect())
}

pub fn split_long(text: &str, max_chars: usize) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max_chars {
        return vec![text.to_string()];
    }
    chars
        .chunks(max_chars)
        .map(|chunk| chunk.iter().collect())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{numbered, packs, parse_numbered};

    #[test]
    fn packs_by_character_budget_and_roundtrips_numbers() {
        let texts = vec!["甲".repeat(5), "乙".repeat(5), "丙".into()];
        let packed = packs(&texts, 8);
        assert_eq!(packed, vec![vec![0], vec![1, 2]]);
        let body = numbered(&["hello", "world"]);
        assert_eq!(
            parse_numbered(&format!("备注\n{body}"), 2).as_deref(),
            Some(["hello".to_string(), "world".to_string()].as_slice())
        );
        assert!(parse_numbered("1. only", 2).is_none());
    }
}
