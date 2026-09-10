//! Vendored tree-sitter grammars for languages with no maintained,
//! tree-sitter-0.26-compatible crate on crates.io. See `VENDORED.md` for the
//! upstream repo/commit each grammar was vendored from and why.

use tree_sitter_language::LanguageFn;

macro_rules! vendored_language {
    ($vis_fn:ident, $extern_name:ident) => {
        extern "C" {
            fn $extern_name() -> *const ();
        }

        pub const $vis_fn: LanguageFn = unsafe { LanguageFn::from_raw($extern_name) };
    };
}

pub mod elisp {
    use super::*;
    vendored_language!(LANGUAGE, tree_sitter_elisp);
}

pub mod embedded_template {
    use super::*;
    vendored_language!(LANGUAGE, tree_sitter_embedded_template);
}

pub mod ql {
    use super::*;
    vendored_language!(LANGUAGE, tree_sitter_ql);
}

pub mod rescript {
    use super::*;
    vendored_language!(LANGUAGE, tree_sitter_rescript);
}

pub mod solidity {
    use super::*;
    vendored_language!(LANGUAGE, tree_sitter_solidity);
}

pub mod systemrdl {
    use super::*;
    vendored_language!(LANGUAGE, tree_sitter_systemrdl);
}

pub mod tlaplus {
    use super::*;
    vendored_language!(LANGUAGE, tree_sitter_tlaplus);
}

pub mod vue {
    use super::*;
    vendored_language!(LANGUAGE, tree_sitter_vue);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_loads(f: LanguageFn) {
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&f.into()).expect("language loads");
    }

    #[test]
    fn all_vendored_grammars_load() {
        assert_loads(elisp::LANGUAGE);
        assert_loads(embedded_template::LANGUAGE);
        assert_loads(ql::LANGUAGE);
        assert_loads(rescript::LANGUAGE);
        assert_loads(solidity::LANGUAGE);
        assert_loads(systemrdl::LANGUAGE);
        assert_loads(tlaplus::LANGUAGE);
        assert_loads(vue::LANGUAGE);
    }
}
