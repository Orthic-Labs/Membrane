//! Open Knowledge Format v0.1 bundle support.
//!
//! Extracted from the config crate's okf module during R0.2 workspace migration.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const OKF_VERSION: &str = "0.1";

#[derive(Debug, Error)]
pub enum OkfError {
    #[error("io error at {path}: {message}")]
    Io { path: PathBuf, message: String },
    #[error("OKF bundle missing index.md at {0}")]
    MissingIndex(PathBuf),
    #[error("OKF markdown file missing frontmatter: {0}")]
    MissingFrontmatter(PathBuf),
    #[error("OKF markdown file missing required `type` frontmatter: {0}")]
    MissingType(PathBuf),
    #[error("invalid OKF concept name {0:?}")]
    InvalidConceptName(String),
    #[error("invalid OKF bundle: {0}")]
    InvalidBundle(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct OkfLink {
    pub title: String,
    pub target: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct OkfConcept {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub resource: Option<String>,
    pub timestamp: Option<String>,
    pub tags: Vec<String>,
    pub body: String,
    pub links: Vec<OkfLink>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OkfBundle {
    pub version: &'static str,
    pub index: OkfConcept,
    pub concepts: Vec<OkfConcept>,
    pub graph: BTreeMap<String, Vec<OkfLink>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OkfBundleStats {
    pub out_dir: PathBuf,
    pub files: usize,
}

pub fn parse_bundle(dir: impl AsRef<Path>) -> Result<OkfBundle, OkfError> {
    let dir = dir.as_ref();
    let index_path = dir.join("index.md");
    if !index_path.exists() {
        return Err(OkfError::MissingIndex(index_path));
    }

    let index = parse_markdown_concept(&index_path)?;
    if index.kind != "index" {
        return Err(OkfError::InvalidBundle(format!(
            "{} must have `type: index`",
            index_path.display()
        )));
    }

    let mut markdown_files = Vec::new();
    for entry in fs::read_dir(dir).map_err(|err| io_error(dir, err))? {
        let entry = entry.map_err(|err| io_error(dir, err))?;
        let path = entry.path();
        if path.is_file()
            && path.extension().is_some_and(|ext| ext == "md")
            && path.file_name().is_some_and(|name| name != "index.md")
        {
            markdown_files.push(path);
        }
    }
    markdown_files.sort();

    let mut concepts = Vec::new();
    for path in markdown_files {
        concepts.push(parse_markdown_concept(&path)?);
    }

    let graph = concepts
        .iter()
        .map(|concept| (concept.name.clone(), concept.links.clone()))
        .collect();

    Ok(OkfBundle {
        version: OKF_VERSION,
        index,
        concepts,
        graph,
    })
}

pub fn emit_bundle(
    out_dir: impl AsRef<Path>,
    concepts: &[OkfConcept],
) -> Result<OkfBundleStats, OkfError> {
    let out_dir = out_dir.as_ref();
    fs::create_dir_all(out_dir).map_err(|err| io_error(out_dir, err))?;

    for concept in concepts {
        validate_concept_name(&concept.name)?;
        let path = out_dir.join(format!("{}.md", concept.name));
        let content = render_concept(concept, &path)?;
        fs::write(&path, content).map_err(|err| io_error(&path, err))?;
    }

    let index_path = out_dir.join("index.md");
    fs::write(&index_path, render_index(concepts)).map_err(|err| io_error(&index_path, err))?;

    Ok(OkfBundleStats {
        out_dir: out_dir.to_path_buf(),
        files: concepts.len() + 1,
    })
}

fn parse_markdown_concept(path: &Path) -> Result<OkfConcept, OkfError> {
    let content = fs::read_to_string(path).map_err(|err| io_error(path, err))?;
    let (fields, body) = parse_frontmatter(path, &content)?;
    let kind = fields
        .scalars
        .get("type")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| OkfError::MissingType(path.to_path_buf()))?
        .to_string();

    let name = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default()
        .to_string();
    validate_concept_name(&name)?;

    let (body, related_links) = split_related_section(&strip_frontmatter_separator(body));
    let links = if related_links.is_empty() {
        extract_markdown_links(&body)
    } else {
        related_links
    };

    Ok(OkfConcept {
        name,
        kind,
        title: fields.scalars.get("title").cloned(),
        description: fields.scalars.get("description").cloned(),
        resource: fields.scalars.get("resource").cloned(),
        timestamp: fields.scalars.get("timestamp").cloned(),
        tags: fields.lists.get("tags").cloned().unwrap_or_default(),
        body,
        links,
    })
}

#[derive(Debug, Default)]
struct FrontmatterFields {
    scalars: BTreeMap<String, String>,
    lists: BTreeMap<String, Vec<String>>,
}

fn parse_frontmatter(path: &Path, text: &str) -> Result<(FrontmatterFields, String), OkfError> {
    let normalized = text.replace("\r\n", "\n");
    let mut lines = normalized.lines().collect::<Vec<_>>();
    if lines.first().is_none_or(|line| line.trim() != "---") {
        return Err(OkfError::MissingFrontmatter(path.to_path_buf()));
    }
    lines.remove(0);

    let closing = lines
        .iter()
        .position(|line| line.trim() == "---")
        .ok_or_else(|| OkfError::MissingFrontmatter(path.to_path_buf()))?;
    let frontmatter = lines[..closing].join("\n");
    let body = lines[closing + 1..].join("\n");

    Ok((parse_frontmatter_fields(&frontmatter), body))
}

fn parse_frontmatter_fields(text: &str) -> FrontmatterFields {
    let mut fields = FrontmatterFields::default();
    let lines = text.lines().collect::<Vec<_>>();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i].trim();
        if line.is_empty() || line.starts_with('#') {
            i += 1;
            continue;
        }

        if let Some(colon) = line.find(':') {
            let key = line[..colon].trim().to_string();
            let value = line[colon + 1..].trim();
            if key == "tags" {
                fields
                    .lists
                    .insert(key, parse_frontmatter_list(value, &lines, &mut i));
            } else {
                fields.scalars.insert(key, unquote(value).to_string());
            }
        }

        i += 1;
    }

    fields
}

fn parse_frontmatter_list(inline: &str, lines: &[&str], i: &mut usize) -> Vec<String> {
    if inline.starts_with('[') && inline.ends_with(']') {
        return inline[1..inline.len() - 1]
            .split(',')
            .map(|item| unquote(item.trim()).to_string())
            .filter(|item| !item.is_empty())
            .collect();
    }

    if !inline.is_empty() {
        return vec![unquote(inline).to_string()];
    }

    let mut items = Vec::new();
    *i += 1;
    while *i < lines.len() {
        let trimmed = lines[*i].trim();
        if trimmed.is_empty() {
            *i += 1;
            continue;
        }
        if !trimmed.starts_with('-') {
            break;
        }

        let item = unquote(trimmed[1..].trim());
        if !item.is_empty() {
            items.push(item.to_string());
        }
        *i += 1;
    }
    *i = (*i).saturating_sub(1);
    items
}

fn unquote(value: &str) -> &str {
    value.trim().trim_matches('"').trim_matches('\'').trim()
}

fn strip_frontmatter_separator(body: String) -> String {
    if let Some(stripped) = body.strip_prefix('\n') {
        stripped.to_string()
    } else {
        body
    }
}

fn split_related_section(body: &str) -> (String, Vec<OkfLink>) {
    let lines = body.lines().collect::<Vec<_>>();
    for idx in (0..lines.len()).rev() {
        if lines[idx].trim() != "## Related" {
            continue;
        }

        let related_lines = &lines[idx + 1..];
        let only_link_list = related_lines.iter().all(|line| {
            let trimmed = line.trim();
            trimmed.is_empty() || trimmed.starts_with("- [") || trimmed.starts_with("* [")
        });
        if !only_link_list {
            continue;
        }

        let body = lines[..idx].join("\n").trim_end().to_string();
        let links = extract_markdown_links(&related_lines.join("\n"));
        return (body, links);
    }

    (body.trim_end().to_string(), Vec::new())
}

fn render_concept(concept: &OkfConcept, path: &Path) -> Result<String, OkfError> {
    if concept.kind.trim().is_empty() {
        return Err(OkfError::MissingType(path.to_path_buf()));
    }

    let body = concept.body.clone();

    let mut out = String::new();
    out.push_str(&render_frontmatter(concept));
    out.push('\n');
    out.push_str(body.trim_end());
    out.push('\n');

    if !concept.links.is_empty() {
        out.push_str("\n## Related\n");
        for link in &concept.links {
            out.push_str(&format!("- [{}]({})\n", link.title, link.target));
        }
    }

    Ok(out)
}

fn render_frontmatter(concept: &OkfConcept) -> String {
    let mut lines = vec!["---".to_string(), format!("type: {}", concept.kind.trim())];

    for (key, value) in [
        ("title", concept.title.as_deref()),
        ("description", concept.description.as_deref()),
        ("resource", concept.resource.as_deref()),
        ("timestamp", concept.timestamp.as_deref()),
    ] {
        if let Some(value) = value.filter(|value| !value.trim().is_empty()) {
            lines.push(format!("{key}: {}", value.trim()));
        }
    }

    if !concept.tags.is_empty() {
        lines.push(format!("tags: [{}]", concept.tags.join(", ")));
    }

    lines.push("---".to_string());
    lines.join("\n")
}

fn render_index(concepts: &[OkfConcept]) -> String {
    let mut by_type: BTreeMap<&str, Vec<&OkfConcept>> = BTreeMap::new();
    for concept in concepts {
        by_type
            .entry(concept.kind.as_str())
            .or_default()
            .push(concept);
    }

    let mut lines = vec![
        "---".to_string(),
        "type: index".to_string(),
        "title: Index".to_string(),
        "---".to_string(),
        String::new(),
        "# Index".to_string(),
        String::new(),
    ];

    for (kind, concepts) in by_type {
        lines.push(format!("## {kind}"));
        for concept in concepts {
            let title = concept.title.as_deref().unwrap_or(&concept.name);
            let description = concept
                .description
                .as_deref()
                .filter(|value| !value.is_empty())
                .map(|value| format!(" - {value}"))
                .unwrap_or_default();
            lines.push(format!("- [{title}]({}.md){description}", concept.name));
        }
        lines.push(String::new());
    }

    lines.join("\n")
}

fn extract_markdown_links(text: &str) -> Vec<OkfLink> {
    let mut links = Vec::new();
    let mut search_from = 0;

    while let Some(open_rel) = text[search_from..].find('[') {
        let open = search_from + open_rel;
        if open > 0 && text.as_bytes()[open - 1] == b'!' {
            search_from = open + 1;
            continue;
        }

        let Some(close_rel) = text[open + 1..].find(']') else {
            break;
        };
        let close = open + 1 + close_rel;
        if !text[close + 1..].starts_with('(') {
            search_from = close + 1;
            continue;
        }

        let target_start = close + 2;
        let Some(target_end_rel) = text[target_start..].find(')') else {
            break;
        };
        let target_end = target_start + target_end_rel;
        let title = &text[open + 1..close];
        let target = &text[target_start..target_end];
        if !title.is_empty() && !target.is_empty() {
            links.push(OkfLink {
                title: title.to_string(),
                target: target.to_string(),
            });
        }
        search_from = target_end + 1;
    }

    links
}

fn validate_concept_name(name: &str) -> Result<(), OkfError> {
    if name.trim().is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
    {
        return Err(OkfError::InvalidConceptName(name.to_string()));
    }
    Ok(())
}

fn io_error(path: &Path, err: std::io::Error) -> OkfError {
    OkfError::Io {
        path: path.to_path_buf(),
        message: err.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn concept(name: &str, kind: &str, body: &str) -> OkfConcept {
        OkfConcept {
            name: name.to_string(),
            kind: kind.to_string(),
            title: Some(name.to_string()),
            description: Some(format!("{name} description")),
            body: body.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn okf_bundle_round_trips_concepts_index_and_link_graph() {
        let dir = tempfile::tempdir().unwrap();
        let mut overview = concept("overview", "guide", "See [Details](details.md).");
        overview.links = vec![OkfLink {
            title: "Details".to_string(),
            target: "details.md".to_string(),
        }];
        let details = concept("details", "reference", "Implementation notes.");

        let stats = emit_bundle(dir.path(), &[overview.clone(), details.clone()]).unwrap();
        assert_eq!(stats.files, 3);

        let bundle = parse_bundle(dir.path()).unwrap();

        assert_eq!(bundle.version, OKF_VERSION);
        assert_eq!(bundle.index.kind, "index");
        assert_eq!(bundle.concepts.len(), 2);
        assert_eq!(bundle.concepts[0].name, "details");
        assert_eq!(bundle.concepts[0].kind, "reference");
        assert_eq!(bundle.concepts[1].name, "overview");
        assert_eq!(bundle.concepts[1].kind, "guide");
        assert_eq!(bundle.concepts[1].body, overview.body);
        assert_eq!(
            bundle.graph.get("overview").unwrap(),
            &[OkfLink {
                title: "Details".to_string(),
                target: "details.md".to_string(),
            }]
        );
        assert!(dir.path().join("index.md").exists());
    }

    #[test]
    fn okf_parse_requires_type_frontmatter_for_every_markdown_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("index.md"),
            "---\ntype: index\ntitle: Index\n---\n\n# Index\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("missing-type.md"),
            "---\ntitle: Missing Type\n---\n\nBody\n",
        )
        .unwrap();

        let err = parse_bundle(dir.path()).unwrap_err();

        assert!(matches!(err, OkfError::MissingType(path) if path.ends_with("missing-type.md")));
    }
}
