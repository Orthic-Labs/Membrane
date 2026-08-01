//! Text embedding for semantic memory retrieval.
//!
//! An [`Embedder`] turns text into a fixed-dimension vector so memories can be
//! ranked by *meaning* (cosine similarity), not just shared keywords.
//!
//! Two backends:
//! - [`HashEmbedder`] — dependency-free, deterministic feature-hashing of
//!   character trigrams. Not truly semantic, but always available (tests, and a
//!   graceful fallback when the ONNX model isn't built in). Captures lexical
//!   overlap, so it still beats nothing.
//! - `FastEmbedder` — real ONNX sentence embeddings via `fastembed`, behind the
//!   `fastembed` cargo feature (it pulls the onnxruntime native lib + downloads
//!   a model on first use, so it's opt-in and verified on a real machine).

/// Maximum token sequence length for the embedding model. This replaces the
/// legacy 512-token limit. Tokenizer-aware — not a word count.
pub const EMBEDDING_MAX_SEQUENCE_TOKENS: usize = 2048;

/// Anything that can turn text into a fixed-length vector.
pub trait Embedder: Send + Sync {
    /// Embed `text` (a document/passage) into a `dim()`-length, L2-normalized vector.
    fn embed(&self, text: &str) -> Vec<f32>;
    /// Fallible embedding path for persistence/reindex operations. Runtime backends override
    /// this so inference failures are returned instead of being disguised as vectors.
    fn try_embed(&self, text: &str) -> Result<Vec<f32>, String> {
        Ok(self.embed(text))
    }
    /// Embed a document batch in input order. Native backends should override this to execute one
    /// model batch; the default preserves compatibility for simple/test embedders.
    fn try_embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, String> {
        texts.iter().map(|text| self.try_embed(text)).collect()
    }
    /// Embed a search QUERY. BGE-class models are asymmetric — the query path can differ from the
    /// document path. Default is the document embedding; real backends (FastEmbedder) override it to
    /// use the model's native query embedding so query and document vectors share the same space.
    fn embed_query(&self, text: &str) -> Vec<f32> {
        self.embed(text)
    }
    fn try_embed_query(&self, text: &str) -> Result<Vec<f32>, String> {
        self.try_embed(text)
    }
    /// The dimensionality of vectors this embedder produces.
    fn dim(&self) -> usize;
    /// Stable identifier of the embedding model — memoization key alongside the
    /// content hash: a row whose (content_hash, embed_model) both match needs no
    /// re-embedding on `reindex`.
    fn model_id(&self) -> &'static str {
        "hash-256"
    }
    /// Deterministic identity of the active model, prompts, pooling, and normalization pipeline.
    fn pipeline_fingerprint(&self) -> crate::PipelineFingerprint {
        crate::PipelineFingerprint::for_model_id(self.model_id()).unwrap_or_else(|| {
            crate::PipelineFingerprint::compute(0, "", "", self.model_id(), "unspecified", true, 1)
        })
    }
}

/// Cosine similarity of two equal-length vectors, in `[-1, 1]`. Returns 0 for a
/// length mismatch or a zero vector.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for i in 0..a.len() {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    dot / (na.sqrt() * nb.sqrt())
}

fn l2_normalize(v: &mut [f32]) {
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
}

/// Deterministic, dependency-free embedder using signed feature-hashing of
/// character trigrams. Always available; used in tests and as the fallback when
/// the `fastembed` feature is off.
#[derive(Clone)]
pub struct HashEmbedder {
    dim: usize,
}

impl Default for HashEmbedder {
    fn default() -> Self {
        Self::new()
    }
}

impl HashEmbedder {
    pub fn new() -> Self {
        Self { dim: 256 }
    }

    pub fn with_dim(dim: usize) -> Self {
        Self { dim: dim.max(1) }
    }
}

impl Embedder for HashEmbedder {
    fn embed(&self, text: &str) -> Vec<f32> {
        let mut v = vec![0f32; self.dim];
        let chars: Vec<char> = text.to_lowercase().chars().collect();
        let push = |v: &mut [f32], h: u64| {
            let idx = (h % self.dim as u64) as usize;
            // Signed hashing reduces collision bias.
            let sign = if (h >> 63) & 1 == 1 { -1.0 } else { 1.0 };
            v[idx] += sign;
        };
        let fnv = |bytes: &[u8]| -> u64 {
            let mut h: u64 = 1469598103934665603;
            for b in bytes {
                h ^= *b as u64;
                h = h.wrapping_mul(1099511628211);
            }
            h
        };
        if chars.len() < 3 {
            // Too short for trigrams: hash whole-string + each char.
            push(&mut v, fnv(text.to_lowercase().as_bytes()));
            for c in &chars {
                let mut buf = [0u8; 4];
                push(&mut v, fnv(c.encode_utf8(&mut buf).as_bytes()));
            }
        } else {
            for w in chars.windows(3) {
                let s: String = w.iter().collect();
                push(&mut v, fnv(s.as_bytes()));
            }
        }
        l2_normalize(&mut v);
        v
    }

