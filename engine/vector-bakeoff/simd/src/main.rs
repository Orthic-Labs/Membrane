//! Follow-on SIMD/BLAS lane (plan §13.2): same fixtures and measure loop as the
//! Round 1 control runner, seven parity-gated arms.
//!   A  — byte-identical to control arm A (in-process paired reference)
//!   B2 — Accelerate cblas_sgemv over a contiguous dequantized matrix, bounded top-N
//!   B3 — quantized-RESIDENT bounded top-N (the arm the spec called "B"; the Round 1
//!        control runner re-quantized every row per query, which pessimized it)
//! Additional arms measure Rayon parallel scans, exact i8 SIMD, and eligible-row gather.
//! macOS uses Accelerate for B2; x86 Windows uses runtime-gated explicit AVX2/FMA
//! intrinsics with a scalar fallback.

use crypt_core::{cosine, QuantizedVector};
use rayon::prelude::*;
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::time::Instant;
use vector_bakeoff_common::{
    eligible, generate_fixture, hex, load_config, measure, parse_runner_args, selected_cell,
    write_bundle_atomic, Candidate, RunBundle, GENERATOR_ID,
};

#[cfg(target_arch = "x86")]
use std::arch::x86::{
    __m256i, _mm256_abs_epi8, _mm256_add_epi32, _mm256_dpbusd_avx_epi32, _mm256_fmadd_ps,
    _mm256_loadu_ps, _mm256_loadu_si256, _mm256_madd_epi16, _mm256_maddubs_epi16,
    _mm256_set1_epi16, _mm256_setzero_ps, _mm256_setzero_si256, _mm256_sign_epi8, _mm256_storeu_ps,
    _mm256_storeu_si256,
};
#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::{
    __m256i, _mm256_abs_epi8, _mm256_add_epi32, _mm256_dpbusd_avx_epi32, _mm256_fmadd_ps,
    _mm256_loadu_ps, _mm256_loadu_si256, _mm256_madd_epi16, _mm256_maddubs_epi16,
    _mm256_set1_epi16, _mm256_setzero_ps, _mm256_setzero_si256, _mm256_sign_epi8, _mm256_storeu_ps,
    _mm256_storeu_si256,
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

#[cfg(target_os = "macos")]
fn score_b2(matrix: &[f32], query: &[f32], scores: &mut [f32], dimension: usize) {
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
}

#[cfg(not(target_os = "macos"))]
fn score_b2(matrix: &[f32], query: &[f32], scores: &mut [f32], dimension: usize) {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    if std::is_x86_feature_detected!("avx2") && std::is_x86_feature_detected!("fma") {
        unsafe { avx2_fma_scores(matrix, query, scores, dimension) };
        return;
    }
    scalar_scores(matrix, query, scores, dimension);
}

#[cfg(target_os = "macos")]
fn b2_kernel_name() -> &'static str {
    "Accelerate cblas_sgemv"
}

#[cfg(target_os = "macos")]
fn b2_backend_name() -> &'static str {
    "accelerate-sgemv-full-scores-bounded-topn"
}

#[cfg(not(target_os = "macos"))]
fn b2_kernel_name() -> &'static str {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    if std::is_x86_feature_detected!("avx2") && std::is_x86_feature_detected!("fma") {
        return "rust-avx2-fma-intrinsics";
    }
    "rust-scalar-fallback"
}

#[cfg(not(target_os = "macos"))]
fn b2_backend_name() -> &'static str {
    "rust-avx2-fma-full-scores-bounded-topn"
}

#[cfg(any(test, not(target_os = "macos")))]
fn scalar_scores(matrix: &[f32], query: &[f32], scores: &mut [f32], dimension: usize) {
    for (row, score) in matrix.chunks_exact(dimension).zip(scores.iter_mut()) {
        *score = row
            .iter()
            .zip(query)
            .map(|(left, right)| left * right)
            .sum();
    }
}

fn score_b2_parallel(matrix: &[f32], query: &[f32], scores: &mut [f32], dimension: usize) {
    let threads = rayon::current_num_threads().max(1);
    let chunk_rows = scores.len().div_ceil(threads * 4).max(1);
    scores
        .par_chunks_mut(chunk_rows)
        .zip(matrix.par_chunks(chunk_rows * dimension))
        .for_each(|(output, rows)| score_b2(rows, query, output, dimension));
}

