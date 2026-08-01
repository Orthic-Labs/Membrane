use memright_core::{cosine, QuantizedVector};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::fs;
use std::path::Path;
use std::time::Instant;

pub const GENERATOR_ID: &str = "memright-vector-fixture-v1";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Config {
    pub schema_version: u32,
    pub seed: u64,
    pub dimension: usize,
    pub model: String,
    pub forecast: Forecast,
    pub cells: Vec<CellConfig>,
    pub dependencies: Dependencies,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Forecast {
    pub observed_at: String,
    pub n0: usize,
    pub july_rows_per_month: usize,
    pub n6: usize,
    pub n12: usize,
    pub n24: usize,
    pub formula: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Dependencies {
    pub sqlite_vec_stable: String,
    pub sqlite_vec_alpha: String,
    pub lancedb: String,
    pub rust_minimum: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CellConfig {
    pub id: String,
    pub rows: usize,
    pub queries: usize,
    pub warmups: usize,
    pub iterations: usize,
    pub scope_count: u32,
    pub selectivity_bps: u32,
    pub top_k: usize,
}

#[derive(Clone, Debug)]
pub struct Row {
    pub id: u64,
    pub scope_id: u32,
    pub authority: u8,
    pub active: bool,
    pub effective_from_ms: i64,
    pub effective_until_ms: i64,
    pub content_hash: [u8; 32],
    pub embedding: Vec<f32>,
}

#[derive(Clone, Debug)]
pub struct Query {
    pub id: u32,
    pub target_id: u64,
    pub allowed_scope_exclusive: u32,
    pub max_authority: u8,
    pub effective_at_ms: i64,
    pub embedding: Vec<f32>,
}

#[derive(Clone, Debug)]
pub struct Fixture {
    pub cell: CellConfig,
    pub rows: Vec<Row>,
    pub queries: Vec<Query>,
    pub sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Candidate {
    pub id: u64,
    pub cosine: f32,
    pub content_hash: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QueryMeasurement {
    pub query_id: u32,
    pub elapsed_ns: u64,
    pub candidate_ids: Vec<u64>,
    pub target_present: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArmResult {
    pub arm: String,
    pub backend: String,
    pub exact: bool,
    pub research_only: bool,
    pub build_ns: u64,
    pub measurements: Vec<QueryMeasurement>,
    pub config: serde_json::Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RunBundle {
    pub schema_version: u32,
    pub generator_id: String,
    pub runner: String,
    pub runner_version: String,
    pub cell_id: String,
    pub fixture_sha256: String,
    pub rows: usize,
    pub queries: usize,
    pub dimension: usize,
    pub arms: Vec<ArmResult>,
}

#[derive(Clone, Debug)]
pub struct RunnerArgs {
    pub config: String,
    pub cell: String,
    pub output: String,
}

#[derive(Clone, Copy, Debug)]
struct ScoredId {
    score: f32,
    id: u64,
}

impl PartialEq for ScoredId {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.score.to_bits() == other.score.to_bits()
    }
}

impl Eq for ScoredId {}

impl PartialOrd for ScoredId {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ScoredId {
    fn cmp(&self, other: &Self) -> Ordering {
        self.score
            .total_cmp(&other.score)
            .then_with(|| other.id.cmp(&self.id))
    }
}

#[derive(Clone, Copy)]
struct XorShift64(u64);

impl XorShift64 {
    fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }

    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
}

pub fn load_config(path: impl AsRef<Path>) -> Result<Config, String> {
    let bytes = fs::read(path.as_ref()).map_err(|e| format!("read config: {e}"))?;
    let config: Config =
        serde_json::from_slice(&bytes).map_err(|e| format!("parse config: {e}"))?;
    if config.schema_version != 1 || config.dimension == 0 || config.cells.is_empty() {
        return Err("unsupported or empty benchmark config".into());
    }
    Ok(config)
}

pub fn parse_runner_args() -> Result<RunnerArgs, String> {
    let mut args = std::env::args().skip(1);
    let mut config = None;
    let mut cell = None;
    let mut output = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" => config = args.next(),
            "--cell" => cell = args.next(),
            "--output" => output = args.next(),
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok(RunnerArgs {
        config: config.ok_or("missing --config")?,
        cell: cell.ok_or("missing --cell")?,
        output: output.ok_or("missing --output")?,
    })
}

pub fn selected_cell<'a>(config: &'a Config, id: &str) -> Result<&'a CellConfig, String> {
    config
        .cells
        .iter()
        .find(|cell| cell.id == id)
        .ok_or_else(|| format!("unknown cell: {id}"))
}

pub fn generate_fixture(config: &Config, cell: &CellConfig) -> Result<Fixture, String> {
    if cell.rows < 2 || cell.queries == 0 || cell.scope_count == 0 || cell.top_k == 0 {
        return Err(format!("invalid cell: {}", cell.id));
    }
    let mut rng = XorShift64::new(config.seed ^ stable_id_hash(&cell.id));
    let mut rows = Vec::with_capacity(cell.rows);
    let effective_at_ms = 1_800_000_000_000_i64;
    for id in 0..cell.rows as u64 {
        let scope_id = (rng.next() % cell.scope_count as u64) as u32;
        let authority = (rng.next() % 5 + 1) as u8;
        let active = id % 23 != 0;
        let effective_from_ms = effective_at_ms - (rng.next() % 86_400_000) as i64;
        let effective_until_ms = if id % 29 == 0 {
            effective_at_ms - 1
        } else {
            effective_at_ms + 86_400_000
        };
        let embedding = (0..config.dimension)
            .map(|_| ((rng.next() % 2001) as i32 - 1000) as f32 / 1000.0)
            .collect::<Vec<_>>();
        let content_hash = row_content_hash(
            id,
            scope_id,
            authority,
            active,
            effective_from_ms,
            effective_until_ms,
            &embedding,
        );
        rows.push(Row {
            id,
            scope_id,
            authority,
            active,
            effective_from_ms,
            effective_until_ms,
            content_hash,
            embedding,
        });
    }
    let allowed_scope_exclusive =
        (cell.scope_count as u64 * cell.selectivity_bps.max(1) as u64).div_ceil(10_000)
            .clamp(1, cell.scope_count as u64) as u32;
    let mut queries = Vec::with_capacity(cell.queries);
    for query_id in 0..cell.queries as u32 {
        let start = (stable_id_hash(&cell.id).wrapping_add(query_id as u64 * 7_919)
            % rows.len() as u64) as usize;
        let target = (0..rows.len())
            .map(|offset| &rows[(start + offset) % rows.len()])
            .find(|row| {
                row.scope_id < allowed_scope_exclusive
                    && row.authority <= 3
                    && row.active
                    && row.effective_from_ms <= effective_at_ms
                    && effective_at_ms < row.effective_until_ms
            })
            .ok_or("cell has no eligible target")?;
        queries.push(Query {
            id: query_id,
            target_id: target.id,
            allowed_scope_exclusive,
            max_authority: 3,
            effective_at_ms,
            embedding: target.embedding.clone(),
        });
    }
    let sha256 = fixture_sha(config, cell, &rows, &queries);
    Ok(Fixture {
        cell: cell.clone(),
        rows,
        queries,
        sha256,
    })
}

fn stable_id_hash(value: &str) -> u64 {
    value.bytes().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ byte as u64).wrapping_mul(0x100000001b3)
    })
}

fn row_content_hash(
    id: u64,
    scope_id: u32,
    authority: u8,
    active: bool,
    effective_from_ms: i64,
    effective_until_ms: i64,
    embedding: &[f32],
) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(id.to_le_bytes());
    hash.update(scope_id.to_le_bytes());
    hash.update([authority, u8::from(active)]);
    hash.update(effective_from_ms.to_le_bytes());
    hash.update(effective_until_ms.to_le_bytes());
    for value in embedding {
        hash.update(value.to_bits().to_le_bytes());
    }
    hash.finalize().into()
}

