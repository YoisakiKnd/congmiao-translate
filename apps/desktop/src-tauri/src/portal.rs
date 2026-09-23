pub fn file_from_monitor(text: &str) -> Option<String> {
    if response_cancelled(text) {
        return None;
    }
    let marker = "file://";
    let start = text.find(marker)?;
    let rest = &text[start + marker.len()..];
    let end = rest.find('"').unwrap_or(rest.len());
    let path = percent_decode(rest[..end].trim());
    if path.is_empty() {
        None
    } else {
        Some(path)
    }
}

fn response_cancelled(text: &str) -> bool {
    text.contains("uint32 1") && !text.contains("file://")
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
    use super::file_from_monitor;

    #[test]
    fn reads_the_uri_from_a_portal_response_signal() {
        let sample = r#"
signal time=1.0 path=/org/freedesktop/portal/desktop/request/1/congmiao; interface=org.freedesktop.portal.Request; member=Response
   uint32 0
   array [
      dict entry(
         string "uri"
         variant string "file:///tmp/shot%20.png"
      )
   ]
"#;
        assert_eq!(file_from_monitor(sample).as_deref(), Some("/tmp/shot .png"));
    }

    #[test]
    fn cancelled_response_has_no_file() {
        let sample = "member=Response\n   uint32 1\n   array [\n   ]\n";
        assert_eq!(file_from_monitor(sample), None);
    }
}
