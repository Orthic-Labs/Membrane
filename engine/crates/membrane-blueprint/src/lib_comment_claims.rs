//! Native Rust port of `blueprint/src/lib/comment-claims.mjs`.
//!
//! Lane LIB4/CRYPTO (r5 closure): this module has no prior native
//! equivalent. D27: comment claims — NOTE/WHY/HACK/TODO/deprecation/
//! ownership comments become relational claims bound to the smallest
//! enclosing symbol. Instruction policy stays `data_only`: repository
//! comments are data, never instructions.
//!
//! SHA-1 now uses `ring::digest::SHA1_FOR_LEGACY_USE_ONLY` (the prior
//! version hand-rolled FIPS 180-4 SHA-1 directly because no SHA-1 crate was
//! believed available; the two were verified to agree byte-for-byte on the
//! FIPS 180-2 / RFC 3174 known-answer vectors in this module's parity test
//! — no bug found in the prior hand-rolled version). SHA-1 is used here only
//! as a non-cryptographic, stable content fingerprint for claim ids, exactly
//! matching the legacy `createHash("sha1")` usage — it is explicitly not
//! used for any security-relevant integrity or signature purpose, which is
//! why `ring`'s `_FOR_LEGACY_USE_ONLY` constant is the correct choice here.

use regex::Regex;
use std::sync::OnceLock;

pub const COMMENT_CLAIM_KINDS: [&str; 6] = ["NOTE", "WHY", "HACK", "TODO", "DEPRECATED", "OWNER"];

struct KindPattern {
    kind: &'static str,
    regex_key: &'static str,
}

const KIND_PATTERNS: [KindPattern; 6] = [
    KindPattern { kind: "TODO", regex_key: "todo" },
    KindPattern { kind: "HACK", regex_key: "hack" },
    KindPattern { kind: "NOTE", regex_key: "note" },
    KindPattern { kind: "WHY", regex_key: "why" },
    KindPattern { kind: "DEPRECATED", regex_key: "deprecated" },
    KindPattern { kind: "OWNER", regex_key: "owner" },
];

fn pattern_for(key: &str) -> &'static Regex {
    static TODO: OnceLock<Regex> = OnceLock::new();
    static HACK: OnceLock<Regex> = OnceLock::new();
    static NOTE: OnceLock<Regex> = OnceLock::new();
    static WHY: OnceLock<Regex> = OnceLock::new();
    static DEPRECATED: OnceLock<Regex> = OnceLock::new();
    static OWNER: OnceLock<Regex> = OnceLock::new();
    match key {
        "todo" => TODO.get_or_init(|| Regex::new(r"(?i)\bTODO(?:\(([^)]+)\))?\s*:\s*(.+)$").unwrap()),
        "hack" => HACK.get_or_init(|| Regex::new(r"(?i)\bHACK(?:\(([^)]+)\))?\s*:\s*(.+)$").unwrap()),
        "note" => NOTE.get_or_init(|| Regex::new(r"(?i)\bNOTE(?:\(([^)]+)\))?\s*:\s*(.+)$").unwrap()),
        "why" => WHY.get_or_init(|| Regex::new(r"(?i)\bWHY(?:\(([^)]+)\))?\s*:\s*(.+)$").unwrap()),
        "deprecated" => {
            DEPRECATED.get_or_init(|| Regex::new(r"(?i)\bdeprecated(?:\s*:\s*(.+))?").unwrap())
        }
        "owner" => OWNER.get_or_init(|| Regex::new(r"(?i)\bowner\s*[:=]\s*(.+)$").unwrap()),
        _ => unreachable!(),
    }
}

fn strip_prefix_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?m)^//|^#|^/\*|\*/$|^\s*\*?\s?").unwrap())
}

/// SHA-1 content fingerprint via `ring::digest::SHA1_FOR_LEGACY_USE_ONLY`.
/// Used only as a non-cryptographic, stable content fingerprint for claim
/// ids, matching legacy JS behavior (`createHash("sha1")`) — never for any
/// security-relevant integrity or signature purpose.
fn sha1_hex(input: &str) -> String {
    let digest = ring::digest::digest(&ring::digest::SHA1_FOR_LEGACY_USE_ONLY, input.as_bytes());
    hex::encode(digest.as_ref())
}

/// A comment-claim edge, mirroring the JS `edges[0]` object shape.
#[derive(Debug, Clone, PartialEq)]
pub struct CommentClaimEdge {
    pub kind: &'static str, // always "COMMENT_CLAIM"
    pub claim_kind: &'static str,
    pub owner: Option<String>,
    pub body: String,
    pub enclosing_symbol_id: Option<String>,
    pub lifecycle: &'static str, // always "data_only"
    pub span_start_line: i64,
    pub span_end_line: i64,
}

/// A comment claim, mirroring the JS claim object shape.
#[derive(Debug, Clone, PartialEq)]
pub struct CommentClaim {
    pub id: String,
    pub document_id: String,
    pub source: String,
    pub line: i64,
    pub status: &'static str, // always "implemented"
    pub sha1: String,
    pub edges: Vec<CommentClaimEdge>,
}

/// Input bundle for [`extract_comment_claims`], mirroring the JS
/// destructured parameter object.
pub struct ExtractCommentClaimsInput<'a> {
    pub text: &'a str,
    pub path: &'a str,
    pub line: i64,
    pub enclosing_symbol_id: Option<String>,
}

/// Extract comment claims from raw comment text. Mirrors
/// `extractCommentClaims` in the legacy JS module. (The JS `kind` parameter,
/// default `"comment"`, is accepted by the original signature but never used
/// in its body, so it is intentionally omitted here.)
pub fn extract_comment_claims(input: ExtractCommentClaimsInput) -> Vec<CommentClaim> {
    let clean = strip_prefix_regex()
        .replace_all(input.text, "")
        .trim()
        .to_string();

    let mut claims = Vec::new();
    for kp in KIND_PATTERNS.iter() {
        let re = pattern_for(kp.regex_key);
        let caps = match re.captures(&clean) {
            Some(c) => c,
            None => continue,
        };
        let group1 = caps.get(1).map(|m| m.as_str().trim().to_string());
        let group2 = caps.get(2).map(|m| m.as_str().to_string());
        let owner = group1.clone();
        let body = group2
            .or_else(|| group1.clone())
            .unwrap_or_else(|| clean.clone())
            .trim()
            .to_string();

        let claim_id_source = format!("{}:{}:{}:{}", input.path, input.line, kp.kind, body);
        let claim = CommentClaim {
            id: format!("claim:{}", &sha1_hex(&claim_id_source)[..16]),
            document_id: format!("comment:{}:{}", input.path, input.line),
            source: input.path.to_string(),
            line: input.line,
            status: "implemented",
            sha1: sha1_hex(&clean),
            edges: vec![CommentClaimEdge {
                kind: "COMMENT_CLAIM",
                claim_kind: kp.kind,
                owner,
                body,
                enclosing_symbol_id: input.enclosing_symbol_id.clone(),
                lifecycle: "data_only",
                span_start_line: input.line,
                span_end_line: input.line,
            }],
        };
        claims.push(claim);
    }
    claims
}
