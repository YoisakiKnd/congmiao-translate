use std::fs;
use std::path::{Path, PathBuf};

use congmiao_core::{Error, NATIVE_HOST_NAME};

pub struct InstallRequest {
    pub chrome_extension_id: Option<String>,
    pub firefox_extension_id: Option<String>,
    pub host_path: PathBuf,
    pub dry_run: bool,
}

pub fn host_manifest(host_path: &Path, extension_id: &str) -> Result<String, Error> {
    chromium_manifest(host_path, extension_id)
}

pub fn firefox_host_manifest(host_path: &Path, extension_id: &str) -> Result<String, Error> {
    validate_firefox_id(extension_id)?;
    manifest_json(
        host_path,
        serde_json::json!({ "allowed_extensions": [extension_id] }),
    )
}

pub fn install_host(request: &InstallRequest) -> Result<Vec<PathBuf>, Error> {
    if request.chrome_extension_id.is_none() && request.firefox_extension_id.is_none() {
        return Err(Error::Io(
            "需要 Chrome 扩展 ID，或 Firefox / Waterfox 扩展 ID".into(),
        ));
    }
    let mut written = Vec::new();
    if let Some(extension_id) = &request.chrome_extension_id {
        let manifest = chromium_manifest(&request.host_path, extension_id)?;
        let paths = write_manifests(&chromium_dirs()?, &manifest, request.dry_run)?;
        if cfg!(target_os = "windows") && !request.dry_run {
            register_windows(extension_id, &paths)?;
        }
        written.extend(paths);
    }
    if let Some(extension_id) = &request.firefox_extension_id {
        let manifest = firefox_host_manifest(&request.host_path, extension_id)?;
        written.extend(write_manifests(
            &firefox_dirs()?,
            &manifest,
            request.dry_run,
        )?);
    }
    Ok(written)
}

fn chromium_manifest(host_path: &Path, extension_id: &str) -> Result<String, Error> {
    validate_extension_id(extension_id)?;
    manifest_json(
        host_path,
        serde_json::json!({
            "allowed_origins": [format!("chrome-extension://{extension_id}/")]
        }),
    )
}

fn manifest_json(host_path: &Path, extra: serde_json::Value) -> Result<String, Error> {
    let path = host_path
        .to_str()
        .ok_or_else(|| Error::Io("宿主程序路径含有无法编码的字符".into()))?;
    let mut value = serde_json::json!({
        "name": NATIVE_HOST_NAME,
        "description": "从喵翻译本机宿主",
        "path": path,
        "type": "stdio"
    });
    let object = value
        .as_object_mut()
        .ok_or_else(|| Error::Json("宿主清单不是对象".into()))?;
    let extra = extra
        .as_object()
        .ok_or_else(|| Error::Json("宿主清单附加字段不是对象".into()))?;
    for (key, item) in extra {
        object.insert(key.clone(), item.clone());
    }
    let mut raw =
        serde_json::to_string_pretty(&value).map_err(|err| Error::Json(err.to_string()))?;
    raw.push('\n');
    Ok(raw)
}

fn write_manifests(dirs: &[PathBuf], manifest: &str, dry_run: bool) -> Result<Vec<PathBuf>, Error> {
    if dry_run {
        return Ok(dirs.to_vec());
    }
    let mut written = Vec::new();
    let mut errors = Vec::new();
    for dir in dirs {
        let path = dir.join(format!("{NATIVE_HOST_NAME}.json"));
        if let Err(err) = fs::create_dir_all(dir).and_then(|_| fs::write(&path, manifest)) {
            errors.push(format!("{}: {err}", path.display()));
            continue;
        }
        written.push(path);
    }
    if written.is_empty() {
        return Err(Error::Io(format!(
            "没有写成任何宿主清单：{}",
            errors.join("；")
        )));
    }
    Ok(written)
}

fn register_windows(_extension_id: &str, manifests: &[PathBuf]) -> Result<(), Error> {
    let Some(manifest) = manifests.first() else {
        return Ok(());
    };
    let path = manifest
        .to_str()
        .ok_or_else(|| Error::Io("清单路径含有无法编码的字符".into()))?;
    let status = std::process::Command::new("reg")
        .args([
            "add",
            &format!("HKCU\\Software\\Google\\Chrome\\NativeMessagingHosts\\{NATIVE_HOST_NAME}"),
            "/ve",
            "/d",
            path,
            "/f",
        ])
        .status()
        .map_err(|err| Error::Io(err.to_string()))?;
    if status.success() {
        Ok(())
    } else {
        Err(Error::Io("写入 Chrome Native Messaging 注册表失败".into()))
    }
}

fn chromium_dirs() -> Result<Vec<PathBuf>, Error> {
    let home = dirs::home_dir().ok_or_else(|| Error::Io("无法定位用户目录".into()))?;
    if cfg!(target_os = "macos") {
        let base = home.join("Library/Application Support");
        Ok(vec![
            base.join("Google/Chrome/NativeMessagingHosts"),
            base.join("Microsoft Edge/NativeMessagingHosts"),
            base.join("BraveSoftware/Brave-Browser/NativeMessagingHosts"),
            base.join("Chromium/NativeMessagingHosts"),
        ])
    } else if cfg!(target_os = "windows") {
        let local = dirs::data_local_dir().unwrap_or_else(|| home.join("AppData").join("Local"));
        Ok(vec![local
            .join("congmiao-translate")
            .join("NativeMessagingHosts")])
    } else {
        Ok(linux_chromium_dirs(&home))
    }
}

