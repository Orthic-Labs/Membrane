//! Read-only Cortex relevance spot-check harness.
//!
//! This ports the predecessor's sampling, strict/useful judging, and shadow-ranking
//! semantics without its retired HTTP dependency. Recall is replayed through
//! [`MemoryStore::recall_scored_detailed`], which does not log a recall or increment
//! injection counters.

use crate::{MemDb, MemoryStore};
use cortex_core::MemoryEntry;
use membrane_transcript::{discover_open, parse_source_events};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

pub const REPORT_SCHEMA: &str = "cortex.relevance-spotcheck.v1";
pub const DEFAULT_SAMPLE_SIZE: usize = 20;
pub const TOP_K: usize = 5;
const MIN_QUERY_CHARS: usize = 10;
static REPORT_TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(1);
const STOP: &[&str] = &[
    "the",
    "and",
    "with",
    "that",
    "this",
    "from",
    "into",
    "your",
    "create",
    "make",
    "file",
    "then",
    "have",
    "should",
    "would",
    "could",
    "about",
    "yeah",
    "please",
    "sorry",
    "okay",
    "now",
    "finally",
    "back",
    "want",
    "need",
    "done",
    "start",
    "check",
    "ahead",
    "everything",
    "how",
    "are",
    "was",
    "were",
    "been",
    "being",
    "does",
    "did",
    "doing",
    "looking",
    "going",
    "got",
    "through",
    "using",
    "take",
    "word",
    "right",
    "best",
    "issue",
    "thing",
    "things",
    "none",
    "what",
    "why",
    "when",
    "where",
    "who",
];

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RecallSampleV1 {
    pub ts: String,
    pub scope: String,
    pub query: String,
    pub client: String,
    pub session_id: String,
    pub source: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct ThresholdSweepRowV1 {
    pub percentile: u8,
    pub cosine: f32,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct ThresholdCalibrationV1 {
    pub relevant: f32,
    pub partial: f32,
    pub provenance: String,
    pub distribution_size: usize,
    pub sweep: Vec<ThresholdSweepRowV1>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum VerdictV1 {
    Relevant,
    Partial,
    Irrelevant,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RankedCandidateV1 {
    pub ranker: String,
    pub id: String,
    pub verdict: VerdictV1,
    pub overlap: f32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ShadowReportV1 {
    pub bm25: Vec<String>,
    pub cosine_proxy: Vec<String>,
    pub hybrid_proxy: Vec<String>,
    pub novel_candidates: Vec<RankedCandidateV1>,
    pub would_have_helped: Vec<RankedCandidateV1>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InjectedPreviewV1 {
    pub id: String,
    pub cosine: f32,
    pub preview: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SampleEvaluationV1 {
    pub ts: String,
    pub scope: String,
    pub client: String,
    pub session_id: String,
    pub source: String,
    pub query: String,
    pub top_injected: Vec<InjectedPreviewV1>,
    pub verdict: VerdictV1,
    pub shadow: ShadowReportV1,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SkipCountsV1 {
    pub total: usize,
    pub by_reason: BTreeMap<String, usize>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MeasurementOnlyV1 {
    pub observing_recall: bool,
    pub traffic_class: String,
    pub production_injection_count: u64,
    pub recall_rows_added: i64,
    pub injection_counter_delta: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RelevanceSpotcheckReportV1 {
    pub schema: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub generated_at: String,
    pub query_source: String,
    pub sample_size_requested: usize,
    pub sample_size_attempted: usize,
    pub sample_size_evaluated: usize,
    pub skipped: SkipCountsV1,
    pub fetch_errors: usize,
    pub verdicts: BTreeMap<String, usize>,
    pub relevance_rate: f64,
    pub useful_rate: f64,
    pub thresholds: ThresholdCalibrationV1,
    pub variance_status: String,
    pub judge_basis: String,
    pub shadow: BTreeMap<String, usize>,
    pub recommended_action: String,
    pub confidence: String,
    pub measurement_only: MeasurementOnlyV1,
    pub samples: Vec<SampleEvaluationV1>,
}

#[derive(Clone, Debug)]
struct StoreCounters {
    recall_rows: i64,
    injection_sum: i64,
}

#[derive(Clone, Debug, Deserialize)]
struct FixtureQueries {
    queries: Vec<FixtureQuery>,
}

#[derive(Clone, Debug, Deserialize)]
struct FixtureQuery {
    query: String,
    scope: String,
    source: String,
}

/// Port of the predecessor's phase-3 prompt hygiene classifier. A useful
/// task-notification is unwrapped; wrapper-only or divider input is skipped.
pub fn classify_query(query: &str) -> Result<String, &'static str> {
    let trimmed = query.trim();
    if trimmed.chars().count() < 4 {
        return Err("empty_or_short");
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("<task-notification") {
        let Some(start) = trimmed.find('>') else {
            return Err("task_notification_empty");
        };
        let Some(end) = lower.rfind("</task-notification>") else {
            return Err("task_notification_empty");
        };
        if end <= start {
            return Err("task_notification_empty");
        }
        let inner = &trimmed[start + 1..end];
        let words = tokens(inner)
            .into_iter()
            .filter(|word| {
                let is_hex_id = word.chars().count() >= 8
                    && word.chars().all(|ch| ch.is_ascii_hexdigit());
                !is_hex_id
                    && !matches!(
                        word.as_str(),
                        "task"
                            | "id"
                            | "notification"
                            | "tool"
                            | "use"
                            | "output"
                            | "file"
                            | "call"
                            | "users"
                            | "adrds"
                            | "appdata"
                            | "local"
                            | "temp"
                            | "claude"
                    )
            })
            .collect::<Vec<_>>();
        if words.len() < 2 {
            return Err("task_notification_empty");
        }
        return Ok(inner.trim().to_string());
    }
    if trimmed.contains("в”") || trimmed.contains("â”") {
        return Err("mojibake_divider");
    }
    let non_whitespace = trimmed.chars().filter(|ch| !ch.is_whitespace()).count();
    let divider = trimmed
        .chars()
        .filter(|ch| {
            matches!(
                ch,
                '─' | '━' | '═' | '—' | '-' | '_' | '=' | '*' | '•' | '·' | '.'
            )
        })
        .count();
    if non_whitespace > 0
        && trimmed.chars().count() < 200
        && divider as f64 / non_whitespace as f64 >= 0.6
    {
        return Err("divider_or_box");
    }
    let nonempty_lines = trimmed
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>();
    if !nonempty_lines.is_empty()
        && nonempty_lines.iter().all(|line| {
            let line = line.trim();
            line.chars().count() >= 3
                && line.chars().all(|ch| {
                    ch.is_whitespace()
                        || matches!(
                            ch,
                            '─' | '━' | '═' | '—' | '-' | '_' | '=' | '*' | '•' | '·' | '.'
                        )
                })
        })
    {
        return Err("all_box_lines");
    }
    Ok(trimmed.to_string())
}

pub fn recommendation(
    relevant: usize,
    partial: usize,
    irrelevant: usize,
    would_have_helped_count: usize,
    fetch_errors: usize,
    attempted: usize,
) -> (String, String, bool, Option<String>) {
    let evaluated = relevant + partial + irrelevant;
    if attempted == 0 {
        return (
            "evaluation failed — no qualifying samples; collect production traffic and rerun"
                .into(),
            "no-data".into(),
            false,
            Some("no qualifying samples".into()),
        );
    }
    if fetch_errors == attempted {
        return (
            "evaluation failed — all recall fetches failed; restore the store and rerun".into(),
            "failed".into(),
            false,
            Some("all recall fetches failed".into()),
        );
    }
    if evaluated == 0 {
        return (
            "evaluation failed — no queries were evaluated".into(),
            "failed".into(),
            false,
            Some("no queries evaluated".into()),
        );
    }
    let strict = relevant as f64 / evaluated as f64;
    let useful = (relevant + partial) as f64 / evaluated as f64;
    let (action, confidence) = if useful >= 0.7 && strict >= 0.4 {
        ("keep — relevance rate is healthy", "earning")
    } else if useful >= 0.7 {
        (
            "remeasure after attributed traffic — useful rate is healthy; strict relevance is settling",
            "watch",
        )
    } else if useful >= 0.4 {
        (
            "tune threshold — recalibrate against the current embedder distribution or improve previews",
            "watch",
        )
    } else if would_have_helped_count > 2 {
        (
            "investigate ranking — shadow rankers surface useful alternatives",
            "kill-candidate",
        )
    } else {
        (
            "investigate retrieval — relevance is low and shadows find no alternatives",
            "kill-candidate",
        )
    };
    (action.into(), confidence.into(), true, None)
}

/// Recalibrate rather than carrying the predecessor's 0.45/0.35 constants into
/// a new embedding space. The dev/query sample chooses p75 as the strict gate
/// and p50 as the useful gate; a held-out corpus may report but never tune these.
pub fn calibrate_thresholds(
    scores: &[f32],
    provenance: impl Into<String>,
) -> Result<ThresholdCalibrationV1, String> {
    let mut finite = scores
        .iter()
        .copied()
        .filter(|score| score.is_finite())
        .collect::<Vec<_>>();
    if finite.is_empty() {
        return Err("threshold calibration has no finite current-embedder scores".into());
    }
    finite.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    let percentile = |pct: usize| -> f32 {
        let index = ((finite.len() - 1) * pct + 50) / 100;
        finite[index.min(finite.len() - 1)]
    };
    let partial = percentile(50);
    let relevant = percentile(75).max(partial);
    Ok(ThresholdCalibrationV1 {
        relevant,
        partial,
        provenance: provenance.into(),
        distribution_size: finite.len(),
        sweep: [25usize, 50, 75, 90]
            .into_iter()
            .map(|pct| ThresholdSweepRowV1 {
                percentile: pct as u8,
                cosine: percentile(pct),
            })
            .collect(),
    })
}

/// Run against an explicit durable DB. Opening through `MemDb` performs normal
/// schema handling; replay itself is read-only and is proven by before/after
/// recall-log and aggregate injection counters.
pub fn run_spotcheck(
    db_path: &Path,
    requested: usize,
    home: Option<&Path>,
    fixture_path: Option<&Path>,
) -> Result<RelevanceSpotcheckReportV1, String> {
    run_spotcheck_inner(db_path, requested, home, fixture_path)
        .map_err(|reason| RelevanceSpotcheckReportV1::failed(requested, reason).to_json_string())
}

fn run_spotcheck_inner(
    db_path: &Path,
    requested: usize,
    home: Option<&Path>,
    fixture_path: Option<&Path>,
) -> Result<RelevanceSpotcheckReportV1, String> {
    if std::env::var("MEMBRANE_ALLOW_HASH").as_deref() == Ok("1") {
        return Err("MEMBRANE_ALLOW_HASH=1 is forbidden for relevance evaluation".into());
    }
    if !db_path.is_file() {
        return Err(format!("Cortex DB not found at {}", db_path.display()));
    }
    let store = MemoryStore::try_open(MemDb::open(db_path).map_err(|error| error.to_string())?)?;
    let health = store.health();
    if !health.ok || health.embedder_model == "hash-256" || health.embedder_dim == 256 {
        return Err(format!(
            "real embedder unavailable: model={} dim={} issue={}",
            health.embedder_model,
            health.embedder_dim,
            health.embedder_issue.unwrap_or_else(|| "unknown".into())
        ));
    }
    let before = store_counters(&store)?;
    // The newest queries are the blind evaluation set. Older queries form a
    // disjoint calibration set, so no threshold is a function of an evaluated
    // query's outcome.
    let source_limit = requested.saturating_mul(2).max(10);
    let (raw, query_source) = collect_queries(&store, source_limit, home, fixture_path)?;
    let mut skipped = BTreeMap::<String, usize>::new();
    let mut qualified = Vec::new();
    for sample in raw {
        if sample.query.chars().count() < MIN_QUERY_CHARS {
            *skipped.entry("below_min_query_chars".into()).or_default() += 1;
            continue;
        }
        match classify_query(&sample.query) {
            Ok(query) => qualified.push(RecallSampleV1 { query, ..sample }),
            Err(reason) => *skipped.entry(reason.into()).or_default() += 1,
        }
    }
    if qualified.is_empty() {
        return Err("no qualifying samples".into());
    }
    if qualified.len() < 6 {
        return Err(format!(
            "independent calibration requires at least 6 qualifying queries; found {}",
            qualified.len()
        ));
    }
    let calibration_count = (qualified.len() / 2).max(5);
    let evaluation_count = requested.min(qualified.len() - calibration_count);
    let calibration_samples = qualified.split_off(evaluation_count);
    let samples = qualified;

    let corpus = store.entries(usize::MAX);
    if corpus.is_empty() {
        return Err("Cortex corpus is empty".into());
    }
    let calibration_scores = calibration_samples
        .iter()
        .flat_map(|sample| {
            recall(&store, sample, corpus.len())
                .into_iter()
                .map(|hit| hit.1)
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let thresholds = calibrate_thresholds(
        &calibration_scores,
        format!(
            "current-live-embedder:{}; disjoint older-query dev replay (n={}) percentiles p75 relevant / p50 partial; newest queries are blind evaluation; old 0.45/0.35 not reused",
            health.embedder_fingerprint,
            calibration_samples.len()
        ),
    )?;

    let mut verdicts = BTreeMap::<String, usize>::from([
        ("relevant".into(), 0),
        ("partial".into(), 0),
        ("irrelevant".into(), 0),
    ]);
    let mut evaluations = Vec::new();
    let fetch_errors = 0;
    let mut novel_count = 0;
    let mut helpful_count = 0;
    for sample in &samples {
        let hits = recall(&store, sample, TOP_K);
        let verdict = judge_hits(&sample.query, &hits, &thresholds);
        *verdicts.entry(verdict_name(verdict).into()).or_default() += 1;
        let live_ids = hits
            .iter()
            .map(|(entry, _)| entry.id.clone())
            .collect::<BTreeSet<_>>();
        let shadow = shadow_rankers(&sample.query, &sample.scope, &corpus, &live_ids, TOP_K);
        novel_count += shadow.novel_candidates.len();
        helpful_count += shadow.would_have_helped.len();
        evaluations.push(SampleEvaluationV1 {
            ts: sample.ts.clone(),
            scope: sample.scope.clone(),
            client: sample.client.clone(),
            session_id: sample.session_id.clone(),
            source: sample.source.clone(),
            query: sample.query.clone(),
            top_injected: hits
                .iter()
                .take(3)
                .map(|(entry, cosine)| InjectedPreviewV1 {
                    id: entry.id.clone(),
                    cosine: *cosine,
                    preview: preview(&entry.content, 200),
                })
                .collect(),
            verdict,
            shadow,
        });
    }
    let relevant = *verdicts.get("relevant").unwrap_or(&0);
    let partial = *verdicts.get("partial").unwrap_or(&0);
    let irrelevant = *verdicts.get("irrelevant").unwrap_or(&0);
    let evaluated = relevant + partial + irrelevant;
    let (action, confidence, ok, reason) = recommendation(
        relevant,
        partial,
        irrelevant,
        helpful_count,
        fetch_errors,
        samples.len(),
    );
    let after = store_counters(&store)?;
    let recall_delta = after.recall_rows - before.recall_rows;
    let injection_delta = after.injection_sum - before.injection_sum;
    if recall_delta != 0 || injection_delta != 0 {
        return Err(format!(
            "measurement-only invariant failed: recall_rows_added={recall_delta}, injection_counter_delta={injection_delta}"
        ));
    }
    let report_ok = ok && recall_delta == 0 && injection_delta == 0;
    Ok(RelevanceSpotcheckReportV1 {
        schema: REPORT_SCHEMA.into(),
        ok: report_ok,
        reason,
        generated_at: crate::time::now_iso(),
        query_source,
        sample_size_requested: requested,
        sample_size_attempted: samples.len(),
        sample_size_evaluated: evaluated,
        skipped: SkipCountsV1 {
            total: skipped.values().sum(),
            by_reason: skipped,
        },
        fetch_errors,
        verdicts,
        relevance_rate: if evaluated == 0 {
            0.0
        } else {
            relevant as f64 / evaluated as f64
        },
        useful_rate: if evaluated == 0 {
            0.0
        } else {
            (relevant + partial) as f64 / evaluated as f64
        },
        thresholds,
        variance_status: "not_measured_single_judge".into(),
        judge_basis: "full_content_when_available_preview_fallback".into(),
        shadow: BTreeMap::from([
            ("novel_candidates_count".into(), novel_count),
            ("would_have_helped_count".into(), helpful_count),
        ]),
        recommended_action: action,
        confidence,
        measurement_only: MeasurementOnlyV1 {
            observing_recall: false,
            traffic_class: "evaluation".into(),
            production_injection_count: 0,
            recall_rows_added: recall_delta,
            injection_counter_delta: injection_delta,
        },
        samples: evaluations,
    })
}

impl RelevanceSpotcheckReportV1 {
    pub fn failed(requested: usize, reason: impl Into<String>) -> Self {
        Self {
            schema: REPORT_SCHEMA.into(),
            ok: false,
            reason: Some(reason.into()),
            generated_at: crate::time::now_iso(),
            query_source: "unavailable".into(),
            sample_size_requested: requested,
            sample_size_attempted: 0,
            sample_size_evaluated: 0,
            skipped: SkipCountsV1 {
                total: 0,
                by_reason: BTreeMap::new(),
            },
            fetch_errors: 0,
            verdicts: BTreeMap::from([
                ("relevant".into(), 0),
                ("partial".into(), 0),
                ("irrelevant".into(), 0),
            ]),
            relevance_rate: 0.0,
            useful_rate: 0.0,
            thresholds: ThresholdCalibrationV1 {
                relevant: 0.0,
                partial: 0.0,
                provenance: "unavailable: evaluation failed before calibration".into(),
                distribution_size: 0,
                sweep: Vec::new(),
            },
            variance_status: "not_measured_single_judge".into(),
            judge_basis: "full_content_when_available_preview_fallback".into(),
            shadow: BTreeMap::from([
                ("novel_candidates_count".into(), 0),
                ("would_have_helped_count".into(), 0),
            ]),
            recommended_action: "evaluation failed — restore inputs and rerun".into(),
            confidence: "failed".into(),
            measurement_only: MeasurementOnlyV1 {
                observing_recall: false,
                traffic_class: "evaluation".into(),
                production_injection_count: 0,
                recall_rows_added: 0,
                injection_counter_delta: 0,
            },
            samples: Vec::new(),
        }
    }

    pub fn to_json_string(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| {
            "{\"schema\":\"cortex.relevance-spotcheck.v1\",\"ok\":false,\"reason\":\"report serialization failed\"}".into()
        })
    }
}

pub fn write_report_atomic(path: &Path, report: &RelevanceSpotcheckReportV1) -> Result<(), String> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let sequence = REPORT_TEMP_SEQUENCE.fetch_add(1, AtomicOrdering::Relaxed);
    let temp = parent.join(format!(
        ".{}.{}.{}.tmp",
        path.file_name().unwrap_or_default().to_string_lossy(),
        std::process::id(),
        sequence
    ));
    let bytes = serde_json::to_vec_pretty(report).map_err(|error| error.to_string())?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(|error| error.to_string())?;
    if let Err(error) = file
        .write_all(&bytes)
        .and_then(|_| file.write_all(b"\n"))
        .and_then(|_| file.sync_all())
    {
        let _ = fs::remove_file(&temp);
        return Err(error.to_string());
    }
    if let Err(error) = fs::rename(&temp, path) {
        let _ = fs::remove_file(path);
        fs::rename(&temp, path).map_err(|_| error.to_string())?;
    }
    Ok(())
}

fn collect_queries(
    store: &MemoryStore,
    requested: usize,
    home: Option<&Path>,
    fixture_path: Option<&Path>,
) -> Result<(Vec<RecallSampleV1>, String), String> {
    let production = recent_production_queries(store, requested)?;
    if !production.is_empty() {
        return Ok((
            production,
            "recall_log:source=serve,traffic_class=production".into(),
        ));
    }
    let home = home.map(Path::to_path_buf).or_else(default_home);
    if let Some(home) = home {
        let transcripts = transcript_queries(&home, requested.saturating_mul(4));
        if !transcripts.is_empty() {
            return Ok((
                transcripts,
                "membrane-transcript:recent-user-messages".into(),
            ));
        }
    }
    if let Some(path) = fixture_path {
        return fixture_queries(path).map(|queries| (queries, "redacted-frozen-fixture".into()));
    }
    Err("no recall-log, transcript, or fixture queries available".into())
}

fn recent_production_queries(
    store: &MemoryStore,
    requested: usize,
) -> Result<Vec<RecallSampleV1>, String> {
    let conn = store.db().lock();
    let mut statement = conn
        .prepare(
            "SELECT ts, COALESCE(scope,''), COALESCE(query_preview,''), COALESCE(client,'unknown'), \
                    COALESCE(session_id,''), source \
             FROM recall_log \
             WHERE source='serve' AND traffic_class='production' \
               AND lower(COALESCE(client,'unknown')) NOT LIKE '%smoke%' \
               AND lower(COALESCE(client,'unknown')) NOT IN ('manual','cli') \
               AND lower(COALESCE(query_preview,'')) NOT LIKE '%smoke-test%' \
             ORDER BY ts DESC, id DESC LIMIT ?1",
        )
        .map_err(|error| error.to_string())?;
    let samples = statement
        .query_map([requested as i64], |row| {
            Ok(RecallSampleV1 {
                ts: row.get(0)?,
                scope: row.get(1)?,
                query: row.get(2)?,
                client: row.get(3)?,
                session_id: row.get(4)?,
                source: row.get(5)?,
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| error.to_string());
    samples
}

fn transcript_queries(home: &Path, cap: usize) -> Vec<RecallSampleV1> {
    let mut discovered = discover_open(home);
    discovered.sort_by(|a, b| {
        let modified = |path: &Path| fs::metadata(path).and_then(|meta| meta.modified()).ok();
        modified(&b.path)
            .cmp(&modified(&a.path))
            .then(a.path.cmp(&b.path))
    });
    let mut queries = Vec::new();
    for source in discovered {
        let Ok(events) = parse_source_events(&source.path, Some(&source.host)) else {
            continue;
        };
        for event in events
            .into_iter()
            .rev()
            .filter(|event| event.kind == "user_message")
        {
            queries.push(RecallSampleV1 {
                ts: event.timestamp.unwrap_or_default(),
                scope: event
                    .cwd
                    .as_deref()
                    .map(|cwd| crate::path_to_scope(cwd))
                    .unwrap_or_else(|| "global".into()),
                query: event.text,
                client: source.host.clone(),
                session_id: event.session_id,
                source: "transcript".into(),
            });
            if queries.len() >= cap {
                return queries;
            }
        }
    }
    queries
}

fn fixture_queries(path: &Path) -> Result<Vec<RecallSampleV1>, String> {
    let value =
        fs::read_to_string(path).map_err(|error| format!("read {}: {error}", path.display()))?;
    let fixture: FixtureQueries =
        serde_json::from_str(&value).map_err(|error| error.to_string())?;
    Ok(fixture
        .queries
        .into_iter()
        .map(|query| RecallSampleV1 {
            ts: "fixture".into(),
            scope: query.scope,
            query: membrane_transcript::redact::redact(&query.query),
            client: "fixture".into(),
            session_id: "fixture".into(),
            source: query.source,
        })
        .collect())
}

fn default_home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

fn recall(store: &MemoryStore, sample: &RecallSampleV1, limit: usize) -> Vec<(MemoryEntry, f32)> {
    let scopes = if sample.scope.trim().is_empty() || sample.scope == "global" {
        vec!["global".into()]
    } else {
        crate::scope_chain(&crate::normalize_scope(&sample.scope), &store.scopes())
    };
    store
        .recall_scored_detailed(&sample.query, limit, &scopes)
        .into_iter()
        .map(|hit| (hit.entry, hit.score))
        .collect()
}

fn store_counters(store: &MemoryStore) -> Result<StoreCounters, String> {
    let conn = store.db().lock();
    conn.query_row(
        "SELECT (SELECT COUNT(*) FROM recall_log), (SELECT COALESCE(SUM(inject_count),0) FROM memories)",
        [],
        |row| Ok(StoreCounters { recall_rows: row.get(0)?, injection_sum: row.get(1)? }),
    )
    .map_err(|error| error.to_string())
}

fn verdict_name(verdict: VerdictV1) -> &'static str {
    match verdict {
        VerdictV1::Relevant => "relevant",
        VerdictV1::Partial => "partial",
        VerdictV1::Irrelevant => "irrelevant",
    }
}

pub fn judge_hits(
    query: &str,
    hits: &[(MemoryEntry, f32)],
    thresholds: &ThresholdCalibrationV1,
) -> VerdictV1 {
    let query_tokens = tokens(query).into_iter().collect::<BTreeSet<_>>();
    if query_tokens.len() < 3 {
        return VerdictV1::Irrelevant;
    }
    if hits.iter().any(|(entry, cosine)| {
        *cosine >= thresholds.relevant && overlap_count(&query_tokens, &entry.content) >= 2
    }) {
        return VerdictV1::Relevant;
    }
    if hits.iter().any(|(entry, cosine)| {
        *cosine >= thresholds.partial && overlap_count(&query_tokens, &entry.content) >= 1
    }) {
        return VerdictV1::Partial;
    }
    VerdictV1::Irrelevant
}

fn shadow_rankers(
    query: &str,
    scope: &str,
    corpus: &[MemoryEntry],
    live_ids: &BTreeSet<String>,
    limit: usize,
) -> ShadowReportV1 {
    let scoped = corpus
        .iter()
        .filter(|entry| scope.is_empty() || entry.scope_id == scope || entry.scope_id == "global")
        .collect::<Vec<_>>();
    let rank = |scorer: &dyn Fn(&MemoryEntry) -> f32| -> Vec<String> {
        let mut scored = scoped
            .iter()
            .map(|entry| (entry.id.clone(), scorer(entry)))
            .collect::<Vec<_>>();
        scored.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(Ordering::Equal)
                .then(a.0.cmp(&b.0))
        });
        scored.into_iter().take(limit).map(|row| row.0).collect()
    };
    let bm25 = rank(&|entry| bm25_score(query, &entry.content));
    let cosine_proxy = rank(&|entry| cosine_proxy_score(query, &entry.content));
    let hybrid_proxy = rank(&|entry| {
        bm25_score(query, &entry.content) / 10.0 * 0.5
            + cosine_proxy_score(query, &entry.content) * 0.5
    });
    let by_id = corpus
        .iter()
        .map(|entry| (entry.id.as_str(), entry))
        .collect::<HashMap<_, _>>();
    let mut novel = Vec::new();
    let mut helpful = Vec::new();
    for (label, ids) in [
        ("bm25", &bm25),
        ("cosine_proxy", &cosine_proxy),
        ("hybrid_proxy", &hybrid_proxy),
    ] {
        for id in ids {
            if live_ids.contains(id) {
                continue;
            }
            let Some(entry) = by_id.get(id.as_str()) else {
                continue;
            };
            let verdict = shadow_verdict(query, &entry.content);
            let candidate = RankedCandidateV1 {
                ranker: label.into(),
                id: id.clone(),
                verdict,
                overlap: term_overlap(query, &entry.content),
            };
            novel.push(candidate.clone());
            if verdict != VerdictV1::Irrelevant {
                helpful.push(candidate);
            }
        }
    }
    novel.truncate(5);
    helpful.truncate(5);
    ShadowReportV1 {
        bm25,
        cosine_proxy,
        hybrid_proxy,
        novel_candidates: novel,
        would_have_helped: helpful,
    }
}

fn shadow_verdict(query: &str, content: &str) -> VerdictV1 {
    let query_tokens = tokens(query).into_iter().collect::<BTreeSet<_>>();
    if query_tokens.is_empty() {
        return VerdictV1::Irrelevant;
    }
    let content_tokens = tokens(content).into_iter().collect::<BTreeSet<_>>();
    let overlap = query_tokens.intersection(&content_tokens).count();
    let ratio = overlap as f32 / query_tokens.len() as f32;
    if overlap >= 4 && ratio >= 0.35 {
        VerdictV1::Relevant
    } else if overlap >= 3 && ratio >= 0.25 {
        VerdictV1::Partial
    } else {
        VerdictV1::Irrelevant
    }
}

fn term_overlap(query: &str, content: &str) -> f32 {
    let query_tokens = tokens(query);
    if query_tokens.is_empty() {
        return 0.0;
    }
    let content_tokens = tokens(content).into_iter().collect::<BTreeSet<_>>();
    query_tokens
        .iter()
        .filter(|token| content_tokens.contains(*token))
        .count() as f32
        / query_tokens.len() as f32
}

fn overlap_count(query_tokens: &BTreeSet<String>, content: &str) -> usize {
    let content_tokens = tokens(content).into_iter().collect::<BTreeSet<_>>();
    query_tokens.intersection(&content_tokens).count()
}

fn bm25_score(query: &str, document: &str) -> f32 {
    let query_tokens = tokens(query);
    let doc_tokens = tokens(document);
    if query_tokens.is_empty() || doc_tokens.is_empty() {
        return 0.0;
    }
    let mut frequencies = HashMap::<String, usize>::new();
    for token in &doc_tokens {
        *frequencies.entry(token.clone()).or_default() += 1;
    }
    let norm = 1.0 - 0.75 + 0.75 * doc_tokens.len() as f32 / 400.0;
    query_tokens
        .iter()
        .map(|token| {
            let frequency = *frequencies.get(token).unwrap_or(&0) as f32;
            if frequency == 0.0 {
                0.0
            } else {
                frequency * 2.5 / (frequency + 1.5 * norm)
            }
        })
        .sum()
}

fn cosine_proxy_score(query: &str, document: &str) -> f32 {
    let query_tokens = tokens(query).into_iter().collect::<BTreeSet<_>>();
    let doc_tokens = tokens(document).into_iter().collect::<BTreeSet<_>>();
    if query_tokens.is_empty() || doc_tokens.is_empty() {
        return 0.0;
    }
    query_tokens.intersection(&doc_tokens).count() as f32
        / ((query_tokens.len() * doc_tokens.len()) as f32).sqrt()
}

fn tokens(text: &str) -> Vec<String> {
    let stop = STOP.iter().copied().collect::<BTreeSet<_>>();
    let mut out = Vec::new();
    let mut current = String::new();
    for character in text.chars() {
        if character.is_alphanumeric() || character.is_mark_nonspacing() {
            current.extend(character.to_lowercase());
        } else if !current.is_empty() {
            if current.chars().count() >= 2 && !stop.contains(current.as_str()) {
                out.push(std::mem::take(&mut current));
            } else {
                current.clear();
            }
        }
    }
    if current.chars().count() >= 2 && !stop.contains(current.as_str()) {
        out.push(current);
    }
    out
}

trait UnicodeMark {
    fn is_mark_nonspacing(self) -> bool;
}
impl UnicodeMark for char {
    fn is_mark_nonspacing(self) -> bool {
        matches!(self as u32, 0x0300..=0x036f | 0x0900..=0x0dff)
    }
}

fn preview(content: &str, limit: usize) -> String {
    let mut value = content.chars().take(limit).collect::<String>();
    if content.chars().count() > limit {
        value.push('…');
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn low_signal_rules_skip_noise_and_unwrap_real_task() {
        assert_eq!(classify_query("   "), Err("empty_or_short"));
        assert_eq!(classify_query("──────"), Err("divider_or_box"));
        assert_eq!(
            classify_query(
                "<task-notification><task-id>abcd1234abcd</task-id></task-notification>"
            ),
            Err("task_notification_empty")
        );
        assert_eq!(
            classify_query(
                "<task-notification>Fix durable memory replay regression</task-notification>"
            )
            .unwrap(),
            "Fix durable memory replay regression"
        );
        assert_eq!(
            classify_query("部署数据库 память हिन्दी स्मृति").unwrap(),
            "部署数据库 память हिन्दी स्मृति"
        );
    }

    #[test]
    fn recommendation_ladder_matches_predecessor_gates() {
        assert_eq!(
            recommendation(4, 3, 3, 0, 0, 10).0,
            "keep — relevance rate is healthy"
        );
        assert!(recommendation(2, 6, 2, 0, 0, 10).0.starts_with("remeasure"));
        assert!(recommendation(2, 2, 6, 0, 0, 10)
            .0
            .starts_with("tune threshold"));
        assert!(recommendation(1, 1, 8, 3, 0, 10)
            .0
            .starts_with("investigate ranking"));
        assert!(recommendation(1, 1, 8, 2, 0, 10)
            .0
            .starts_with("investigate retrieval"));
        assert!(!recommendation(0, 0, 0, 0, 0, 0).2);
        assert!(!recommendation(0, 0, 0, 0, 2, 2).2);
    }

    #[test]
    fn calibration_uses_current_distribution_percentiles() {
        let result =
            calibrate_thresholds(&[0.05, 0.10, 0.20, 0.30, 0.40], "fixture-current-embedder")
                .unwrap();
        assert_eq!(result.partial, 0.20);
        assert_eq!(result.relevant, 0.30);
        assert_eq!(result.distribution_size, 5);
        assert!(result.provenance.contains("current"));
    }
}
