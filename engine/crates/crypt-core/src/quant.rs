//! Quantized vector storage and scoring for memory embeddings.
//!
//! This is the small, dependency-free core behind the TurboQuant/turbovec work:
//! normalized `f32` embeddings can be compressed to signed 8-bit vectors, then
//! scored without expanding them back into full `Vec<f32>` allocations.

/// Signed 8-bit quantized embedding with a per-vector scale.
#[derive(Debug, Clone, PartialEq)]
pub struct QuantizedVector {
    scale: f32,
    values: Vec<i8>,
}

impl QuantizedVector {
    /// Quantize a vector into i8 values using symmetric per-vector scaling.
    pub fn quantize(vector: &[f32]) -> Self {
        let max_abs = vector
            .iter()
            .fold(0.0_f32, |acc, value| acc.max(value.abs()));
        if vector.is_empty() || max_abs == 0.0 {
            return Self {
                scale: 1.0,
                values: vec![0; vector.len()],
            };
        }
        let scale = max_abs / 127.0;
        let values = vector
            .iter()
            .map(|value| (value / scale).round().clamp(-127.0, 127.0) as i8)
            .collect();
        Self { scale, values }
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub fn scale(&self) -> f32 {
        self.scale
    }

    pub fn values(&self) -> &[i8] {
        &self.values
    }

    /// Approximate cosine similarity against a full-precision query vector.
    pub fn cosine_with(&self, query: &[f32]) -> f32 {
        if self.values.len() != query.len() || self.values.is_empty() {
            return 0.0;
        }
        let mut dot = 0.0_f32;
        let mut quant_norm = 0.0_f32;
        let mut query_norm = 0.0_f32;
        for (quantized, query_value) in self.values.iter().zip(query.iter()) {
            let value = *quantized as f32 * self.scale;
            dot += value * query_value;
            quant_norm += value * value;
            query_norm += query_value * query_value;
        }
        if quant_norm == 0.0 || query_norm == 0.0 {
            return 0.0;
        }
        dot / (quant_norm.sqrt() * query_norm.sqrt())
    }

    /// Lossy decompression, useful for diagnostics and migration tooling.
    pub fn dequantize(&self) -> Vec<f32> {
        self.values
            .iter()
            .map(|value| *value as f32 * self.scale)
            .collect()
    }

    /// Compact binary form: 4-byte little-endian scale followed by i8 values.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(4 + self.values.len());
        bytes.extend_from_slice(&self.scale.to_le_bytes());
        bytes.extend(self.values.iter().map(|value| *value as u8));
        bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 4 {
            return None;
        }
        let scale = f32::from_le_bytes(bytes[0..4].try_into().ok()?);
        if !scale.is_finite() || scale <= 0.0 {
            return None;
        }
        let values = bytes[4..].iter().map(|byte| *byte as i8).collect();
        Some(Self { scale, values })
    }
}

pub fn quantized_cosine(embedding: &[f32], query: &[f32]) -> f32 {
    QuantizedVector::quantize(embedding).cosine_with(query)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{cosine, Embedder, HashEmbedder};

    #[test]
    fn quantized_vector_roundtrips_compact_binary() {
        let original = QuantizedVector::quantize(&[0.0, 0.25, -0.5, 1.0]);
        let bytes = original.to_bytes();
        assert_eq!(bytes.len(), 8);

        let decoded = QuantizedVector::from_bytes(&bytes).expect("decode quantized vector");
        assert_eq!(decoded.len(), 4);
        assert_eq!(decoded.values(), original.values());
        assert_eq!(decoded.scale(), original.scale());
    }

    #[test]
    fn quantized_cosine_tracks_full_precision_similarity() {
        let embedder = HashEmbedder::new();
        let query = embedder.embed("cloudflare worker deployment");
        let related = embedder.embed("deploy the worker to cloudflare");
        let unrelated = embedder.embed("write marketing copy for the homepage");

        let full_related = cosine(&related, &query);
        let quant_related = quantized_cosine(&related, &query);
        assert!(
            (full_related - quant_related).abs() < 0.02,
            "full={full_related} quantized={quant_related}"
        );
        assert!(
            quantized_cosine(&related, &query) > quantized_cosine(&unrelated, &query),
            "quantized scoring should preserve the related > unrelated ordering"
        );
    }

    #[test]
    fn invalid_quantized_binary_is_rejected() {
        assert!(QuantizedVector::from_bytes(&[]).is_none());
        assert!(QuantizedVector::from_bytes(&0.0_f32.to_le_bytes()).is_none());
        assert!(QuantizedVector::from_bytes(&f32::NAN.to_le_bytes()).is_none());
    }

    #[test]
    fn zero_vector_uses_unit_scale_and_zero_values() {
        let vector = QuantizedVector::quantize(&[0.0, -0.0, 0.0]);

        assert_eq!(vector.scale(), 1.0);
        assert_eq!(vector.values(), &[0, 0, 0]);
        assert_eq!(vector.dequantize(), vec![0.0, 0.0, 0.0]);
        assert_eq!(vector.cosine_with(&[1.0, 0.0, 0.0]), 0.0);
    }

    #[test]
    fn cosine_returns_zero_for_dimension_or_query_norm_mismatch() {
        let vector = QuantizedVector::quantize(&[1.0, -1.0]);

        assert_eq!(vector.cosine_with(&[1.0]), 0.0);
        assert_eq!(vector.cosine_with(&[0.0, 0.0]), 0.0);
        assert_eq!(QuantizedVector::quantize(&[]).cosine_with(&[]), 0.0);
    }
}
