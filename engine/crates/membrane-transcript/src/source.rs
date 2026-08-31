//! Native transcript discovery plus file/database source loading.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::error::{Result, TranscriptError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredTranscript {
    pub host: String,
    pub path: PathBuf,
}

#[derive(Debug)]
pub(crate) struct SourceRow {
    pub row_index: u64,
    pub byte_start: u64,
    pub byte_end: u64,
    pub value: Value,
}

#[derive(Debug)]
pub(crate) struct LoadedSource {
    pub source_len: u64,
    pub source_digest: String,
    pub rows: Vec<SourceRow>,
}

fn loaded(bytes: Vec<u8>, rows: Vec<SourceRow>) -> LoadedSource {
    LoadedSource {
        source_len: bytes.len() as u64,
        source_digest: hex::encode(Sha256::digest(&bytes)),
        rows,
    }
}

fn fingerprint_file(path: &Path) -> Result<(u64, String)> {
    let mut file = fs::File::open(path).map_err(|error| inaccessible(path, error))?;
    let mut digest = Sha256::new();
    let mut length = 0u64;
    let mut chunk = [0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut chunk)
            .map_err(|error| inaccessible(path, error))?;
        if read == 0 {
            break;
        }
        digest.update(&chunk[..read]);
        length += read as u64;
    }
    Ok((length, hex::encode(digest.finalize())))
}

fn inaccessible(path: &Path, error: impl ToString) -> TranscriptError {
    TranscriptError::Inaccessible {
        path: path.to_path_buf(),
        detail: error.to_string(),
    }
}

fn malformed(
    path: &Path,
    row_index: u64,
    start: u64,
    end: u64,
    detail: impl ToString,
) -> TranscriptError {
    TranscriptError::MalformedRow {
        path: path.to_path_buf(),
        row_index,
        byte_start: start,
        byte_end: end,
        detail: detail.to_string(),
    }
}

fn load_jsonl(path: &Path, bytes: Vec<u8>) -> Result<LoadedSource> {
    let mut rows = Vec::new();
    let mut start = 0usize;
    for (index, chunk) in bytes.split_inclusive(|byte| *byte == b'\n').enumerate() {
        if chunk.is_empty() {
            continue;
        }
        let end = start + chunk.len();
        let text = std::str::from_utf8(chunk)
            .map_err(|error| malformed(path, index as u64 + 1, start as u64, end as u64, error))?;
        let value: Value = serde_json::from_str(text)
            .map_err(|error| malformed(path, index as u64 + 1, start as u64, end as u64, error))?;
        if !value.is_object() {
            return Err(malformed(
                path,
                index as u64 + 1,
                start as u64,
                end as u64,
                "expected a JSON object",
            ));
        }
        rows.push(SourceRow {
            row_index: index as u64 + 1,
            byte_start: start as u64,
            byte_end: end as u64,
            value,
        });
        start = end;
    }
    if rows.is_empty() {
        return Err(TranscriptError::NoCompleteRow {
            path: path.to_path_buf(),
        });
    }
    Ok(loaded(bytes, rows))
}

fn load_json_document(path: &Path, bytes: Vec<u8>) -> Result<LoadedSource> {
    if bytes.is_empty() {
        return Err(TranscriptError::NoCompleteRow {
            path: path.to_path_buf(),
        });
    }
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|error| malformed(path, 1, 0, bytes.len() as u64, error))?;
    if !value.is_object() && !value.is_array() {
        return Err(malformed(
            path,
            1,
            0,
            bytes.len() as u64,
            "expected a JSON object or array",
        ));
    }
    let row = SourceRow {
        row_index: 1,
        byte_start: 0,
        byte_end: bytes.len() as u64,
        value,
    };
    Ok(loaded(bytes, vec![row]))
}

fn json_text(value: rusqlite::types::ValueRef<'_>) -> rusqlite::Result<String> {
    match value {
        rusqlite::types::ValueRef::Text(bytes) | rusqlite::types::ValueRef::Blob(bytes) => {
            Ok(String::from_utf8_lossy(bytes).into_owned())
        }
        _ => Ok(String::new()),
    }
}

