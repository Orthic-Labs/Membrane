//! Pipeline fingerprint for deterministic reindex skip decisions.
//!
//! A `PipelineFingerprint` captures the full embedding pipeline configuration
//! as a SHA-256 hash. Rows whose `(content_hash, full_pipeline_digest)` both
//! match need no re-embedding on `reindex`.

use sha2::{Digest, Sha256};

/// Pipeline fingerprint — SHA-256 of the embedding pipeline configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineFingerprint {
    /// The 64-hex-character SHA-256 digest.
    pub digest: String,
    /// Human-readable label for metrics: `pf:v1:<64-hex>`.
    pub label: String,
    /// Maximum sequence length used by the tokenizer.
    pub max_sequence_length: usize,
    /// Model identifier (e.g., "embeddinggemma-300m-q4").
    pub model_id: String,
}

impl PipelineFingerprint {
    /// Compute a fingerprint from the pipeline configuration.
    pub fn compute(
        max_sequence_length: usize,
        prompt_template_doc: &str,
        prompt_template_query: &str,
        model_id: &str,
        pooling: &str,
        normalize: bool,
        pipeline_version: u32,
    ) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(max_sequence_length.to_le_bytes());
        hasher.update(prompt_template_doc.as_bytes());
        hasher.update(prompt_template_query.as_bytes());
        hasher.update(model_id.as_bytes());
        hasher.update(pooling.as_bytes());
        hasher.update([normalize as u8]);
        hasher.update(pipeline_version.to_le_bytes());

        let digest = format!("{:x}", hasher.finalize());
        let label = format!("pf:v1:{digest}");

        Self {
            digest,
            label,
            max_sequence_length,
            model_id: model_id.to_string(),
        }
    }

    /// Create a fingerprint for the HashEmbedder (no real model).
    pub fn hash_embedder() -> Self {
        Self::compute(0, "", "", "hash-256", "mean", true, 1)
    }

    /// Create a fingerprint for the selectable BGE-small embedding pipeline.
    pub fn bge_small_en_v1_5() -> Self {
        Self::compute(
            super::embed::EMBEDDING_MAX_SEQUENCE_TOKENS,
            "{text}",
            "Represent this sentence for searching relevant passages: {text}",
            "bge-small-en-v1.5",
            "mean",
            true,
            1,
        )
    }

    /// Create a fingerprint for the selectable unquantized EmbeddingGemma pipeline.
    pub fn gemma_300m() -> Self {
        Self::compute(
            super::embed::EMBEDDING_MAX_SEQUENCE_TOKENS,
            "title: none | text: {text}",
            "task: search result | query: {text}",
            "embeddinggemma-300m",
            "mean",
            true,
            1,
        )
    }

    /// Create a fingerprint for the current production Gemma-300M-Q4 pipeline.
    pub fn gemma_300m_q4() -> Self {
        Self::compute(
            super::embed::EMBEDDING_MAX_SEQUENCE_TOKENS,
            "title: none | text: {text}",
            "task: search result | query: {text}",
            "embeddinggemma-300m-q4",
            "mean",
            true,
            1,
        )
    }

    /// Resolve the declared pipeline for a built-in embedder model identifier.
    pub fn for_model_id(model_id: &str) -> Option<Self> {
        match model_id {
            "hash-256" => Some(Self::hash_embedder()),
            "bge-small-en-v1.5" => Some(Self::bge_small_en_v1_5()),
            "embeddinggemma-300m" => Some(Self::gemma_300m()),
            "embeddinggemma-300m-q4" => Some(Self::gemma_300m_q4()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_is_deterministic() {
        let a = PipelineFingerprint::compute(2048, "tmpl_d", "tmpl_q", "model", "mean", true, 1);
        let b = PipelineFingerprint::compute(2048, "tmpl_d", "tmpl_q", "model", "mean", true, 1);
        assert_eq!(a.digest, b.digest);
    }

    #[test]
    fn fingerprint_changes_with_max_length() {
        let a = PipelineFingerprint::compute(512, "d", "q", "m", "mean", true, 1);
        let b = PipelineFingerprint::compute(2048, "d", "q", "m", "mean", true, 1);
        assert_ne!(a.digest, b.digest);
    }

    #[test]
    fn fingerprint_changes_with_model() {
        let a = PipelineFingerprint::compute(2048, "d", "q", "model-a", "mean", true, 1);
        let b = PipelineFingerprint::compute(2048, "d", "q", "model-b", "mean", true, 1);
        assert_ne!(a.digest, b.digest);
    }

    #[test]
    fn fingerprint_changes_with_template() {
        let a = PipelineFingerprint::compute(2048, "template-a", "q", "m", "mean", true, 1);
        let b = PipelineFingerprint::compute(2048, "template-b", "q", "m", "mean", true, 1);
        assert_ne!(a.digest, b.digest);
    }

    #[test]
    fn fingerprint_label_format() {
        let fp = PipelineFingerprint::hash_embedder();
        assert!(fp.label.starts_with("pf:v1:"));
        assert_eq!(fp.label.len(), "pf:v1:".len() + 64);
    }

    #[test]
    fn hash_embedder_fingerprint_differs_from_gemma() {
        let hash = PipelineFingerprint::hash_embedder();
        let gemma = PipelineFingerprint::gemma_300m_q4();
        assert_ne!(hash.digest, gemma.digest);
        assert_eq!(gemma.max_sequence_length, 2048);
    }

    #[test]
    fn supported_model_ids_resolve_deterministic_pipeline_fingerprints() {
        for model_id in [
            "hash-256",
            "bge-small-en-v1.5",
            "embeddinggemma-300m",
            "embeddinggemma-300m-q4",
        ] {
            let first = PipelineFingerprint::for_model_id(model_id).unwrap();
            let second = PipelineFingerprint::for_model_id(model_id).unwrap();

            assert_eq!(first, second, "fingerprint drift for {model_id}");
            assert_eq!(first.model_id, model_id);
            assert!(first.label.starts_with("pf:v1:"));
        }
    }

    #[test]
    fn unsupported_model_id_has_no_declared_pipeline_fingerprint() {
        assert!(PipelineFingerprint::for_model_id("unknown-model").is_none());
    }
}
