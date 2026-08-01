//! Follow-on SIMD/BLAS lane (plan §13.2): same fixtures and measure loop as the
//! Round 1 control runner, three exact arms.
//!   A  — byte-identical to control arm A (in-process paired reference)
//!   B2 — Accelerate cblas_sgemv over a contiguous dequantized matrix, bounded top-N
//!   B3 — quantized-RESIDENT bounded top-N (the arm the spec called "B"; the Round 1
//!        control runner re-quantized every row per query, which pessimized it)
//! B2 uses Accelerate on macOS & AVX2/FMA when available on x86_64.

use crypt_core::{cosine, QuantizedVector};
use half::f16;
use rayon::prelude::*;
use rayon::ThreadPoolBuilder;
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::time::Instant;
use vector_bakeoff_common::{
    eligible, generate_fixture, hex, load_config, measure, parse_runner_args, selected_cell,
    write_bundle_atomic, Candidate, RunBundle, GENERATOR_ID,
};

#[cfg(target_os = "macos")]
#[link(name = "Accelerate", kind = "framework")]
extern "C" {
    fn cblas_sgemv(
        order: i32,
        trans: i32,
        m: i32,
        n: i32,
        alpha: f32,
        a: *const f32,
        lda: i32,
        x: *const f32,
        incx: i32,
        beta: f32,
        y: *mut f32,
        incy: i32,
    );
}

#[cfg(target_os = "macos")]
const CBLAS_ROW_MAJOR: i32 = 101;
#[cfg(target_os = "macos")]
const CBLAS_NO_TRANS: i32 = 111;

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
unsafe fn dot_avx2_fma(left: &[f32], right: &[f32]) -> f32 {
    use std::arch::x86_64::*;

    let chunks = left.len() / 8;
    let mut sum = _mm256_setzero_ps();
    for index in 0..chunks {
        let offset = index * 8;
        sum = _mm256_fmadd_ps(
            _mm256_loadu_ps(left.as_ptr().add(offset)),
            _mm256_loadu_ps(right.as_ptr().add(offset)),
            sum,
        );
    }
    let mut lanes = [0.0_f32; 8];
    _mm256_storeu_ps(lanes.as_mut_ptr(), sum);
    lanes.into_iter().sum::<f32>()
        + left[chunks * 8..]
            .iter()
            .zip(&right[chunks * 8..])
            .map(|(a, b)| a * b)
            .sum::<f32>()
}

#[cfg(not(target_os = "macos"))]
fn full_scores(matrix: &[f32], query: &[f32], scores: &mut [f32]) {
    for (row, score) in matrix.chunks_exact(query.len()).zip(scores) {
        #[cfg(target_arch = "x86_64")]
        let value = if std::is_x86_feature_detected!("avx2") && std::is_x86_feature_detected!("fma")
        {
            unsafe { dot_avx2_fma(row, query) }
        } else {
            row.iter().zip(query).map(|(a, b)| a * b).sum()
        };
        #[cfg(not(target_arch = "x86_64"))]
        let value = row.iter().zip(query).map(|(a, b)| a * b).sum();
        *score = value;
    }
}

#[derive(Clone, Copy)]
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
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for ScoredId {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.score
            .total_cmp(&other.score)
            .then_with(|| other.id.cmp(&self.id))
    }
}

fn bounded_top_n(scored: impl Iterator<Item = ScoredId>, limit: usize) -> Vec<ScoredId> {
    let mut heap = BinaryHeap::with_capacity(limit.saturating_add(1));
    for item in scored {
        heap.push(Reverse(item));
        if heap.len() > limit {
            heap.pop();
        }
    }
    let mut items = heap
        .into_iter()
        .map(|Reverse(item)| item)
        .collect::<Vec<_>>();
    items.sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| a.id.cmp(&b.id)));
    items
}

fn inv_norm(vector: &[f32]) -> f32 {
    let norm = vector.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm > 0.0 {
        1.0 / norm
    } else {
        0.0
    }
}