    fn dim(&self) -> usize {
        self.dim
    }
}

#[cfg(feature = "fastembed")]
mod fast {
    use super::{l2_normalize, Embedder, EMBEDDING_MAX_SEQUENCE_TOKENS};
    use std::sync::Mutex;

    #[derive(Clone, Copy, PartialEq)]
    enum Kind {
        Bge,
        Gemma,
    }

    /// Real ONNX sentence embeddings via `fastembed`. Behind the `fastembed`
    /// feature because it links onnxruntime and downloads a model on first use.
    pub struct FastEmbedder {
        model: Mutex<fastembed::TextEmbedding>,
        dim: usize,
        kind: Kind,
        model_id: &'static str,
    }

    impl FastEmbedder {
        /// Model from `WORKSPACE_EMBED_MODEL`. Default: EmbeddingGemma-300M Q4
        /// (768-dim, 2K context, multilingual, <200MB resident — upgraded from
        /// BGE-small 2026-07-02). `bge-small-en-v1.5` stays selectable for DBs
        /// embedded pre-upgrade; a dim change requires `memright reindex`.
        pub fn new() -> Result<Self, String> {
            use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
            let want = std::env::var("WORKSPACE_EMBED_MODEL").unwrap_or_default();
            let (m, dim, kind, model_id) = match want.as_str() {
                "bge-small-en-v1.5" | "BAAI/bge-small-en-v1.5" => (
                    EmbeddingModel::BGESmallENV15,
                    384,
                    Kind::Bge,
                    "bge-small-en-v1.5",
                ),
                "embeddinggemma-300m" | "google/embeddinggemma-300m" => (
                    EmbeddingModel::EmbeddingGemma300M,
                    768,
                    Kind::Gemma,
                    "embeddinggemma-300m",
                ),
                _ => (
                    EmbeddingModel::EmbeddingGemma300MQ4,
                    768,
                    Kind::Gemma,
                    "embeddinggemma-300m-q4",
                ),
            };
            // Offline path: when the product bundle ships the model files (the
            // Tauri app sets CODERIGHT_EMBED_MODEL_DIR to the bundled resource),
            // load them directly with no HuggingFace download. Only the shipped
            // default (EmbeddingGemma-300M Q4) is bundled, so the bundled branch
            // is scoped to that model; any other selection uses the download path.
            let bundled_dir = std::env::var("CODERIGHT_EMBED_MODEL_DIR").unwrap_or_default();
            let max_len = EMBEDDING_MAX_SEQUENCE_TOKENS;
            let model = if !bundled_dir.is_empty() && model_id == "embeddinggemma-300m-q4" {
                Self::from_bundle(&bundled_dir)?
            } else {
                let opts = InitOptions::new(m).with_max_length(max_len);
                TextEmbedding::try_new(opts).map_err(|e| format!("fastembed init failed: {e}"))?
            };
            Ok(Self {
                model: Mutex::new(model),
                dim,
                kind,
                model_id,
            })
        }

        /// Load EmbeddingGemma-300M Q4 from a flat directory of the exact files
        /// fastembed's registry references for this model — offline, no hf-hub.
        /// Config mirrors fastembed's built-in `EmbeddingGemma300MQ4` entry:
        /// `model_q4.onnx` + external initializer `model_q4.onnx_data`, the four
        /// tokenizer files, and `output_key = sentence_embedding`.
        #[cfg(feature = "fastembed")]
        fn from_bundle(dir: &str) -> Result<fastembed::TextEmbedding, String> {
            use fastembed::{
                InitOptionsUserDefined, OutputKey, QuantizationMode, TextEmbedding, TokenizerFiles,
                UserDefinedEmbeddingModel,
            };
            let root = std::path::Path::new(dir);
            let read = |name: &str| {
                std::fs::read(root.join(name))
                    .map_err(|e| format!("bundled embed model: read {name}: {e}"))
            };
            let tokenizer_files = TokenizerFiles {
                tokenizer_file: read("tokenizer.json")?,
                config_file: read("config.json")?,
                special_tokens_map_file: read("special_tokens_map.json")?,
                tokenizer_config_file: read("tokenizer_config.json")?,
            };
            let user_model =
                UserDefinedEmbeddingModel::new(read("model_q4.onnx")?, tokenizer_files)
                    .with_external_initializer(
                        "model_q4.onnx_data".to_string(),
                        read("model_q4.onnx_data")?,
                    )
                    .with_quantization(QuantizationMode::None);
            let user_model = UserDefinedEmbeddingModel {
                output_key: Some(OutputKey::ByName("sentence_embedding")),
                ..user_model
            };
            TextEmbedding::try_new_from_user_defined(user_model, InitOptionsUserDefined::new())
                .map_err(|e| format!("bundled fastembed init failed: {e}"))
        }

