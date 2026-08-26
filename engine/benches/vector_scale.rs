//! Deterministic scale harness for the resident `cortex_core::VectorIndex`.
//!
//! The file is intentionally dependency-light so an integrator can attach it
//! to a temporary Cargo bench target without changing the workspace manifest.
//! It measures the exact index only; a future ANN arm must compare its IDs to
//! this arm for `recall_at_k` before it can be considered.

use std::hint::black_box;
use std::time::Instant;

use cortex_core::{HostVectorPolicy, MemoryEntry, MemoryTier, VectorCandidate, VectorIndex};

const DIMENSION: usize = 256;
const TOP_K: usize = 128;
const WARM_QUERIES: usize = 8;

#[derive(Clone, Copy)]
pub struct CaseSpec {
    pub records: usize,
    pub scope_filter_bps: u32,
}

pub struct BenchmarkConfig {
    pub cases: Vec<CaseSpec>,
    pub warm_queries: usize,
}

impl BenchmarkConfig {
    pub fn full() -> Self {
        Self {
            cases: vec![
                CaseSpec {
                    records: 2_361,
                    scope_filter_bps: 10_000,
                },
                CaseSpec {
                    records: 2_361,
                    scope_filter_bps: 1_000,
                },
                CaseSpec {
                    records: 30_549,
                    scope_filter_bps: 10_000,
                },
                CaseSpec {
                    records: 30_549,
                    scope_filter_bps: 1_000,
                },
                CaseSpec {
                    records: 100_000,
                    scope_filter_bps: 10_000,
                },
                CaseSpec {
                    records: 100_000,
                    scope_filter_bps: 1_000,
                },
            ],
            warm_queries: WARM_QUERIES,
        }
    }

    pub fn quick() -> Self {
        Self {
            cases: vec![
                CaseSpec {
                    records: 256,
                    scope_filter_bps: 10_000,
                },
                CaseSpec {
                    records: 256,
                    scope_filter_bps: 1_000,
                },
                CaseSpec {
                    records: 2_361,
                    scope_filter_bps: 10_000,
                },
            ],
            warm_queries: 2,
        }
    }
}

impl Default for BenchmarkConfig {
    fn default() -> Self {
        Self::full()
    }
}

struct CaseResult {
    spec: CaseSpec,
    eligible: usize,
    resident_bytes: usize,
    kernel: &'static str,
    build_ns: u128,
    cold_query_ns: u128,
    warm_query_mean_ns: u128,
    warm_query_p50_ns: u128,
    warm_query_p95_ns: u128,
    update_p95_ns: u128,
    delete_p95_ns: u128,
    recall_at_k: f64,
}

pub fn run(config: &BenchmarkConfig) -> Vec<String> {
    config
        .cases
        .iter()
        .map(|spec| render_case(run_case(*spec, config.warm_queries)))
        .collect()
}

fn run_case(spec: CaseSpec, warm_queries: usize) -> CaseResult {
    let mut index = VectorIndex::new();
    let build_started = Instant::now();
    for row in 0..spec.records {
        index.upsert(&entry(row));
    }
    let build_ns = build_started.elapsed().as_nanos();

    let scopes = scope_filter(spec.scope_filter_bps);
    let scope_refs = scopes.iter().map(String::as_str).collect::<Vec<_>>();
    let eligible = if spec.scope_filter_bps == 10_000 {
        spec.records
    } else {
        (0..spec.records)
            .filter(|row| row % 10 < scope_refs.len())
            .count()
    };
    let query = vector(spec.records.wrapping_add(17));
    let eligible_arg = (spec.scope_filter_bps != 10_000).then_some(scope_refs.as_slice());
    let cold_started = Instant::now();
    let oracle = index
        .top_k(&query, eligible_arg, TOP_K)
        .expect("exact VectorIndex benchmark query");
    let cold_query_ns = cold_started.elapsed().as_nanos();

    let mut checksum = 0_u64;
    let mut observed = Vec::new();
    let mut warm_samples = Vec::with_capacity(warm_queries.max(1));
    for _ in 0..warm_queries.max(1) {
        let query_started = Instant::now();
        observed = index
            .top_k(&query, eligible_arg, TOP_K)
            .expect("warm exact VectorIndex benchmark query");
        warm_samples.push(query_started.elapsed().as_nanos());
        checksum = checksum.wrapping_add(checksum_candidates(&observed));
    }
    let warm_query_ns = warm_samples.iter().sum::<u128>() / warm_samples.len() as u128;
    black_box(checksum);

    let updated = entry(spec.records / 2);
    let mutation_samples = warm_queries.max(1);
    let mut update_samples = Vec::with_capacity(mutation_samples);
    let mut delete_samples = Vec::with_capacity(mutation_samples);
    for _ in 0..mutation_samples {
        let update_started = Instant::now();
        index.upsert(&updated);
        update_samples.push(update_started.elapsed().as_nanos());
        let delete_started = Instant::now();
        index.remove(&updated.id);
        delete_samples.push(delete_started.elapsed().as_nanos());
        index.upsert(&updated);
    }

    // This arm is both candidate and correctness oracle. Future ANN arms must
    // compare their ordered IDs with `oracle` through this same metric.
    let recall_at_k = recall_at_k(&oracle, &observed);
    let kernel = kernel_name(HostVectorPolicy::current().kernel(spec.records, eligible));
    CaseResult {
        spec,
        eligible,
        resident_bytes: index.resident_bytes(),
        kernel,
        build_ns,
        cold_query_ns,
        warm_query_mean_ns: warm_query_ns,
        warm_query_p50_ns: percentile(&warm_samples, 1, 2),
        warm_query_p95_ns: percentile(&warm_samples, 95, 100),
        update_p95_ns: percentile(&update_samples, 95, 100),
        delete_p95_ns: percentile(&delete_samples, 95, 100),
        recall_at_k,
    }
}

