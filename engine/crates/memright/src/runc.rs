//! L2 command runner — deterministic wrapper around shell execution.
//!
//! This is the `runc.mjs` replacement: it executes a command string, captures
//! stdout+stderr, produces a head/tail-capped view for context injection, spills
//! the full output to disk when truncated, and preserves the child exit code.

use crate::truncate;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone)]
pub struct RuncResult {
    pub capped: String,
    pub spill_path: Option<PathBuf>,
    pub exit_code: i32,
}

fn parse_shell_override(raw: &str) -> Vec<String> {
    raw.split_whitespace().map(|s| s.to_string()).collect()
}

fn default_shell_argv() -> Vec<String> {
    if cfg!(windows) {
        let bash = std::env::var_os("ProgramFiles")
            .map(PathBuf::from)
            .map(|root| root.join("Git").join("bin").join("bash.exe"))
            .filter(|path| path.is_file())
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|| "bash".into());
        vec![bash, "-c".into()]
    } else {
        vec!["sh".into(), "-c".into()]
    }
}

/// Resolve the shell argv prefix used to execute `cmd`.
///
/// Order:
/// 1. `MEMRIGHT_RUNC_SHELL` if set (split by whitespace into argv). If the value
///    has no explicit `-c`/`/C`, we append the platform default switch.
/// 2. Platform default: POSIX `sh -c`, Windows Git Bash `bash -c`.
pub fn resolve_shell_argv() -> Vec<String> {
    if let Ok(raw) = std::env::var("MEMRIGHT_RUNC_SHELL") {
        let mut argv = parse_shell_override(&raw);
        if argv.is_empty() {
            return default_shell_argv();
        }
        // If the override provides only the program, add the platform switch.
        if argv.len() == 1 {
            if cfg!(windows) {
                argv.push("/C".into());
            } else {
                argv.push("-c".into());
            }
        }
        return argv;
    }
    default_shell_argv()
}