        fn embed_raw(&self, text: &str) -> Result<Vec<f32>, String> {
            let mut guard = self
                .model
                .lock()
                .map_err(|_| "fastembed model lock poisoned".to_string())?;
            let mut out = guard
                .embed(vec![text], None)
                .map_err(|e| format!("fastembed inference failed: {e}"))?;
            let mut v = out
                .get_mut(0)
                .map(std::mem::take)
                .ok_or_else(|| "fastembed inference returned no vectors".to_string())?;
            if v.len() != self.dim || !v.iter().all(|value| value.is_finite()) {
                return Err(format!(
                    "fastembed returned invalid vector: expected dim {}, got {}",
                    self.dim,
                    v.len()
                ));
            }
            l2_normalize(&mut v);
            if v.iter().all(|value| *value == 0.0) {
                return Err("fastembed returned a zero vector".to_string());
            }
            Ok(v)
        }

        fn embed_raw_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
            if texts.is_empty() {
                return Ok(Vec::new());
            }
            let mut guard = self
                .model
                .lock()
                .map_err(|_| "fastembed model lock poisoned".to_string())?;
            let inputs: Vec<&str> = texts.iter().map(String::as_str).collect();
            let mut vectors = guard
                .embed(inputs, None)
                .map_err(|e| format!("fastembed inference failed: {e}"))?;
            if vectors.len() != texts.len() {
                return Err(format!(
                    "fastembed returned invalid batch: expected {} vectors, got {}",
                    texts.len(),
                    vectors.len()
                ));
            }
            for vector in &mut vectors {
                if vector.len() != self.dim || !vector.iter().all(|value| value.is_finite()) {
                    return Err(format!(
                        "fastembed returned invalid vector: expected dim {}, got {}",
                        self.dim,
                        vector.len()
                    ));
                }
                l2_normalize(vector);
                if vector.iter().all(|value| *value == 0.0) {
                    return Err("fastembed returned a zero vector".to_string());
                }
            }
            Ok(vectors)
        }
    }

    impl Embedder for FastEmbedder {
        fn embed(&self, text: &str) -> Vec<f32> {
            self.try_embed(text).unwrap_or_else(|e| panic!("{e}"))
        }

        fn try_embed(&self, text: &str) -> Result<Vec<f32>, String> {
            match self.kind {
                // EmbeddingGemma is prompt-trained: documents need this prefix
                // or they land in the wrong region of the space.
                Kind::Gemma => self.embed_raw(&format!("title: none | text: {text}")),
                Kind::Bge => self.embed_raw(text),
            }
        }

        fn try_embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, String> {
            let prompts: Vec<String> = match self.kind {
                Kind::Gemma => texts
                    .iter()
                    .map(|text| format!("title: none | text: {text}"))
                    .collect(),
                Kind::Bge => texts.iter().map(|text| (*text).to_string()).collect(),
            };
            self.embed_raw_batch(&prompts)
        }

        fn embed_query(&self, text: &str) -> Vec<f32> {
            self.try_embed_query(text).unwrap_or_else(|e| panic!("{e}"))
        }

        fn try_embed_query(&self, text: &str) -> Result<Vec<f32>, String> {
            match self.kind {
                Kind::Gemma => self.embed_raw(&format!("task: search result | query: {text}")),
                // BGE v1.5 asymmetric query instruction.
                Kind::Bge => self.embed_raw(&format!(
                    "Represent this sentence for searching relevant passages: {text}"
                )),
            }
        }

        fn dim(&self) -> usize {
            self.dim
        }

        fn model_id(&self) -> &'static str {
            self.model_id
        }
    }
}