fn percentile(samples: &[u128], numerator: usize, denominator: usize) -> u128 {
    let mut ordered = samples.to_vec();
    ordered.sort_unstable();
    let rank = (ordered.len().saturating_mul(numerator) + denominator.saturating_sub(1))
        .checked_div(denominator)
        .unwrap_or(1)
        .max(1)
        .min(ordered.len());
    ordered[rank - 1]
}

fn entry(row: usize) -> MemoryEntry {
    MemoryEntry {
        id: format!("scale-{row:06}"),
        tier: MemoryTier::Working,
        content: format!("vector-scale fixture row {row}"),
        keywords: Vec::new(),
        score: 0.0,
        created_at: "2026-08-23T00:00:00Z".to_string(),
        access_count: 0,
        embedding: Some(vector(row)),
        scope_id: format!("scope-{}", row % 10),
    }
}

fn vector(seed: usize) -> Vec<f32> {
    let mut values = (0..DIMENSION)
        .map(|column| {
            let mixed = seed
                .wrapping_mul(1_103_515_245)
                .wrapping_add(column.wrapping_mul(12_345))
                % 65_521;
            (mixed as f32 - 32_760.0) / 32_760.0
        })
        .collect::<Vec<_>>();
    let norm = values.iter().map(|value| value * value).sum::<f32>().sqrt();
    if norm > 0.0 {
        for value in &mut values {
            *value /= norm;
        }
    }
    values
}

fn scope_filter(bps: u32) -> Vec<String> {
    if bps == 10_000 {
        Vec::new()
    } else {
        let count = ((bps as usize).saturating_mul(10) / 10_000).max(1);
        (0..count).map(|scope| format!("scope-{scope}")).collect()
    }
}

fn recall_at_k(reference: &[VectorCandidate], observed: &[VectorCandidate]) -> f64 {
    if reference.is_empty() {
        return 1.0;
    }
    let hits = observed
        .iter()
        .filter(|candidate| reference.iter().any(|expected| expected.id == candidate.id))
        .count();
    hits as f64 / reference.len() as f64
}

fn checksum_candidates(candidates: &[VectorCandidate]) -> u64 {
    candidates.iter().fold(0_u64, |checksum, candidate| {
        candidate.id.bytes().fold(
            checksum ^ u64::from(candidate.score.to_bits()),
            |value, byte| value.rotate_left(5) ^ u64::from(byte),
        )
    })
}

fn kernel_name(kernel: cortex_core::VectorKernel) -> &'static str {
    match kernel {
        cortex_core::VectorKernel::ScalarFull => "scalar-full",
        cortex_core::VectorKernel::ParallelFull => "parallel-full",
        cortex_core::VectorKernel::Gather => "gather",
    }
}

fn render_case(result: CaseResult) -> String {
    format!(
        "{{\"records\":{},\"dimension\":{},\"quantization\":\"f32\",\"scopeFilterBps\":{},\"eligible\":{},\"residentBytes\":{},\"kernel\":\"{}\",\"buildNs\":{},\"coldQueryNs\":{},\"warmQueryMeanNs\":{},\"warmQueryP50Ns\":{},\"warmQueryP95Ns\":{},\"updateP95Ns\":{},\"deleteP95Ns\":{},\"recallAtK\":{:.6}}}",
        result.spec.records,
        DIMENSION,
        result.spec.scope_filter_bps,
        result.eligible,
        result.resident_bytes,
        result.kernel,
        result.build_ns,
        result.cold_query_ns,
        result.warm_query_mean_ns,
        result.warm_query_p50_ns,
        result.warm_query_p95_ns,
        result.update_p95_ns,
        result.delete_p95_ns,
        result.recall_at_k,
    )
}

fn platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "other"
    }
}

fn host_threads() -> usize {
    std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
}

fn simd_features() -> &'static str {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        if std::is_x86_feature_detected!("avx2") && std::is_x86_feature_detected!("fma") {
            return "avx2,fma";
        }
        return "scalar-fallback";
    }
    #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
    "platform-vector"
}

fn main() {
    let config = if std::env::args().any(|arg| arg == "--quick") {
        BenchmarkConfig::quick()
    } else {
        BenchmarkConfig::full()
    };
    let rows = run(&config);
    println!(
        "{{\"schemaVersion\":1,\"benchmarkId\":\"vector-scale-v1\",\"oracle\":\"cortex_core::VectorIndex\",\"os\":\"{}\",\"platform\":\"{}\",\"architecture\":\"{}\",\"simd\":\"{}\",\"threads\":{},\"warmQueries\":{},\"results\":[{}]}}",
        std::env::consts::OS,
        platform(),
        std::env::consts::ARCH,
        simd_features(),
        host_threads(),
        config.warm_queries,
        rows.join(",")
    );
}