pub fn linux_chromium_dirs(home: &Path) -> Vec<PathBuf> {
    vec![
        home.join(".config/google-chrome/NativeMessagingHosts"),
        home.join(".config/chromium/NativeMessagingHosts"),
        home.join(".config/microsoft-edge/NativeMessagingHosts"),
        home.join(".config/BraveSoftware/Brave-Browser/NativeMessagingHosts"),
    ]
}

pub fn linux_firefox_dirs(home: &Path) -> Vec<PathBuf> {
    vec![
        home.join(".mozilla/native-messaging-hosts"),
        home.join(".config/mozilla/native-messaging-hosts"),
    ]
}

fn firefox_dirs() -> Result<Vec<PathBuf>, Error> {
    let home = dirs::home_dir().ok_or_else(|| Error::Io("无法定位用户目录".into()))?;
    if cfg!(target_os = "macos") {
        let base = home.join("Library/Application Support");
        Ok(vec![
            base.join("Mozilla/NativeMessagingHosts"),
            base.join("Waterfox/NativeMessagingHosts"),
        ])
    } else if cfg!(target_os = "windows") {
        let roaming = dirs::config_dir().unwrap_or_else(|| home.join("AppData").join("Roaming"));
        Ok(vec![
            roaming.join("Mozilla/NativeMessagingHosts"),
            roaming.join("Waterfox/NativeMessagingHosts"),
        ])
    } else {
        Ok(linux_firefox_dirs(&home))
    }
}

fn validate_extension_id(id: &str) -> Result<(), Error> {
    if id.len() == 32 && id.bytes().all(|byte| (b'a'..=b'p').contains(&byte)) {
        Ok(())
    } else {
        Err(Error::Io(
            "扩展 ID 必须是 32 位、只含 a 到 p 的 Chrome 扩展标识".into(),
        ))
    }
}

fn validate_firefox_id(id: &str) -> Result<(), Error> {
    let email_like = id.contains('@')
        && !id.starts_with('@')
        && !id.ends_with('@')
        && id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '@' | '.' | '_' | '-'));
    if email_like {
        Ok(())
    } else {
        Err(Error::Io(
            "Firefox 扩展 ID 必须类似 translate@congmiao.app".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_pins_the_host_and_extension_origin() {
        let id = "abcdefghijklmnopabcdefghijklmnop";
        let json = host_manifest(Path::new("/tmp/congmiao-host"), id).unwrap();
        assert!(json.contains("/tmp/congmiao-host"));
        assert!(json.contains("chrome-extension://abcdefghijklmnopabcdefghijklmnop/"));
        assert!(json.contains(NATIVE_HOST_NAME));
    }

    #[test]
    fn rejects_a_malformed_extension_id() {
        let err = host_manifest(Path::new("/tmp/congmiao-host"), "not-an-id").unwrap_err();
        assert!(err.to_string().contains("扩展 ID"));
    }

    #[test]
    fn linux_host_directories_match_the_browser_layout() {
        let home = Path::new("/home/me");
        let chromium = linux_chromium_dirs(home);
        assert!(chromium
            .iter()
            .any(|path| { path.ends_with(".config/google-chrome/NativeMessagingHosts") }));
        let firefox = linux_firefox_dirs(home);
        assert!(firefox
            .iter()
            .any(|path| path.ends_with(".mozilla/native-messaging-hosts")));
    }

    #[test]
    fn firefox_manifest_allows_the_pinned_extension() {
        let json = firefox_host_manifest(
            Path::new("/tmp/congmiao-host"),
            congmiao_core::FIREFOX_EXTENSION_ID,
        )
        .unwrap();
        assert!(json.contains("allowed_extensions"));
        assert!(json.contains(congmiao_core::FIREFOX_EXTENSION_ID));
        assert!(!json.contains("allowed_origins"));
    }

    #[cfg(windows)]
    #[test]
    fn windows_native_host_registry_roundtrip() {
        let manifest =
            std::env::temp_dir().join(format!("congmiao-host-{}.json", std::process::id()));
        let key = format!(r"HKCU\Software\Google\Chrome\NativeMessagingHosts\{NATIVE_HOST_NAME}");
        let _ = std::process::Command::new("reg")
            .args(["delete", &key, "/f"])
            .status();
        register_windows("abcdefghijklmnopabcdefghijklmnop", &[manifest.clone()]).unwrap();
        let output = std::process::Command::new("reg")
            .args(["query", &key, "/ve"])
            .output()
            .expect("reg query");
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(output.status.success(), "{text}");
        assert!(text.contains(&manifest.display().to_string()));
        let deleted = std::process::Command::new("reg")
            .args(["delete", &key, "/f"])
            .status()
            .expect("reg delete");
        assert!(deleted.success());
        let gone = std::process::Command::new("reg")
            .args(["query", &key])
            .status()
            .expect("reg query after delete");
        assert!(!gone.success());
    }
}
