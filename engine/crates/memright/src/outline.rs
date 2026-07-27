//! Deterministic markdown section index used by `doc-outline` and prep routing.

use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Section {
    pub depth: u8,
    pub title: String,
    pub slug: String,
    pub start_line: usize,
    pub end_line: usize,
    pub start_byte: usize,
    pub end_byte: usize,
    pub est_tokens: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct OutlineDocument {
    pub path: String,
    pub total_bytes: usize,
    pub total_est_tokens: usize,
    pub content_hash: String,
    pub sections: Vec<Section>,
}

fn slugify(title: &str, seen: &mut std::collections::HashMap<String, usize>) -> String {
    let mut slug = String::new();
    for ch in title.chars().flat_map(|c| c.to_lowercase()) {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            slug.push(ch);
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_matches('-').to_string();
    let base = if slug.is_empty() {
        "section".to_string()
    } else {
        slug
    };
    let count = seen.entry(base.clone()).or_insert(0);
    let result = if *count == 0 {
        base
    } else {
        format!("{base}-{}", *count)
    };
    *count += 1;
    result
}

fn line_start(bytes: &[u8], line: usize) -> usize {
    bytes
        .iter()
        .enumerate()
        .filter(|(_, b)| **b == b'\n')
        .take(line.saturating_sub(1))
        .map(|(i, _)| i + 1)
        .last()
        .unwrap_or(0)
}

fn heading(lines: &[&str], index: usize, fenced: bool) -> Option<(u8, String)> {
    if fenced {
        return None;
    }
    let line = lines[index].trim_end_matches('\r');
    let hashes = line.chars().take_while(|c| *c == '#').count();
    if (1..=6).contains(&hashes) && line.chars().nth(hashes) == Some(' ') {
        return Some((
            hashes as u8,
            line[hashes + 1..]
                .trim()
                .trim_end_matches('#')
                .trim()
                .to_string(),
        ));
    }
    let underline = lines.get(index + 1).map(|value| value.trim()).unwrap_or("");
    let is_setext = !underline.is_empty()
        && (underline.chars().all(|c| c == '=') || underline.chars().all(|c| c == '-'));
    if index + 1 < lines.len() && !line.trim().is_empty() && is_setext {
        return Some((
            if underline.starts_with('=') { 1 } else { 2 },
            line.trim().to_string(),
        ));
    }
    None
}

pub fn outline(text: &str) -> Vec<Section> {
    let lines: Vec<&str> = text.split('\n').collect();
    let bytes = text.as_bytes();
    let mut headings: Vec<(usize, u8, String)> = Vec::new();
    let mut fenced = false;
    for index in 0..lines.len() {
        let trimmed = lines[index].trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fenced = !fenced;
            continue;
        }
        if let Some((depth, title)) = heading(&lines, index, fenced) {
            headings.push((index, depth, title));
        }
    }
    let mut result = Vec::new();
    let mut seen = std::collections::HashMap::new();
    if text.starts_with("---\n") {
        if let Some(end) = lines.iter().skip(1).position(|line| line.trim() == "---") {
            let end_line = end + 2;
            let start = 0;
            let end_byte = line_start(bytes, end_line + 1).min(bytes.len());
            result.push(Section {
                depth: 0,
                title: "_frontmatter".into(),
                slug: "_frontmatter".into(),
                start_line: 1,
                end_line,
                start_byte: start,
                end_byte,
                est_tokens: (end_byte - start) / 4,
            });
        }
    }
    for (position, &(index, depth, ref title)) in headings.iter().enumerate() {
        let next = headings
            .iter()
            .skip(position + 1)
            .find(|(_, next_depth, _)| *next_depth <= depth)
            .map(|(i, _, _)| *i)
            .unwrap_or(lines.len());
        let start_line = index + 1;
        let end_line = next.max(start_line) as usize;
        let start_byte = line_start(bytes, start_line);
        let end_byte = line_start(bytes, end_line + 1).min(bytes.len());
        result.push(Section {
            depth,
            title: title.clone(),
            slug: slugify(title, &mut seen),
            start_line,
            end_line,
            start_byte,
            end_byte,
            est_tokens: (end_byte - start_byte) / 4,
        });
    }
    if result.is_empty() {
        result.push(Section {
            depth: 0,
            title: "_document".into(),
            slug: "_document".into(),
            start_line: 1,
            end_line: lines.len(),
            start_byte: 0,
            end_byte: bytes.len(),
            est_tokens: bytes.len() / 4,
        });
    }
    result
}

pub fn document(path: &str, text: &str, max_depth: Option<u8>) -> OutlineDocument {
    let sections = outline(text)
        .into_iter()
        .filter(|s| {
            max_depth
                .map(|max| s.depth == 0 || s.depth <= max)
                .unwrap_or(true)
        })
        .collect();
    OutlineDocument {
        path: path.into(),
        total_bytes: text.len(),
        total_est_tokens: text.len() / 4,
        content_hash: format!("{:x}", Sha256::digest(text.as_bytes())),
        sections,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_frontmatter_fences_setext_and_duplicate_slugs() {
        let text = "---\ntitle: x\n---\n# One\n```\n# no\n```\n## Two\nSame\n---\n# One\n";
        let rows = outline(text);
        assert_eq!(rows[0].slug, "_frontmatter");
        assert_eq!(rows.iter().filter(|s| s.title == "no").count(), 0);
        assert_eq!(
            rows.iter()
                .filter(|s| s.title == "One")
                .map(|s| s.slug.clone())
                .collect::<Vec<_>>(),
            vec!["one", "one-1"]
        );
        assert!(rows.iter().any(|s| s.title == "Same" && s.depth == 2));
    }

    #[test]
    fn empty_and_crlf_docs_have_stable_ranges() {
        assert_eq!(outline("")[0].end_byte, 0);
        let row = &outline("# A\r\nbody\r\n")[0];
        assert_eq!((row.start_line, row.end_line, row.start_byte), (1, 3, 0));
    }
}
