//! cortex-core — pure in-memory logic for the Cortex memory tier system.
//!
//! Models memory tiers, retrieval ranking, a quality eval gate, and rough token
//! accounting. Persistence lives in a separate storage crate; this crate has no I/O.
//!
//! The re-exports below form the stable crate API. Compatibility is governed
//! by crate semver and each typed record's schema version; internal modules
//! remain private unless explicitly marked as part of this surface.

pub mod absorbed;
mod accounting;
mod calibration;
mod dream;
mod effectiveness;
pub mod embed;
mod eval_gate;
pub mod fingerprint;
mod graph;
mod markdown;
pub mod planner;
mod quant;
mod registry;
pub mod retriever;
mod routing;
pub mod transcript;
pub mod types;
mod vector_index;

pub use absorbed::{
    content_hash, event_range, validate_event, validate_event_import, AbsorbedValidationError,
    ArtifactRecord, EventCursor, ProvenanceRef, RecordGovernance, SessionEvent, SessionRecord,
    TaskRecord, ABSORBED_SCHEMA_VERSION,
};
pub use accounting::{estimate_tokens, ContextTokenAccounting};
pub use calibration::ConfidenceCalibrator;
pub use dream::{
    consolidate_dream_memories, DreamAgentPolicy, DreamConsolidatedMemory, DreamPlan, DreamStatus,
};
pub use effectiveness::{EffectivenessGate, MemoryUsageRecord, Outcome};
#[cfg(feature = "fastembed")]
pub use embed::FastEmbedder;
pub use embed::{cosine, Embedder, HashEmbedder, EMBEDDING_MAX_SEQUENCE_TOKENS};
pub use eval_gate::{EvalGateConfig, MemoryRetrievalEvalGate};
pub use fingerprint::PipelineFingerprint;
pub use graph::{MemoryEdge, MemoryGraph, MemoryNode};
pub use markdown::{parse_markdown, wikilinks, ParsedDoc};
pub use planner::{
    plan, BlockV1, BudgetV1, CandidateV1, ContextCandidateSetV1, ContextPacketV1, ContextReceiptV2,
    DegradationReason, FallbackMode, OmissionV1, PlannerError, PlannerInput, PlannerOutput,
    ProviderCeilingV1, ProviderStatus, StructuredFallbackEvent,
};
pub use quant::{quantized_cosine, QuantizedVector};
pub use registry::{MemoryRegistry, RegistryError};
pub use retriever::{LexicalHit, MemoryRetriever};
pub use routing::{QueryFeatures, RetrievalTier, RoutingPolicy};
pub use transcript::{
    build_transcript_chunks, retrieve_transcript_chunks, TranscriptChunk, TranscriptChunkBuilder,
    TranscriptChunkConfig, TranscriptError, TranscriptRetrievalHit,
    TRANSCRIPT_CHUNK_SCHEMA_VERSION,
};
pub use types::{default_scope, MemoryEntry, MemoryTier};
pub use vector_index::{
    HostVectorPolicy, VectorCandidate, VectorIndex, VectorKernel, VectorSearchError,
};

