//! Deterministic transcript parsing: rows → byte spans → events + receipt.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::adapters::{self, RawEvent, GENERIC_HOSTS};
use crate::canonical;
use crate::classify::{self, ClassifyInput};
use crate::error::{Result, TranscriptError};
use crate::event::{EventFlags, PrefixReceipt, PrefixReceiptObserved, TranscriptEventV1};
use crate::redact::{compact_text, looks_redacted};
use crate::source::{self, LoadedSource};

/// Default cap applied to the lowest admission class by [`parse`].
pub const CLASS_PRIORITY_CAP: usize = 6;

fn is_supported_host(host: &str) -> bool {
    host == "claude_code" || host == "codex" || GENERIC_HOSTS.contains(&host)
}

/// Compute the frozen prefix receipt (host/session/transcript identity, prefix
/// length + digest, parser digest). Fails closed on zero complete rows.
fn read_prefix_receipt(
    path: &Path,
    host: &str,
    transcript_id: &str,
    source: &LoadedSource,
) -> Result<(PrefixReceipt, String)> {
    // Session id: first resolvable id in the prefix.
    let mut session_id = String::new();
    for row in &source.rows {
        let obj = &row.value;
        let Some(map) = obj.as_object() else { continue };
        if host == "codex" {
            if let Some(payload) = map.get("payload").filter(|p| p.is_object()) {
                let sid = payload
                    .get("id")
                    .or_else(|| payload.get("session_id"))
                    .and_then(serde_json::Value::as_str);
                if let Some(sid) = sid {
                    session_id = sid.to_string();
                    break;
                }
            }
        } else {
            let sid = map
                .get("sessionId")
                .or_else(|| map.get("session_id"))
                .or_else(|| map.get("event").and_then(|event| event.get("sessionId")))
                .or_else(|| map.get("event").and_then(|event| event.get("session_id")))
                .or_else(|| {
                    if map.get("type").and_then(serde_json::Value::as_str) == Some("session") {
                        map.get("id")
                    } else {
                        None
                    }
                })
                .or_else(|| map.get("sessionId"))
                .and_then(serde_json::Value::as_str);
            if let Some(sid) = sid {
                session_id = sid.to_string();
                break;
            }
        }
    }

    let total = source.source_len;
    if total < 1 {
        return Err(TranscriptError::NoCompleteRow {
            path: path.to_path_buf(),
        });
    }

    let prefix_digest = source.source_digest.clone();
    if session_id.is_empty() {
        session_id = format!("derived:{host}:{prefix_digest}");
    }
    let receipt = PrefixReceipt {
        host: host.to_string(),
        session_id,
        transcript_id: transcript_id.to_string(),
        prefix_length: total,
        prefix_digest: "sha256:".to_string() + &prefix_digest,
        parser_digest: canonical::parser_digest(),
        parser_version: crate::PARSER_VERSION.to_string(),
    };
    let session_id = receipt.session_id.clone();
    Ok((receipt, session_id))
}

/// Flags for one normalized event (plan 5.1 contract).
fn event_flags(raw: &RawEvent, compacted: &str) -> EventFlags {
    EventFlags {
        synthetic: raw.synthetic,
        meta: raw.meta,
        private_reasoning_omitted: raw.private_reasoning_omitted || raw.kind == "thinking",
        redacted: raw.redacted.unwrap_or_else(|| looks_redacted(compacted)),
        is_error: raw.is_error,
        is_sidechain: raw.is_sidechain.unwrap_or(false),
    }
}

