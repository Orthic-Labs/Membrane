//! Guide's generated per-session ledger document.
//!
//! A ledger is a hash-bound document projection, not durable memory. It links human-readable
//! tasks, artifacts, and decisions while retaining source cursor and derivation metadata.

use super::doc_projection::{
    replace_doc_projections, DocumentProjectionStoreInputV1, DocumentProjectionV1,
    ProjectionKind, ProjectionProvenanceV1,
};
use super::GuideDb;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const SESSION_LEDGER_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LedgerSourceCursor {
    pub session_id: String,
    pub last_seq: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LedgerEventV1 {
    pub event_id: String,
    pub seq: u64,
    pub event_type: String,
    pub content: String,
    pub occurred_at_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LedgerTaskV1 {
    pub task_id: String,
    pub title: String,
    pub status: String,
    pub link: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LedgerArtifactV1 {
    pub artifact_id: String,
    pub handle: String,
    pub media_type: String,
    pub link: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LedgerDecisionV1 {
    pub decision_id: String,
    pub title: String,
    pub content: String,
    pub link: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionLedgerInputV1 {
    pub session_id: String,
    pub title: Option<String>,
    pub source_cursor: LedgerSourceCursor,
    pub source_content_hash: String,
    pub events: Vec<LedgerEventV1>,
    #[serde(default)]
    pub tasks: Vec<LedgerTaskV1>,
    #[serde(default)]
    pub artifacts: Vec<LedgerArtifactV1>,
    #[serde(default)]
    pub decisions: Vec<LedgerDecisionV1>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LedgerLinkV1 {
    pub kind: String,
    pub id: String,
    pub target: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionLedgerDocumentV1 {
    pub schema_version: u32,
    pub document_id: String,
    pub session_id: String,
    pub title: String,
    pub markdown: String,
    pub source_cursor: LedgerSourceCursor,
    pub source_content_hash: String,
    pub derivation: String,
    pub invalidation_parent: String,
    pub content_hash: String,
    pub links: Vec<LedgerLinkV1>,
    pub omissions: Vec<String>,
    pub generated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum LedgerError {
    #[error("ledger session identity is empty or mismatched")]
    SessionMismatch,
    #[error("ledger source cursor is invalid")]
    InvalidCursor,
    #[error("ledger source content hash is empty")]
    MissingSourceHash,
    #[error("guide ledger projection: {0}")]
    Projection(String),
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SessionLedgerBuilder;

impl SessionLedgerBuilder {
    pub fn build(
        &self,
        input: &SessionLedgerInputV1,
    ) -> Result<SessionLedgerDocumentV1, LedgerError> {
        if input.session_id.trim().is_empty()
            || input.source_cursor.session_id != input.session_id
            || input.source_content_hash.trim().is_empty()
        {
            return Err(if input.source_content_hash.trim().is_empty() {
                LedgerError::MissingSourceHash
            } else {
                LedgerError::SessionMismatch
            });
        }
        let mut events = input.events.clone();
        events.sort_by_key(|event| (event.seq, event.event_id.clone()));
        let mut omissions = Vec::new();
        let mut expected = events.first().map(|event| event.seq).unwrap_or(1);
        if expected > 1 {
            omissions.push(format!("missing event sequence 1..{expected}"));
        }
        for event in &events {
            if event.seq != expected {
                if event.seq > expected {
                    omissions.push(format!("missing event sequence {expected}..{}", event.seq));
                } else {
                    omissions.push(format!("event sequence {} is out of order", event.seq));
                }
            }
            expected = event.seq.saturating_add(1);
        }
        if expected <= input.source_cursor.last_seq {
            omissions.push(format!(
                "missing event sequence {}..{}",
                expected,
                input.source_cursor.last_seq.saturating_add(1)
            ));
        }
        let mut markdown = String::new();
        let title = input
            .title
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| format!("Session {}", input.session_id));
        markdown.push_str(&format!("# {}\n\n", markdown_text(&title)));
        markdown.push_str(&format!(
            "- Session: `{}`\n- Source cursor: `{}`\n- Source hash: `{}`\n- Derivation: deterministic event/task/artifact/decision projection\n\n",
            input.session_id,
            input.source_cursor.last_seq,
            input.source_content_hash
        ));
        markdown.push_str("## Handoff\n\n");
        if events.is_empty() {
            markdown.push_str("No session events were available.\n\n");
        } else {
            markdown.push_str("Latest recorded events:\n\n");
            for event in events.iter().rev().take(8).rev() {
                markdown.push_str(&format!(
                    "- `{}` {}: {}\n",
                    event.seq,
                    markdown_text(&event.event_type),
                    markdown_text(&event.content)
                ));
            }
            markdown.push('\n');
        }
        markdown.push_str("## Tasks\n\n");
        for task in &input.tasks {
            markdown.push_str(&format!(
                "- [{}] {}{}\n",
                markdown_text(&task.status),
                markdown_text(&task.title),
                link_suffix(task.link.as_deref())
            ));
        }
        if input.tasks.is_empty() {
            markdown.push_str("- None recorded\n");
        }
        markdown.push_str("\n## Artifacts\n\n");
        for artifact in &input.artifacts {
            markdown.push_str(&format!(
                "- `{}` ({}){}\n",
                markdown_text(&artifact.handle),
                markdown_text(&artifact.media_type),
                link_suffix(artifact.link.as_deref())
            ));
        }
        if input.artifacts.is_empty() {
            markdown.push_str("- None recorded\n");
        }
        markdown.push_str("\n## Decisions\n\n");
        for decision in &input.decisions {
            markdown.push_str(&format!(
                "- **{}**: {}{}\n",
                markdown_text(&decision.title),
                markdown_text(&decision.content),
                link_suffix(decision.link.as_deref())
            ));
        }
        if input.decisions.is_empty() {
            markdown.push_str("- None recorded\n");
        }
        if !omissions.is_empty() {
            markdown.push_str("\n## Omissions\n\n");
            for omission in &omissions {
                markdown.push_str(&format!("- {}\n", markdown_text(omission)));
            }
        }
        let links = links(input);
        let content_hash = sha256(&markdown);
        Ok(SessionLedgerDocumentV1 {
            schema_version: SESSION_LEDGER_SCHEMA_VERSION,
            document_id: format!("session-ledger:{}", input.session_id),
            session_id: input.session_id.clone(),
            title,
            markdown,
            source_cursor: input.source_cursor.clone(),
            source_content_hash: input.source_content_hash.clone(),
            derivation: "deterministic event/task/artifact/decision projection".to_owned(),
            invalidation_parent: format!(
                "session://{}/cursor/{}",
                input.session_id, input.source_cursor.last_seq
            ),
            content_hash,
            links,
            omissions,
            generated: true,
        })
    }
}

pub fn build_session_ledger(
    input: &SessionLedgerInputV1,
) -> Result<SessionLedgerDocumentV1, LedgerError> {
    SessionLedgerBuilder::default().build(input)
}

impl SessionLedgerDocumentV1 {
    pub fn invalidated_by(&self, cursor: &LedgerSourceCursor, source_hash: &str) -> bool {
        self.session_id != cursor.session_id
            || cursor.last_seq > self.source_cursor.last_seq
            || source_hash != self.source_content_hash
    }
}

/// Store generated Markdown through Guide's existing hash-bound projection mechanism.
pub fn index_session_ledger(
    db: &GuideDb,
    document: &SessionLedgerDocumentV1,
    source_revision: &str,
    index_generation: i64,
) -> Result<(), LedgerError> {
    let input = DocumentProjectionStoreInputV1 {
        parent_doc_id: document.document_id.clone(),
        source_content_hash: document.source_content_hash.clone(),
        source_revision: source_revision.to_owned(),
        index_generation,
        projections: vec![DocumentProjectionV1 {
            kind: ProjectionKind::Lexical,
            content: document.markdown.clone(),
            token_count: document.markdown.split_whitespace().count(),
            provenance: ProjectionProvenanceV1 {
                anchor_id: "document".to_owned(),
                collapsed_to_parent: None,
            },
        }],
    };
    replace_doc_projections(db, &input).map_err(|error| LedgerError::Projection(error.to_string()))
}

fn links(input: &SessionLedgerInputV1) -> Vec<LedgerLinkV1> {
    let mut links = Vec::new();
    for task in &input.tasks {
        links.push(LedgerLinkV1 {
            kind: "task".to_owned(),
            id: task.task_id.clone(),
            target: task.link.clone().unwrap_or_else(|| format!("task://{}", task.task_id)),
        });
    }
    for artifact in &input.artifacts {
        links.push(LedgerLinkV1 {
            kind: "artifact".to_owned(),
            id: artifact.artifact_id.clone(),
            target: artifact
                .link
                .clone()
                .unwrap_or_else(|| format!("artifact://{}", artifact.artifact_id)),
        });
    }
    for decision in &input.decisions {
        links.push(LedgerLinkV1 {
            kind: "decision".to_owned(),
            id: decision.decision_id.clone(),
            target: decision
                .link
                .clone()
                .unwrap_or_else(|| format!("decision://{}", decision.decision_id)),
        });
    }
    links.sort_by(|left, right| {
        left.kind
            .cmp(&right.kind)
            .then_with(|| left.id.cmp(&right.id))
            .then_with(|| left.target.cmp(&right.target))
    });
    links
}

fn markdown_text(value: &str) -> String {
    value.replace('\n', " ").replace('\r', " ").replace('`', "'")
}

fn link_suffix(link: Option<&str>) -> String {
    link.filter(|value| !value.trim().is_empty())
        .map(|value| format!(" ([link]({}))", markdown_text(value)))
        .unwrap_or_default()
}

fn sha256(value: &str) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(value.as_bytes())))
}
