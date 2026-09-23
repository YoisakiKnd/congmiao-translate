use crate::batch::split_long;
use crate::batch::MAX_PACK_CHARS;
use crate::textfmt::OutputMode;

#[derive(Debug)]
enum Block {
    Keep(String),
    Prose(String),
}

pub fn extract(text: &str) -> Vec<String> {
    blocks(text)
        .into_iter()
        .filter_map(|block| match block {
            Block::Prose(text) => Some(text),
            Block::Keep(_) => None,
        })
        .flat_map(|text| split_long(&mask_inline(&text).0, MAX_PACK_CHARS))
        .collect()
}

pub fn rebuild(original: &str, translated: &[String], mode: OutputMode) -> Result<String, String> {
    let mut cursor = 0;
    let mut out = Vec::new();
    for block in blocks(original) {
        match block {
            Block::Keep(text) => out.push(text),
            Block::Prose(text) => {
                let (masked, slots) = mask_inline(&text);
                let parts = split_long(&masked, MAX_PACK_CHARS);
                let mut joined = String::new();
                for _part in &parts {
                    let piece = translated.get(cursor).ok_or("译文数量和原文段落不一致")?;
                    cursor += 1;
                    joined.push_str(piece);
                }
                let restored = crate::mask::restore(&joined, &slots)?;
                out.push(match mode {
                    OutputMode::TranslatedOnly => restored.clone(),
                    OutputMode::Bilingual => format!("{text}\n\n{restored}"),
                });
            }
        }
    }
    if cursor != translated.len() {
        return Err("译文数量和原文段落不一致".into());
    }
    Ok(out.join("\n\n"))
}

fn blocks(text: &str) -> Vec<Block> {
    let mut lines = text.lines().peekable();
    let mut blocks = Vec::new();
    if lines.peek().is_some_and(|line| line.trim() == "---") {
        let mut front = vec![lines.next().unwrap().to_string()];
        for line in lines.by_ref() {
            let done = line.trim() == "---";
            front.push(line.to_string());
            if done {
                break;
            }
        }
        blocks.push(Block::Keep(front.join("\n")));
    }
    let mut prose = Vec::new();
    let flush = |prose: &mut Vec<String>, blocks: &mut Vec<Block>| {
        if !prose.is_empty() {
            blocks.push(Block::Prose(prose.join("\n")));
            prose.clear();
        }
    };
    while let Some(line) = lines.next() {
        if line.trim().starts_with("```") {
            flush(&mut prose, &mut blocks);
            let mut code = vec![line.to_string()];
            for next in lines.by_ref() {
                let done = next.trim().starts_with("```");
                code.push(next.to_string());
                if done {
                    break;
                }
            }
            blocks.push(Block::Keep(code.join("\n")));
            continue;
        }
        if line.trim().is_empty() {
            flush(&mut prose, &mut blocks);
            continue;
        }
        prose.push(line.to_string());
    }
    flush(&mut prose, &mut blocks);
    blocks
}

fn mask_inline(text: &str) -> (String, Vec<String>) {
    let mut slots = Vec::new();
    let mut out = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '`' {
            let start = index;
            index += 1;
            while index < chars.len() && chars[index] != '`' {
                index += 1;
            }
            if index < chars.len() {
                index += 1;
            }
            let token = format!("⟦{}⟧", slots.len());
            slots.push(chars[start..index].iter().collect());
            out.push_str(&token);
            continue;
        }
        out.push(chars[index]);
        index += 1;
    }
    let pattern = regex::Regex::new(r"\]\(([^)]+)\)").expect("链接");
    let mut linked = String::new();
    let mut last = 0;
    for found in pattern.find_iter(&out) {
        linked.push_str(&out[last..found.start()]);
        let url = &out[found.start() + 2..found.end() - 1];
        let token = format!("⟦{}⟧", slots.len());
        slots.push(url.to_string());
        linked.push_str(&format!("]({token})"));
        last = found.end();
    }
    linked.push_str(&out[last..]);
    (linked, slots)
}

#[cfg(test)]
mod tests {
    use super::{extract, rebuild};
    use crate::textfmt::OutputMode;

    #[test]
    fn skips_front_matter_and_code_and_keeps_links() {
        let source = "---\ntitle: Hello\n---\n\nSee [docs](https://example.com) and `code`.\n\n```\nfn main() {}\n```\n";
        let parts = extract(source);
        assert_eq!(parts.len(), 1);
        assert!(!parts[0].contains("https://example.com"));
        assert!(!parts[0].contains("fn main"));
        let translated = vec![parts[0].replace("See", "见").replace("and", "和")];
        let output = rebuild(source, &translated, OutputMode::TranslatedOnly).unwrap();
        assert!(output.contains("https://example.com"));
        assert!(output.contains("`code`"));
        assert!(output.contains("fn main()"));
        assert!(output.contains("title: Hello"));
    }
}
