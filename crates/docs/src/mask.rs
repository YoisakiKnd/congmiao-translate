use regex::Regex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Masked {
    pub text: String,
    pub slots: Vec<String>,
}

pub fn protect(input: &str, glossary: &[(&str, &str)]) -> Masked {
    let mut slots = Vec::new();
    let mut text = mask_patterns(input, &mut slots);
    let mut entries: Vec<(&str, &str)> = glossary
        .iter()
        .copied()
        .filter(|(source, _)| !source.is_empty())
        .collect();
    entries.sort_by_key(|(source, _)| std::cmp::Reverse(source.len()));
    for (source, target) in entries {
        while let Some(index) = text.find(source) {
            let token = format!("⟦{}⟧", slots.len());
            slots.push(target.to_string());
            text.replace_range(index..index + source.len(), &token);
        }
    }
    Masked { text, slots }
}

pub fn restore(text: &str, slots: &[String]) -> Result<String, String> {
    let mut out = String::new();
    let mut rest = text;
    let mut seen = vec![false; slots.len()];
    while let Some(start) = rest.find('⟦') {
        out.push_str(&rest[..start]);
        let Some(end) = rest[start..].find('⟧') else {
            return Err("占位符没有闭合".into());
        };
        let token = &rest[start + '⟦'.len_utf8()..start + end];
        let index: usize = token
            .parse()
            .map_err(|_| format!("无法识别占位符 ⟦{token}⟧"))?;
        let slot = slots
            .get(index)
            .ok_or_else(|| format!("占位符 ⟦{index}⟧ 不存在"))?;
        seen[index] = true;
        out.push_str(slot);
        rest = &rest[start + end + '⟧'.len_utf8()..];
    }
    out.push_str(rest);
    if seen.iter().any(|item| !item) {
        return Err("译文丢掉了占位符".into());
    }
    Ok(out)
}

fn mask_patterns(input: &str, slots: &mut Vec<String>) -> String {
    let pattern = Regex::new(
        r"(?x)
        §[0-9a-fk-orA-FK-OR]
        | &[0-9a-fk-orA-FK-OR]
        | %\d+\$[-+0\#\ ]?\d*(?:\.\d+)?[sdif]
        | %[-+0\#\ ]?\d*(?:\.\d+)?[sdif]
        | \{\d+\}
        | \\n
        | \$\([^)]*\)
        | \{image:[^}]*\}
        ",
    )
    .expect("占位符表达式");
    let mut out = String::new();
    let mut last = 0;
    for found in pattern.find_iter(input) {
        out.push_str(&input[last..found.start()]);
        out.push_str(&format!("⟦{}⟧", slots.len()));
        slots.push(found.as_str().to_string());
        last = found.end();
    }
    out.push_str(&input[last..]);
    out
}

#[cfg(test)]
mod tests {
    use super::{protect, restore};

    #[test]
    fn keeps_color_codes_placeholders_and_glossary() {
        let masked = protect(
            "§aHello %s {0} Nether $(br) {image:a.png}",
            &[("Nether", "下界")],
        );
        assert!(!masked.text.contains("Nether"));
        assert!(!masked.text.contains("§a"));
        let restored = restore(&masked.text.replace("Hello", "你好"), &masked.slots).unwrap();
        assert_eq!(restored, "§a你好 %s {0} 下界 $(br) {image:a.png}");
    }

    #[test]
    fn rejects_a_dropped_token() {
        let masked = protect("§aHi", &[]);
        assert!(restore("你好", &masked.slots).is_err());
    }
}
