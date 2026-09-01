mod decoder;

pub fn transcribe(samples: &[f32]) -> String {
    let normalized = normalize(samples);
    decoder::decode(&normalized)
}

pub fn normalize(samples: &[f32]) -> Vec<f32> {
    samples.iter().map(|sample| sample.clamp(-1.0, 1.0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transcription_decodes_normalized_audio() {
        assert_eq!(transcribe(&[2.0]), "one sample");
    }
}