fn fixture_sha(config: &Config, cell: &CellConfig, rows: &[Row], queries: &[Query]) -> String {
    let mut hash = Sha256::new();
    hash.update(GENERATOR_ID.as_bytes());
    hash.update(config.seed.to_le_bytes());
    hash.update((config.dimension as u64).to_le_bytes());
    hash.update(cell.id.as_bytes());
    hash.update((cell.rows as u64).to_le_bytes());
    hash.update(cell.selectivity_bps.to_le_bytes());
    for row in rows {
        hash.update(row.id.to_le_bytes());
        hash.update(row.scope_id.to_le_bytes());
        hash.update([row.authority, u8::from(row.active)]);
        hash.update(row.effective_from_ms.to_le_bytes());
        hash.update(row.effective_until_ms.to_le_bytes());
        hash.update(row.content_hash);
        for value in &row.embedding {
            hash.update(value.to_bits().to_le_bytes());
        }
    }
    for query in queries {
        hash.update(query.id.to_le_bytes());
        hash.update(query.target_id.to_le_bytes());
        hash.update(query.allowed_scope_exclusive.to_le_bytes());
        hash.update([query.max_authority]);
        hash.update(query.effective_at_ms.to_le_bytes());
        for value in &query.embedding {
            hash.update(value.to_bits().to_le_bytes());
        }
    }
    format!("{:x}", hash.finalize())
}