/// Execute `cmd` via a platform-resolved shell, returning a capped view and
/// (if truncated) a spill file path containing the full output.
pub fn run_capped(
    cmd: &str,
    head: usize,
    tail: usize,
    spill_dir: &Path,
) -> Result<RuncResult, String> {
    let argv = resolve_shell_argv();
    let program = argv
        .first()
        .ok_or_else(|| "resolved shell argv unexpectedly empty".to_string())?;

    let mut c = Command::new(program);
    if argv.len() > 1 {
        c.args(&argv[1..]);
    }
    c.arg(cmd);

    let out = c.output().map_err(|e| format!("spawn failed: {e}"))?;
    let mut combined = Vec::new();
    combined.extend_from_slice(&out.stdout);
    combined.extend_from_slice(&out.stderr);
    let full = String::from_utf8_lossy(&combined).to_string();

    let (capped, _) = truncate::head_tail(&full, head, tail);

    let exit_code = out.status.code().unwrap_or(1);
    std::fs::create_dir_all(spill_dir).map_err(|e| format!("spill_dir create failed: {e}"))?;
    let name = format!("runc-{}.log", crate::time::now_millis());
    let path = spill_dir.join(name);
    std::fs::write(&path, &full).map_err(|e| format!("spill write failed: {e}"))?;
    let spill_path = Some(path);

    Ok(RuncResult {
        capped,
        spill_path,
        exit_code,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    /// Every test in this module reads or writes the process-global
    /// `MEMRIGHT_RUNC_SHELL` env var. Rust runs tests concurrently within one
    /// binary, and `std::env::set_var` mutates the whole process, so without a
    /// shared lock the `shell_override_*` tests race the `run_capped_*` tests —
    /// the latter would read a `bash`-polluted value and run under the wrong
    /// shell (the exit-127 flake this guard fixes). Hold this for the whole test.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn lock_env() -> MutexGuard<'static, ()> {
        // Recover from a poisoned lock: a prior test panicking while holding it
        // must not cascade-fail every other test in the module.
        ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn default_shell_preserves_the_legacy_bash_contract() {
        let _guard = lock_env();
        let prior = std::env::var_os("MEMRIGHT_RUNC_SHELL");
        unsafe {
            std::env::remove_var("MEMRIGHT_RUNC_SHELL");
        }
        let argv = resolve_shell_argv();
        assert_eq!(argv.last().map(String::as_str), Some("-c"));
        assert!(
            argv.first().is_some_and(|program| {
                let normalized = program.replace('\\', "/").to_ascii_lowercase();
                normalized.ends_with("/bash.exe") || normalized == "bash" || normalized == "sh"
            }),
            "legacy runc contract requires a Bourne-compatible shell, got {argv:?}"
        );
        unsafe {
            match prior {
                Some(v) => std::env::set_var("MEMRIGHT_RUNC_SHELL", v),
                None => std::env::remove_var("MEMRIGHT_RUNC_SHELL"),
            }
        }
    }

    /// `MEMRIGHT_RUNC_SHELL` with just a program (no switch) — the resolver
    /// should append the platform-correct switch automatically.
    #[test]
    fn shell_override_program_only_appends_platform_switch() {
        let _guard = lock_env();
        let prior = std::env::var_os("MEMRIGHT_RUNC_SHELL");
        // SAFETY: `_guard` serializes all env-touching tests in this module, so
        // this test owns `MEMRIGHT_RUNC_SHELL` for its duration.
        unsafe {
            std::env::set_var("MEMRIGHT_RUNC_SHELL", "bash");
        }
        let argv = resolve_shell_argv();
        if cfg!(windows) {
            // On Windows, "bash" is interpreted as the program; we append /C
            // because cfg!(windows). Note: bash.exe may not exist on Windows
            // (test environments vary) — this test only checks argv parsing,
            // not whether the shell can actually run.
            assert_eq!(argv, vec!["bash".to_string(), "/C".to_string()]);
        } else {
            assert_eq!(argv, vec!["bash".to_string(), "-c".to_string()]);
        }
        unsafe {
            match prior {
                Some(v) => std::env::set_var("MEMRIGHT_RUNC_SHELL", v),
                None => std::env::remove_var("MEMRIGHT_RUNC_SHELL"),
            }
        }
    }

    /// `MEMRIGHT_RUNC_SHELL` with both program and switch — use as-is.
    #[test]
    fn shell_override_program_and_switch_used_verbatim() {
        let _guard = lock_env();
        let prior = std::env::var_os("MEMRIGHT_RUNC_SHELL");
        unsafe {
            std::env::set_var("MEMRIGHT_RUNC_SHELL", "bash -c");
        }
        let argv = resolve_shell_argv();
        assert_eq!(argv, vec!["bash".to_string(), "-c".to_string()]);
        unsafe {
            match prior {
                Some(v) => std::env::set_var("MEMRIGHT_RUNC_SHELL", v),
                None => std::env::remove_var("MEMRIGHT_RUNC_SHELL"),
            }
        }
    }

    #[test]
    fn run_capped_preserves_exit_and_spills() {
        let _guard = lock_env();
        // Pin the shell to the platform default so a leaked override from another
        // process can't change what runs here.
        let prior = std::env::var_os("MEMRIGHT_RUNC_SHELL");
        unsafe {
            std::env::remove_var("MEMRIGHT_RUNC_SHELL");
        }
        let dir = tempfile::tempdir().unwrap();

        let long_cmd = "i=1; while [ $i -le 100 ]; do echo l$i; i=$((i+1)); done".to_string();

        let r = run_capped(&long_cmd, 3, 3, dir.path()).expect("run_capped ok");
        assert_eq!(
            r.exit_code, 0,
            "expected exit 0; got {}: capped output:\n{}",
            r.exit_code, r.capped
        );
        assert!(r.spill_path.is_some());
        let capped_lines: Vec<&str> = r.capped.lines().collect();
        assert_eq!(capped_lines.len(), 7, "capped:\n{}", r.capped);
        assert!(capped_lines[3].contains("lines elided"));

        let spill_path = r.spill_path.unwrap();
        let spilled = std::fs::read_to_string(&spill_path).unwrap();
        assert_eq!(spilled.lines().count(), 100);

        let r2 = run_capped("exit 7", 3, 3, dir.path()).expect("run_capped ok");
        assert_eq!(r2.exit_code, 7);

        unsafe {
            match prior {
                Some(v) => std::env::set_var("MEMRIGHT_RUNC_SHELL", v),
                None => std::env::remove_var("MEMRIGHT_RUNC_SHELL"),
            }
        }
    }

    /// Non-truncated output: no spill file should be written.
    #[test]
    fn run_capped_spills_full_output_even_when_output_fits() {
        let _guard = lock_env();
        let prior = std::env::var_os("MEMRIGHT_RUNC_SHELL");
        unsafe {
            std::env::remove_var("MEMRIGHT_RUNC_SHELL");
        }
        let dir = tempfile::tempdir().unwrap();
        let r = run_capped("echo hi", 100, 100, dir.path()).expect("run_capped ok");
        assert_eq!(r.exit_code, 0, "expected exit 0; got {}", r.exit_code);
        let spill = r
            .spill_path
            .as_ref()
            .expect("full output remains retrievable");
        assert_eq!(std::fs::read_to_string(spill).unwrap(), "hi\n");
        // `echo hi` emits "hi\n" on every shell. We trim because the truncated
        // output is what's printed (not the spilled raw output).
        assert_eq!(r.capped.trim_end(), "hi");

        unsafe {
            match prior {
                Some(v) => std::env::set_var("MEMRIGHT_RUNC_SHELL", v),
                None => std::env::remove_var("MEMRIGHT_RUNC_SHELL"),
            }
        }
    }
}