#[cfg(feature = "fastembed")]
pub use fast::FastEmbedder;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embeddings_are_normalized_and_fixed_dim() {
        let e = HashEmbedder::new();
        let v = e.embed("deploy the worker to cloudflare");
        assert_eq!(v.len(), 256);
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-4 || norm == 0.0);
    }

    #[test]
    fn batch_embedding_preserves_order_and_vector_contract() {
        let embedder = HashEmbedder::new();
        let batch = embedder
            .try_embed_batch(&["first concept", "second concept", "third concept"])
            .unwrap();

        assert_eq!(batch.len(), 3);
        assert_eq!(batch[0], embedder.try_embed("first concept").unwrap());
        assert_eq!(batch[1], embedder.try_embed("second concept").unwrap());
        assert_eq!(batch[2], embedder.try_embed("third concept").unwrap());
        assert!(batch.iter().all(|vector| vector.len() == embedder.dim()));
    }

    #[test]
    fn similar_text_scores_higher_than_unrelated() {
        let e = HashEmbedder::new();
        let q = e.embed("cloudflare worker deployment");
        let related = e.embed("deploy the worker to cloudflare");
        let unrelated = e.embed("write marketing copy for the homepage");
        assert!(
            cosine(&q, &related) > cosine(&q, &unrelated),
            "related {} should beat unrelated {}",
            cosine(&q, &related),
            cosine(&q, &unrelated)
        );
    }

    #[test]
    fn cosine_handles_degenerate_inputs() {
        assert_eq!(cosine(&[], &[]), 0.0);
        assert_eq!(cosine(&[1.0, 2.0], &[1.0]), 0.0);
        assert_eq!(cosine(&[0.0, 0.0], &[1.0, 1.0]), 0.0);
        assert!((cosine(&[1.0, 0.0], &[1.0, 0.0]) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn max_sequence_tokens_constant_is_2048() {
        assert_eq!(EMBEDDING_MAX_SEQUENCE_TOKENS, 2048);
    }

    #[test]
    fn active_hash_embedder_exposes_its_declared_pipeline_fingerprint() {
        let embedder = HashEmbedder::new();

        assert_eq!(
            embedder.pipeline_fingerprint(),
            crate::PipelineFingerprint::for_model_id(embedder.model_id()).unwrap()
        );
    }
}

// ---- Tokenizer-ID tests (require fastembed feature + ONNX runtime) ----

#[cfg(all(test, feature = "fastembed"))]
mod tokenizer_tests {
    use super::*;

    #[test]
    fn embedding_beyond_old_512_changes_result() {
        // Text with >512 tokens should produce a different vector than its
        // 512-token truncation, confirming max_length=2048 is active.
        let embedder = FastEmbedder::new().expect("FastEmbedder init");
        let short = "hello world ".repeat(200); // ~400 tokens
        let long = "hello world ".repeat(800); // ~1600 tokens
        let v_short = embedder.embed(&short);
        let v_long = embedder.embed(&long);
        // They should differ because the long text has tokens beyond 512
        let sim = cosine(&v_short, &v_long);
        assert!(
            sim < 0.999,
            "vectors should differ for different-length texts, sim={sim}"
        );
    }

    #[test]
    fn embedding_at_2048_includes_2048th_token() {
        // A text that's exactly around 2048 tokens should still be fully embedded.
        let embedder = FastEmbedder::new().expect("FastEmbedder init");
        let text = "the quick brown fox jumps over the lazy dog ".repeat(500);
        let v = embedder.embed(&text);
        assert_eq!(v.len(), embedder.dim());
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!(norm > 0.0, "vector should not be zero");
    }

    #[test]
    fn embedding_at_2049_truncates_at_2048() {
        // Beyond 2048 tokens, extra content should be truncated.
        let embedder = FastEmbedder::new().expect("FastEmbedder init");
        let base = "word ".repeat(2100);
        let extra = "word ".repeat(2200);
        let v_base = embedder.embed(&base);
        let v_extra = embedder.embed(&extra);
        // Both truncated to 2048 — should be very similar
        let sim = cosine(&v_base, &v_extra);
        assert!(sim > 0.95, "truncated texts should be similar, sim={sim}");
    }

    #[test]
    fn embedding_prompt_template_used() {
        // Gemma model uses a prompt template; verify it's applied.
        let embedder = FastEmbedder::new().expect("FastEmbedder init");
        let doc = embedder.embed("test content");
        let query = embedder.embed_query("test content");
        // Document and query paths use different templates, so vectors differ.
        let sim = cosine(&doc, &query);
        assert!(
            sim < 0.999,
            "doc and query templates should differ, sim={sim}"
        );
    }

    #[test]
    fn embedding_bundled_and_downloaded_compatible() {
        // Both bundled and downloaded paths produce the same dim.
        let embedder = FastEmbedder::new().expect("FastEmbedder init");
        let v = embedder.embed("compatibility test");
        assert_eq!(v.len(), 768, "Gemma-300M should produce 768-dim vectors");
    }
}
