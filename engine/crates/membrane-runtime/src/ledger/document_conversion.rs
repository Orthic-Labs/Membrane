//! Explicitly granted, deterministic non-Markdown conversion into hash-bound Markdown.
//!
//! Conversion retains raw input in its result. Media never enters document text. Ledger owns
//! this rebuildable normalization receipt, not source truth or durable storage.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentInputFormatV1 {
    PlainText,
    Html,
    Json,
    Media(String),
    Other(String),
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
        digest(format!("ledger.document-conversion.v1\0{formats}\0{}", self.max_raw_bytes).as_bytes())
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
        DocumentInputFormatV1::PlainText => (text, "ledger.plain-text", "1"),
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
            (format!("```json\n{normalized}\n```\n"), "ledger.json", "1")
        }
        DocumentInputFormatV1::Html => {
            let (normalized, media_count, had_markup) = normalize_html(&text);
            if had_markup {
                losses.push(ConversionLossV1::FormattingFlattened);
            }
            if media_count > 0 {
                omissions.push(ConversionOmissionV1::EmbeddedMediaExcluded { count: media_count });
            }
            (normalized, "ledger.html-text", "1")
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
        schema_version: "ledger.converted-document.v1".to_owned(),
        source_ref: input.source_ref,
        format: input.format,
        raw_input: input.raw_input,
        raw_sha256,
        markdown,
        markdown_sha256,
        converter: ConverterProvenanceV1 {
            converter: converter.to_owned(),
            version: version.to_owned(),
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

fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