/// Parse the transcript into normalized events plus its prefix receipt.
///
/// When `apply_cap` is true the class-priority cap squeezes only the last
/// class (`successful_readonly`) to [`CLASS_PRIORITY_CAP`] entries; every other
/// class survives intact and output is bucketed by class priority. When false,
/// every source event survives in original order (canonical semantic source).
pub fn parse_transcript(
    path: &Path,
    host: Option<&str>,
    projection: Option<&str>,
    apply_cap: bool,
) -> Result<(Vec<TranscriptEventV1>, PrefixReceipt)> {
    if !path.is_file() {
        return Err(TranscriptError::Missing {
            path: path.to_path_buf(),
        });
    }

    let host: String = match host {
        Some(h) => h.to_string(),
        None => adapters::detect_host(path)
            .map_err(|e| TranscriptError::Inaccessible {
                path: path.to_path_buf(),
                detail: e.to_string(),
            })?
            .to_string(),
    };
    if !is_supported_host(&host) {
        return Err(TranscriptError::UnsupportedHost { host });
    }

    let transcript_id = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let source = source::load(path, &host)?;
    let (receipt, session_id) = read_prefix_receipt(path, &host, &transcript_id, &source)?;

    let projection_label = projection.unwrap_or("default").to_string();
    let mut events: Vec<TranscriptEventV1> = Vec::new();
    let mut sequence: u64 = 0;
    // call_id -> number of prior occurrences (pair-occurrence counter; later
    // tool_results never trample earlier ones).
    let mut pair_occurrence: BTreeMap<String, u64> = BTreeMap::new();
    // Tool linkage: call_id -> ordered event ids of tool_calls seen so far.
    let mut call_index: BTreeMap<String, Vec<String>> = BTreeMap::new();
    // Tool linkage: call_id -> number of tool_results already linked.
    let mut result_counter: BTreeMap<String, u64> = BTreeMap::new();

    for row in &source.rows {
        for (block_index, raw) in adapters::iter_events_for_host(&host, &row.value)
            .into_iter()
            .enumerate()
        {
            sequence += 1;
            let mut text = compact_text(&raw.text);
            if text.is_empty() && raw.private_reasoning_omitted {
                text = "private reasoning omitted".to_string();
            }
            // Mirrors the retired normalizer: an event with no surviving text
            // is recorded only via the prefix receipt, except omission markers
            // required to prove private reasoning was deliberately excluded.
            if text.is_empty() {
                continue;
            }

            let occurrence = raw.call_id.as_ref().map(|cid| {
                let counter = pair_occurrence.entry(cid.clone()).or_insert(0);
                let occ = *counter;
                *counter += 1;
                occ
            });

            let fingerprint_seed = canonical::fingerprint_payload(
                &raw.kind,
                raw.tool.as_deref(),
                raw.call_id.as_deref(),
                occurrence,
                &text,
                raw.timestamp.as_deref(),
            );

            let flags = event_flags(&raw, &text);
            let classification = classify::classify(ClassifyInput {
                kind: &raw.kind,
                tool: raw.tool.as_deref(),
                text: &text,
                is_error: flags.is_error,
            })
            .as_str()
            .to_string();

            let effective_session_id = raw.session_id.as_deref().unwrap_or(&session_id);
            let event_id = canonical::event_id(
                &host,
                effective_session_id,
                row.row_index,
                block_index,
                sequence,
                &raw.kind,
                raw.call_id.as_deref(),
                &fingerprint_seed,
            );

            // Linkage: a tool_result points at the matching tool_call event.
            let mut result_ordinal: Option<u64> = None;
            if raw.kind == "tool_result" {
                if let Some(cid) = &raw.call_id {
                    let counter = result_counter.entry(cid.clone()).or_insert(0);
                    *counter += 1;
                    result_ordinal = Some(*counter - 1);
                }
            }
            let tool_call_event_id = if raw.kind == "tool_result" {
                raw.call_id
                    .as_ref()
                    .and_then(|cid| match call_index.get(cid) {
                        Some(ids) => ids
                            .get(result_ordinal.unwrap_or(0) as usize)
                            .or_else(|| ids.last())
                            .cloned(),
                        None => None,
                    })
            } else {
                None
            };
            if raw.kind == "tool_call" {
                if let Some(cid) = &raw.call_id {
                    call_index
                        .entry(cid.clone())
                        .or_default()
                        .push(event_id.clone());
                }
            }

            events.push(TranscriptEventV1 {
                event_id,
                row_index: row.row_index,
                byte_start: row.byte_start,
                byte_end: row.byte_end,
                block_index,
                sequence,
                kind: raw.kind.clone(),
                role: raw.role.clone(),
                tool: raw.tool.clone(),
                call_id: raw.call_id.clone(),
                occurrence,
                tool_call_event_id,
                text,
                timestamp: raw.timestamp.clone(),
                classification: classification.clone(),
                class_alias: classification,
                projection: projection_label.clone(),
                host: host.clone(),
                session_id: effective_session_id.to_string(),
                transcript_id: transcript_id.clone(),
                parser_digest: receipt.parser_digest.clone(),
                agent_role: raw.agent_role.clone(),
                thread_source: raw.thread_source.clone(),
                parent_thread_id: raw.parent_thread_id.clone(),
                cwd: raw.cwd.clone(),
                repo: raw.repo.clone(),
                synthetic: flags.synthetic,
                meta: flags.meta,
                private_reasoning_omitted: flags.private_reasoning_omitted,
                redacted: flags.redacted,
                flags,
            });
        }
    }

    if events.is_empty() {
        return Err(TranscriptError::NoEvents {
            path: path.to_path_buf(),
            host,
        });
    }

    if apply_cap {
        events = apply_class_priority_cap(events, CLASS_PRIORITY_CAP);
    }
    Ok((events, receipt))
}

