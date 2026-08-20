//! Deterministic head/tail truncation helper (L2).
//!
//! This is a pure function: it does not execute commands or touch disk. The
//! command runner (`runc`) composes this helper and adds exec + spill behavior.

/// Return a `(capped_text, truncated)` pair.
///
/// If the input has `<= head + tail` lines, returns the input unchanged and
/// `false`. Otherwise keeps the first `head` and last `tail` lines, inserting a
/// single marker line: `… N lines elided …`.
pub fn head_tail(text: &str, head: usize, tail: usize) -> (String, bool) {
    // Normalize newlines so line accounting is stable across platforms.
    let normalized = text.replace("\r\n", "\n");
    // `.lines()` intentionally ignores a trailing empty line when the input ends
    // with `\n`, which matches how most tools count "lines of output".
    let lines: Vec<&str> = normalized.lines().collect();

    let limit = head.saturating_add(tail);
    if lines.len() <= limit {
        return (normalized, false);
    }

    let elided = lines.len().saturating_sub(limit);
    let mut out: Vec<String> = Vec::with_capacity(head + tail + 1);
    out.extend(lines.iter().take(head).map(|s| (*s).to_string()));
    out.push(format!("… {elided} lines elided …"));
    out.extend(
        lines
            .iter()
            .skip(lines.len().saturating_sub(tail))
            .map(|s| (*s).to_string()),
    );
    (out.join("\n"), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn head_tail_caps_and_flags() {
        let input = (1..=100)
            .map(|i| format!("l{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let (out, truncated) = head_tail(&input, 5, 5);
        assert!(truncated);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 11);
        assert_eq!(lines[0], "l1");
        assert_eq!(lines[4], "l5");
        assert_eq!(lines[5], "… 90 lines elided …");
        assert_eq!(lines[6], "l96");
        assert_eq!(lines[10], "l100");

        let small = (1..=8)
            .map(|i| format!("l{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let (out2, truncated2) = head_tail(&small, 5, 5);
        assert!(!truncated2);
        assert_eq!(out2, small);
    }
}
