//! Follow-on SIMD/BLAS lane (plan §13.2): same fixtures and measure loop as the
//! Round 1 control runner, three exact arms.
//!   A  — byte-identical to control arm A (in-process paired reference)
//!   B2 — Accelerate cblas_sgemv over a contiguous dequantized matrix, bounded top-N
//!   B3 — quantized-RESIDENT bounded top-N (the arm the spec called "B"; the Round 1
//!        control runner re-quantized every row per query, which pessimized it)
//! B2 uses Accelerate on macOS & AVX2/FMA when available on x86_64.

use half::f16;
use memright_core::{cosine, QuantizedVector};
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

fn score_matrix(matrix: &[f32], query: &[f32], scores: &mut [f32], dimension: usize) {
    #[cfg(target_os = "macos")]
    unsafe {
        cblas_sgemv(
            CBLAS_ROW_MAJOR,
            CBLAS_NO_TRANS,
            i32::try_from(scores.len()).expect("row count exceeds i32"),
            i32::try_from(dimension).expect("dimension exceeds i32"),
            1.0,
            matrix.as_ptr(),
            i32::try_from(dimension).expect("dimension exceeds i32"),
            query.as_ptr(),
            1,
            0.0,
            scores.as_mut_ptr(),
            1,
        );
    }
    #[cfg(not(target_os = "macos"))]
    full_scores(matrix, query, scores);
}

fn integer_dot_scalar(left: &[i8], right: &[i8]) -> i32 {
    left.iter()
        .zip(right)
        .map(|(left, right)| *left as i32 * *right as i32)
        .sum()
}

#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "dotprod")]
unsafe fn integer_dot_neon_sdot(left: &[i8], right: &[i8]) -> i32 {
    use std::arch::asm;

    let chunks = left.len() / 16;
    let mut partial = [0_i32; 4];
    if chunks > 0 {
        asm!(
            "movi v0.4s, #0",
            "2:",
            "ld1 {{v1.16b}}, [{left}], #16",
            "ld1 {{v2.16b}}, [{right}], #16",
            "sdot v0.4s, v1.16b, v2.16b",
            "subs {remaining}, {remaining}, #1",
            "b.ne 2b",
            "st1 {{v0.4s}}, [{output}]",
            left = inout(reg) left.as_ptr() => _,
            right = inout(reg) right.as_ptr() => _,
            remaining = inout(reg) chunks => _,
            output = in(reg) partial.as_mut_ptr(),
            out("v0") _,
            out("v1") _,
            out("v2") _,
            options(nostack)
        );
    }
    partial.into_iter().sum::<i32>()
        + left[chunks * 16..]
            .iter()
            .zip(&right[chunks * 16..])
            .map(|(left, right)| *left as i32 * *right as i32)
            .sum::<i32>()
}

fn integer_dot_simd(left: &[i8], right: &[i8]) -> i32 {
    #[cfg(target_arch = "aarch64")]
    if std::arch::is_aarch64_feature_detected!("dotprod") {
        return unsafe { integer_dot_neon_sdot(left, right) };
    }
    integer_dot_scalar(left, right)
}

fn integer_residual_cosine(
    row: &QuantizedVector,
    query: &[f32],
    quantized_query: &QuantizedVector,
) -> f32 {
    let integer_dot = integer_dot_simd(row.values(), quantized_query.values());
    let residual_dot = row
        .values()
        .iter()
        .zip(query)
        .zip(quantized_query.values())
        .map(|((row_value, query_value), query_quantized)| {
            *row_value as f32 * (*query_value - *query_quantized as f32 * quantized_query.scale())
        })
        .sum::<f32>();
    let dot = row.scale() * (integer_dot as f32 * quantized_query.scale() + residual_dot);
    let mut row_norm = 0.0_f32;
    let mut query_norm = 0.0_f32;
    for (row_value, query_value) in row.values().iter().zip(query) {
        let value = *row_value as f32 * row.scale();
        row_norm += value * value;
        query_norm += query_value * query_value;
    }
    if row_norm == 0.0 || query_norm == 0.0 {
        0.0
    } else {
        dot / (row_norm.sqrt() * query_norm.sqrt())
    }
}

