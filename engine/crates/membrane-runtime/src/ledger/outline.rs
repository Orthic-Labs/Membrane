use comrak::nodes::NodeValue;
use comrak::{parse_document, Arena, Options};
use serde::Serialize;
use sha2::{Digest, Sha256};

const PARSER_NAME: &str = "comrak";
const PARSER_VERSION: &str = "0.54.0";
const DEFAULT_MAX_OUTLINE_SECTIONS: usize = 256;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocOutlineV1 {
    pub schema_version: &'static str,
    pub source_ref: String,
    pub content_hash: String,
    pub outline_hash: String,
    pub parser: ParserProjectionV1,
    pub sections: Vec<DocSectionV1>,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub continuation_cursor: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParserProjectionV1 {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocSectionV1 {
    pub anchor_id: String,
    pub parent_anchor_id: Option<String>,
    pub level: u8,
    pub heading: String,
    pub breadcrumb: Vec<String>,
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_line: usize,
    pub end_line: usize,
    pub span_hash: String,
    pub span: DocSpanV1,
    pub token_estimates: TokenEstimatesV1,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub continuation_cursor: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenEstimatesV1 {
    pub per_model: std::collections::BTreeMap<String, usize>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocReadV1 {
    pub schema_version: &'static str,
    pub source_ref: String,
    pub content_hash: String,
    pub anchor_id: String,
    pub content: String,
    pub breadcrumb: Vec<String>,
    pub span: DocSpanV1,
    pub neighbor_anchors: NeighborAnchorsV1,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub continuation_cursor: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocSpanV1 {
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_line: usize,
    pub end_line: usize,
    pub span_hash: String,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NeighborAnchorsV1 {
    pub parent: Option<String>,
    pub previous: Option<String>,
    pub next: Option<String>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DocReadError {
    #[error("source_changed")]
    SourceChanged,
    #[error("source_missing")]
    SourceMissing,
    #[error("relocated")]
    Relocated,
    #[error("deny")]
    Deny,
}

impl DocReadError {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SourceChanged => "source_changed",
            Self::SourceMissing => "source_missing",
            Self::Relocated => "relocated",
            Self::Deny => "deny",
        }
    }
}

pub fn read_section(
    source_ref: &str,
    markdown: &str,
    anchor: &str,
    expected_hash: &str,
    max_bytes: usize,
) -> Result<DocReadV1, DocReadError> {
    read_section_with_cursor(source_ref, markdown, anchor, expected_hash, max_bytes, None)
}

pub fn read_section_with_cursor(
    source_ref: &str,
    markdown: &str,
    anchor: &str,
    expected_hash: &str,
    max_bytes: usize,
    continuation_cursor: Option<&str>,
) -> Result<DocReadV1, DocReadError> {
    let outline = build_outline_page(source_ref, markdown, "ignored", usize::MAX, None)
        .expect("an unpaged outline has no continuation cursor");
    if outline.content_hash != expected_hash {
        return Err(DocReadError::SourceChanged);
    }
    let index = outline
        .sections
        .iter()
        .position(|section| section.anchor_id == anchor)
        .ok_or(DocReadError::Relocated)?;
    let section = &outline.sections[index];
    let full = &markdown[section.start_byte..section.end_byte];
    let start = continuation_cursor
        .map(|cursor| parse_cursor(cursor, anchor, full))
        .transpose()?
        .unwrap_or(0);
    let end = start + utf8_prefix_len(&full[start..], max_bytes);
    let truncated = full.len() > end;
    Ok(DocReadV1 {
        schema_version: "DocReadV1",
        source_ref: source_ref.to_owned(),
        content_hash: outline.content_hash,
        anchor_id: section.anchor_id.clone(),
        content: full[start..end].to_owned(),
        breadcrumb: section.breadcrumb.clone(),
        span: DocSpanV1 {
            start_byte: section.start_byte,
            end_byte: section.end_byte,
            start_line: section.start_line,
            end_line: section.end_line,
            span_hash: section.span_hash.clone(),
        },
        neighbor_anchors: NeighborAnchorsV1 {
            parent: section.parent_anchor_id.clone(),
            previous: index
                .checked_sub(1)
                .map(|prior| outline.sections[prior].anchor_id.clone()),
            next: outline
                .sections
                .get(index + 1)
                .map(|next| next.anchor_id.clone()),
        },
        truncated,
        continuation_cursor: truncated.then(|| format!("{}:{}", section.anchor_id, end)),
    })
}

#[derive(Clone)]
struct PendingSection {
    anchor_id: String,
    parent_anchor_id: Option<String>,
    level: u8,
    heading: String,
    breadcrumb: Vec<String>,
    start_byte: usize,
    start_line: usize,
}

pub fn build_outline(source_ref: &str, markdown: &str, parser: &str) -> DocOutlineV1 {
    build_outline_page(
        source_ref,
        markdown,
        parser,
        DEFAULT_MAX_OUTLINE_SECTIONS,
        None,
    )
    .expect("an initial outline page has no continuation cursor")
}

pub fn build_outline_page(
    source_ref: &str,
    markdown: &str,
    _parser: &str,
    max_sections: usize,
    continuation_cursor: Option<&str>,
) -> Result<DocOutlineV1, DocReadError> {
    let content_hash = hash(markdown);
    let line_starts = line_starts(markdown);
    let mut pending = Vec::new();
    let arena = Arena::new();
    let mut options = Options::default();
    options.extension.front_matter_delimiter = Some("---".to_owned());
    options.extension.table = true;
    options.extension.strikethrough = true;
    options.extension.tasklist = true;
    options.extension.autolink = true;
    options.render.sourcepos = true;
    let root = parse_document(&arena, markdown, &options);
    let mut ordinals = std::collections::BTreeMap::<String, usize>::new();
    let mut stack: Vec<(u8, String, String)> = Vec::new();

    for node in root.descendants() {
        let data = node.data();
        let NodeValue::Heading(heading) = &data.value else {
            continue;
        };
        let title = inline_text(node).trim().to_owned();
        if title.is_empty() {
            continue;
        }
        let slug = slugify(&title);
        let ordinal = ordinals
            .entry(slug.clone())
            .and_modify(|n| *n += 1)
            .or_insert(1);
        let anchor_id = format!("sec:{slug}:{ordinal}");
        let level = heading.level as u8;
        while stack
            .last()
            .is_some_and(|(prior_level, _, _)| *prior_level >= level)
        {
            stack.pop();
        }
        let parent_anchor_id = stack.last().map(|(_, anchor, _)| anchor.clone());
        let mut breadcrumb = stack
            .iter()
            .map(|(_, _, title)| title.clone())
            .collect::<Vec<_>>();
        breadcrumb.push(title.clone());
        let start_line = data.sourcepos.start.line.max(1) as usize;
        let start_byte = byte_at_line(&line_starts, start_line, markdown.len());
        pending.push(PendingSection {
            anchor_id: anchor_id.clone(),
            parent_anchor_id,
            level,
            heading: title.clone(),
            breadcrumb,
            start_byte,
            start_line,
        });
        stack.push((level, anchor_id, title));
    }
    let mut with_pseudo = pseudo_sections(
        markdown,
        &line_starts,
        pending
            .first()
            .map(|section: &PendingSection| section.start_byte),
    );
    with_pseudo.extend(pending);
    let pending = with_pseudo;

    let all_sections = pending
        .iter()
        .enumerate()
        .map(|(index, section)| {
            let end_byte = next_section_boundary(&pending, index, markdown.len());
            let span = &markdown[section.start_byte..end_byte];
            DocSectionV1 {
                anchor_id: section.anchor_id.clone(),
                parent_anchor_id: section.parent_anchor_id.clone(),
                level: section.level,
                heading: section.heading.clone(),
                breadcrumb: section.breadcrumb.clone(),
                start_byte: section.start_byte,
                end_byte,
                start_line: section.start_line,
                end_line: line_for_byte(&line_starts, end_byte.saturating_sub(1)),
                span_hash: hash(span),
                span: DocSpanV1 {
                    start_byte: section.start_byte,
                    end_byte,
                    start_line: section.start_line,
                    end_line: line_for_byte(&line_starts, end_byte.saturating_sub(1)),
                    span_hash: hash(span),
                },
                token_estimates: TokenEstimatesV1 {
                    per_model: std::collections::BTreeMap::from([(
                        "o200k_base".to_owned(),
                        cortex_core::estimate_tokens(span),
                    )]),
                },
                truncated: false,
                continuation_cursor: None,
            }
        })
        .collect::<Vec<_>>();

    let parser = ParserProjectionV1 {
        name: PARSER_NAME.to_owned(),
        version: PARSER_VERSION.to_owned(),
    };
    let outline_hash = hash(
        &serde_json::to_string(&(&parser, &all_sections))
            .expect("DocOutline parser and sections serialize"),
    );
    let start = continuation_cursor
        .map(|cursor| parse_outline_cursor(cursor, &content_hash, all_sections.len()))
        .transpose()?
        .unwrap_or(0);
    let page_size = max_sections.max(1);
    let end = start.saturating_add(page_size).min(all_sections.len());
    let truncated = end < all_sections.len();
    let sections = all_sections
        .into_iter()
        .skip(start)
        .take(page_size)
        .collect();

    Ok(DocOutlineV1 {
        schema_version: "DocOutlineV1",
        source_ref: source_ref.to_owned(),
        content_hash: content_hash.clone(),
        outline_hash,
        parser,
        sections,
        truncated,
        continuation_cursor: truncated.then(|| format!("outline:{content_hash}:{end}")),
    })
}

fn pseudo_sections(
    markdown: &str,
    starts: &[usize],
    first_heading: Option<usize>,
) -> Vec<PendingSection> {
    let mut sections = Vec::new();
    let mut body_start_line = 1;
    if markdown.starts_with("---\n") || markdown.starts_with("---\r\n") {
        let lines = markdown.lines().collect::<Vec<_>>();
        if let Some(close) = lines.iter().skip(1).position(|line| line.trim() == "---") {
            let end_line = close + 2;
            sections.push(PendingSection {
                anchor_id: "sec:_frontmatter:1".to_owned(),
                parent_anchor_id: None,
                level: 0,
                heading: "_frontmatter".to_owned(),
                breadcrumb: vec!["_frontmatter".to_owned()],
                start_byte: 0,
                start_line: 1,
            });
            body_start_line = end_line + 1;
        }
    }
    let body_start = byte_at_line(starts, body_start_line, markdown.len());
    let preamble_end = first_heading.unwrap_or(markdown.len());
    if !markdown[body_start..preamble_end].trim().is_empty() {
        sections.push(PendingSection {
            anchor_id: "sec:preamble:1".to_owned(),
            parent_anchor_id: None,
            level: 0,
            heading: "preamble".to_owned(),
            breadcrumb: vec!["preamble".to_owned()],
            start_byte: body_start,
            start_line: body_start_line,
        });
    }
    sections
}

fn next_section_boundary(sections: &[PendingSection], index: usize, document_end: usize) -> usize {
    let section = &sections[index];
    if section.level == 0 {
        return sections
            .get(index + 1)
            .map(|next| next.start_byte)
            .unwrap_or(document_end);
    }
    sections[index + 1..]
        .iter()
        .find(|next| next.level > 0 && next.level <= section.level)
        .map(|next| next.start_byte)
        .unwrap_or(document_end)
}

fn inline_text<'a>(node: &'a comrak::nodes::AstNode<'a>) -> String {
    node.descendants()
        .filter_map(|child| match &child.data().value {
            NodeValue::Text(text) => Some(text.to_string()),
            NodeValue::Code(code) => Some(code.literal.to_string()),
            NodeValue::SoftBreak | NodeValue::LineBreak => Some(" ".to_owned()),
            _ => None,
        })
        .collect()
}

fn line_starts(text: &str) -> Vec<usize> {
    std::iter::once(0)
        .chain(text.match_indices('\n').map(|(index, _)| index + 1))
        .collect()
}
fn byte_at_line(starts: &[usize], line: usize, fallback: usize) -> usize {
    starts
        .get(line.saturating_sub(1))
        .copied()
        .unwrap_or(fallback)
}
fn line_for_byte(starts: &[usize], byte: usize) -> usize {
    starts.partition_point(|start| *start <= byte).max(1)
}
fn hash(text: &str) -> String {
    hex::encode(Sha256::digest(text.as_bytes()))
}
fn slugify(text: &str) -> String {
    let slug = text
        .to_ascii_lowercase()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect::<String>();
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "section".to_owned()
    } else {
        slug.split('-')
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("-")
    }
}

fn parse_cursor(cursor: &str, anchor: &str, content: &str) -> Result<usize, DocReadError> {
    let (cursor_anchor, offset) = cursor.rsplit_once(':').ok_or(DocReadError::Relocated)?;
    if cursor_anchor != anchor {
        return Err(DocReadError::Relocated);
    }
    let offset = offset
        .parse::<usize>()
        .map_err(|_| DocReadError::Relocated)?;
    if offset > content.len() || !content.is_char_boundary(offset) {
        return Err(DocReadError::Relocated);
    }
    Ok(offset)
}

fn parse_outline_cursor(
    cursor: &str,
    content_hash: &str,
    section_count: usize,
) -> Result<usize, DocReadError> {
    let mut parts = cursor.rsplitn(3, ':');
    let offset = parts
        .next()
        .ok_or(DocReadError::Relocated)?
        .parse::<usize>()
        .map_err(|_| DocReadError::Relocated)?;
    let cursor_hash = parts.next().ok_or(DocReadError::Relocated)?;
    let kind = parts.next().ok_or(DocReadError::Relocated)?;
    if kind != "outline" {
        return Err(DocReadError::Relocated);
    }
    if cursor_hash != content_hash {
        return Err(DocReadError::SourceChanged);
    }
    if offset > section_count {
        return Err(DocReadError::Relocated);
    }
    Ok(offset)
}

fn utf8_prefix_len(content: &str, max_bytes: usize) -> usize {
    content
        .char_indices()
        .take_while(|(offset, character)| offset + character.len_utf8() <= max_bytes)
        .last()
        .map(|(offset, character)| offset + character.len_utf8())
        .unwrap_or(0)
}
