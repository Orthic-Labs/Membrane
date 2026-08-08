//! Session/source keyed exact-read deduplication with immutable hash expansion.
use membrane_protocol::{digest_str, CompressionReceiptV1, NonEmptyString};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadMeta {
    pub session_id: String,
    pub source_id: String,
    pub generation: String,
    pub resolver: String,
    pub receipt: CompressionReceiptV1,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReadEmission {
    Full {
        meta: ReadMeta,
        text: String,
    },
    Unchanged {
        meta: ReadMeta,
    },
    Diff {
        meta: ReadMeta,
        base_hash: String,
        prefix_len: usize,
        suffix_len: usize,
        replacement: String,
    },
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DedupError {
    MissingSource(String),
    TamperedSource(String),
    GenerationMismatch { expected: String, actual: String },
    Store(String),
    Resolver(String),
}
#[derive(Debug, Default)]
pub struct ReadDedupLedger {
    entries: HashMap<(String, String), (String, String)>,
    bindings: HashMap<String, (String, String, String, String)>,
    raw_root: PathBuf,
}
impl ReadDedupLedger {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            raw_root: root.into(),
            ..Self::default()
        }
    }
    pub fn reset_session(&mut self, session: &str) {
        self.entries.retain(|(s, _), _| s != session);
    }
    pub fn observe(
        &mut self,
        session: &str,
        source: &str,
        generation: impl Into<String>,
        text: &str,
    ) -> Result<ReadEmission, DedupError> {
        let generation = generation.into();
        let hash = digest_str(text);
        let key = (session.into(), source.into());
        if let Some((old_generation, old_text)) = self.entries.get(&key) {
            if old_generation == &generation && digest_str(old_text) == hash {
                let resolver = self.binding_resolver(session, source, &generation, &hash);
                let e = unchanged(
                    session,
                    source,
                    &hash,
                    &generation,
                    resolver.clone(),
                    estimate_tokens(&hash) as i64 - estimate_tokens(text) as i64,
                );
                self.store(&hash, text)?;
                if serde_json::to_vec(&e)
                    .map(|v| v.len())
                    .unwrap_or(usize::MAX)
                    >= serde_json::to_vec(&full(
                        session,
                        source,
                        &hash,
                        &generation,
                        resolver.clone(),
                        text,
                    ))
                    .map(|v| v.len())
                    .unwrap_or(usize::MAX)
                {
                    let full = full(session, source, &hash, &generation, resolver, text);
                    self.bind(&full);
                    return Ok(full);
                }
                self.bind(&e);
                return Ok(e);
            }
            if old_generation == &generation {
                if let Some(e) = make_diff(
                    session,
                    source,
                    old_text,
                    &hash,
                    &generation,
                    self.binding_resolver(session, source, &generation, &hash),
                    text,
                ) {
                    self.entries.insert(key, (generation, text.into()));
                    self.store(&hash, text)?;
                    self.bind(&e);
                    return Ok(e);
                }
            }
        }
        self.entries.insert(key, (generation.clone(), text.into()));
        self.store(&hash, text)?;
        let e = full(
            session,
            source,
            &hash,
            &generation,
            self.binding_resolver(session, source, &generation, &hash),
            text,
        );
        self.bind(&e);
        Ok(e)
    }
    pub fn expand(
        &self,
        session: &str,
        source: &str,
        generation: &str,
        emission: &ReadEmission,
    ) -> Result<String, DedupError> {
        match emission {
            ReadEmission::Full { meta, text } => {
                self.check(meta, session, source, generation)?;
                verify(&meta.receipt.original_hash, text)
            }
            ReadEmission::Unchanged { meta } => {
                self.check(meta, session, source, generation)?;
                self.base(&meta.receipt.original_hash)
            }
            ReadEmission::Diff {
                meta,
                base_hash,
                prefix_len,
                suffix_len,
                replacement,
            } => {
                self.check(meta, session, source, generation)?;
                let base = self.base(base_hash)?;
                let end = base
                    .len()
                    .checked_sub(*suffix_len)
                    .ok_or_else(|| DedupError::TamperedSource(base_hash.clone()))?;
                if *prefix_len > end
                    || !base.is_char_boundary(*prefix_len)
                    || !base.is_char_boundary(end)
                {
                    return Err(DedupError::TamperedSource(base_hash.clone()));
                }
                verify(
                    &meta.receipt.original_hash,
                    &format!("{}{replacement}{}", &base[..*prefix_len], &base[end..]),
                )
            }
        }
    }
    fn check(
        &self,
        meta: &ReadMeta,
        session: &str,
        source: &str,
        generation: &str,
    ) -> Result<(), DedupError> {
        if meta.session_id != session || meta.source_id != source {
            return Err(DedupError::MissingSource(source.into()));
        }
        if meta.generation != generation {
            return Err(DedupError::GenerationMismatch {
                expected: generation.into(),
                actual: meta.generation.clone(),
            });
        }
        if meta.resolver != meta.receipt.resolver.as_str() {
            return Err(DedupError::Resolver(meta.resolver.clone()));
        }
        match self.bindings.get(&meta.resolver) {
            Some((s, d, g, h))
                if s == session
                    && d == source
                    && g == generation
                    && h == &meta.receipt.original_hash =>
            {
                Ok(())
            }
            _ => Err(DedupError::Resolver(meta.resolver.clone())),
        }
    }
    pub fn resolve(&self, handle: &str) -> Result<String, DedupError> {
        let value = handle
            .strip_prefix("membrane-read-dedup:get:")
            .ok_or_else(|| DedupError::Resolver(handle.into()))?;
        let (binding, hash) = value
            .rsplit_once(":sha256:")
            .ok_or_else(|| DedupError::Resolver(handle.into()))?;
        let hash = format!("sha256:{hash}");
        let canonical = format!("membrane-read-dedup:get:{binding}:{hash}");
        match self.bindings.get(&canonical) {
            Some((_, _, _, h)) if h == &hash && canonical == handle => self.base(&hash),
            _ => Err(DedupError::Resolver(handle.into())),
        }
    }
    fn base(&self, hash: &str) -> Result<String, DedupError> {
        let text =
            super::compression::retrieve_raw(&self.raw_root, hash).map_err(DedupError::Resolver)?;
        verify(hash, &text)
    }
    fn binding_resolver(
        &self,
        session: &str,
        source: &str,
        generation: &str,
        hash: &str,
    ) -> String {
        let binding = digest_str(&format!("{session}\0{source}\0{generation}\0{hash}"));
        format!("membrane-read-dedup:get:{binding}:{hash}")
    }
    fn bind(&mut self, emission: &ReadEmission) {
        let meta = match emission {
            ReadEmission::Full { meta, .. }
            | ReadEmission::Unchanged { meta }
            | ReadEmission::Diff { meta, .. } => meta,
        };
        self.bindings.insert(
            meta.resolver.clone(),
            (
                meta.session_id.clone(),
                meta.source_id.clone(),
                meta.generation.clone(),
                meta.receipt.original_hash.clone(),
            ),
        );
    }
    fn store(&self, hash: &str, text: &str) -> Result<(), DedupError> {
        super::compression::store_raw(&self.raw_root, hash, text)
            .map(|_| ())
            .map_err(DedupError::Store)
    }
}
fn receipt(hash: &str, resolver: &str, delta: i64) -> CompressionReceiptV1 {
    CompressionReceiptV1 {
        schema_version: CompressionReceiptV1::SCHEMA_VERSION,
        original_hash: hash.into(),
        transform: NonEmptyString::new("read-dedup").unwrap(),
        transform_version: NonEmptyString::new("1").unwrap(),
        kept_spans: vec![],
        protected_spans: vec![],
        dropped: vec![],
        resolver: NonEmptyString::new(resolver).unwrap(),
        risk: NonEmptyString::new("low").unwrap(),
        token_delta: delta,
    }
}
fn meta(
    session: &str,
    source: &str,
    hash: &str,
    generation: &str,
    resolver: String,
    delta: i64,
) -> ReadMeta {
    ReadMeta {
        session_id: session.into(),
        source_id: source.into(),
        generation: generation.into(),
        receipt: receipt(hash, &resolver, delta),
        resolver,
    }
}
fn full(
    session: &str,
    source: &str,
    hash: &str,
    generation: &str,
    resolver: String,
    text: &str,
) -> ReadEmission {
    ReadEmission::Full {
        meta: meta(session, source, hash, generation, resolver, 0),
        text: text.into(),
    }
}
fn unchanged(
    session: &str,
    source: &str,
    hash: &str,
    generation: &str,
    resolver: String,
    delta: i64,
) -> ReadEmission {
    ReadEmission::Unchanged {
        meta: meta(session, source, hash, generation, resolver, delta.min(0)),
    }
}
fn verify(hash: &str, text: &str) -> Result<String, DedupError> {
    super::compression::verify_source_hash(hash, text)
        .map(|_| text.into())
        .map_err(|_| DedupError::TamperedSource(hash.into()))
}
fn make_diff(
    session: &str,
    source: &str,
    old: &str,
    hash: &str,
    generation: &str,
    resolver: String,
    new: &str,
) -> Option<ReadEmission> {
    let mut prefix = old
        .bytes()
        .zip(new.bytes())
        .take_while(|(a, b)| a == b)
        .count();
    while prefix > 0 && (!old.is_char_boundary(prefix) || !new.is_char_boundary(prefix)) {
        prefix -= 1;
    }
    let mut suffix = old
        .bytes()
        .rev()
        .zip(new.bytes().rev())
        .take_while(|(a, b)| a == b)
        .count()
        .min(old.len() - prefix)
        .min(new.len() - prefix);
    while suffix > 0
        && (!old.is_char_boundary(old.len() - suffix) || !new.is_char_boundary(new.len() - suffix))
    {
        suffix -= 1;
    }
    let replacement: String = new[prefix..new.len() - suffix].into();
    let full_resolver = resolver.clone();
    let candidate = ReadEmission::Diff {
        meta: meta(
            session,
            source,
            hash,
            generation,
            resolver,
            estimate_tokens(&replacement) as i64 - estimate_tokens(new) as i64,
        ),
        base_hash: digest_str(old),
        prefix_len: prefix,
        suffix_len: suffix,
        replacement,
    };
    let full_size =
        serde_json::to_vec(&full(session, source, hash, generation, full_resolver, new))
            .map(|v| v.len())
            .unwrap_or(usize::MAX);
    (serde_json::to_vec(&candidate)
        .map(|v| v.len())
        .unwrap_or(usize::MAX)
        < full_size)
        .then_some(candidate)
}
fn estimate_tokens(text: &str) -> usize {
    text.split_whitespace().count().max((text.len() + 3) / 4)
}
#[cfg(test)]
#[rustfmt::skip]
mod tests {
    use super::*;
    #[test]
    fn sessions_refs_diffs_unicode_and_reset_are_isolated() {
        let root = tempfile::tempdir().unwrap(); let mut l = ReadDedupLedger::new(root.path());
        let base = format!("α\n{}\n", "keep ".repeat(1000));
        let next = format!("α\n{}X\n", "keep ".repeat(1000));
        let same = { l.observe("s1", "doc", "g1", &base).unwrap(); l.observe("s1", "doc", "g1", &base).unwrap() };
        if let ReadEmission::Unchanged { meta } = &same { assert!(meta.receipt.token_delta < 0); assert_eq!(l.resolve(&meta.resolver).unwrap(), base); }
        let changed = l.observe("s1", "doc", "g1", &next).unwrap();
        assert_eq!(l.expand("s1", "doc", "g1", &changed).unwrap(), next);
        l.reset_session("s1");
        assert_eq!(l.expand("s1", "doc", "g1", &same).unwrap(), base);
    }
    #[test] fn prior_generation_ref_survives_later_generation_and_tamper_is_typed() {
        let root = tempfile::tempdir().unwrap(); let mut l = ReadDedupLedger::new(root.path());
        let old = l.observe("s", "d", "g1", "old").unwrap(); l.observe("s", "d", "g2", "new").unwrap();
        assert!(matches!(&old, ReadEmission::Full { .. }));
        assert_eq!(l.expand("s", "d", "g1", &old).unwrap(), "old");
        assert!(matches!(l.expand("s", "d", "g2", &old), Err(DedupError::GenerationMismatch { .. })));
        if let ReadEmission::Full { mut meta, text } = old {
            assert_eq!(serde_json::from_slice::<CompressionReceiptV1>(&serde_json::to_vec(&meta.receipt).unwrap()).unwrap().schema_version, CompressionReceiptV1::SCHEMA_VERSION);
            meta.receipt.original_hash = "sha256:bad".into();
            let bad = ReadEmission::Full { meta, text };
            assert!(matches!(
                l.expand("s", "d", "g1", &bad),
                Err(DedupError::TamperedSource(_)) | Err(DedupError::Resolver(_))
            ));
        }
    }
}
