use crypt::{doc_spine, MemDb};
use rusqlite::Transaction;
use sha2::{Digest, Sha256};
use std::time::Instant;

fn digest(content: &str) -> String {
    format!("{:x}", Sha256::digest(content.as_bytes()))
}

fn create_schema(db: &MemDb) {
    db.lock()
        .execute_batch(
            "CREATE TABLE doc_artifacts (
                doc_id TEXT NOT NULL,
                repository_root TEXT NOT NULL,
                path TEXT NOT NULL,
                content_hash TEXT NOT NULL,
                lifecycle_state TEXT NOT NULL,
                sensitivity TEXT NOT NULL,
                revision TEXT NOT NULL,
                index_generation INTEGER NOT NULL
            );
            CREATE TABLE doc_projections (
                parent_doc_id TEXT NOT NULL,
                kind TEXT NOT NULL,
                content TEXT NOT NULL,
                source_content_hash TEXT NOT NULL,
                source_revision TEXT NOT NULL,
                index_generation INTEGER NOT NULL
            );",
        )
        .unwrap();
}

fn insert_projection(
    tx: &Transaction<'_>,
    root: &str,
    doc_id: &str,
    path: &str,
    markdown: &str,
    lifecycle: &str,
    sensitivity: &str,
) {
    let hash = digest(markdown);
    tx.execute(
        "INSERT INTO doc_artifacts
         (doc_id, repository_root, path, content_hash, lifecycle_state, sensitivity, revision, index_generation)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'rev-1', 1)",
        rusqlite::params![doc_id, root, path, hash, lifecycle, sensitivity],
    )
    .unwrap();
    tx.execute(
        "INSERT INTO doc_projections
         (parent_doc_id, kind, content, source_content_hash, source_revision, index_generation)
         VALUES (?1, 'lexical', ?2, ?3, 'rev-1', 1)",
        rusqlite::params![doc_id, markdown, hash],
    )
    .unwrap();
}

fn seed(corpus_size: usize) -> (tempfile::TempDir, MemDb) {
    let root = tempfile::tempdir().unwrap();
    let root_text = root.path().to_string_lossy().into_owned();
    let db = MemDb::open_in_memory();
    create_schema(&db);
    {
        let mut conn = db.lock();
        let tx = conn.transaction().unwrap();
        for index in 0..corpus_size {
            let doc_id = format!("background-{index:05}");
            let path = format!("background/{index:05}.md");
            let content = format!(
                "# Background {index}\nordinary filler context {}\n",
                "x".repeat(384)
            );
            insert_projection(
                &tx, &root_text, &doc_id, &path, &content, "active", "normal",
            );
        }
        for (doc_id, path, markdown, lifecycle, sensitivity) in [
            (
                "doc-a",
                "docs/a.md",
                "# Requested Anchor\nquest quest alpha\n",
                "active",
                "normal",
            ),
            (
                "doc-b",
                "docs/b.md",
                "# Beta\nbeta alpha\n",
                "active",
                "normal",
            ),
            ("doc-c", "docs/c.md", "# ID\nmarker\n", "active", "normal"),
            (
                "doc-d",
                "docs/d.md",
                "# Tie\nonly once\n",
                "active",
                "normal",
            ),
            (
                "doc-e",
                "docs/e.md",
                "# Tie\nonly once\n",
                "active",
                "normal",
            ),
            (
                "doc-restricted",
                "docs/restricted.md",
                "# Secret\nsecret term\n",
                "active",
                "restricted",
            ),
            (
                "doc-retired",
                "docs/retired.md",
                "# Retired\nretired term\n",
                "retired",
                "normal",
            ),
            (
                "doc-mismatch",
                "docs/mismatch.md",
                "# Mismatch\nmismatch term\n",
                "active",
                "normal",
            ),
            (
                "doc-benchmark",
                "docs/benchmark.md",
                "# Benchmark\nbenchmarkneedle\n",
                "active",
                "normal",
            ),
        ] {
            insert_projection(
                &tx,
                &root_text,
                doc_id,
                path,
                markdown,
                lifecycle,
                sensitivity,
            );
        }
        tx.execute(
            "UPDATE doc_projections SET source_content_hash='stale' WHERE parent_doc_id='doc-retired'",
            [],
        )
        .unwrap();
        tx.commit().unwrap();
    }
    for (path, content) in [
        ("docs/a.md", "# Requested Anchor\nquest quest alpha\n"),
        ("docs/b.md", "# Beta\nbeta alpha\n"),
        ("docs/c.md", "# ID\nmarker\n"),
        ("docs/d.md", "# Tie\nonly once\n"),
        ("docs/e.md", "# Tie\nonly once\n"),
        ("docs/restricted.md", "# Secret\nsecret term\n"),
        ("docs/retired.md", "# Retired\nretired term\n"),
        ("docs/mismatch.md", "# Changed\nchanged source\n"),
        ("docs/benchmark.md", "# Benchmark\nbenchmarkneedle\n"),
    ] {
        let destination = root.path().join(path);
        std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
        std::fs::write(destination, content).unwrap();
    }
    (root, db)
}

fn ids(db: &MemDb, query: &str, k: usize) -> Vec<String> {
    doc_spine::recall(db, query, k)
        .unwrap()
        .into_iter()
        .map(|hit| hit.doc_id)
        .collect()
}

#[test]
fn lexical_recall_contract_freezes_substrings_short_terms_ranking_and_filters() {
    let (_root, db) = seed(32);
    assert_eq!(ids(&db, "quest", 5), ["doc-a"]);
    assert_eq!(ids(&db, "REQUESTED", 5), ["doc-a"]);
    assert_eq!(ids(&db, "alpha beta", 5), ["doc-b", "doc-a"]);
    assert_eq!(ids(&db, "id", 5), ["doc-c"]);
    assert_eq!(ids(&db, "tie", 5), ["doc-d", "doc-e"]);
    assert!(ids(&db, "---", 5).is_empty());
    assert!(ids(&db, "secret retired mismatch", 5).is_empty());

    let hit = doc_spine::recall(&db, "quest", 1).unwrap().remove(0);
    assert_eq!(hit.source_ref, "doc://repo/worktree/docs/a.md");
    assert_eq!(hit.anchor_id, "sec:requested-anchor:1");
    assert_eq!(
        hit.expected_hash,
        digest("# Requested Anchor\nquest quest alpha\n")
    );
}

#[test]
#[ignore = "manual before/after lexical index measurement"]
fn measure_warm_lexical_recall_over_twelve_thousand_projections() {
    let (_root, db) = seed(12_000);
    let query = "benchmarkneedle";
    assert_eq!(ids(&db, query, 1), ["doc-benchmark"]);

    let mut micros = Vec::new();
    for _ in 0..9 {
        let started = Instant::now();
        let result = ids(&db, std::hint::black_box(query), 1);
        std::hint::black_box(result);
        micros.push(started.elapsed().as_micros() as u64);
    }
    micros.sort_unstable();
    println!(
        "{}",
        serde_json::json!({
            "schema_version": 1,
            "corpus_projections": 12_009,
            "query": query,
            "samples": micros.len(),
            "p50_us": micros[micros.len() / 2],
            "p95_us": micros[micros.len() - 1],
            "result_ids": ["doc-benchmark"],
        })
    );
}
