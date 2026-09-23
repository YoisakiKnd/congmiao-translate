#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnbtValue {
    String(String),
    List(Vec<String>),
}

pub fn parse(input: &str) -> Result<Vec<(String, SnbtValue)>, String> {
    let mut parser = Parser {
        chars: input.chars().collect(),
        index: 0,
    };
    parser.skip();
    parser.expect('{')?;
    let mut entries = Vec::new();
    loop {
        parser.skip();
        if parser.eat('}') {
            break;
        }
        if parser.eof() {
            return Err("SNBT 没有结束".into());
        }
        let key = parser.string_or_word()?;
        parser.skip();
        parser.expect(':')?;
        parser.skip();
        let value = parser.value()?;
        entries.push((key, value));
    }
    Ok(entries)
}

pub fn write(entries: &[(String, SnbtValue)]) -> String {
    let mut body = String::from("{\n");
    for (key, value) in entries {
        body.push('\t');
        body.push_str(&quote(key));
        body.push_str(": ");
        match value {
            SnbtValue::String(text) => body.push_str(&quote(text)),
            SnbtValue::List(items) => {
                body.push('[');
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        body.push_str(", ");
                    }
                    body.push_str(&quote(item));
                }
                body.push(']');
            }
        }
        body.push('\n');
    }
    body.push_str("}\n");
    body
}

struct Parser {
    chars: Vec<char>,
    index: usize,
}

impl Parser {
    fn eof(&self) -> bool {
        self.index >= self.chars.len()
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.index).copied()
    }

    fn skip(&mut self) {
        while matches!(self.peek(), Some(ch) if ch.is_whitespace()) {
            self.index += 1;
        }
    }

    fn eat(&mut self, expected: char) -> bool {
        if self.peek() == Some(expected) {
            self.index += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, expected: char) -> Result<(), String> {
        if self.eat(expected) {
            Ok(())
        } else {
            Err(format!("SNBT 在这里需要 {expected}"))
        }
    }

    fn value(&mut self) -> Result<SnbtValue, String> {
        if self.peek() == Some('[') {
            self.index += 1;
            let mut items = Vec::new();
            loop {
                self.skip();
                if self.eat(']') {
                    break;
                }
                items.push(self.quoted()?);
                self.skip();
                let _ = self.eat(',');
            }
            return Ok(SnbtValue::List(items));
        }
        Ok(SnbtValue::String(self.quoted()?))
    }

    fn string_or_word(&mut self) -> Result<String, String> {
        if self.peek() == Some('"') {
            self.quoted()
        } else {
            let start = self.index;
            while matches!(self.peek(), Some(ch) if !ch.is_whitespace() && ch != ':') {
                self.index += 1;
            }
            if start == self.index {
                return Err("SNBT 缺少键".into());
            }
            Ok(self.chars[start..self.index].iter().collect())
        }
    }

    fn quoted(&mut self) -> Result<String, String> {
        self.expect('"')?;
        let mut out = String::new();
        while let Some(ch) = self.peek() {
            self.index += 1;
            if ch == '\\' {
                let escaped = self.peek().ok_or("SNBT 转义不完整")?;
                self.index += 1;
                out.push(match escaped {
                    'n' => '\n',
                    't' => '\t',
                    'r' => '\r',
                    other => other,
                });
                continue;
            }
            if ch == '"' {
                return Ok(out);
            }
            out.push(ch);
        }
        Err("SNBT 字符串没有结束".into())
    }
}

fn quote(text: &str) -> String {
    let mut out = String::from("\"");
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::{parse, write, SnbtValue};

    #[test]
    fn roundtrips_multiline_strings_and_lists() {
        let source = "{\n\t\"title\": \"Hello\\nWorld\"\n\t\"lines\": [\"A\", \"B\"]\n}\n";
        let entries = parse(source).unwrap();
        assert_eq!(entries[0].1, SnbtValue::String("Hello\nWorld".into()));
        assert_eq!(entries[1].1, SnbtValue::List(vec!["A".into(), "B".into()]));
        let again = parse(&write(&entries)).unwrap();
        assert_eq!(again, entries);
    }
}
