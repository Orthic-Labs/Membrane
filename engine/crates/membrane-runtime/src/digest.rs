//! Small `sha256:<hex>` digest helpers shared across the runtime.

use sha2::{Digest, Sha256};

/// `sha256:<hex>` of an arbitrary UTF-8 string.
pub fn digest_str(text: &str) -> String {
    format!("sha256:{:x}", Sha256::digest(text.as_bytes()))
}

/// `sha256:<hex>` of an arbitrary byte slice.
pub fn digest_bytes(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digest_str_matches_independently_computed_sha256() {
        // sha256("abc")
        assert_eq!(
            digest_str("abc"),
            "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