fn main() -> Result<(), String> {
    let args = parse_runner_args()?;
    let config = load_config(&args.config)?;
    let cell = selected_cell(&config, &args.cell)?;
    let dimension = config.dimension;

    // Build phase, timed per arm's residency shape.
    let build_a_started = Instant::now();
    let fixture = generate_fixture(&config, cell)?;
    let hydrated = fixture
        .rows
        .iter()
        .map(|row| (row, QuantizedVector::quantize(&row.embedding).dequantize()))
        .collect::<Vec<_>>();
    let build_a_ns = build_a_started.elapsed().as_nanos().min(u64::MAX as u128) as u64;

    let build_b2_started = Instant::now();
    let mut matrix = Vec::with_capacity(fixture.rows.len() * dimension);
    let mut inv_norms = Vec::with_capacity(fixture.rows.len());
    for (_, embedding) in &hydrated {
        matrix.extend_from_slice(embedding);
        inv_norms.push(inv_norm(embedding));
    }
    let build_b2_ns = build_b2_started.elapsed().as_nanos().min(u64::MAX as u128) as u64;

    let build_b3_started = Instant::now();
    let quantized = fixture
        .rows
        .iter()
        .map(|row| QuantizedVector::quantize(&row.embedding))
        .collect::<Vec<_>>();
    let build_b3_ns = build_b3_started.elapsed().as_nanos().min(u64::MAX as u128) as u64;

    let build_f16_started = Instant::now();
    let matrix_f16 = matrix
        .iter()
        .copied()
        .map(f16::from_f32)
        .collect::<Vec<_>>();
    let build_f16_ns = build_f16_started.elapsed().as_nanos().min(u64::MAX as u128) as u64;
    let parallel_pool = ThreadPoolBuilder::new()
        .num_threads(std::thread::available_parallelism().map_or(1, usize::from))
        .build()
        .map_err(|error| format!("create Rayon pool: {error}"))?;

    let mut arm_a = measure(&fixture, "A", |query, limit| {
        let mut scoped = hydrated
            .iter()
            .filter(|(row, _)| eligible(row, query))
            .map(|(row, embedding)| Candidate {
                id: row.id,
                cosine: cosine(embedding, &query.embedding),
                content_hash: hex(&row.content_hash),
            })
            .collect::<Vec<_>>();
        scoped.sort_by(|a, b| b.cosine.total_cmp(&a.cosine).then_with(|| a.id.cmp(&b.id)));
        scoped.truncate(limit);
        scoped
    });
    arm_a.backend = "current-rust-dequantized-scope-clone-full-sort".into();
    arm_a.build_ns = build_a_ns;
    arm_a.config = serde_json::json!({"resident":"f32","selection":"full-sort"});

    #[cfg(target_os = "macos")]
    let rows_i32 = i32::try_from(fixture.rows.len()).map_err(|_| "row count exceeds i32")?;
    #[cfg(target_os = "macos")]
    let dim_i32 = i32::try_from(dimension).map_err(|_| "dimension exceeds i32")?;
    let mut scores = vec![0.0_f32; fixture.rows.len()];
    let mut arm_b2 = measure(&fixture, "B2", |query, limit| {
        #[cfg(target_os = "macos")]
        unsafe {
            cblas_sgemv(
                CBLAS_ROW_MAJOR,
                CBLAS_NO_TRANS,
                rows_i32,
                dim_i32,
                1.0,
                matrix.as_ptr(),
                dim_i32,
                query.embedding.as_ptr(),
                1,
                0.0,
                scores.as_mut_ptr(),
                1,
            );
        }
        #[cfg(not(target_os = "macos"))]
        full_scores(&matrix, &query.embedding, &mut scores);
        let query_inv_norm = inv_norm(&query.embedding);
        let top = bounded_top_n(
            fixture
                .rows
                .iter()
                .enumerate()
                .filter(|(_, row)| eligible(row, query))
                .map(|(index, row)| ScoredId {
                    score: scores[index] * inv_norms[index] * query_inv_norm,
                    id: row.id,
                }),
            limit,
        );
        top.into_iter()
            .map(|item| Candidate {
                id: item.id,
                cosine: item.score,
                content_hash: hex(&fixture.rows[item.id as usize].content_hash),
            })
            .collect()
    });
    arm_b2.backend = "accelerate-sgemv-full-scores-bounded-topn".into();
    arm_b2.build_ns = build_b2_ns;
    arm_b2.config = serde_json::json!({
        "resident":"f32-contiguous-matrix",
        "selection":"bounded-top-n",
        "kernel": if cfg!(target_os = "macos") { "Accelerate cblas_sgemv" } else { "runtime AVX2/FMA or scalar Rust" },
        "scoresComputed":"all rows (filter after)"
    });

    let mut arm_b3 = measure(&fixture, "B3", |query, limit| {
        let top = bounded_top_n(
            fixture
                .rows
                .iter()
                .zip(&quantized)
                .filter(|(row, _)| eligible(row, query))
                .map(|(row, vector)| ScoredId {
                    score: vector.cosine_with(&query.embedding),
                    id: row.id,
                }),
            limit,
        );
        top.into_iter()
            .map(|item| Candidate {
                id: item.id,
                cosine: item.score,
                content_hash: hex(&fixture.rows[item.id as usize].content_hash),
            })
            .collect()
    });
    arm_b3.backend = "optimized-rust-quantized-RESIDENT-bounded-topn".into();
    arm_b3.build_ns = build_b3_ns;
    arm_b3.config = serde_json::json!({"resident":"i8-per-vector-scale-prequantized","selection":"bounded-top-n"});

    let mut arm_b3_parallel = measure(&fixture, "B3-parallel", |query, limit| {
        let top = parallel_pool.install(|| {
            let scored = fixture
                .rows
                .par_iter()
                .zip(quantized.par_iter())
                .filter(|(row, _)| eligible(row, query))
                .map(|(row, vector)| ScoredId {
                    score: vector.cosine_with(&query.embedding),
                    id: row.id,
                })
                .collect::<Vec<_>>();
            bounded_top_n(scored.into_iter(), limit)
        });
        top.into_iter()
            .map(|item| Candidate {
                id: item.id,
                cosine: item.score,
                content_hash: hex(&fixture.rows[item.id as usize].content_hash),
            })
            .collect()
    });
    arm_b3_parallel.backend = "quantized-resident-rayon-bounded-topn".into();
    arm_b3_parallel.build_ns = build_b3_ns;
    arm_b3_parallel.config = serde_json::json!({
        "resident":"i8-per-vector-scale-prequantized",
        "selection":"bounded-top-n",
        "threads": parallel_pool.current_num_threads()
    });

    let mut arm_f16 = measure(&fixture, "B4-f16", |query, limit| {
        let query_f16 = query
            .embedding
            .iter()
            .copied()
            .map(f16::from_f32)
            .collect::<Vec<_>>();
        let query_inv_norm = inv_norm(&query.embedding);
        let top = bounded_top_n(
            fixture
                .rows
                .iter()
                .enumerate()
                .filter(|(_, row)| eligible(row, query))
                .map(|(index, row)| {
                    let offset = index * dimension;
                    let score = matrix_f16[offset..offset + dimension]
                        .iter()
                        .zip(&query_f16)
                        .map(|(a, b)| a.to_f32() * b.to_f32())
                        .sum::<f32>();
                    ScoredId {
                        score: score * inv_norms[index] * query_inv_norm,
                        id: row.id,
                    }
                }),
            limit,
        );
        top.into_iter()
            .map(|item| Candidate {
                id: item.id,
                cosine: item.score,
                content_hash: hex(&fixture.rows[item.id as usize].content_hash),
            })
            .collect()
    });
    arm_f16.backend = "f16-resident-scalar-convert-on-load-bounded-topn".into();
    arm_f16.build_ns = build_f16_ns;
    arm_f16.config = serde_json::json!({"resident":"f16-contiguous-matrix","selection":"bounded-top-n","rssBytesDelta":-(matrix.len() as i64 * 2)});

    // Parity gates. B3 vs A: exact ID equality (same guarantee the control runner
    // enforced). B2 vs A: BLAS reassociates float adds, so near-ties may swap —
    // require target presence plus per-query overlap within one candidate.
    let limit = fixture.cell.top_k;
    let mut min_overlap = limit;
    let mut overlap_sum = 0_usize;
    for (((a, b2), b3), b3_parallel) in arm_a
        .measurements
        .iter()
        .zip(&arm_b2.measurements)
        .zip(&arm_b3.measurements)
        .zip(&arm_b3_parallel.measurements)
    {
        if a.candidate_ids != b3.candidate_ids {
            return Err(format!("A/B3 parity failure at query {}", a.query_id));
        }
        if a.candidate_ids != b3_parallel.candidate_ids {
            return Err(format!(
                "A/B3-parallel parity failure at query {}",
                a.query_id
            ));
        }
        let overlap = b2
            .candidate_ids
            .iter()
            .filter(|id| a.candidate_ids.contains(id))
            .count();
        min_overlap = min_overlap.min(overlap);
        overlap_sum += overlap;
        if overlap + 1 < a.candidate_ids.len() || !b2.target_present {
            return Err(format!(
                "A/B2 parity failure at query {}: overlap {overlap}/{}",
                a.query_id,
                a.candidate_ids.len()
            ));
        }
    }
    let queries = arm_a.measurements.len().max(1);
    arm_b2.config["minOverlap"] = serde_json::json!(min_overlap);
    arm_b2.config["meanOverlap"] = serde_json::json!(overlap_sum as f64 / queries as f64);

    let bundle = RunBundle {
        schema_version: 1,
        generator_id: GENERATOR_ID.into(),
        runner: "simd".into(),
        runner_version: env!("CARGO_PKG_VERSION").into(),
        cell_id: cell.id.clone(),
        fixture_sha256: fixture.sha256,
        rows: fixture.rows.len(),
        queries: fixture.queries.len(),
        dimension,
        arms: vec![arm_a, arm_b2, arm_b3, arm_b3_parallel, arm_f16],
    };
    write_bundle_atomic(&args.output, &bundle)?;
    println!(
        "PASS runner=simd cell={} fixture={}",
        cell.id, bundle.fixture_sha256
    );
    Ok(())
}
