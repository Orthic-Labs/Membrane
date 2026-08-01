//! Resident contiguous f32 vector projection with host-specific scoring dispatch.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::fmt;
use std::sync::OnceLock;

use rayon::prelude::*;
use rayon::{ThreadPool, ThreadPoolBuilder};

use crate::MemoryEntry;

#[cfg(target_arch = "x86")]
use std::arch::x86::{_mm256_fmadd_ps, _mm256_loadu_ps, _mm256_setzero_ps, _mm256_storeu_ps};
#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::{_mm256_fmadd_ps, _mm256_loadu_ps, _mm256_setzero_ps, _mm256_storeu_ps};

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

/// Selected scoring shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VectorKernel {
    ScalarFull,
    ParallelFull,
    Gather,
}

/// Measured host policy, exposed for deterministic boundary tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostVectorPolicy {
    Mac,
    Windows,
    Portable,
}

impl HostVectorPolicy {
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::Mac
        } else if cfg!(target_os = "windows") {
            Self::Windows
        } else {
            Self::Portable
        }
    }

    pub fn kernel(self, rows: usize, eligible: usize) -> VectorKernel {
        match self {
            Self::Mac if rows < 4_096 => VectorKernel::ScalarFull,
            Self::Mac if eligible.saturating_mul(4) < rows => VectorKernel::Gather,
            Self::Mac => VectorKernel::ParallelFull,
            Self::Windows if rows < 4_096 => VectorKernel::Gather,
            Self::Windows if eligible.saturating_mul(2) < rows => VectorKernel::Gather,
            Self::Windows => VectorKernel::ParallelFull,
            Self::Portable => VectorKernel::ScalarFull,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct VectorCandidate {
    pub id: String,
    pub score: f32,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum VectorSearchError {
    #[error("query dimension mismatch: expected {expected}, got {actual}")]
    DimensionMismatch { expected: usize, actual: usize },
    #[error("resident index has {incompatible} incompatible embedding dimensions")]
    HeterogeneousDimensions { incompatible: usize },
}

/// Projection updates happen with registry mutations; queries never rebuild it.
pub struct VectorIndex {
    dimension: Option<usize>,
    ids: Vec<String>,
    scopes: Vec<String>,
    rows: Vec<f32>,
    row_by_id: HashMap<String, usize>,
    incompatible_ids: HashSet<String>,
    generation: u64,
    pool: OnceLock<Option<ThreadPool>>,
}

impl fmt::Debug for VectorIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VectorIndex")
            .field("dimension", &self.dimension)
            .field("rows", &self.ids.len())
            .field("generation", &self.generation)
            .field("incompatible_rows", &self.incompatible_ids.len())
            .finish()
    }
}

impl Default for VectorIndex {
    fn default() -> Self {
        Self::new()
    }
}

impl VectorIndex {
    pub fn new() -> Self {
        Self {
            dimension: None,
            ids: Vec::new(),
            scopes: Vec::new(),
            rows: Vec::new(),
            row_by_id: HashMap::new(),
            incompatible_ids: HashSet::new(),
            generation: 0,
            pool: OnceLock::new(),
        }
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn row_count(&self) -> usize {
        self.ids.len()
    }

    pub fn dimension(&self) -> Option<usize> {
        self.dimension
    }

    pub fn resident_bytes(&self) -> usize {
        self.rows.len() * std::mem::size_of::<f32>()
    }

    pub fn upsert(&mut self, entry: &MemoryEntry) {
        let Some(embedding) = entry.embedding.as_deref().filter(|value| !value.is_empty()) else {
            self.remove(&entry.id);
            return;
        };
        if let Some(expected) = self.dimension {
            if embedding.len() != expected {
                let removed = self.remove(&entry.id);
                if self.incompatible_ids.insert(entry.id.clone()) && !removed {
                    self.generation = self.generation.saturating_add(1);
                }
                return;
            }
        } else {
            self.dimension = Some(embedding.len());
        }

        let normalized = normalize(embedding);
        self.incompatible_ids.remove(&entry.id);
        if let Some(&row) = self.row_by_id.get(&entry.id) {
            let dimension = self.dimension.expect("dimension initialized");
            self.rows[row * dimension..(row + 1) * dimension].copy_from_slice(&normalized);
            self.scopes[row].clone_from(&entry.scope_id);
        } else {
            let row = self.ids.len();
            self.row_by_id.insert(entry.id.clone(), row);
            self.ids.push(entry.id.clone());
            self.scopes.push(entry.scope_id.clone());
            self.rows.extend_from_slice(&normalized);
        }
        self.generation = self.generation.saturating_add(1);
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let removed_incompatible = self.incompatible_ids.remove(id);
        let Some(row) = self.row_by_id.remove(id) else {
            if removed_incompatible {
                self.generation = self.generation.saturating_add(1);
            }
            return removed_incompatible;
        };
        let dimension = self.dimension.expect("non-empty index has dimension");
        self.ids.remove(row);
        self.scopes.remove(row);
        self.rows.drain(row * dimension..(row + 1) * dimension);
        for current in row..self.ids.len() {
            self.row_by_id.insert(self.ids[current].clone(), current);
        }
        if self.ids.is_empty() {
            self.dimension = None;
        }
        self.generation = self.generation.saturating_add(1);
        true
    }

    pub fn top_k(
        &self,
        query: &[f32],
        eligible_scopes: Option<&[&str]>,
        limit: usize,
    ) -> Result<Vec<VectorCandidate>, VectorSearchError> {
        let Some(dimension) = self.dimension else {
            return Ok(Vec::new());
        };
        if query.len() != dimension {
            return Err(VectorSearchError::DimensionMismatch {
                expected: dimension,
                actual: query.len(),
            });
        }
        if !self.incompatible_ids.is_empty() {
            return Err(VectorSearchError::HeterogeneousDimensions {
                incompatible: self.incompatible_ids.len(),
            });
        }
        if limit == 0 {
            return Ok(Vec::new());
        }

        let eligible_scope_set = eligible_scopes.map(|scopes| scopes.iter().copied().collect());
        let eligible = self.eligible_rows(eligible_scope_set.as_ref());
        if eligible.is_empty() {
            return Ok(Vec::new());
        }
        let normalized_query = normalize(query);
        let kernel = HostVectorPolicy::current().kernel(self.row_count(), eligible.len());
        let scored = match kernel {
            VectorKernel::ScalarFull => self.score_full(&normalized_query, &eligible, false),
            VectorKernel::ParallelFull => self.score_full(&normalized_query, &eligible, true),
            VectorKernel::Gather => self.score_gather(&normalized_query, &eligible),
        };
        Ok(bounded_top_k(scored, limit))
    }

    fn eligible_rows(&self, scopes: Option<&HashSet<&str>>) -> Vec<usize> {
        match scopes {
            None => (0..self.ids.len()).collect(),
            Some(scopes) => self
                .scopes
                .iter()
                .enumerate()
                .filter_map(|(row, scope)| scopes.contains(scope.as_str()).then_some(row))
                .collect(),
        }
    }

    fn score_full(&self, query: &[f32], eligible: &[usize], parallel: bool) -> Vec<HeapCandidate> {
        let dimension = self.dimension.expect("dimension initialized");
        let mut scores = vec![0.0_f32; self.ids.len()];
        if parallel {
            let mut run = || {
                let threads = rayon::current_num_threads().max(1);
                let chunk_rows = scores.len().div_ceil(threads * 4).max(1);
                scores
                    .par_chunks_mut(chunk_rows)
                    .zip(self.rows.par_chunks(chunk_rows * dimension))
                    .for_each(|(output, rows)| score_matrix(rows, query, output, dimension));
            };
            if let Some(pool) = self.parallel_pool() {
                pool.install(run);
            } else {
                run();
            }
        } else {
            score_matrix(&self.rows, query, &mut scores, dimension);
        }
        eligible
            .iter()
            .map(|&row| HeapCandidate::new(self.ids[row].clone(), finite(scores[row])))
            .collect()
    }

    fn parallel_pool(&self) -> Option<&ThreadPool> {
        self.pool
            .get_or_init(|| {
                let threads = std::thread::available_parallelism()
                    .map(usize::from)
                    .unwrap_or(1)
                    .max(1);
                ThreadPoolBuilder::new()
                    .num_threads(threads)
                    .thread_name(|index| format!("crypt-vector-{index}"))
                    .build()
                    .ok()
            })
            .as_ref()
    }

    fn score_gather(&self, query: &[f32], eligible: &[usize]) -> Vec<HeapCandidate> {
        let dimension = self.dimension.expect("dimension initialized");
        let mut matrix = Vec::with_capacity(eligible.len() * dimension);
        for &row in eligible {
            matrix.extend_from_slice(&self.rows[row * dimension..(row + 1) * dimension]);
        }
        let mut scores = vec![0.0_f32; eligible.len()];
        score_matrix(&matrix, query, &mut scores, dimension);
        eligible
            .iter()
            .zip(scores)
            .map(|(&row, score)| HeapCandidate::new(self.ids[row].clone(), finite(score)))
            .collect()
    }
}

#[derive(Clone)]
struct HeapCandidate {
    id: String,
    score: f32,
}

impl HeapCandidate {
    fn new(id: String, score: f32) -> Self {
        Self { id, score }
    }
}

impl PartialEq for HeapCandidate {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.score.to_bits() == other.score.to_bits()
    }
}

impl Eq for HeapCandidate {}

impl PartialOrd for HeapCandidate {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for HeapCandidate {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.score
            .total_cmp(&other.score)
            .then_with(|| other.id.cmp(&self.id))
    }
}

fn bounded_top_k(scored: Vec<HeapCandidate>, limit: usize) -> Vec<VectorCandidate> {
    let mut heap = BinaryHeap::with_capacity(limit.saturating_add(1));
    for candidate in scored {
        heap.push(Reverse(candidate));
        if heap.len() > limit {
            heap.pop();
        }
    }
    let mut candidates = heap
        .into_iter()
        .map(|Reverse(candidate)| candidate)
        .collect::<Vec<_>>();
    candidates.sort_by(|a, b| b.cmp(a));
    candidates
        .into_iter()
        .map(|candidate| VectorCandidate {
            id: candidate.id,
            score: candidate.score,
        })
        .collect()
}

fn normalize(values: &[f32]) -> Vec<f32> {
    let norm = values.iter().map(|value| value * value).sum::<f32>().sqrt();
    if norm > 0.0 && norm.is_finite() {
        values.iter().map(|value| value / norm).collect()
    } else {
        vec![0.0; values.len()]
    }
}

fn finite(score: f32) -> f32 {
    if score.is_finite() {
        score
    } else {
        0.0
    }
}

#[cfg(target_os = "macos")]
fn score_matrix(matrix: &[f32], query: &[f32], scores: &mut [f32], dimension: usize) {
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
fn score_matrix(matrix: &[f32], query: &[f32], scores: &mut [f32], dimension: usize) {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    if std::is_x86_feature_detected!("avx2") && std::is_x86_feature_detected!("fma") {
        unsafe { avx2_fma_scores(matrix, query, scores, dimension) };
        return;
    }
    scalar_scores(matrix, query, scores, dimension);
}

#[cfg(not(target_os = "macos"))]
fn scalar_scores(matrix: &[f32], query: &[f32], scores: &mut [f32], dimension: usize) {
    for (row, score) in matrix.chunks_exact(dimension).zip(scores.iter_mut()) {
        *score = row
            .iter()
            .zip(query)
            .map(|(left, right)| left * right)
            .sum();
    }
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