fn score_b2_gather(
    matrix: &[f32],
    query: &[f32],
    row_indices: &[usize],
    dimension: usize,
) -> Vec<f32> {
    row_indices
        .iter()
        .map(|index| {
            let start = index * dimension;
            let mut score = [0.0_f32];
            score_b2(
                &matrix[start..start + dimension],
                query,
                &mut score,
                dimension,
            );
            score[0]
        })
        .collect()
}

fn scalar_i8_dot(left: &[i8], right: &[i8]) -> i32 {
    left.iter()
        .zip(right)
        .map(|(a, b)| i32::from(*a) * i32::from(*b))
        .sum()
}

fn i8_norm_sq(values: &[i8]) -> i32 {
    values
        .iter()
        .map(|value| i32::from(*value) * i32::from(*value))
        .sum()
}

fn quantized_inv_norm(vector: &QuantizedVector) -> f32 {
    let norm_sq = i8_norm_sq(vector.values());
    if norm_sq == 0 {
        0.0
    } else {
        1.0 / ((norm_sq as f32).sqrt() * vector.scale())
    }
}

fn quantized_cosine_from_dot(
    row: &QuantizedVector,
    query: &QuantizedVector,
    row_inv_norm: f32,
    query_inv_norm: f32,
    dot: i32,
) -> f32 {
    if row.len() != query.len() || row.is_empty() {
        return 0.0;
    }
    if row_inv_norm == 0.0 || query_inv_norm == 0.0 {
        return 0.0;
    }
    dot as f32 * row.scale() * query.scale() * row_inv_norm * query_inv_norm
}

fn scalar_b3_score(
    row: &QuantizedVector,
    query: &QuantizedVector,
    row_inv_norm: f32,
    query_inv_norm: f32,
) -> f32 {
    quantized_cosine_from_dot(
        row,
        query,
        row_inv_norm,
        query_inv_norm,
        scalar_i8_dot(row.values(), query.values()),
    )
}

fn simd_b3_score(
    row: &QuantizedVector,
    query: &QuantizedVector,
    row_inv_norm: f32,
    query_inv_norm: f32,
) -> f32 {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    let dot = if std::is_x86_feature_detected!("avxvnni") {
        unsafe { avxvnni_i8_dot(row.values(), query.values()) }
    } else if std::is_x86_feature_detected!("avx2") {
        unsafe { avx2_i8_dot(row.values(), query.values()) }
    } else {
        scalar_i8_dot(row.values(), query.values())
    };
    #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
    let dot = scalar_i8_dot(row.values(), query.values());
    quantized_cosine_from_dot(row, query, row_inv_norm, query_inv_norm, dot)
}

