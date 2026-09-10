//! Parity port of `blueprint/tests/merkle-ledger.test.mjs`.
//!
//! These are relational parity tests (root changes iff a leaf changes; a
//! delta-built root equals a clean full-build root; diff is exact) — the
//! legacy JS test file asserts equality/inequality between computed digests,
//! not against literal pinned digest strings, so this port preserves that
//! same shape rather than inventing fixture constants the legacy suite never
//! had.

use membrane_blueprint::merkle_ledger::{
    compute_full_ledger, diff_ledger_against_tree, update_leaf_chain, LedgerFile,
};
use membrane_blueprint::store::open_store;

fn tree_a() -> Vec<LedgerFile> {
    vec![
        LedgerFile {
            path: "src/a.ts".to_string(),
            content_digest: "xxh128:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
        },
        LedgerFile {
            path: "src/nested/b.ts".to_string(),
            content_digest: "xxh128:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string(),
        },
    ]
}

fn tree_b() -> Vec<LedgerFile> {
    vec![
        LedgerFile {
            path: "src/a.ts".to_string(),
            content_digest: "xxh128:cccccccccccccccccccccccccccccccc".to_string(),
        },
        LedgerFile {
            path: "src/nested/c.ts".to_string(),
            content_digest: "xxh128:dddddddddddddddddddddddddddddddd".to_string(),
        },
    ]
}

#[test]
fn merkle_root_changes_only_when_a_leaf_changes_and_diff_is_exact() {
    let conn = open_store(None).expect("open store");
    let root_a = compute_full_ledger(&conn, &tree_a()).expect("full ledger a");

    let no_op_diff = diff_ledger_against_tree(&conn, Some(&root_a), &tree_a()).expect("diff a-a");
    assert!(no_op_diff.changed.is_empty());

    let diff = diff_ledger_against_tree(&conn, Some(&root_a), &tree_b()).expect("diff a-b");
    assert_eq!(diff.changed, vec!["src/a.ts".to_string()]);
    assert_eq!(diff.added, vec!["src/nested/c.ts".to_string()]);
    assert_eq!(diff.removed, vec!["src/nested/b.ts".to_string()]);

    let root_b = update_leaf_chain(&conn, "src/a.ts", Some("xxh128:cccccccccccccccccccccccccccccccc"))
        .expect("update a.ts");
    update_leaf_chain(&conn, "src/nested/b.ts", None).expect("delete nested/b.ts");
    update_leaf_chain(
        &conn,
        "src/nested/c.ts",
        Some("xxh128:dddddddddddddddddddddddddddddddd"),
    )
    .expect("insert nested/c.ts");

    assert_ne!(root_a, root_b);
}

#[test]
fn delta_built_merkle_root_equals_clean_full_build_root() {
    let delta_conn = open_store(None).expect("open delta store");
    let clean_conn = open_store(None).expect("open clean store");

    compute_full_ledger(&delta_conn, &tree_a()).expect("full ledger a");
    update_leaf_chain(
        &delta_conn,
        "src/a.ts",
        Some("xxh128:cccccccccccccccccccccccccccccccc"),
    )
    .expect("update a.ts");
    update_leaf_chain(&delta_conn, "src/nested/b.ts", None).expect("delete nested/b.ts");
    update_leaf_chain(
        &delta_conn,
        "src/nested/c.ts",
        Some("xxh128:dddddddddddddddddddddddddddddddd"),
    )
    .expect("insert nested/c.ts");

    let delta_root: String = delta_conn
        .query_row(
            "SELECT digest FROM generation_leaf WHERE path = ''",
            [],
            |row| row.get(0),
        )
        .expect("delta root row");

    let clean_root = compute_full_ledger(&clean_conn, &tree_b()).expect("full ledger b");

    assert_eq!(delta_root, clean_root);
}
