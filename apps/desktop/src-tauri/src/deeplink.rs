use tauri::AppHandle;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Launch {
    ShowMain,
    Translate(String),
}

pub(crate) fn open_from_args(app: &AppHandle, args: &[String]) {
    let urls: Vec<&str> = args
        .iter()
        .map(String::as_str)
        .filter(|arg| arg.starts_with("congmiao:"))
        .collect();
    if urls.is_empty() {
        crate::show_main(app);
        return;
    }
    open_raw(app, &urls);
}

pub(crate) fn open_from_urls(app: &AppHandle, urls: &[impl std::fmt::Display]) {
    if urls.is_empty() {
        crate::show_main(app);
        return;
    }
    let raw: Vec<String> = urls.iter().map(|url| url.to_string()).collect();
    let borrowed: Vec<&str> = raw.iter().map(String::as_str).collect();
    open_raw(app, &borrowed);
}

fn open_raw(app: &AppHandle, urls: &[&str]) {
    let mut translated = false;
    for url in urls {
        if let Launch::Translate(text) = parse_congmiao(url) {
            crate::popup::open_at_cursor(app, &text, "translate");
            translated = true;
        }
    }
    if !translated {
        crate::show_main(app);
    }
}

pub(crate) fn parse_congmiao(raw: &str) -> Launch {
    let raw = raw.trim();
    if !raw.to_ascii_lowercase().starts_with("congmiao://") {
        return Launch::ShowMain;
    }
    let query = raw.split_once('?').map(|(_, query)| query).unwrap_or("");
    for pair in query.split('&') {
        let mut parts = pair.splitn(2, '=');
        let key = parts.next().unwrap_or("");
        let value = parts.next().unwrap_or("");
        if key == "text" {
            let text = percent_decode(value);
            if text.trim().is_empty() {
                return Launch::ShowMain;
            }
            return Launch::Translate(text);
        }
    }
    Launch::ShowMain
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(
                std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or(""),
                16,
            ) {
                out.push(byte);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::{parse_congmiao, Launch};

    #[test]
    fn translate_link_keeps_the_query_text() {
        assert_eq!(
            parse_congmiao("congmiao://translate?text=hello%20world"),
            Launch::Translate("hello world".into())
        );
    }

    #[test]
    fn link_without_text_opens_the_main_window() {
        assert_eq!(parse_congmiao("congmiao://translate"), Launch::ShowMain);
        assert_eq!(
            parse_congmiao("congmiao://translate?text="),
            Launch::ShowMain
        );
    }
}
