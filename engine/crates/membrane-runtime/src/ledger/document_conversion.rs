//! Explicitly granted, deterministic non-Markdown conversion into hash-bound Markdown.
//!
//! Conversion retains raw input in its result. Media never enters document text. Ledger owns
//! this rebuildable normalization receipt, not source truth or durable storage.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::io::Read;
use std::process::{Command, Stdio};

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentInputFormatV1 {
    PlainText,
    Html,
    Json,
    Docx,
    Pdf,
    Media(String),
    Other(String),
}

impl DocumentInputFormatV1 {
    pub fn storage_name(&self) -> String {
        match self {
            Self::PlainText => "plain_text".to_owned(),
            Self::Html => "html".to_owned(),
            Self::Json => "json".to_owned(),
            Self::Docx => "docx".to_owned(),
            Self::Pdf => "pdf".to_owned(),
            Self::Media(media_type) => format!("media:{media_type}"),
            Self::Other(format) => format!("other:{format}"),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct DocumentConversionGrantV1 {
    allowed_formats: BTreeSet<DocumentInputFormatV1>,
    max_raw_bytes: usize,
}

impl DocumentConversionGrantV1 {
    /// Construct an explicit host grant. Empty format sets remain fail-closed.
    pub fn new(
        allowed_formats: impl IntoIterator<Item = DocumentInputFormatV1>,
        max_raw_bytes: usize,
    ) -> Self {
        Self {
            allowed_formats: allowed_formats.into_iter().collect(),
            max_raw_bytes,
        }
    }

    pub fn denied() -> Self {
        Self {
            allowed_formats: BTreeSet::new(),
            max_raw_bytes: 0,
        }
    }

    pub fn config_digest(&self) -> String {
        let formats = self
            .allowed_formats
            .iter()
            .map(|format| format!("{format:?}"))
            .collect::<Vec<_>>()
            .join("\0");
        digest(format!("ledger.document-conversion.v2\0{formats}\0{}", self.max_raw_bytes).as_bytes())
    }
}

impl Default for DocumentConversionGrantV1 {
    fn default() -> Self {
        Self::denied()
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct DocumentConversionInputV1 {
    pub source_ref: String,
    pub format: DocumentInputFormatV1,
    pub raw_input: Vec<u8>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct ConverterProvenanceV1 {
    pub converter: String,
    pub version: String,
    pub config_digest: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversionLossV1 {
    Utf8Replacement { replacement_count: usize },
    FormattingFlattened,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversionOmissionV1 {
    EmbeddedMediaExcluded { count: usize },
    CompressedPdfStreamExcluded { count: usize },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct ConvertedDocumentV1 {
    pub schema_version: String,
    pub source_ref: String,
    pub format: DocumentInputFormatV1,
    /// Exact raw input retained for source resolution or deterministic replay.
    pub raw_input: Vec<u8>,
    pub raw_sha256: String,
    pub markdown: String,
    pub markdown_sha256: String,
    pub converter: ConverterProvenanceV1,
    pub losses: Vec<ConversionLossV1>,
    pub omissions: Vec<ConversionOmissionV1>,
}

#[derive(Clone, Debug, thiserror::Error, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentConversionErrorV1 {
    #[error("conversion_not_granted")]
    NotGranted,
    #[error("conversion_input_too_large")]
    InputTooLarge { actual: usize, maximum: usize },
    #[error("media_excluded")]
    MediaExcluded { media_type: String },
    #[error("unsupported_format")]
    UnsupportedFormat { format: String },
    #[error("invalid_input")]
    InvalidInput { detail: String },
    #[error("converter_unavailable")]
    ConverterUnavailable { converter: String },
    #[error("conversion_output_too_large")]
    OutputTooLarge { actual: usize, maximum: usize },
}

/// Convert one explicitly granted input; default grant denies every format.
pub fn convert_granted_document(
    grant: &DocumentConversionGrantV1,
    input: DocumentConversionInputV1,
) -> Result<ConvertedDocumentV1, DocumentConversionErrorV1> {
    if matches!(&input.format, DocumentInputFormatV1::Media(_)) {
        let DocumentInputFormatV1::Media(media_type) = &input.format else {
            unreachable!()
        };
        return Err(DocumentConversionErrorV1::MediaExcluded {
            media_type: media_type.clone(),
        });
    }
    if !grant.allowed_formats.contains(&input.format) {
        return Err(DocumentConversionErrorV1::NotGranted);
    }
    if input.raw_input.len() > grant.max_raw_bytes {
        return Err(DocumentConversionErrorV1::InputTooLarge {
            actual: input.raw_input.len(),
            maximum: grant.max_raw_bytes,
        });
    }
    let raw_sha256 = digest(&input.raw_input);
    let (text, utf8_replacements) = decode_utf8(&input.raw_input);
    let mut losses = Vec::new();
    if utf8_replacements > 0 {
        losses.push(ConversionLossV1::Utf8Replacement {
            replacement_count: utf8_replacements,
        });
    }
    let mut omissions = Vec::new();
    let (markdown, converter, version) = match &input.format {
        DocumentInputFormatV1::PlainText => (
            text,
            "ledger.plain-text".to_owned(),
            "1".to_owned(),
        ),
        DocumentInputFormatV1::Json => {
            let value: serde_json::Value = serde_json::from_str(&text).map_err(|error| {
                DocumentConversionErrorV1::InvalidInput {
                    detail: format!("json:{error}"),
                }
            })?;
            let normalized = serde_json::to_string_pretty(&value).map_err(|error| {
                DocumentConversionErrorV1::InvalidInput {
                    detail: format!("json:{error}"),
                }
            })?;
            (
                format!("```json\n{normalized}\n```\n"),
                "ledger.json".to_owned(),
                "1".to_owned(),
            )
        }
        DocumentInputFormatV1::Html => {
            let (normalized, media_count, had_markup) = normalize_html(&text);
            if had_markup {
                losses.push(ConversionLossV1::FormattingFlattened);
            }
            if media_count > 0 {
                omissions.push(ConversionOmissionV1::EmbeddedMediaExcluded { count: media_count });
            }
            (
                normalized,
                "ledger.html-text".to_owned(),
                "1".to_owned(),
            )
        }
        DocumentInputFormatV1::Pdf => {
            let (normalized, media_count, compressed_count) = normalize_pdf(&input.raw_input)?;
            losses.push(ConversionLossV1::FormattingFlattened);
            if media_count > 0 {
                omissions.push(ConversionOmissionV1::EmbeddedMediaExcluded { count: media_count });
            }
            if compressed_count > 0 {
                omissions.push(ConversionOmissionV1::CompressedPdfStreamExcluded {
                    count: compressed_count,
                });
            }
            (
                normalized,
                "ledger.pdf-literal-text".to_owned(),
                "1".to_owned(),
            )
        }
        DocumentInputFormatV1::Docx => {
            let output_limit = grant.max_raw_bytes.saturating_mul(8).min(16 * 1024 * 1024);
            let (normalized, media_count, tar_version) =
                normalize_docx(&input.raw_input, output_limit)?;
            losses.push(ConversionLossV1::FormattingFlattened);
            if media_count > 0 {
                omissions.push(ConversionOmissionV1::EmbeddedMediaExcluded { count: media_count });
            }
            (
                normalized,
                "ledger.docx-wordprocessingml".to_owned(),
                tar_version,
            )
        }
        DocumentInputFormatV1::Other(format) => {
            return Err(DocumentConversionErrorV1::UnsupportedFormat {
                format: format.clone(),
            });
        }
        DocumentInputFormatV1::Media(_) => unreachable!(),
    };
    let markdown_sha256 = digest(markdown.as_bytes());
    Ok(ConvertedDocumentV1 {
        schema_version: "ledger.converted-document.v2".to_owned(),
        source_ref: input.source_ref,
        format: input.format,
        raw_input: input.raw_input,
        raw_sha256,
        markdown,
        markdown_sha256,
        converter: ConverterProvenanceV1 {
            converter,
            version,
            config_digest: grant.config_digest(),
        },
        losses,
        omissions,
    })
}

fn decode_utf8(raw: &[u8]) -> (String, usize) {
    match std::str::from_utf8(raw) {
        Ok(text) => (text.to_owned(), 0),
        Err(_) => {
            let text = String::from_utf8_lossy(raw).into_owned();
            let replacements = text.matches('\u{fffd}').count();
            (text, replacements)
        }
    }
}

fn normalize_html(html: &str) -> (String, usize, bool) {
    let lower = html.to_ascii_lowercase();
    let media_count = ["<img", "<video", "<audio", "<source"]
        .iter()
        .map(|needle| lower.match_indices(needle).count())
        .sum();
    let mut output = String::with_capacity(html.len());
    let mut tag = String::new();
    let mut in_tag = false;
    let mut had_markup = false;
    for character in html.chars() {
        if character == '<' {
            in_tag = true;
            had_markup = true;
            tag.clear();
            continue;
        }
        if in_tag && character == '>' {
            in_tag = false;
            let name = tag
                .trim()
                .trim_start_matches('/')
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_ascii_lowercase();
            if matches!(
                name.as_str(),
                "p" | "div" | "br" | "li" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
            ) && !output.ends_with('\n')
            {
                output.push('\n');
            }
            continue;
        }
        if in_tag {
            tag.push(character);
        } else {
            output.push(character);
        }
    }
    let output = output
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'");
    let normalized = output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    (format!("{normalized}\n"), media_count, had_markup)
}

fn normalize_pdf(raw: &[u8]) -> Result<(String, usize, usize), DocumentConversionErrorV1> {
    if !raw.starts_with(b"%PDF-") {
        return Err(DocumentConversionErrorV1::InvalidInput {
            detail: "pdf_header_missing".to_owned(),
        });
    }
    let text = String::from_utf8_lossy(raw);
    let media_count = text.match_indices("/Subtype /Image").count();
    let compressed_count = text.match_indices("/FlateDecode").count();
    let bytes = text.as_bytes();
    let mut fragments = Vec::new();
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] != b'(' {
            index += 1;
            continue;
        }
        let mut value = String::new();
        let mut cursor = index + 1;
        let mut depth = 1usize;
        while cursor < bytes.len() && depth > 0 {
            match bytes[cursor] {
                b'\\' if cursor + 1 < bytes.len() => {
                    cursor += 1;
                    value.push(match bytes[cursor] {
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        byte => byte as char,
                    });
                }
                b'(' => {
                    depth += 1;
                    value.push('(');
                }
                b')' => {
                    depth -= 1;
                    if depth > 0 {
                        value.push(')');
                    }
                }
                byte => value.push(byte as char),
            }
            cursor += 1;
        }
        let operator = String::from_utf8_lossy(&bytes[cursor..bytes.len().min(cursor + 16)]);
        let operator = operator.trim_start();
        if depth == 0 && (operator.starts_with("Tj") || operator.starts_with("TJ")) {
            let value = value.trim();
            if !value.is_empty() {
                fragments.push(value.to_owned());
            }
        }
        index = cursor.max(index + 1);
    }
    if fragments.is_empty() {
        return Err(DocumentConversionErrorV1::InvalidInput {
            detail: "pdf_no_extractable_literal_text".to_owned(),
        });
    }
    Ok((format!("{}\n", fragments.join("\n\n")), media_count, compressed_count))
}

fn normalize_docx(
    raw: &[u8],
    output_limit: usize,
) -> Result<(String, usize, String), DocumentConversionErrorV1> {
    if raw.len() < 4 || &raw[..4] != b"PK\x03\x04" {
        return Err(DocumentConversionErrorV1::InvalidInput {
            detail: "docx_zip_header_missing".to_owned(),
        });
    }
    let tar_version_output = tar_command()
        .arg("--version")
        .output()
        .map_err(|_| DocumentConversionErrorV1::ConverterUnavailable {
            converter: "tar".to_owned(),
        })?;
    let tar_version = String::from_utf8_lossy(&tar_version_output.stdout)
        .lines()
        .next()
        .unwrap_or("tar-version-unknown")
        .trim()
        .to_owned();
    let hash = digest(raw);
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let temp_path = std::env::temp_dir().join(format!(
        "membrane-ledger-docx-{}-{}-{}.docx",
        std::process::id(),
        &hash[..16],
        nonce
    ));
    std::fs::write(&temp_path, raw).map_err(|error| DocumentConversionErrorV1::InvalidInput {
        detail: format!("docx_temp_write:{error}"),
    })?;
    let child = tar_command()
        .arg("-xOf")
        .arg(&temp_path)
        .arg("word/document.xml")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(_) => {
            let _ = std::fs::remove_file(&temp_path);
            return Err(DocumentConversionErrorV1::ConverterUnavailable {
                converter: "tar".to_owned(),
            });
        }
    };
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        let _ = std::fs::remove_file(&temp_path);
        return Err(DocumentConversionErrorV1::InvalidInput {
            detail: "docx_stdout_missing".to_owned(),
        });
    };
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let reader = std::thread::spawn(move || {
        let mut xml = Vec::new();
        let result = stdout
            .take(output_limit.saturating_add(1) as u64)
            .read_to_end(&mut xml)
            .map(|_| xml);
        let _ = sender.send(result);
    });
    let xml = match receiver.recv_timeout(std::time::Duration::from_secs(5)) {
        Ok(Ok(xml)) => xml,
        Ok(Err(error)) => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = reader.join();
            let _ = std::fs::remove_file(&temp_path);
            return Err(DocumentConversionErrorV1::InvalidInput {
                detail: format!("docx_read:{error}"),
            });
        }
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = reader.join();
            let _ = std::fs::remove_file(&temp_path);
            return Err(DocumentConversionErrorV1::InvalidInput {
                detail: "docx_converter_timeout".to_owned(),
            });
        }
    };
    let _ = reader.join();
    if xml.len() > output_limit {
        let _ = child.kill();
        let _ = child.wait();
        let _ = std::fs::remove_file(&temp_path);
        return Err(DocumentConversionErrorV1::OutputTooLarge {
            actual: xml.len(),
            maximum: output_limit,
        });
    }
    let status = child.wait().map_err(|error| DocumentConversionErrorV1::InvalidInput {
        detail: format!("docx_wait:{error}"),
    })?;
    let _ = std::fs::remove_file(&temp_path);
    if !status.success() {
        return Err(DocumentConversionErrorV1::InvalidInput {
            detail: "docx_document_xml_missing".to_owned(),
        });
    }
    let xml = String::from_utf8(xml).map_err(|error| DocumentConversionErrorV1::InvalidInput {
        detail: format!("docx_xml_utf8:{error}"),
    })?;
    let markdown = wordprocessingml_text(&xml);
    if markdown.trim().is_empty() {
        return Err(DocumentConversionErrorV1::InvalidInput {
            detail: "docx_no_text".to_owned(),
        });
    }
    let media_occurrences = String::from_utf8_lossy(raw)
        .match_indices("word/media/")
        .count();
    Ok((markdown, media_occurrences.div_ceil(2), tar_version))
}

fn wordprocessingml_text(xml: &str) -> String {
    let mut output = String::new();
    let mut cursor = 0usize;
    while let Some(relative) = xml[cursor..].find("<w:t") {
        let start_tag = cursor + relative;
        let Some(open_end) = xml[start_tag..].find('>').map(|value| start_tag + value + 1) else {
            break;
        };
        let Some(close) = xml[open_end..]
            .find("</w:t>")
            .map(|value| open_end + value)
        else {
            break;
        };
        output.push_str(&decode_xml_entities(&xml[open_end..close]));
        let after = close + "</w:t>".len();
        if xml[after..].find("</w:p>").is_some_and(|paragraph| {
            xml[after..]
                .find("<w:t")
                .is_none_or(|next_text| paragraph < next_text)
        }) {
            output.push_str("\n\n");
        } else {
            output.push(' ');
        }
        cursor = after;
    }
    format!("{}\n", output.trim())
}

fn tar_command() -> Command {
    let mut command = Command::new("tar");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command
}

fn decode_xml_entities(value: &str) -> String {
    value
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
