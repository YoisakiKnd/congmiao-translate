mod batch;
mod docx;
mod epub;
mod markdown;
mod mask;
mod minecraft;
mod pdf;
mod snbt;
mod textfmt;

use std::path::{Path, PathBuf};

pub use batch::{numbered, packs, parse_numbered, split_long, MAX_PACK_CHARS};
pub use mask::{protect, restore, Masked};
pub use minecraft::{apply_instance, builtin_terms, scan_instance, Scan, Segment as McSegment};
pub use textfmt::{sibling_output, OutputMode};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Piece {
    pub text: String,
}

pub fn extract_path(path: &Path) -> Result<(String, Vec<Piece>), String> {
    let bytes = std::fs::read(path).map_err(|err| err.to_string())?;
    let name = path
        .extension()
        .and_then(|item| item.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match name.as_str() {
        "txt" => Ok((
            "txt".into(),
            textfmt::paragraphs(&String::from_utf8_lossy(&bytes))
                .into_iter()
                .map(|text| Piece { text })
                .collect(),
        )),
        "md" | "markdown" => Ok((
            "md".into(),
            markdown::extract(&String::from_utf8_lossy(&bytes))
                .into_iter()
                .map(|text| Piece { text })
                .collect(),
        )),
        "docx" => Ok((
            "docx".into(),
            docx::extract(&bytes)?
                .into_iter()
                .map(|text| Piece { text })
                .collect(),
        )),
        "epub" => Ok((
            "epub".into(),
            epub::extract(&bytes)?
                .into_iter()
                .map(|text| Piece { text })
                .collect(),
        )),
        "pdf" => Ok((
            "pdf".into(),
            pdf::extract(&bytes)?
                .into_iter()
                .map(|text| Piece { text })
                .collect(),
        )),
        other => Err(format!("还不支持 {other} 文件")),
    }
}

pub fn rebuild_path(
    path: &Path,
    kind: &str,
    translated: &[String],
    mode: OutputMode,
    target_lang: &str,
) -> Result<PathBuf, String> {
    let bytes = std::fs::read(path).map_err(|err| err.to_string())?;
    let (extension, output) = match kind {
        "txt" => {
            let original = textfmt::paragraphs(&String::from_utf8_lossy(&bytes));
            (
                "txt",
                textfmt::join_paragraphs(&original, translated, mode).into_bytes(),
            )
        }
        "md" => (
            "md",
            markdown::rebuild(&String::from_utf8_lossy(&bytes), translated, mode)?.into_bytes(),
        ),
        "docx" => ("docx", docx::rebuild(&bytes, translated, mode)?),
        "epub" => ("epub", epub::rebuild(&bytes, translated, mode)?),
        "pdf" => {
            let pages = pdf::extract(&bytes)?;
            ("md", pdf::rebuild(&pages, translated, mode)?.into_bytes())
        }
        other => return Err(format!("无法回填 {other}")),
    };
    let destination = sibling_output(path, target_lang, extension);
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    std::fs::write(&destination, output).map_err(|err| err.to_string())?;
    Ok(destination)
}

pub fn pack_format_for(version: &str) -> u32 {
    match version {
        "1.20.1" => 15,
        "1.20.4" => 22,
        "1.21" | "1.21.1" => 34,
        "1.21.4" => 46,
        "1.21.8" => 64,
        _ => 34,
    }
}
