use regex::Regex;
use std::path::Path;
use std::sync::LazyLock;

static SECRET_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    vec![
        Regex::new(r"\bsk-[A-Za-z0-9_-]{16,}\b").expect("valid secret regex"),
        Regex::new(r"\bgh[pousr]_[A-Za-z0-9_]{20,}\b").expect("valid secret regex"),
        Regex::new(r"\bgithub_pat_[A-Za-z0-9_]{20,}\b").expect("valid secret regex"),
        Regex::new(r"\bAKIA[0-9A-Z]{16}\b").expect("valid secret regex"),
        Regex::new(r"\bxox[bap]-[A-Za-z0-9-]{10,}\b").expect("valid secret regex"),
        Regex::new(r"(?i)\bbearer\s+[A-Za-z0-9._~+/=-]{16,}").expect("valid secret regex"),
        Regex::new(r"\beyJ[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]{5,}\b")
            .expect("valid secret regex"),
        Regex::new(r"(?i)\b(password|passphrase|api[_-]?key|secret|token)\s*[:=]\s*\S{6,}")
            .expect("valid secret regex"),
        Regex::new(
            r"-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z ]*PRIVATE KEY-----",
        )
        .expect("valid secret regex"),
    ]
});

pub(crate) fn redact_for_egress(text: &str) -> String {
    SECRET_PATTERNS.iter().fold(text.to_owned(), |value, pattern| {
        pattern.replace_all(&value, "[REDACTED]").into_owned()
    })
}

pub(crate) fn is_sensitive_path(path: &str) -> bool {
    let normalized = path.replace('\\', "/").to_ascii_lowercase();
    let file_name = Path::new(&normalized)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    normalized == ".env"
        || normalized.starts_with(".env.")
        || normalized.ends_with("/.env")
        || normalized.contains("/.env.")
        || normalized == ".aws/credentials"
        || normalized.ends_with("/.aws/credentials")
        || matches!(file_name, "id_rsa" | "id_dsa" | "id_ecdsa" | "id_ed25519")
        || matches!(
            Path::new(file_name).extension().and_then(|value| value.to_str()),
            Some("pem" | "key" | "p12" | "pfx")
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_common_secret_shapes() {
        let input = "token=abcdef123456 ghp_abcdefghijklmnopqrstuvwx";
        let output = redact_for_egress(input);
        assert!(!output.contains("abcdef123456"));
        assert!(!output.contains("ghp_"));
        assert!(output.contains("[REDACTED]"));
    }

    #[test]
    fn rejects_sensitive_paths_without_rejecting_normal_sources() {
        assert!(is_sensitive_path(".env.local"));
        assert!(is_sensitive_path("home/.aws/credentials"));
        assert!(is_sensitive_path("keys/service.pem"));
        assert!(is_sensitive_path("keys/id_ed25519"));
        assert!(!is_sensitive_path("src/config.rs"));
        assert!(!is_sensitive_path("docs/credentials.md"));
    }
}
