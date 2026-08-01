use std::path::{Path, PathBuf};
use std::process::Command;

const REPOSITORY: &str = "https://github.com/asg017/sqlite-vec.git";
const REVISION: &str = "04d28bd21773981e2d266bbf6aa4efbd011eb4f6";

fn run(git: &mut Command, action: &str) {
    let output = git
        .output()
        .unwrap_or_else(|error| panic!("{action}: {error}"));
    if !output.status.success() {
        panic!(
            "{action} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
}

fn source_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("VECTOR_BAKEOFF_SQLITE_VEC_ALPHA_SRC") {
        return PathBuf::from(path);
    }
    let output = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    let source = output.join("sqlite-vec-source");
    if !source.join(".git").exists() {
        std::fs::create_dir_all(&source).expect("create alpha source directory");
        run(
            Command::new("git")
                .args(["init", "--quiet"])
                .current_dir(&source),
            "git init",
        );
        run(
            Command::new("git")
                .args(["remote", "add", "origin", REPOSITORY])
                .current_dir(&source),
            "git remote add",
        );
    }
    run(
        Command::new("git")
            .args(["fetch", "--quiet", "--depth", "1", "origin", REVISION])
            .current_dir(&source),
        "git fetch pinned sqlite-vec",
    );
    run(
        Command::new("git")
            .args(["checkout", "--quiet", "--detach", "FETCH_HEAD"])
            .current_dir(&source),
        "git checkout pinned sqlite-vec",
    );
    source
}

fn verify_revision(source: &Path) {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(source)
        .output()
        .expect("git rev-parse pinned sqlite-vec");
    let observed = String::from_utf8_lossy(&output.stdout);
    assert_eq!(observed.trim(), REVISION, "sqlite-vec alpha source drift");
    for file in ["sqlite-vec.c", "sqlite-vec-diskann.c"] {
        assert!(
            source.join(file).is_file(),
            "missing pinned alpha source {file}"
        );
    }
}

fn main() {
    println!("cargo:rerun-if-env-changed=VECTOR_BAKEOFF_SQLITE_VEC_ALPHA_SRC");
    println!("cargo:rerun-if-changed=build.rs");
    let source = source_dir();
    verify_revision(&source);
    let output = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    let template = std::fs::read_to_string(source.join("sqlite-vec.h.tmpl"))
        .expect("read sqlite-vec header template");
    let header = template
        .replace("${VERSION}", "0.1.10-alpha.4")
        .replace("${DATE}", "2026-03-31")
        .replace("${SOURCE}", REVISION)
        .replace("${VERSION_MAJOR}", "0")
        .replace("${VERSION_MINOR}", "1")
        .replace("${VERSION_PATCH}", "10");
    std::fs::write(output.join("sqlite-vec.h"), header).expect("write generated sqlite-vec header");
    let python = std::env::var("PYTHON").unwrap_or_else(|_| {
        if cfg!(windows) {
            "python".into()
        } else {
            "python3".into()
        }
    });
    let amalgamation = Command::new(python)
        .arg(source.join("scripts/amalgamate.py"))
        .arg(source.join("sqlite-vec.c"))
        .output()
        .expect("run official sqlite-vec amalgamation script");
    assert!(
        amalgamation.status.success(),
        "sqlite-vec amalgamation failed: {}",
        String::from_utf8_lossy(&amalgamation.stderr)
    );
    let amalgamated = output.join("sqlite-vec-amalgamated.c");
    std::fs::write(&amalgamated, amalgamation.stdout).expect("write sqlite-vec amalgamation");
    let sqlite_include = std::env::var_os("DEP_SQLITE3_INCLUDE").expect("DEP_SQLITE3_INCLUDE");
    cc::Build::new()
        .file(amalgamated)
        .include(&output)
        .include(sqlite_include)
        .define("SQLITE_CORE", None)
        .compile("sqlite_vec0");
}