fn load_opencode(path: &Path, source_len: u64, source_digest: String) -> Result<LoadedSource> {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| inaccessible(path, error))?;
    let mut sessions = Vec::new();
    let mut statement = connection.prepare(
        "SELECT id, directory, parent_id, agent, model, time_created FROM session WHERE time_archived IS NULL ORDER BY time_created, id"
    ).map_err(|error| inaccessible(path, error))?;
    let session_rows = statement
        .query_map([], |row| {
            Ok(json!({
                "id": row.get::<_, String>(0)?,
                "cwd": row.get::<_, Option<String>>(1)?,
                "parent_id": row.get::<_, Option<String>>(2)?,
                "agent": row.get::<_, Option<String>>(3)?,
                "model": row.get::<_, Option<String>>(4)?,
                "time_created": row.get::<_, i64>(5)?,
            }))
        })
        .map_err(|error| inaccessible(path, error))?;
    for session in session_rows {
        let mut session = session.map_err(|error| inaccessible(path, error))?;
        let session_id = session["id"].as_str().unwrap_or_default().to_string();
        let mut message_statement = connection.prepare(
            "SELECT m.id, m.time_created, m.data, p.id, p.time_created, p.data FROM message m LEFT JOIN part p ON p.message_id = m.id WHERE m.session_id = ? ORDER BY m.time_created, m.id, p.time_created, p.id"
        ).map_err(|error| inaccessible(path, error))?;
        let records = message_statement.query_map([&session_id], |row| Ok(json!({
            "message_id": row.get::<_, String>(0)?,
            "message_time": row.get::<_, i64>(1)?,
            "message": json_text(row.get_ref(2)?)?,
            "part_id": row.get::<_, Option<String>>(3)?,
            "part_time": row.get::<_, Option<i64>>(4)?,
            "part": match row.get_ref(5)? { rusqlite::types::ValueRef::Null => String::new(), value => json_text(value)? },
        }))).map_err(|error| inaccessible(path, error))?;
        let mut values = Vec::new();
        for record in records {
            values.push(record.map_err(|error| inaccessible(path, error))?);
        }
        session
            .as_object_mut()
            .unwrap()
            .insert("records".into(), Value::Array(values));
        sessions.push(session);
    }
    let value = json!({"type":"native_database","host":"opencode","sessions":sessions});
    let row = SourceRow {
        row_index: 1,
        byte_start: 0,
        byte_end: source_len,
        value,
    };
    Ok(LoadedSource {
        source_len,
        source_digest,
        rows: vec![row],
    })
}

fn load_cursor(path: &Path, source_len: u64, source_digest: String) -> Result<LoadedSource> {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| inaccessible(path, error))?;
    let mut sessions = Vec::new();
    let mut statement = connection.prepare(
        "SELECT composerId, workspaceId, createdAt, value FROM composerHeaders WHERE isArchived = 0 AND isSubagent = 0 ORDER BY createdAt, composerId"
    ).map_err(|error| inaccessible(path, error))?;
    let headers = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<i64>>(2)?,
                json_text(row.get_ref(3)?)?,
            ))
        })
        .map_err(|error| inaccessible(path, error))?;
    for header in headers {
        let (id, workspace_id, created_at, value) =
            header.map_err(|error| inaccessible(path, error))?;
        let mut bubble_statement = connection
            .prepare("SELECT key, value FROM cursorDiskKV WHERE key LIKE ? ORDER BY key")
            .map_err(|error| inaccessible(path, error))?;
        let pattern = format!("bubbleId:{id}:%");
        let bubbles = bubble_statement
            .query_map([pattern], |row| {
                Ok(json!({
                    "key": row.get::<_, String>(0)?, "value": json_text(row.get_ref(1)?)?
                }))
            })
            .map_err(|error| inaccessible(path, error))?;
        let mut values = Vec::new();
        for bubble in bubbles {
            values.push(bubble.map_err(|error| inaccessible(path, error))?);
        }
        sessions.push(json!({"id":id,"workspace_id":workspace_id,"created_at":created_at,"header":value,"bubbles":values}));
    }
    let value = json!({"type":"native_database","host":"cursor","sessions":sessions});
    let row = SourceRow {
        row_index: 1,
        byte_start: 0,
        byte_end: source_len,
        value,
    };
    Ok(LoadedSource {
        source_len,
        source_digest,
        rows: vec![row],
    })
}

pub(crate) fn load(path: &Path, host: &str) -> Result<LoadedSource> {
    match host {
        "opencode" => {
            let (len, digest) = fingerprint_file(path)?;
            load_opencode(path, len, digest)
        }
        "cursor" => {
            let (len, digest) = fingerprint_file(path)?;
            load_cursor(path, len, digest)
        }
        "cline" | "roo_cline" => load_json_document(
            path,
            fs::read(path).map_err(|error| inaccessible(path, error))?,
        ),
        "gemini" if path.extension().and_then(|value| value.to_str()) == Some("json") => {
            load_json_document(
                path,
                fs::read(path).map_err(|error| inaccessible(path, error))?,
            )
        }
        _ => load_jsonl(
            path,
            fs::read(path).map_err(|error| inaccessible(path, error))?,
        ),
    }
}

