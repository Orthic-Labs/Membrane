//! Native Rust dispatch for the CLI `mcp` verb
//! (`blueprint/scripts/cli/commands.mjs:482` case `"mcp"`).
//!
//! Lane V3 (r5 closure). Legacy's `mcp` verb only implements `mcp serve`,
//! which spawns the legacy JS stdio server in-process
//! (`blueprint/scripts/blueprint-mcp.mjs`); that server is superseded by
//! the shipped native `engine/crates/membrane-mcp` Rust MCP server
//! (parity-audit.json: "CLI blueprint-mcp.mjs ... EXECUTED-NATIVELY"), so
//! porting `mcp serve` itself would stand up a second, competing server
//! process rather than close a gap.
//!
//! What legacy's `init` flow (`blueprint/src/lib/init/apply.mjs`) *does*
//! write, for every MCP-capable host config file
//! (`.mcp.json`, `.claude/settings.json` — the JSON members of
//! [`crate::lib_init_apply::allowed_targets`]), is an `mcpServers.blueprint`
//! stanza pointing at the legacy server's launch command. This module is
//! the native `mcp` verb's replacement for that: it prints, or installs,
//! the equivalent stanza for the native stdio server
//! (`{"command":"membrane","args":["stdio-mcp"]}`), using the same
//! allowlisted JSON host files and the same non-destructive merge
//! discipline (only `mcpServers.blueprint` is ever touched; every other key
//! in the file is preserved byte-for-byte modulo JSON re-serialization).

use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

/// The two JSON-shaped host config files legacy `apply.mjs` ever writes an
/// `mcpServers` stanza into, mirrored from
/// [`crate::lib_init_apply::allowed_targets`]'s `.json` members.
pub fn mcp_host_config_files(root: &Path) -> Vec<PathBuf> {
    vec![root.join(".mcp.json"), root.join(".claude").join("settings.json")]
}

/// The native stdio MCP server declaration this verb installs, replacing
/// legacy's `{command: process.execPath, args: [blueprint-mcp.mjs, "--root", root]}`.
pub fn native_mcp_declaration() -> Value {
    json!({"command": "membrane", "args": ["stdio-mcp"]})
}

#[derive(Debug, Clone, PartialEq)]
pub enum McpCliError {
    /// Mirrors `machineError("usage", "blueprint mcp <sub> is not a known subcommand; use blueprint mcp install|print")`.
    UnknownSubcommand(String),
    /// The host file exists but is not valid JSON (mirrors `mergeJsonFile`'s `"<path> is not valid JSON"`).
    InvalidJson(String),
    Io(String),
}

/// Mirrors the `print` path: report the declaration without touching disk.
pub fn print(_root: &Path) -> Value {
    json!({"schemaVersion": 1, "action": "print", "declaration": native_mcp_declaration()})
}

/// Mirrors an `install` path analogous to `applyInitPlan`'s `.mcp.json`/
/// `settings.json` merge: for every host config file that already exists,
/// merge `mcpServers.blueprint` in place, creating no new host files (the
/// native verb never invents a host's presence; that is `init`'s job).
/// Returns the list of files actually written.
pub fn install(root: &Path) -> Result<Value, McpCliError> {
    let mut written = Vec::new();
    for path in mcp_host_config_files(root) {
        if !path.exists() {
            continue;
        }
        let current = fs::read_to_string(&path).map_err(|e| McpCliError::Io(e.to_string()))?;
        let mut value: Value = serde_json::from_str(&current)
            .map_err(|_| McpCliError::InvalidJson(path.display().to_string()))?;
        if !value.is_object() {
            return Err(McpCliError::InvalidJson(path.display().to_string()));
        }
        let obj = value.as_object_mut().unwrap();
        let servers = obj.entry("mcpServers").or_insert_with(|| json!({}));
        if !servers.is_object() {
            *servers = json!({});
        }
        servers
            .as_object_mut()
            .unwrap()
            .insert("blueprint".to_string(), native_mcp_declaration());
        let rendered = format!("{}\n", serde_json::to_string_pretty(&value).map_err(|e| McpCliError::Io(e.to_string()))?);
        fs::write(&path, rendered).map_err(|e| McpCliError::Io(e.to_string()))?;
        written.push(path.display().to_string());
    }
    Ok(json!({"schemaVersion": 1, "action": "install", "declaration": native_mcp_declaration(), "written": written}))
}

/// Mirrors the `case "mcp"` dispatch body.
pub fn run(root: &Path, subcommand: Option<&str>) -> Result<Value, McpCliError> {
    match subcommand.unwrap_or("print") {
        "print" => Ok(print(root)),
        "install" => install(root),
        other => Err(McpCliError::UnknownSubcommand(other.to_string())),
    }
}

impl McpCliError {
    pub fn code(&self) -> &'static str {
        match self {
            McpCliError::UnknownSubcommand(_) => "usage",
            McpCliError::InvalidJson(_) => "host_config_invalid",
            McpCliError::Io(_) => "io_error",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn print_reports_the_native_declaration_without_touching_disk() {
        let dir = tempfile::tempdir().unwrap();
        let payload = print(dir.path());
        assert_eq!(payload["declaration"]["command"], "membrane");
        assert_eq!(payload["declaration"]["args"][0], "stdio-mcp");
        assert!(!dir.path().join(".mcp.json").exists());
    }

    #[test]
    fn install_merges_into_existing_mcp_json_preserving_other_keys() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(".mcp.json"), r#"{"mcpServers":{"other":{"command":"x"}}}"#).unwrap();
        let payload = install(dir.path()).unwrap();
        assert_eq!(payload["written"].as_array().unwrap().len(), 1);
        let written: Value = serde_json::from_str(&fs::read_to_string(dir.path().join(".mcp.json")).unwrap()).unwrap();
        assert_eq!(written["mcpServers"]["other"]["command"], "x");
        assert_eq!(written["mcpServers"]["blueprint"]["command"], "membrane");
        assert_eq!(written["mcpServers"]["blueprint"]["args"][0], "stdio-mcp");
    }

    #[test]
    fn install_skips_host_files_that_do_not_exist() {
        let dir = tempfile::tempdir().unwrap();
        let payload = install(dir.path()).unwrap();
        assert_eq!(payload["written"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn install_rejects_invalid_json_host_file() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(".mcp.json"), "not json").unwrap();
        let err = install(dir.path()).unwrap_err();
        assert_eq!(err.code(), "host_config_invalid");
    }

    #[test]
    fn unknown_subcommand_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let err = run(dir.path(), Some("serve")).unwrap_err();
        assert_eq!(err.code(), "usage");
    }
}