/// Bucket events by class priority; only `successful_readonly` is capped (to
/// its LAST entries). All other classes survive intact, in original order.
fn apply_class_priority_cap(events: Vec<TranscriptEventV1>, cap: usize) -> Vec<TranscriptEventV1> {
    use crate::classify::Classification;
    let mut buckets: BTreeMap<Classification, Vec<TranscriptEventV1>> = BTreeMap::new();
    for ev in events {
        buckets.entry(ev.classification()).or_default().push(ev);
    }
    let mut ordered = Vec::new();
    for cls in Classification::ALL {
        let Some(mut bucket) = buckets.remove(&cls) else {
            continue;
        };
        if cls == Classification::SuccessfulReadonly && bucket.len() > cap {
            bucket = bucket.split_off(bucket.len() - cap);
        }
        ordered.extend(bucket);
    }
    ordered
}

// ---- Public entry points ----

fn prepare_path(transcript_path: &Path) -> Result<PathBuf> {
    // Expand a leading `~` without any external runtime dependency.
    let text = transcript_path.to_string_lossy();
    if let Some(rest) = text.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return Ok(PathBuf::from(home).join(rest));
        }
    }
    Ok(transcript_path.to_path_buf())
}

/// Canonical parse entry point: normalized events with the class-priority cap
/// applied (`projection` defaults to `"default"`).
pub fn parse(
    transcript_path: &Path,
    host: Option<&str>,
    projection: Option<&str>,
) -> Result<Vec<TranscriptEventV1>> {
    let path = prepare_path(transcript_path)?;
    Ok(parse_transcript(&path, host, projection, true)?.0)
}

/// Canonical semantic source: every normalized event in original transcript
/// order with byte spans preserved — never applies the projection cap.
pub fn parse_source_events(
    transcript_path: &Path,
    host: Option<&str>,
) -> Result<Vec<TranscriptEventV1>> {
    let path = prepare_path(transcript_path)?;
    Ok(parse_transcript(&path, host, None, false)?.0)
}

/// The frozen prefix receipt binding what a consumer saw, plus the number of
/// events observed inside the bound prefix.
pub fn parse_prefix_receipt(
    transcript_path: &Path,
    host: Option<&str>,
) -> Result<PrefixReceiptObserved> {
    let path = prepare_path(transcript_path)?;
    let (events, receipt) = parse_transcript(&path, host, None, true)?;
    Ok(PrefixReceiptObserved {
        receipt,
        events_observed: events.len(),
    })
}

/// Exact-match session resolver (substring containment explicitly rejected).
/// A candidate matches only when `requested == candidate.session_id`, or — as
/// a last resort, only when `requested` equals the candidate's file stem.
pub fn resolve_session<'a>(
    requested_session_id: &str,
    candidates: &'a [SessionCandidate],
) -> Option<&'a SessionCandidate> {
    if requested_session_id.is_empty() {
        return None;
    }
    candidates
        .iter()
        .find(|c| c.session_id == requested_session_id)
        .or_else(|| {
            candidates.iter().find(|c| {
                Path::new(&c.path)
                    .file_stem()
                    .map(|s| s == requested_session_id)
                    .unwrap_or(false)
            })
        })
}

/// A session candidate offered to [`resolve_session`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionCandidate {
    pub session_id: String,
    pub path: PathBuf,
}