#[cfg(test)]
mod vector_index_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(
        id: &str,
        tier: MemoryTier,
        content: &str,
        keywords: &[&str],
        score: f64,
    ) -> MemoryEntry {
        MemoryEntry {
            id: id.to_string(),
            tier,
            content: content.to_string(),
            keywords: keywords.iter().map(|k| k.to_string()).collect(),
            score,
            created_at: "2026-06-02T00:00:00Z".to_string(),
            access_count: 0,
            embedding: Some(HashEmbedder::new().embed(content)),
            scope_id: crate::default_scope(),
        }
    }

    // ---- registry ----

    #[test]
    fn registry_insert_and_get() {
        let mut reg = MemoryRegistry::new();
        reg.insert(entry(
            "a",
            MemoryTier::Working,
            "hello world",
            &["hello"],
            1.0,
        ));
        assert_eq!(reg.len(), 1);
        assert!(!reg.is_empty());
        let got = reg.get("a").unwrap();
        assert_eq!(got.content, "hello world");
        assert!(reg.get("missing").is_none());
    }

    #[test]
    fn registry_remove() {
        let mut reg = MemoryRegistry::new();
        reg.insert(entry("a", MemoryTier::Working, "content here", &[], 1.0));
        let removed = reg.remove("a").unwrap();
        assert_eq!(removed.id, "a");
        assert!(reg.is_empty());
        assert!(reg.remove("a").is_none());
    }

    #[test]
    fn registry_entries_in_tier() {
        let mut reg = MemoryRegistry::new();
        reg.insert(entry("a", MemoryTier::Working, "one", &[], 1.0));
        reg.insert(entry("b", MemoryTier::Episodic, "two", &[], 1.0));
        reg.insert(entry("c", MemoryTier::Working, "three", &[], 1.0));
        let working = reg.entries_in_tier(MemoryTier::Working);
        assert_eq!(working.len(), 2);
        assert_eq!(reg.entries_in_tier(MemoryTier::Episodic).len(), 1);
        assert_eq!(reg.entries_in_tier(MemoryTier::Semantic).len(), 0);
        assert_eq!(reg.all().len(), 3);
    }

    #[test]
    fn registry_promote_working_to_episodic() {
        let mut reg = MemoryRegistry::new();
        reg.insert(entry("a", MemoryTier::Working, "content", &[], 1.0));
        reg.promote("a", MemoryTier::Episodic).unwrap();
        assert_eq!(reg.get("a").unwrap().tier, MemoryTier::Episodic);
        // Downgrade attempt does not change tier and does not error.
        reg.promote("a", MemoryTier::Working).unwrap();
        assert_eq!(reg.get("a").unwrap().tier, MemoryTier::Episodic);
    }

    #[test]
    fn registry_promote_unknown_errors() {
        let mut reg = MemoryRegistry::new();
        let err = reg.promote("nope", MemoryTier::Semantic).unwrap_err();
        assert_eq!(err, RegistryError::NotFound("nope".to_string()));
    }

    #[test]
    fn registry_record_access_increments() {
        let mut reg = MemoryRegistry::new();
        reg.insert(entry("a", MemoryTier::Working, "content", &[], 1.0));
        assert_eq!(reg.record_access("a").unwrap(), 1);
        assert_eq!(reg.record_access("a").unwrap(), 2);
        assert_eq!(reg.get("a").unwrap().access_count, 2);
        assert!(reg.record_access("missing").is_err());
    }

    // ---- retriever ----

    #[test]
    fn retriever_ranks_keyword_matches_higher() {
        let mut reg = MemoryRegistry::new();
        reg.insert(entry(
            "kw",
            MemoryTier::Working,
            "unrelated text",
            &["rust", "async"],
            0.0,
        ));
        reg.insert(entry(
            "low",
            MemoryTier::Working,
            "some other thing",
            &[],
            0.5,
        ));
        let results = MemoryRetriever::retrieve(&reg, "rust async", 5);
        assert_eq!(results[0].id, "kw");
    }

    #[test]
    fn hybrid_finds_semantic_match_without_shared_keywords() {
        let emb = HashEmbedder::new();
        let mut reg = MemoryRegistry::new();
        // Lexically there is no overlap with the query "deploy worker cloudflare"
        // for this entry's keywords, but the content is semantically close.
        reg.insert(entry(
            "sem",
            MemoryTier::Episodic,
            "deploy the worker to cloudflare",
            &["shipping"],
            1.0,
        ));
        reg.insert(entry(
            "noise",
            MemoryTier::Episodic,
            "write marketing copy for the homepage",
            &["copywriting"],
            1.0,
        ));
        let qvec = emb.embed("cloudflare worker deployment");
        let hits =
            MemoryRetriever::retrieve_hybrid(&reg, "cloudflare worker deployment", Some(&qvec), 2);
        assert_eq!(
            hits[0].id, "sem",
            "semantically closest memory should rank first"
        );
    }

    #[test]
    fn hybrid_quantized_preserves_semantic_top_hit() {
        let emb = HashEmbedder::new();
        let mut reg = MemoryRegistry::new();
        reg.insert(entry(
            "sem",
            MemoryTier::Episodic,
            "deploy the worker to cloudflare",
            &["shipping"],
            1.0,
        ));
        reg.insert(entry(
            "noise",
            MemoryTier::Episodic,
            "write marketing copy for the homepage",
            &["copywriting"],
            1.0,
        ));
        let qvec = emb.embed("cloudflare worker deployment");

        let full =
            MemoryRetriever::retrieve_hybrid(&reg, "cloudflare worker deployment", Some(&qvec), 2);
        let quantized = MemoryRetriever::retrieve_hybrid_quantized(
            &reg,
            "cloudflare worker deployment",
            Some(&qvec),
            2,
        );

        assert_eq!(full[0].id, "sem");
        assert_eq!(quantized[0].id, "sem");
    }

    #[test]
    fn hybrid_falls_back_to_lexical_without_embedding() {
        let mut reg = MemoryRegistry::new();
        reg.insert(entry(
            "kw",
            MemoryTier::Working,
            "unrelated",
            &["rust", "async"],
            0.0,
        ));
        let hits = MemoryRetriever::retrieve_hybrid(&reg, "rust async", None, 5);
        assert_eq!(hits[0].id, "kw");
    }

    #[test]
    fn retriever_respects_limit() {
        let mut reg = MemoryRegistry::new();
        for i in 0..5 {
            reg.insert(entry(
                &format!("e{i}"),
                MemoryTier::Working,
                "rust content",
                &["rust"],
                1.0,
            ));
        }
        let results = MemoryRetriever::retrieve(&reg, "rust", 2);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn retriever_excludes_zero_match_zero_score() {
        let mut reg = MemoryRegistry::new();
        reg.insert(entry(
            "nomatch",
            MemoryTier::Working,
            "apples bananas",
            &["fruit"],
            0.0,
        ));
        reg.insert(entry(
            "match",
            MemoryTier::Working,
            "rust code",
            &["rust"],
            0.0,
        ));
        let results = MemoryRetriever::retrieve(&reg, "rust", 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "match");
    }

    #[test]
    fn retriever_content_substring_match() {
        let mut reg = MemoryRegistry::new();
        reg.insert(entry(
            "c",
            MemoryTier::Working,
            "the tokio runtime is fast",
            &[],
            0.0,
        ));
        let results = MemoryRetriever::retrieve(&reg, "tokio", 5);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "c");
    }

    #[test]
    fn score_entry_keyword_overlap() {
        let e = entry(
            "a",
            MemoryTier::Working,
            "no overlap here",
            &["rust", "async"],
            0.0,
        );
        let terms: Vec<String> = vec!["rust".into(), "async".into()];
        // 2 keyword matches * 2.0 = 4.0, content has neither term => 0, score 0.
        assert_eq!(MemoryRetriever::score_entry(&e, &terms), 4.0);
    }

    #[test]
    fn score_entry_no_match_is_zeroish() {
        let e = entry(
            "a",
            MemoryTier::Working,
            "apples and bananas",
            &["fruit"],
            0.0,
        );
        let terms: Vec<String> = vec!["rust".into()];
        assert_eq!(MemoryRetriever::score_entry(&e, &terms), 0.0);
    }

    // ---- eval gate ----

    #[test]
    fn eval_gate_drops_low_score() {
        let gate = MemoryRetrievalEvalGate::default();
        let low = entry("a", MemoryTier::Working, "long enough content", &[], 0.1);
        assert!(!gate.passes(&low));
        let filtered = gate.filter(vec![&low]);
        assert!(filtered.is_empty());
    }

    #[test]
    fn eval_gate_drops_short_content() {
        let gate = MemoryRetrievalEvalGate::default();
        let short = entry("a", MemoryTier::Working, "short", &[], 0.9);
        assert!(!gate.passes(&short));
        let filtered = gate.filter(vec![&short]);
        assert!(filtered.is_empty());
    }

    #[test]
    fn eval_gate_caps_max_entries() {
        let gate = MemoryRetrievalEvalGate::new(EvalGateConfig {
            min_score: 0.0,
            min_content_len: 0,
            max_entries: 3,
        });
        let entries: Vec<MemoryEntry> = (0..6)
            .map(|i| {
                entry(
                    &format!("e{i}"),
                    MemoryTier::Working,
                    "good content",
                    &[],
                    1.0,
                )
            })
            .collect();
        let refs: Vec<&MemoryEntry> = entries.iter().collect();
        let filtered = gate.filter(refs);
        assert_eq!(filtered.len(), 3);
        // Order preserved.
        assert_eq!(filtered[0].id, "e0");
        assert_eq!(filtered[2].id, "e2");
    }

    #[test]
    fn eval_gate_passes_good_entry() {
        let gate = MemoryRetrievalEvalGate::default();
        let good = entry(
            "a",
            MemoryTier::Working,
            "this is plenty of content",
            &[],
            0.8,
        );
        assert!(gate.passes(&good));
        assert_eq!(gate.filter(vec![&good]).len(), 1);
    }

    #[test]
    fn eval_gate_default_config() {
        let cfg = EvalGateConfig::default();
        assert_eq!(cfg.min_score, 0.3);
        assert_eq!(cfg.min_content_len, 8);
        assert_eq!(cfg.max_entries, 10);
    }

    // ---- accounting ----

    #[test]
    fn accounting_add_and_used() {
        let mut acct = ContextTokenAccounting::new(100);
        let t = estimate_tokens("hello world");
        assert!(t > 0);
        let added = acct.add("hello world");
        assert_eq!(added, t);
        assert_eq!(acct.used(), t);
        acct.add("hello world");
        assert_eq!(acct.used(), t * 2);
    }

    #[test]
    fn accounting_remaining_saturates() {
        let mut acct = ContextTokenAccounting::new(1);
        // "hello world" is at least 2 tokens — exceeds budget of 1.
        acct.add("hello world");
        assert_eq!(acct.remaining(), 0);
    }

    #[test]
    fn accounting_would_exceed() {
        let mut acct = ContextTokenAccounting::new(estimate_tokens("hello world") + 1);
        // budget is exactly one more than the token count of "hello world",
        // so one instance fits, two would exceed.
        assert!(!acct.would_exceed("hello world"));
        acct.add("hello world");
        assert!(acct.would_exceed("hello world"));
    }

    #[test]
    fn accounting_reset() {
        let mut acct = ContextTokenAccounting::new(10);
        acct.add("hello world, this is a somewhat longer string for testing");
        assert!(acct.used() > 0);
        acct.reset();
        assert_eq!(acct.used(), 0);
        assert_eq!(acct.remaining(), 10);
    }

    #[test]
    fn estimate_tokens_nonempty_is_positive() {
        assert!(estimate_tokens("hello") > 0);
        assert!(estimate_tokens("a") > 0);
    }

    #[test]
    fn estimate_tokens_empty_is_zero() {
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn estimate_tokens_monotonic() {
        let short = estimate_tokens("hi");
        let long = estimate_tokens("hello world, this is a longer sentence");
        assert!(long >= short);
    }

    // ---- serde ----

    #[test]
    fn memory_tier_serde_roundtrip() {
        for tier in [
            MemoryTier::Working,
            MemoryTier::Episodic,
            MemoryTier::Semantic,
        ] {
            let json = serde_json::to_string(&tier).unwrap();
            let back: MemoryTier = serde_json::from_str(&json).unwrap();
            assert_eq!(tier, back);
        }
    }
}
