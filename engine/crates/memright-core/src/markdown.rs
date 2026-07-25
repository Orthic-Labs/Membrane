//! Markdown memory parsing — frontmatter + `[[wikilink]]` graph, dependency-free. Shared by the
//! workspace ingest path and CodeRight so there is ONE parser, not two copies.

/// A parsed markdown memory: flat frontmatter fields + body + wikilink targets. Consumers map this
/// onto their own entry type (CR's `MemoryEntry`, the workspace store, …) and decide the scope.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParsedDoc {
    /// Filename stem (the bare id; consumers scope-qualify it).
    pub id: String,
    pub name: String,
    pub description: String,
    pub body: String,
    /// `type:` frontmatter (architecture|audit|design|feedback|reference|…). Defaults to `reference`.
    pub type_: String,
    /// `[[wikilink]]` targets found in the body, in order.
    pub links: Vec<String>,
    pub pinned: bool,
}

fn split_frontmatter(raw: &str) -> (Vec<(String, String)>, String) {
    if let Some(rest) = raw.strip_prefix("---") {
        if let Some(end) = rest.find("\n---") {
            let fm = &rest[..end];
            let body = rest[end + 4..].trim_start_matches(['\r', '\n']).to_string();
            let mut fields = Vec::new();
            for line in fm.lines() {
                let line = line.trim_end();
                if line.is_empty() || line.starts_with(' ') || line.starts_with('\t') {
                    continue; // flat top-level key: value only
                }
                if let Some((k, v)) = line.split_once(':') {
                    fields.push((k.trim().to_string(), v.trim().to_string()));
                }
            }
            return (fields, body);
        }
    }
    (Vec::new(), raw.to_string())
}

fn fm_get<'a>(fields: &'a [(String, String)], key: &str) -> Option<&'a str> {
    fields
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
        .filter(|v| !v.is_empty())
}

/// Extract `[[wikilink]]` targets from the body (the OKF link graph), in order.
pub fn wikilinks(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let b = body.as_bytes();
    let mut i = 0;
    while i + 1 < b.len() {
        if b[i] == b'[' && b[i + 1] == b'[' {
            if let Some(close) = body[i + 2..].find("]]") {
                let inner = &body[i + 2..i + 2 + close];
                if !inner.is_empty() {
                    out.push(inner.to_string());
                }
                i = i + 2 + close + 2;
                continue;
            }
        }
        i += 1;
    }
    out
}

fn first_body_line(body: &str) -> String {
    body.lines()
        .next()
        .unwrap_or("")
        .chars()
        .take(200)
        .collect()
}

/// Parse a markdown memory from its filename stem + raw contents.
pub fn parse_markdown(stem: &str, raw: &str) -> ParsedDoc {
    let (fm, body) = split_frontmatter(raw);
    let name = fm_get(&fm, "name").unwrap_or(stem).to_string();
    let description = fm_get(&fm, "description")
        .map(|s| s.to_string())
        .unwrap_or_else(|| first_body_line(&body));
    let pinned = matches!(
        fm_get(&fm, "pinned").map(|s| s.to_ascii_lowercase()),
        Some(ref v) if v == "true" || v == "1" || v == "yes"
    );
    ParsedDoc {
        id: stem.to_string(),
        name,
        description,
        type_: fm_get(&fm, "type").unwrap_or("reference").to_string(),
        links: wikilinks(&body),
        pinned,
        body,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_flat_frontmatter_and_links() {
        let raw = "---\nname: my-fact\ndescription: a short desc\ntype: project\npinned: true\nmetadata:\n  type: ignored\n---\nBody line links [[other]] and [[second]].\nmore";
        let d = parse_markdown("stem", raw);
        assert_eq!(d.name, "my-fact");
        assert_eq!(d.description, "a short desc");
        assert_eq!(d.type_, "project");
        assert!(d.pinned);
        assert_eq!(d.links, vec!["other", "second"]);
        assert!(d.body.starts_with("Body line"));
    }

    #[test]
    fn defaults_without_frontmatter() {
        let d = parse_markdown("bare", "just text\nsecond");
        assert_eq!(d.name, "bare");
        assert_eq!(d.type_, "reference");
        assert_eq!(d.description, "just text");
        assert!(d.links.is_empty());
    }
}