fn b3_kernel_name() -> &'static str {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        if std::is_x86_feature_detected!("avxvnni") {
            return "AVX-VNNI vpdpbusd exact-i8";
        }
        if std::is_x86_feature_detected!("avx2") {
            return "AVX2 vpmaddubsw/vpmaddwd exact-i8";
        }
    }
    "rust-scalar exact-i8"
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
unsafe fn avx2_i8_dot(left: &[i8], right: &[i8]) -> i32 {
    debug_assert_eq!(left.len(), right.len());
    let mut index = 0;
    let mut lanes = _mm256_setzero_si256();
    let ones = _mm256_set1_epi16(1);
    while index + 32 <= left.len() {
        let a = _mm256_loadu_si256(left.as_ptr().add(index).cast::<__m256i>());
        let b = _mm256_loadu_si256(right.as_ptr().add(index).cast::<__m256i>());
        let magnitudes = _mm256_abs_epi8(a);
        let signed_b = _mm256_sign_epi8(b, a);
        let pairs = _mm256_maddubs_epi16(magnitudes, signed_b);
        lanes = _mm256_add_epi32(lanes, _mm256_madd_epi16(pairs, ones));
        index += 32;
    }
    let mut partial = [0_i32; 8];
    _mm256_storeu_si256(partial.as_mut_ptr().cast::<__m256i>(), lanes);
    partial.iter().sum::<i32>() + scalar_i8_dot(&left[index..], &right[index..])
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2,avxvnni")]
unsafe fn avxvnni_i8_dot(left: &[i8], right: &[i8]) -> i32 {
    debug_assert_eq!(left.len(), right.len());
    let mut index = 0;
    let mut lanes = _mm256_setzero_si256();
    while index + 32 <= left.len() {
        let a = _mm256_loadu_si256(left.as_ptr().add(index).cast::<__m256i>());
        let b = _mm256_loadu_si256(right.as_ptr().add(index).cast::<__m256i>());
        lanes = _mm256_dpbusd_avx_epi32(lanes, _mm256_abs_epi8(a), _mm256_sign_epi8(b, a));
        index += 32;
    }
    let mut partial = [0_i32; 8];
    _mm256_storeu_si256(partial.as_mut_ptr().cast::<__m256i>(), lanes);
    partial.iter().sum::<i32>() + scalar_i8_dot(&left[index..], &right[index..])
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2,fma")]
unsafe fn avx2_fma_scores(matrix: &[f32], query: &[f32], scores: &mut [f32], dimension: usize) {
    for (row, score) in matrix.chunks_exact(dimension).zip(scores.iter_mut()) {
        let mut index = 0;
        let mut lanes = _mm256_setzero_ps();
        while index + 8 <= dimension {
            let left = _mm256_loadu_ps(row.as_ptr().add(index));
            let right = _mm256_loadu_ps(query.as_ptr().add(index));
            lanes = _mm256_fmadd_ps(left, right, lanes);
            index += 8;
        }
        let mut partial = [0.0_f32; 8];
        _mm256_storeu_ps(partial.as_mut_ptr(), lanes);
        *score = partial.iter().sum::<f32>()
            + row[index..]
                .iter()
                .zip(&query[index..])
                .map(|(left, right)| left * right)
                .sum::<f32>();
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
    let quantized_inv_norms = quantized.iter().map(quantized_inv_norm).collect::<Vec<_>>();
    let build_b3_ns = build_b3_started.elapsed().as_nanos().min(u64::MAX as u128) as u64;

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
        score_b2(&matrix, &query.embedding, &mut scores, dimension);
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
    arm_b2.backend = b2_backend_name().into();
    arm_b2.build_ns = build_b2_ns;
    arm_b2.config = serde_json::json!({
        "resident":"f32-contiguous-matrix",
        "selection":"bounded-top-n",
        "kernel": b2_kernel_name(),
        "scoresComputed":"all rows (filter after)"
    });

    let mut arm_b3 = measure(&fixture, "B3", |query, limit| {
        let quantized_query = QuantizedVector::quantize(&query.embedding);
        let query_inv_norm = quantized_inv_norm(&quantized_query);
        let prefilter = bounded_top_n(
            fixture
                .rows
                .iter()
                .enumerate()
                .filter(|(_, row)| eligible(row, query))
                .map(|(index, row)| ScoredId {
                    score: scalar_b3_score(
                        &quantized[index],
                        &quantized_query,
                        quantized_inv_norms[index],
                        query_inv_norm,
                    ),
                    id: row.id,
                }),
            limit.saturating_mul(32),
        );
        let top = bounded_top_n(
            prefilter.into_iter().map(|item| ScoredId {
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
    arm_b3.backend = "optimized-rust-quantized-RESIDENT-bounded-topn".into();
    arm_b3.build_ns = build_b3_ns;
    arm_b3.config = serde_json::json!({
        "resident":"i8-per-vector-scale-prequantized",
        "selection":"integer-prefilter-plus-exact-rerank",
        "rerankCandidates":fixture.cell.top_k.saturating_mul(32),
        "kernel":"rust-scalar exact-i8",
        "norm":"precomputed-scaled-i8-invnorm"
    });

    let mut arm_parallel_b3 = measure(&fixture, "parallel-B3", |query, limit| {
        let quantized_query = QuantizedVector::quantize(&query.embedding);
        let query_inv_norm = quantized_inv_norm(&quantized_query);
        let scored = fixture
            .rows
            .par_iter()
            .enumerate()
            .filter(|(_, row)| eligible(row, query))
            .map(|(index, row)| ScoredId {
                score: scalar_b3_score(
                    &quantized[index],
                    &quantized_query,
                    quantized_inv_norms[index],
                    query_inv_norm,
                ),
                id: row.id,
            })
            .collect::<Vec<_>>();
        let prefilter = bounded_top_n(scored.into_iter(), limit.saturating_mul(32));
        bounded_top_n(
            prefilter.into_iter().map(|item| ScoredId {
                score: quantized[item.id as usize].cosine_with(&query.embedding),
                id: item.id,
            }),
            limit,
        )
        .into_iter()
        .map(|item| Candidate {
            id: item.id,
            cosine: item.score,
            content_hash: hex(&fixture.rows[item.id as usize].content_hash),
        })
        .collect()
    });
    arm_parallel_b3.backend = "rayon-rust-quantized-resident-bounded-topn".into();
    arm_parallel_b3.build_ns = build_b3_ns;
    arm_parallel_b3.config = serde_json::json!({
        "resident":"i8-per-vector-scale-prequantized",
        "selection":"integer-prefilter-plus-exact-rerank",
        "rerankCandidates":fixture.cell.top_k.saturating_mul(32),
        "kernel":"rayon chunks over rust-scalar exact-i8",
        "norm":"precomputed-scaled-i8-invnorm",
        "threadCount":rayon::current_num_threads()
    });

    let mut parallel_scores = vec![0.0_f32; fixture.rows.len()];
    let mut arm_parallel_b2 = measure(&fixture, "parallel-B2", |query, limit| {
        score_b2_parallel(&matrix, &query.embedding, &mut parallel_scores, dimension);
        let query_inv_norm = inv_norm(&query.embedding);
        let top = bounded_top_n(
            fixture
                .rows
                .iter()
                .enumerate()
                .filter(|(_, row)| eligible(row, query))
                .map(|(index, row)| ScoredId {
                    score: parallel_scores[index] * inv_norms[index] * query_inv_norm,
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
    arm_parallel_b2.backend = if cfg!(target_os = "macos") {
        "rayon-accelerate-sgemv-full-scores-bounded-topn"
    } else {
        "rayon-avx2-fma-full-scores-bounded-topn"
    }
    .into();
    arm_parallel_b2.build_ns = build_b2_ns;
    arm_parallel_b2.config = serde_json::json!({
        "resident":"f32-contiguous-matrix",
        "selection":"bounded-top-n",
        "kernel":format!("rayon chunks over {}", b2_kernel_name()),
        "scoresComputed":"all rows (filter after)",
        "threadCount":rayon::current_num_threads()
    });

    let mut arm_b3_simd = measure(&fixture, "B3-SIMD", |query, limit| {
        let quantized_query = QuantizedVector::quantize(&query.embedding);
        let query_inv_norm = quantized_inv_norm(&quantized_query);
        let prefilter = bounded_top_n(
            fixture
                .rows
                .iter()
                .enumerate()
                .filter(|(_, row)| eligible(row, query))
                .map(|(index, row)| ScoredId {
                    score: simd_b3_score(
                        &quantized[index],
                        &quantized_query,
                        quantized_inv_norms[index],
                        query_inv_norm,
                    ),
                    id: row.id,
                }),
            limit.saturating_mul(32),
        );
        let top = bounded_top_n(
            prefilter.into_iter().map(|item| ScoredId {
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
    arm_b3_simd.backend = "runtime-dispatched-exact-i8-simd-bounded-topn".into();
    arm_b3_simd.build_ns = build_b3_ns;
    arm_b3_simd.config = serde_json::json!({
        "resident":"i8-per-vector-scale-prequantized",
        "selection":"integer-prefilter-plus-exact-rerank",
        "rerankCandidates":fixture.cell.top_k.saturating_mul(32),
        "kernel":b3_kernel_name(),
        "norm":"precomputed-scaled-i8-invnorm",
        "integerParity":"exact"
    });

    let mut arm_b2_gather = measure(&fixture, "B2-gather", |query, limit| {
        let row_indices = fixture
            .rows
            .iter()
            .enumerate()
            .filter(|(_, row)| eligible(row, query))
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let gathered = score_b2_gather(&matrix, &query.embedding, &row_indices, dimension);
        let query_inv_norm = inv_norm(&query.embedding);
        let top = bounded_top_n(
            row_indices
                .iter()
                .zip(gathered)
                .map(|(index, score)| ScoredId {
                    score: score * inv_norms[*index] * query_inv_norm,
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
    arm_b2_gather.backend = if cfg!(target_os = "macos") {
        "eligible-row-gather-accelerate-sgemv-bounded-topn"
    } else {
        "eligible-row-gather-avx2-fma-bounded-topn"
    }
    .into();
    arm_b2_gather.build_ns = build_b2_ns;
    arm_b2_gather.config = serde_json::json!({
        "resident":"f32-contiguous-matrix",
        "selection":"bounded-top-n",
        "kernel":b2_kernel_name(),
        "scoresComputed":"eligible rows only (gather first)"
    });

    // Parity gates. B3 vs A: exact ID equality (same guarantee the control runner
    // enforced). B2 vs A: BLAS reassociates float adds, so near-ties may swap —
    // require target presence plus per-query overlap within one candidate.
    let limit = fixture.cell.top_k;
    let mut min_overlaps = [limit; 3];
    let mut overlap_sums = [0_usize; 3];
    for index in 0..arm_a.measurements.len() {
        let a = &arm_a.measurements[index];
        let b3 = &arm_b3.measurements[index];
        if a.candidate_ids != b3.candidate_ids {
            return Err(format!(
                "A/B3 exact candidate parity failure at query {}",
                a.query_id
            ));
        }
        for (name, exact) in [
            ("parallel-B3", &arm_parallel_b3.measurements[index]),
            ("B3-SIMD", &arm_b3_simd.measurements[index]),
        ] {
            if b3.candidate_ids != exact.candidate_ids {
                return Err(format!(
                    "B3/{name} exact parity failure at query {}",
                    a.query_id
                ));
            }
        }
        for (arm_index, (name, approximate)) in [
            ("B2", &arm_b2.measurements[index]),
            ("parallel-B2", &arm_parallel_b2.measurements[index]),
            ("B2-gather", &arm_b2_gather.measurements[index]),
        ]
        .into_iter()
        .enumerate()
        {
            let overlap = approximate
                .candidate_ids
                .iter()
                .filter(|id| a.candidate_ids.contains(id))
                .count();
            min_overlaps[arm_index] = min_overlaps[arm_index].min(overlap);
            overlap_sums[arm_index] += overlap;
            if overlap + 1 < a.candidate_ids.len() || !approximate.target_present {
                return Err(format!(
                    "A/{name} parity failure at query {}: overlap {overlap}/{}",
                    a.query_id,
                    a.candidate_ids.len()
                ));
            }
        }
    }
    let queries = arm_a.measurements.len().max(1);
    for (arm, index) in [
        (&mut arm_b2, 0_usize),
        (&mut arm_parallel_b2, 1_usize),
        (&mut arm_b2_gather, 2_usize),
    ] {
        arm.config["minOverlap"] = serde_json::json!(min_overlaps[index]);
        arm.config["meanOverlap"] = serde_json::json!(overlap_sums[index] as f64 / queries as f64);
    }
    arm_b3.config["aParity"] = serde_json::json!("exact-ordered");
    arm_parallel_b3.config["scalarB3Parity"] = serde_json::json!("exact-ordered");
    arm_b3_simd.config["scalarB3Parity"] = serde_json::json!("exact-ordered");

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
        arms: vec![
            arm_a,
            arm_b2,
            arm_b3,
            arm_parallel_b3,
            arm_parallel_b2,
            arm_b3_simd,
            arm_b2_gather,
        ],
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

    fn matrix(rows: usize, dimension: usize) -> Vec<f32> {
        (0..rows * dimension)
            .map(|index| ((index * 37 % 101) as f32 - 50.0) / 17.0)
            .collect()
    }

    fn query(dimension: usize) -> Vec<f32> {
        (0..dimension)
            .map(|index| ((index * 19 % 67) as f32 - 33.0) / 13.0)
            .collect()
    }

    #[test]
    fn scalar_scores_cover_every_row_and_tail_element() {
        let dimension = 13;
        let matrix = matrix(4, dimension);
        let query = query(dimension);
        let mut actual = vec![0.0; 4];
        scalar_scores(&matrix, &query, &mut actual, dimension);
        for (row, score) in matrix.chunks_exact(dimension).zip(actual) {
            let expected = row.iter().zip(&query).map(|(a, b)| a * b).sum::<f32>();
            assert!((score - expected).abs() <= 1.0e-5);
        }
    }

    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    #[test]
    fn avx2_fma_scores_match_scalar_with_tail() {
        if !std::is_x86_feature_detected!("avx2") || !std::is_x86_feature_detected!("fma") {
            return;
        }
        for dimension in [8, 13, 32, 37] {
            let matrix = matrix(5, dimension);
            let query = query(dimension);
            let mut scalar = vec![0.0; 5];
            let mut vectorized = vec![0.0; 5];
            scalar_scores(&matrix, &query, &mut scalar, dimension);
            unsafe { avx2_fma_scores(&matrix, &query, &mut vectorized, dimension) };
            for (expected, actual) in scalar.iter().zip(&vectorized) {
                let tolerance = 1.0e-4_f32.max(expected.abs() * 1.0e-5);
                assert!((actual - expected).abs() <= tolerance);
            }
        }
    }

    #[test]
    fn scalar_i8_dot_handles_extremes_and_tail() {
        let left = [-127_i8, 127, -126, 126, -1, 1, 0, 64, -64, 17, -19, 23, -29];
        let right = [127_i8, -127, 126, -126, 1, -1, 0, -64, 64, -17, 19, -23, 29];
        let expected = left
            .iter()
            .zip(&right)
            .map(|(a, b)| i32::from(*a) * i32::from(*b))
            .sum::<i32>();
        assert_eq!(scalar_i8_dot(&left, &right), expected);
    }

    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    #[test]
    fn avx2_i8_dot_matches_scalar_exactly() {
        if !std::is_x86_feature_detected!("avx2") {
            return;
        }
        for dimension in [1, 13, 31, 32, 33, 127, 768, 773] {
            let left = (0..dimension)
                .map(|index| ((index * 73 % 255) as i16 - 127) as i8)
                .collect::<Vec<_>>();
            let right = (0..dimension)
                .map(|index| ((index * 131 % 255) as i16 - 127) as i8)
                .collect::<Vec<_>>();
            assert_eq!(
                unsafe { avx2_i8_dot(&left, &right) },
                scalar_i8_dot(&left, &right)
            );
        }
    }

    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    #[test]
    fn avxvnni_i8_dot_matches_scalar_exactly() {
        if !std::is_x86_feature_detected!("avxvnni") {
            return;
        }
        for dimension in [32, 33, 127, 768, 773] {
            let left = (0..dimension)
                .map(|index| ((index * 73 % 255) as i16 - 127) as i8)
                .collect::<Vec<_>>();
            let right = (0..dimension)
                .map(|index| ((index * 131 % 255) as i16 - 127) as i8)
                .collect::<Vec<_>>();
            assert_eq!(
                unsafe { avxvnni_i8_dot(&left, &right) },
                scalar_i8_dot(&left, &right)
            );
        }
    }

    #[test]
    fn gathered_b2_scores_match_full_scores_for_selected_rows() {
        let dimension = 13;
        let matrix = matrix(8, dimension);
        let query = query(dimension);
        let indices = [0_usize, 2, 5, 7];
        let mut full = vec![0.0; 8];
        score_b2(&matrix, &query, &mut full, dimension);
        let gathered = score_b2_gather(&matrix, &query, &indices, dimension);
        for (index, actual) in indices.into_iter().zip(gathered) {
            let tolerance = 1.0e-4_f32.max(full[index].abs() * 1.0e-5);
            assert!((actual - full[index]).abs() <= tolerance);
        }
    }

    #[test]
    fn parallel_scores_match_serial_scores() {
        let dimension = 37;
        let matrix = matrix(33, dimension);
        let query = query(dimension);
        let mut serial = vec![0.0; 33];
        let mut parallel = vec![0.0; 33];
        score_b2(&matrix, &query, &mut serial, dimension);
        score_b2_parallel(&matrix, &query, &mut parallel, dimension);
        for (expected, actual) in serial.iter().zip(&parallel) {
            let tolerance = 1.0e-4_f32.max(expected.abs() * 1.0e-5);
            assert!((actual - expected).abs() <= tolerance);
        }
    }
}
