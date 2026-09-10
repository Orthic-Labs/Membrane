use std::path::Path;

/// (crate-facing language id, grammar directory name, has external scanner)
const GRAMMARS: &[(&str, &str, bool)] = &[
    ("elisp", "elisp", false),
    ("embedded_template", "embedded_template", false),
    ("ql", "ql", false),
    ("rescript", "rescript", true),
    ("solidity", "solidity", false),
    ("systemrdl", "systemrdl", false),
    ("tlaplus", "tlaplus", true),
    ("vue", "vue", true),
];

fn main() {
    let root = Path::new("grammars");
    for (lang, dir, has_scanner) in GRAMMARS {
        let src_dir = root.join(dir).join("src");
        let parser_path = src_dir.join("parser.c");
        let mut build = cc::Build::new();
        build.include(&src_dir).std("c11");
        #[cfg(target_env = "msvc")]
        build.flag("-utf-8");
        build.file(&parser_path);
        println!("cargo:rerun-if-changed={}", parser_path.display());
        if *has_scanner {
            let scanner_path = src_dir.join("scanner.c");
            if scanner_path.exists() {
                build.file(&scanner_path);
                println!("cargo:rerun-if-changed={}", scanner_path.display());
            }
        }
        build.compile(&format!("tree-sitter-{lang}"));
    }
}
