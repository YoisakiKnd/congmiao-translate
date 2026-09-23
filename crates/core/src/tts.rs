pub fn youdao_voice_url(text: &str) -> String {
    format!(
        "https://dict.youdao.com/dictvoice?audio={}&type=2",
        urlencoding::encode(text)
    )
}

pub fn google_voice_url(text: &str, lang: &str) -> String {
    let lang = if lang == "zh" { "zh-CN" } else { lang };
    format!(
        "https://translate.googleapis.com/translate_tts?ie=UTF-8&client=gtx&tl={lang}&q={}",
        urlencoding::encode(text)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voice_urls_encode_the_text() {
        assert!(youdao_voice_url("你好").contains("%E4%BD%A0%E5%A5%BD"));
        assert!(google_voice_url("hello", "zh").contains("tl=zh-CN"));
    }
}
