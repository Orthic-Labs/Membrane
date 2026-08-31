//! Explicitly granted, deterministic non-Markdown conversion into hash-bound Markdown.
//!
//! Conversion retains raw input in its result. Media never enters document text. Ledger owns
//! this rebuildable normalization receipt, not source truth or durable storage.

use flate2::read::DeflateDecoder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::io::Read;

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
            let (normalized, media_count, converter_version) =
                normalize_docx(&input.raw_input, output_limit)?;
            losses.push(ConversionLossV1::FormattingFlattened);
            if media_count > 0 {
                omissions.push(ConversionOmissionV1::EmbeddedMediaExcluded { count: media_count });
            }
            (
                normalized,
                "ledger.docx-wordprocessingml".to_owned(),
                converter_version,
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
    let xml = extract_zip_entry(raw, b"word/document.xml", output_limit)?;
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
    Ok((
        markdown,
        media_occurrences.div_ceil(2),
        "zip-wordprocessingml-v1".to_owned(),
    ))
}

fn extract_zip_entry(
    archive: &[u8],
    wanted_name: &[u8],
    output_limit: usize,
) -> Result<Vec<u8>, DocumentConversionErrorV1> {
    const EOCD_SIGNATURE: &[u8; 4] = b"PK\x05\x06";
    const CENTRAL_SIGNATURE: &[u8; 4] = b"PK\x01\x02";
    const LOCAL_SIGNATURE: &[u8; 4] = b"PK\x03\x04";
    let search_start = archive.len().saturating_sub(65_557);
    let eocd = (search_start..archive.len().saturating_sub(3))
        .rev()
        .find(|offset| archive.get(*offset..offset + 4) == Some(EOCD_SIGNATURE.as_slice()))
        .ok_or_else(|| DocumentConversionErrorV1::InvalidInput {
            detail: "docx_end_of_central_directory_missing".to_owned(),
        })?;
    if eocd + 22 > archive.len()
        || read_u16(archive, eocd + 4)? != 0
        || read_u16(archive, eocd + 6)? != 0
        || read_u16(archive, eocd + 8)? != read_u16(archive, eocd + 10)?
        || eocd + 22 + usize::from(read_u16(archive, eocd + 20)?) != archive.len()
    {
        return Err(DocumentConversionErrorV1::InvalidInput {
            detail: "docx_multidisk_zip_unsupported".to_owned(),
        });
    }
    let entry_count = usize::from(read_u16(archive, eocd + 10)?);
    let central_size = usize::try_from(read_u32(archive, eocd + 12)?).unwrap_or(usize::MAX);
    let mut cursor = usize::try_from(read_u32(archive, eocd + 16)?).unwrap_or(usize::MAX);
    let central_end = cursor.checked_add(central_size).filter(|end| *end <= archive.len()).ok_or_else(|| {
        DocumentConversionErrorV1::InvalidInput {
            detail: "docx_central_directory_out_of_bounds".to_owned(),
        }
    })?;
    for _ in 0..entry_count {
        if cursor + 46 > central_end
            || archive.get(cursor..cursor + 4) != Some(CENTRAL_SIGNATURE.as_slice())
        {
            return Err(DocumentConversionErrorV1::InvalidInput {
                detail: "docx_central_directory_invalid".to_owned(),
            });
        }
        let flags = read_u16(archive, cursor + 8)?;
        let method = read_u16(archive, cursor + 10)?;
        let expected_crc = read_u32(archive, cursor + 16)?;
        let compressed_size = usize::try_from(read_u32(archive, cursor + 20)?).unwrap_or(usize::MAX);
        let uncompressed_size = usize::try_from(read_u32(archive, cursor + 24)?).unwrap_or(usize::MAX);
        let name_len = usize::from(read_u16(archive, cursor + 28)?);
        let extra_len = usize::from(read_u16(archive, cursor + 30)?);
        let comment_len = usize::from(read_u16(archive, cursor + 32)?);
        let local_offset = usize::try_from(read_u32(archive, cursor + 42)?).unwrap_or(usize::MAX);
        let name_start = cursor + 46;
        let next = name_start
            .checked_add(name_len)
            .and_then(|value| value.checked_add(extra_len))
            .and_then(|value| value.checked_add(comment_len))
            .filter(|value| *value <= central_end)
            .ok_or_else(|| DocumentConversionErrorV1::InvalidInput {
                detail: "docx_central_entry_out_of_bounds".to_owned(),
            })?;
        if archive.get(name_start..name_start + name_len) == Some(wanted_name) {
            if flags & 0x0001 != 0 {
                return Err(DocumentConversionErrorV1::InvalidInput {
                    detail: "docx_encrypted_entry_unsupported".to_owned(),
                });
            }
            if uncompressed_size > output_limit {
                return Err(DocumentConversionErrorV1::OutputTooLarge {
                    actual: uncompressed_size,
                    maximum: output_limit,
                });
            }
            let local_header_end = local_offset.checked_add(30).ok_or_else(|| {
                DocumentConversionErrorV1::InvalidInput {
                    detail: "docx_local_entry_out_of_bounds".to_owned(),
                }
            })?;
            if local_header_end > archive.len()
                || archive.get(local_offset..local_offset + 4)
                    != Some(LOCAL_SIGNATURE.as_slice())
            {
                return Err(DocumentConversionErrorV1::InvalidInput {
                    detail: "docx_local_entry_invalid".to_owned(),
                });
            }
            let local_name_len = usize::from(read_u16(archive, local_offset + 26)?);
            let local_extra_len = usize::from(read_u16(archive, local_offset + 28)?);
            let data_start = local_offset
                .checked_add(30)
                .and_then(|value| value.checked_add(local_name_len))
                .and_then(|value| value.checked_add(local_extra_len))
                .ok_or_else(|| DocumentConversionErrorV1::InvalidInput {
                    detail: "docx_local_entry_out_of_bounds".to_owned(),
                })?;
            let data_end = data_start.checked_add(compressed_size).filter(|end| *end <= archive.len()).ok_or_else(|| {
                DocumentConversionErrorV1::InvalidInput {
                    detail: "docx_compressed_entry_out_of_bounds".to_owned(),
                }
            })?;
            let compressed = &archive[data_start..data_end];
            let mut output = Vec::with_capacity(uncompressed_size.min(output_limit));
            match method {
                0 if compressed.len() <= output_limit => output.extend_from_slice(compressed),
                0 => {
                    return Err(DocumentConversionErrorV1::OutputTooLarge {
                        actual: compressed.len(),
                        maximum: output_limit,
                    });
                }
                8 => {
                    DeflateDecoder::new(compressed)
                        .take(output_limit.saturating_add(1) as u64)
                        .read_to_end(&mut output)
                        .map_err(|error| DocumentConversionErrorV1::InvalidInput {
                            detail: format!("docx_deflate:{error}"),
                        })?;
                }
                unsupported => {
                    return Err(DocumentConversionErrorV1::InvalidInput {
                        detail: format!("docx_compression_unsupported:{unsupported}"),
                    });
                }
            }
            if output.len() > output_limit {
                return Err(DocumentConversionErrorV1::OutputTooLarge {
                    actual: output.len(),
                    maximum: output_limit,
                });
            }
            if output.len() != uncompressed_size {
                return Err(DocumentConversionErrorV1::InvalidInput {
                    detail: "docx_uncompressed_size_mismatch".to_owned(),
                });
            }
            if zip_crc32(&output) != expected_crc {
                return Err(DocumentConversionErrorV1::InvalidInput {
                    detail: "docx_crc_mismatch".to_owned(),
                });
            }
            return Ok(output);
        }
        cursor = next;
    }
    Err(DocumentConversionErrorV1::InvalidInput {
        detail: "docx_document_xml_missing".to_owned(),
    })
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, DocumentConversionErrorV1> {
    let value = bytes.get(offset..offset + 2).ok_or_else(|| DocumentConversionErrorV1::InvalidInput {
        detail: "docx_zip_field_out_of_bounds".to_owned(),
    })?;
    Ok(u16::from_le_bytes([value[0], value[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, DocumentConversionErrorV1> {
    let value = bytes.get(offset..offset + 4).ok_or_else(|| DocumentConversionErrorV1::InvalidInput {
        detail: "docx_zip_field_out_of_bounds".to_owned(),
    })?;
    Ok(u32::from_le_bytes([value[0], value[1], value[2], value[3]]))
}

fn zip_crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320u32 & 0u32.wrapping_sub(crc & 1));
        }
    }
    !crc
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
