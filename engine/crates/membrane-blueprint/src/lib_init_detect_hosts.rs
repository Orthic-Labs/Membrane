//! Native Rust port of `blueprint/src/lib/init/detect-hosts.mjs`.
//!
//! Lane LIB3 (r5 closure): no prior native equivalent found (grep of
//! `detect_host`/`host_detect` across membrane-blueprint/src and
//! membrane-runtime/src produced no matching function). Ported behavior:
//! explicit host flag wins (validated against the fixed host set, `auto`
//! recurses into detection); otherwise existing host config files win;
//! otherwise installed command probes; otherwise `generic`.

use std::path::Path;

pub const HOSTS: [&str; 4] = ["claude-code", "codex", "cursor", "generic"];

fn host_config_paths(host: &str) -> &'static [&'static str] {
    match host {
        "claude-code" => &[".claude/settings.json", "CLAUDE.md", ".mcp.json"],
        "codex" => &["AGENTS.md", ".codex/config.toml"],
        "cursor" => &[".cursor/rules/blueprint.mdc", ".cursor/mcp.json"],
        "generic" => &["BLUEPRINT-AGENT.md"],
        _ => &[],
    }
}

fn host_commands(host: &str) -> &'static [&'static str] {
    match host {
        "claude-code" => &["claude"],
        "codex" => &["codex"],
        "cursor" => &["cursor"],
        _ => &[],
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DetectHostsError {
    #[error("--host must be auto, {0}")]
    InvalidHost(String),
}

/// Explicit flag wins (validated); `auto` recurses into detection; existing
/// host config files win; otherwise a command probe; otherwise `["generic"]`.
pub fn detect_hosts(
    explicit: Option<&str>,
    root: &Path,
    exists: impl Fn(&Path) -> bool,
    command_probe: impl Fn(&str) -> bool,
) -> Result<Vec<String>, DetectHostsError> {
    if let Some(value) = explicit {
        if value == "auto" {
            return detect_hosts(None, root, exists, command_probe);
        }
        if !HOSTS.contains(&value) {
            return Err(DetectHostsError::InvalidHost(HOSTS.join(", ")));
        }
        return Ok(vec![value.to_string()]);
    }
    let mut detected: Vec<String> = Vec::new();
    for host in HOSTS {
        let has_config = host_config_paths(host)
            .iter()
            .any(|path| exists(&root.join(path)));
        if has_config && !detected.iter().any(|h| h == host) {
            detected.push(host.to_string());
        }
    }
    if detected.is_empty() {
        for host in HOSTS {
            let found = host_commands(host).iter().any(|cmd| command_probe(cmd));
            if found && !detected.iter().any(|h| h == host) {
                detected.push(host.to_string());
            }
        }
    }
    if detected.is_empty() {
        detected.push("generic".to_string());
    }
    Ok(detected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::path::PathBuf;

    #[test]
    fn explicit_host_wins() {
        let result = detect_hosts(Some("cursor"), &PathBuf::from("/repo"), |_| false, |_| false)
            .unwrap();
        assert_eq!(result, vec!["cursor".to_string()]);
    }

    #[test]
    fn explicit_auto_falls_back_to_detection() {
        let existing: HashSet<PathBuf> = [PathBuf::from("/repo/CLAUDE.md")].into_iter().collect();
        let result = detect_hosts(
            Some("auto"),
            &PathBuf::from("/repo"),
            |p| existing.contains(p),
            |_| false,
        )
        .unwrap();
        assert_eq!(result, vec!["claude-code".to_string()]);
    }

    #[test]
    fn invalid_explicit_host_errors() {
        let err = detect_hosts(Some("bogus"), &PathBuf::from("/repo"), |_| false, |_| false)
            .unwrap_err();
        assert!(matches!(err, DetectHostsError::InvalidHost(_)));
    }

    #[test]
    fn detects_from_existing_config_files() {
        let existing: HashSet<PathBuf> = [
            PathBuf::from("/repo/AGENTS.md"),
            PathBuf::from("/repo/.cursor/rules/blueprint.mdc"),
        ]
        .into_iter()
        .collect();
        let mut result = detect_hosts(
            None,
            &PathBuf::from("/repo"),
            |p| existing.contains(p),
            |_| false,
        )
        .unwrap();
        result.sort();
        assert_eq!(result, vec!["codex".to_string(), "cursor".to_string()]);
    }

    #[test]
    fn falls_back_to_command_probe_when_no_config_found() {
        let result = detect_hosts(
            None,
            &PathBuf::from("/repo"),
            |_| false,
            |cmd| cmd == "cursor",
        )
        .unwrap();
        assert_eq!(result, vec!["cursor".to_string()]);
    }

    #[test]
    fn falls_back_to_generic_when_nothing_detected() {
        let result = detect_hosts(None, &PathBuf::from("/repo"), |_| false, |_| false).unwrap();
        assert_eq!(result, vec!["generic".to_string()]);
    }
}