pub fn eligible(row: &Row, query: &Query) -> bool {
    row.scope_id < query.allowed_scope_exclusive
        && row.authority <= query.max_authority
        && row.active
        && row.effective_from_ms <= query.effective_at_ms
        && query.effective_at_ms < row.effective_until_ms
}

pub fn exact_candidates(rows: &[Row], query: &Query, limit: usize) -> Vec<Candidate> {
    let mut scored = rows
        .iter()
        .filter(|row| eligible(row, query))
        .map(|row| Candidate {
            id: row.id,
            cosine: cosine(&row.embedding, &query.embedding),
            content_hash: hex(&row.content_hash),
        })
        .collect::<Vec<_>>();
    scored.sort_by(|a, b| b.cosine.total_cmp(&a.cosine).then_with(|| a.id.cmp(&b.id)));
    scored.truncate(limit);
    scored
}

pub fn quantized_candidates(rows: &[Row], query: &Query, limit: usize) -> Vec<Candidate> {
    let mut heap = BinaryHeap::with_capacity(limit.saturating_add(1));
    let mut hashes = std::collections::HashMap::new();
    for row in rows.iter().filter(|row| eligible(row, query)) {
        let score = QuantizedVector::quantize(&row.embedding).cosine_with(&query.embedding);
        hashes.insert(row.id, hex(&row.content_hash));
        heap.push(std::cmp::Reverse(ScoredId { score, id: row.id }));
        if heap.len() > limit {
            heap.pop();
        }
    }
    let mut candidates = heap
        .into_iter()
        .map(|std::cmp::Reverse(item)| Candidate {
            id: item.id,
            cosine: item.score,
            content_hash: hashes.remove(&item.id).unwrap_or_default(),
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|a, b| b.cosine.total_cmp(&a.cosine).then_with(|| a.id.cmp(&b.id)));
    candidates
}

pub fn measure<F>(fixture: &Fixture, arm: &str, mut query_fn: F) -> ArmResult
where
    F: FnMut(&Query, usize) -> Vec<Candidate>,
{
    for _ in 0..fixture.cell.warmups {
        for query in &fixture.queries {
            std::hint::black_box(query_fn(query, fixture.cell.top_k));
        }
    }
    let mut measurements = Vec::with_capacity(fixture.queries.len() * fixture.cell.iterations);
    for _ in 0..fixture.cell.iterations {
        for query in &fixture.queries {
            let started = Instant::now();
            let candidates = query_fn(query, fixture.cell.top_k);
            let elapsed_ns = started.elapsed().as_nanos().min(u64::MAX as u128) as u64;
            measurements.push(QueryMeasurement {
                query_id: query.id,
                elapsed_ns,
                target_present: candidates
                    .iter()
                    .any(|candidate| candidate.id == query.target_id),
                candidate_ids: candidates
                    .into_iter()
                    .map(|candidate| candidate.id)
                    .collect(),
            });
        }
    }
    ArmResult {
        arm: arm.into(),
        backend: String::new(),
        exact: true,
        research_only: false,
        build_ns: 0,
        measurements,
        config: serde_json::json!({}),
    }
}

pub fn write_bundle_atomic(path: impl AsRef<Path>, bundle: &RunBundle) -> Result<(), String> {
    let path = path.as_ref();
    let parent = path.parent().ok_or("output path has no parent")?;
    fs::create_dir_all(parent).map_err(|e| format!("create output directory: {e}"))?;
    let tmp = path.with_extension("tmp");
    let bytes = serde_json::to_vec_pretty(bundle).map_err(|e| format!("serialize bundle: {e}"))?;
    fs::write(&tmp, bytes).map_err(|e| format!("write temporary bundle: {e}"))?;
    fs::rename(&tmp, path).map_err(|e| format!("activate bundle: {e}"))
}

pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(DIGITS[(byte >> 4) as usize] as char);
        output.push(DIGITS[(byte & 0xf) as usize] as char);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> Config {
        serde_json::from_value(serde_json::json!({
            "schemaVersion": 1,
            "seed": 20260801,
            "dimension": 32,
            "model": "fixture",
            "forecast": {"observedAt":"2026-08-01","n0":2361,"julyRowsPerMonth":2349,"n6":16455,"n12":30549,"n24":58737,"formula":"n0 + months * julyRowsPerMonth"},
            "cells": [{"id":"smoke","rows":256,"queries":5,"warmups":1,"iterations":1,"scopeCount":10,"selectivityBps":1000,"topK":16}],
            "dependencies": {"sqliteVecStable":"0.1.9","sqliteVecAlpha":"0.1.10-alpha.4","lancedb":"0.33.0","rustMinimum":"1.91.0"}
        })).unwrap()
    }

    #[test]
    fn fixture_is_repeatable_and_targets_are_eligible() {
        let config = config();
        let a = generate_fixture(&config, &config.cells[0]).unwrap();
        let b = generate_fixture(&config, &config.cells[0]).unwrap();
        assert_eq!(a.sha256, b.sha256);
        assert_eq!(a.rows.len(), 256);
        assert!(a.queries.iter().all(|query| a
            .rows
            .iter()
            .any(|row| row.id == query.target_id && eligible(row, query))));
    }

    #[test]
    fn committed_smoke_fixture_matches_manifest() {
        let config: Config =
            serde_json::from_str(include_str!("../../config/round1-v1.json")).unwrap();
        let manifest: serde_json::Value =
            serde_json::from_str(include_str!("../../fixtures/smoke-v1.manifest.json")).unwrap();
        let cell = selected_cell(&config, "smoke").unwrap();
        let fixture = generate_fixture(&config, cell).unwrap();
        assert_eq!(
            fixture.rows.len(),
            manifest["rows"].as_u64().unwrap() as usize
        );
        assert_eq!(
            fixture.queries.len(),
            manifest["queries"].as_u64().unwrap() as usize
        );
        assert_eq!(fixture.sha256, manifest["fixtureSha256"].as_str().unwrap());
    }

    #[test]
    fn quantized_topn_matches_dequantized_control() {
        let config = config();
        let fixture = generate_fixture(&config, &config.cells[0]).unwrap();
        for query in &fixture.queries {
            let control = fixture
                .rows
                .iter()
                .filter(|row| eligible(row, query))
                .map(|row| {
                    let quantized = QuantizedVector::quantize(&row.embedding);
                    Candidate {
                        id: row.id,
                        cosine: cosine(&quantized.dequantize(), &query.embedding),
                        content_hash: hex(&row.content_hash),
                    }
                })
                .collect::<Vec<_>>();
            let mut control = control;
            control.sort_by(|a, b| b.cosine.total_cmp(&a.cosine).then_with(|| a.id.cmp(&b.id)));
            control.truncate(fixture.cell.top_k);
            let optimized = quantized_candidates(&fixture.rows, query, fixture.cell.top_k);
            assert_eq!(
                control
                    .iter()
                    .map(|candidate| candidate.id)
                    .collect::<Vec<_>>(),
                optimized
                    .iter()
                    .map(|candidate| candidate.id)
                    .collect::<Vec<_>>()
            );
        }
    }
}