fn b3_simd_kernel_name() -> &'static str {
    #[cfg(target_arch = "aarch64")]
    if std::arch::is_aarch64_feature_detected!("dotprod") {
        return if std::arch::is_aarch64_feature_detected!("i8mm") {
            "neon-sdot-runtime-i8mm-detected"
        } else {
            "neon-sdot-runtime"
        };
    }
    "scalar-i8-dot-fallback"
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

    let mut scores = vec![0.0_f32; fixture.rows.len()];
    let mut arm_b2 = measure(&fixture, "B2", |query, limit| {
        score_matrix(&matrix, &query.embedding, &mut scores, dimension);
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

    let mut gather_matrix = Vec::<f32>::new();
    let mut gather_ids = Vec::<usize>::new();
    let mut gather_inv_norms = Vec::<f32>::new();
    let mut gather_scores = Vec::<f32>::new();
    let mut arm_b2_gather = measure(&fixture, "B2-gather", |query, limit| {
        gather_matrix.clear();
        gather_ids.clear();
        gather_inv_norms.clear();
        for (index, (row, embedding)) in hydrated.iter().enumerate() {
            if eligible(row, query) {
                gather_matrix.extend_from_slice(embedding);
                gather_ids.push(index);
                gather_inv_norms.push(inv_norms[index]);
            }
        }
        gather_scores.resize(gather_ids.len(), 0.0);
        score_matrix(
            &gather_matrix,
            &query.embedding,
            &mut gather_scores,
            dimension,
        );
        let query_inv_norm = inv_norm(&query.embedding);
        let top = bounded_top_n(
            gather_scores
                .iter()
                .zip(&gather_ids)
                .zip(&gather_inv_norms)
                .map(|((score, index), row_inv_norm)| ScoredId {
                    score: *score * *row_inv_norm * query_inv_norm,
                    id: fixture.rows[*index].id,
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
    arm_b2_gather.backend = "accelerate-filter-first-gather-sgemv-bounded-topn".into();
    arm_b2_gather.build_ns = build_b2_ns;
    arm_b2_gather.config = serde_json::json!({
        "resident":"f32-contiguous-matrix",
        "selection":"filter-first-gather-bounded-top-n",
        "kernel":"Accelerate cblas_sgemv",
        "scoresComputed":"eligible rows only"
    });

    let mut parallel_scores = vec![0.0_f32; fixture.rows.len()];
    let mut arm_b2_parallel = measure(&fixture, "parallel-B2", |query, limit| {
        score_matrix(&matrix, &query.embedding, &mut parallel_scores, dimension);
        let query_inv_norm = inv_norm(&query.embedding);
        let top = parallel_pool.install(|| {
            let scored = fixture
                .rows
                .par_iter()
                .enumerate()
                .filter(|(_, row)| eligible(row, query))
                .map(|(index, row)| ScoredId {
                    score: parallel_scores[index] * inv_norms[index] * query_inv_norm,
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
    arm_b2_parallel.backend = "accelerate-sgemv-parallel-filter-bounded-topn".into();
    arm_b2_parallel.build_ns = build_b2_ns;
    arm_b2_parallel.config = serde_json::json!({
        "resident":"f32-contiguous-matrix",
        "selection":"parallel-filter-bounded-top-n",
        "kernel":"Accelerate cblas_sgemv",
        "threads":parallel_pool.current_num_threads(),
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

    let mut arm_b3_simd = measure(&fixture, "B3-SIMD", |query, limit| {
        let quantized_query = QuantizedVector::quantize(&query.embedding);
        let preselection = bounded_top_n(
            fixture
                .rows
                .iter()
                .zip(&quantized)
                .filter(|(row, _)| eligible(row, query))
                .map(|(row, vector)| ScoredId {
                    score: integer_residual_cosine(vector, &query.embedding, &quantized_query),
                    id: row.id,
                }),
            limit.saturating_add(64),
        );
        let top = bounded_top_n(
            preselection.into_iter().map(|item| ScoredId {
                score: quantized[item.id as usize].cosine_with(&query.embedding),
                id: item.id,
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
    arm_b3_simd.backend = "quantized-resident-integer-dot-bounded-topn".into();
    arm_b3_simd.build_ns = build_b3_ns;
    arm_b3_simd.config = serde_json::json!({
        "resident":"i8-row-resident-query-i8-plus-f32-residual",
        "selection":"bounded-top-n",
        "kernel":b3_simd_kernel_name(),
        "integerAccumulatorParity":"unit-gated-exact",
        "exactRefineCandidates":fixture.cell.top_k.saturating_add(64)
    });

    let mut arm_b3_parallel = measure(&fixture, "parallel-B3", |query, limit| {
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

    let is_crossover = args.config.ends_with("crossover-v1.json");
    let arm_f16 = (!is_crossover).then(|| {
        let mut arm = measure(&fixture, "B4-f16", |query, limit| {
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
        arm.backend = "f16-resident-scalar-convert-on-load-bounded-topn".into();
        arm.build_ns = build_f16_ns;
        arm.config = serde_json::json!({"resident":"f16-contiguous-matrix","selection":"bounded-top-n","rssBytesDelta":-(matrix.len() as i64 * 2)});
        arm
    });

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
                "A/parallel-B3 parity failure at query {}",
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

    for arm in [&mut arm_b2_gather, &mut arm_b2_parallel] {
        let mut arm_min_overlap = limit;
        let mut arm_overlap_sum = 0_usize;
        for (a, candidate) in arm_a.measurements.iter().zip(&arm.measurements) {
            let overlap = candidate
                .candidate_ids
                .iter()
                .filter(|id| a.candidate_ids.contains(id))
                .count();
            arm_min_overlap = arm_min_overlap.min(overlap);
            arm_overlap_sum += overlap;
            if overlap + 1 < a.candidate_ids.len() || !candidate.target_present {
                return Err(format!(
                    "A/{} parity failure at query {}: overlap {overlap}/{}",
                    arm.arm,
                    a.query_id,
                    a.candidate_ids.len()
                ));
            }
        }
        arm.config["minOverlap"] = serde_json::json!(arm_min_overlap);
        arm.config["meanOverlap"] = serde_json::json!(arm_overlap_sum as f64 / queries as f64);
    }

    for (b3, b3_simd) in arm_b3.measurements.iter().zip(&arm_b3_simd.measurements) {
        if b3.candidate_ids != b3_simd.candidate_ids {
            return Err(format!(
                "B3/B3-SIMD candidate parity failure at query {}",
                b3.query_id
            ));
        }
    }

    let mut arms = vec![
        arm_a,
        arm_b2,
        arm_b2_gather,
        arm_b2_parallel,
        arm_b3,
        arm_b3_simd,
        arm_b3_parallel,
    ];
    if let Some(arm) = arm_f16 {
        arms.push(arm);
    }
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
        arms,
    };
    write_bundle_atomic(&args.output, &bundle)?;
    println!(
        "PASS runner=simd cell={} fixture={}",
        cell.id, bundle.fixture_sha256
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(items: Vec<ScoredId>) -> Vec<u64> {
        items.into_iter().map(|item| item.id).collect()
    }

    #[test]
    fn bounded_top_n_orders_scores_and_breaks_ties_by_id() {
        let items = [
            ScoredId { score: 0.5, id: 4 },
            ScoredId { score: 0.9, id: 3 },
            ScoredId { score: 0.9, id: 2 },
            ScoredId { score: 0.1, id: 1 },
        ];
        assert_eq!(ids(bounded_top_n(items.into_iter(), 3)), vec![2, 3, 4]);
    }

    #[test]
    fn neon_integer_dot_matches_scalar_exactly_with_tails() {
        for dimension in [1, 8, 13, 16, 37, 768] {
            let left = (0..dimension)
                .map(|index| ((index * 31 + 7) % 255) as i16 - 127)
                .map(|value| value as i8)
                .collect::<Vec<_>>();
            let right = (0..dimension)
                .map(|index| ((index * 17 + 3) % 255) as i16 - 127)
                .map(|value| value as i8)
                .collect::<Vec<_>>();
            assert_eq!(
                integer_dot_simd(&left, &right),
                integer_dot_scalar(&left, &right),
                "dimension {dimension}"
            );
        }
    }

    #[test]
    fn parallel_quantized_scan_matches_scalar_exactly() {
        let query = (0..37)
            .map(|index| (index as f32 * 0.13).sin())
            .collect::<Vec<_>>();
        let vectors = (0..64)
            .map(|row| {
                QuantizedVector::quantize(
                    &(0..37)
                        .map(|column| ((row * 37 + column) as f32 * 0.07).cos())
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>();
        let scalar = bounded_top_n(
            vectors.iter().enumerate().map(|(id, vector)| ScoredId {
                score: vector.cosine_with(&query),
                id: id as u64,
            }),
            16,
        );
        let parallel = bounded_top_n(
            vectors
                .par_iter()
                .enumerate()
                .map(|(id, vector)| ScoredId {
                    score: vector.cosine_with(&query),
                    id: id as u64,
                })
                .collect::<Vec<_>>()
                .into_iter(),
            16,
        );
        assert_eq!(ids(parallel), ids(scalar));
    }

    #[test]
    fn f16_top_n_stays_within_one_candidate_of_f32() {
        let dimension = 37;
        let query = (0..dimension)
            .map(|index| (index as f32 * 0.11).sin())
            .collect::<Vec<_>>();
        let rows = (0..64)
            .map(|row| {
                (0..dimension)
                    .map(|column| ((row * dimension + column) as f32 * 0.03).cos())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let full = bounded_top_n(
            rows.iter().enumerate().map(|(id, row)| ScoredId {
                score: cosine(row, &query),
                id: id as u64,
            }),
            16,
        );
        let query_f16 = query.iter().copied().map(f16::from_f32).collect::<Vec<_>>();
        let half = bounded_top_n(
            rows.iter().enumerate().map(|(id, row)| {
                let row_f16 = row.iter().copied().map(f16::from_f32).collect::<Vec<_>>();
                let dot = row_f16
                    .iter()
                    .zip(&query_f16)
                    .map(|(left, right)| left.to_f32() * right.to_f32())
                    .sum::<f32>();
                ScoredId {
                    score: dot * inv_norm(row) * inv_norm(&query),
                    id: id as u64,
                }
            }),
            16,
        );
        let full_ids = ids(full);
        let overlap = ids(half)
            .into_iter()
            .filter(|id| full_ids.contains(id))
            .count();
        assert!(overlap >= 15, "f16 overlap {overlap}/16");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn accelerate_scores_match_scalar_with_tail_dimension() {
        let dimension = 13;
        let rows = 3;
        let matrix = (0..rows * dimension)
            .map(|index| (index as f32 * 0.17).sin())
            .collect::<Vec<_>>();
        let query = (0..dimension)
            .map(|index| (index as f32 * 0.19).cos())
            .collect::<Vec<_>>();
        let expected = matrix
            .chunks_exact(dimension)
            .map(|row| row.iter().zip(&query).map(|(a, b)| a * b).sum::<f32>())
            .collect::<Vec<_>>();
        let mut actual = vec![0.0_f32; rows];
        unsafe {
            cblas_sgemv(
                CBLAS_ROW_MAJOR,
                CBLAS_NO_TRANS,
                rows as i32,
                dimension as i32,
                1.0,
                matrix.as_ptr(),
                dimension as i32,
                query.as_ptr(),
                1,
                0.0,
                actual.as_mut_ptr(),
                1,
            );
        }
        for (actual, expected) in actual.into_iter().zip(expected) {
            assert!((actual - expected).abs() < 1e-4, "{actual} != {expected}");
        }
    }
}