fn collect_files(root: &Path, accept: &dyn Fn(&Path) -> bool, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, accept, out);
        } else if accept(&path) {
            out.push(path);
        }
    }
}

fn add_tree(
    out: &mut Vec<DiscoveredTranscript>,
    host: &str,
    root: PathBuf,
    accept: impl Fn(&Path) -> bool,
) {
    let mut paths = Vec::new();
    collect_files(&root, &accept, &mut paths);
    out.extend(paths.into_iter().map(|path| DiscoveredTranscript {
        host: host.into(),
        path,
    }));
}

/// Discover native transcript stores under `home`. Database hosts expose one
/// source whose loader selects only open root sessions.
pub fn discover_open(home: &Path) -> Vec<DiscoveredTranscript> {
    let mut out = Vec::new();
    add_tree(
        &mut out,
        "claude_code",
        home.join(".claude/projects"),
        |p| p.extension().and_then(|x| x.to_str()) == Some("jsonl"),
    );
    add_tree(&mut out, "codex", home.join(".codex/sessions"), |p| {
        p.extension().and_then(|x| x.to_str()) == Some("jsonl")
    });
    add_tree(
        &mut out,
        "copilot",
        home.join(".copilot/session-state"),
        |p| p.extension().and_then(|x| x.to_str()) == Some("jsonl"),
    );
    add_tree(
        &mut out,
        "antigravity",
        home.join(".gemini/antigravity/brain"),
        |p| {
            p.extension().and_then(|x| x.to_str()) == Some("jsonl")
                && p.to_string_lossy().replace('\\', "/").contains("/.system_generated/logs/")
        },
    );
    add_tree(
        &mut out,
        "command_code",
        home.join(".commandcode/projects"),
        |p| {
            p.extension().and_then(|x| x.to_str()) == Some("jsonl")
                && !p.to_string_lossy().ends_with(".checkpoints.jsonl")
        },
    );
    add_tree(&mut out, "cline", home.join(".cline/data/sessions"), |p| {
        p.to_string_lossy().ends_with(".messages.json")
    });
    add_tree(&mut out, "pi", home.join(".pi/agent/sessions"), |p| {
        p.extension().and_then(|x| x.to_str()) == Some("jsonl")
    });
    add_tree(&mut out, "qwen", home.join(".qwen/sessions"), |p| {
        p.extension().and_then(|x| x.to_str()) == Some("jsonl")
    });
    add_tree(&mut out, "qwen", home.join(".qwen-code/sessions"), |p| {
        p.extension().and_then(|x| x.to_str()) == Some("jsonl")
    });
    add_tree(&mut out, "gemini", home.join(".gemini/tmp"), |p| {
        p.file_name().and_then(|x| x.to_str()).is_some_and(|x| {
            x.starts_with("session-") && (x.ends_with(".json") || x.ends_with(".jsonl"))
        })
    });
    add_tree(&mut out, "grok_build", home.join(".grok/sessions"), |p| {
        p.file_name().and_then(|x| x.to_str()) == Some("chat_history.jsonl")
    });
    for root in [
        home.join("AppData/Roaming/Cursor/User/globalStorage/rooveterinaryinc.roo-cline/tasks"),
        home.join("Library/Application Support/Cursor/User/globalStorage/rooveterinaryinc.roo-cline/tasks"),
        home.join("Library/Application Support/Code/User/globalStorage/rooveterinaryinc.roo-cline/tasks"),
    ] { add_tree(&mut out, "roo_cline", root, |p| p.file_name().and_then(|x| x.to_str()) == Some("api_conversation_history.json")); }
    for (host, path) in [
        ("opencode", home.join(".local/share/opencode/opencode.db")),
        (
            "cursor",
            home.join("Library/Application Support/Cursor/User/globalStorage/state.vscdb"),
        ),
    ] {
        if path.is_file() {
            out.push(DiscoveredTranscript {
                host: host.into(),
                path,
            });
        }
    }
    out.sort_by(|a, b| a.host.cmp(&b.host).then(a.path.cmp(&b.path)));
    out.dedup();
    out
}
