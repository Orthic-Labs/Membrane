use membrane_runtime::guide::{doc_spine, GuideDb};
use rusqlite::Transaction;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::process::Command;
use std::time::Instant;

fn digest(content: &str) -> String {
    hex::encode(Sha256::digest(content.as_bytes()))
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
        "INSERT INTO guide_doc_artifacts
         (doc_id, repository_root, repository_id, revision, path, content_hash,
          parser_version, document_class, lifecycle_state, title, summary, keywords_json,
          superseded_by, trust_label, influence_class, sensitivity, generated,
          index_generation, updated_at_ms)
         VALUES (?1, ?2, 'repo', 'rev-1', ?3, ?4, 'test', 'markdown', ?5,
                 '', '', '[]', NULL, 'trusted', 'normal', ?6, 0, 1, 0)",
        rusqlite::params![doc_id, root, path, hash, lifecycle, sensitivity],
    )
    .unwrap();
    tx.execute(
        "INSERT INTO guide_doc_projections
         (parent_doc_id, kind, content, token_count, anchor_id, collapsed_to_parent,
          source_content_hash, source_revision, index_generation)
         VALUES (?1, 'lexical', ?2, 0, 'document', NULL, ?3, 'rev-1', 1)",
        rusqlite::params![doc_id, markdown, hash],
    )
    .unwrap();
}

fn background_content(index: usize) -> String {
    const WORDS: [&str; 24] = [
        "adapter",
        "anchor",
        "artifact",
        "bounded",
        "cache",
        "catalog",
        "checkpoint",
        "context",
        "deadline",
        "document",
        "fallback",
        "federation",
        "generation",
        "identity",
        "latency",
        "memory",
        "packet",
        "projection",
        "provider",
        "receipt",
        "repository",
        "retrieval",
        "revision",
        "runtime",
    ];
    let body = (0..48)
        .map(|offset| WORDS[(index * 7 + offset * 11) % WORDS.len()])
        .collect::<Vec<_>>()
        .join(" ");
    format!("# Background {index}\n{body}\nrecord {index:05} remains deterministic\n")
}

fn seed_with_storage(corpus_size: usize, on_disk: bool) -> (tempfile::TempDir, GuideDb) {
    let root = tempfile::tempdir().unwrap();
    let root_text = root.path().to_string_lossy().into_owned();
    let db = if on_disk {
        GuideDb::open(root.path().join("guide-index.sqlite3")).unwrap()
    } else {
        GuideDb::open_in_memory()
    };
    {
        let mut conn = db.lock();
        let tx = conn.transaction().unwrap();
        for index in 0..corpus_size {
            let doc_id = format!("background-{index:05}");
            let path = format!("background/{index:05}.md");
            let content = background_content(index);
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
            "UPDATE guide_doc_projections SET source_content_hash='stale' WHERE parent_doc_id='doc-retired'",
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

fn seed(corpus_size: usize) -> (tempfile::TempDir, GuideDb) {
    seed_with_storage(corpus_size, false)
}

fn ids(db: &GuideDb, query: &str, k: usize) -> Vec<String> {
    doc_spine::recall(db, query, k)
        .unwrap()
        .into_iter()
        .map(|hit| hit.doc_id)
        .collect()
}

fn create_fts_index(db: &GuideDb) {
    db.lock()
        .execute_batch(
            "CREATE VIRTUAL TABLE guide_doc_projection_fts
                 USING fts5(content, content='', tokenize='trigram');
             CREATE TRIGGER guide_doc_projection_fts_insert
                 AFTER INSERT ON guide_doc_projections WHEN new.kind='lexical' BEGIN
                   INSERT INTO guide_doc_projection_fts(rowid, content) VALUES (new.rowid, new.content);
                 END;
             CREATE TRIGGER guide_doc_projection_fts_delete
                 AFTER DELETE ON guide_doc_projections WHEN old.kind='lexical' BEGIN
                   INSERT INTO guide_doc_projection_fts(guide_doc_projection_fts, rowid, content)
                     VALUES ('delete', old.rowid, old.content);
                 END;
             INSERT INTO guide_doc_projection_fts(rowid, content)
                 SELECT rowid, content FROM guide_doc_projections WHERE kind='lexical';
             INSERT INTO guide_doc_projection_fts(guide_doc_projection_fts) VALUES('optimize');",
        )
        .unwrap();
}

fn corpus_results(db: &GuideDb) -> Vec<Vec<String>> {
    [
        "quest",
        "REQUESTED",
        "alpha beta",
        "id",
        "tie",
        "---",
        "secret retired mismatch",
    ]
    .iter()
    .map(|query| ids(db, query, 5))
    .collect()
}

fn checkpoint(db: &GuideDb) {
    db.lock()
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
        .unwrap();
}

fn db_bytes(db: &GuideDb) -> u64 {
    let conn = db.lock();
    let pages: i64 = conn
        .query_row("PRAGMA page_count", [], |row| row.get(0))
        .unwrap();
    let page_size: i64 = conn
        .query_row("PRAGMA page_size", [], |row| row.get(0))
        .unwrap();
    (pages * page_size) as u64
}

fn lexical_bytes(db: &GuideDb) -> u64 {
    let bytes: i64 = db
        .lock()
        .query_row(
            "SELECT COALESCE(SUM(length(CAST(content AS BLOB))), 0)
             FROM guide_doc_projections WHERE kind='lexical'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    bytes as u64
}

fn fts_bytes(db: &GuideDb) -> u64 {
    let bytes: i64 = db
        .lock()
        .query_row(
            "SELECT COALESCE(SUM(pgsize), 0) FROM dbstat
             WHERE name GLOB 'guide_doc_projection_fts*'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    bytes as u64
}

fn wal_bytes(path: &Path) -> u64 {
    std::fs::metadata(format!("{}-wal", path.display()))
        .map(|metadata| metadata.len())
        .unwrap_or(0)
}

fn replace_benchmark_projection(db: &GuideDb) {
    let markdown = "# Benchmark\nbenchmarkneedle\n";
    let hash = digest(markdown);
    let mut conn = db.lock();
    let tx = conn.transaction().unwrap();
    tx.execute(
        "DELETE FROM guide_doc_projections WHERE parent_doc_id='doc-benchmark'",
        [],
    )
    .unwrap();
    tx.execute(
        "INSERT INTO guide_doc_projections
         (parent_doc_id, kind, content, token_count, anchor_id, collapsed_to_parent,
          source_content_hash, source_revision, index_generation)
         VALUES ('doc-benchmark', 'lexical', ?1, 0, 'document', NULL, ?2, 'rev-1', 1)",
        rusqlite::params![markdown, hash],
    )
    .unwrap();
    tx.commit().unwrap();
}

#[cfg(unix)]
fn rss_bytes() -> u64 {
    let output = Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .unwrap();
    String::from_utf8(output.stdout)
        .unwrap()
        .trim()
        .parse::<u64>()
        .unwrap()
        * 1024
}

#[cfg(windows)]
fn rss_bytes() -> u64 {
    let script = format!("(Get-Process -Id {}).WorkingSet64", std::process::id());
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-Command", &script])
        .output()
        .unwrap();
    String::from_utf8(output.stdout)
        .unwrap()
        .trim()
        .parse()
        .unwrap()
}

#[test]
fn lexical_recall_contract_freezes_substrings_short_terms_ranking_and_filters() {
    let (_root, db) = seed(32);
    let corpus = [
        ("quest", vec!["doc-a"]),
        ("REQUESTED", vec!["doc-a"]),
        ("alpha beta", vec!["doc-b", "doc-a"]),
        ("id", vec!["doc-c"]),
        ("tie", vec!["doc-d", "doc-e"]),
        ("---", vec![]),
        ("secret retired mismatch", vec![]),
    ];
    for (query, expected) in &corpus {
        assert_eq!(ids(&db, query, 5), *expected, "fallback query: {query}");
    }

    assert_eq!(
        corpus_results(&db),
        corpus
            .iter()
            .map(|(_, ids)| ids.iter().map(|id| (*id).to_owned()).collect::<Vec<_>>())
            .collect::<Vec<_>>()
    );
    create_fts_index(&db);
    for (query, expected) in &corpus {
        assert_eq!(ids(&db, query, 5), *expected, "indexed query: {query}");
    }

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
    const MAX_INDEX_TO_LEXICAL_RATIO: f64 = 0.50;
    const MAX_WRITE_AMPLIFICATION: f64 = 6.0;
    const MAX_RSS_INCREASE_BYTES: u64 = 32 * 1024 * 1024;
    const MAX_REBUILD_US: u64 = 5_000_000;
    const MIN_P95_SPEEDUP: f64 = 3.0;

    let (root, db) = seed_with_storage(12_000, true);
    let db_path = root.path().join("guide-index.sqlite3");
    let query = "benchmarkneedle";
    let fallback_started = Instant::now();
    assert_eq!(ids(&db, query, 1), ["doc-benchmark"]);
    let fallback_cold_us = fallback_started.elapsed().as_micros() as u64;
    let fallback_ordered = corpus_results(&db);

    let mut fallback_micros = Vec::new();
    for _ in 0..9 {
        let started = Instant::now();
        std::hint::black_box(ids(&db, std::hint::black_box(query), 1));
        fallback_micros.push(started.elapsed().as_micros() as u64);
    }
    fallback_micros.sort_unstable();

    checkpoint(&db);
    let base_db_bytes = db_bytes(&db);
    let payload_bytes = lexical_bytes(&db);
    let base_rss_bytes = rss_bytes();
    replace_benchmark_projection(&db);
    let fallback_sync_wal_bytes = wal_bytes(&db_path);
    checkpoint(&db);

    let rebuild_started = Instant::now();
    create_fts_index(&db);
    let rebuild_us = rebuild_started.elapsed().as_micros() as u64;
    assert_eq!(corpus_results(&db), fallback_ordered);
    let cold_started = Instant::now();
    assert_eq!(ids(&db, query, 1), ["doc-benchmark"]);
    let indexed_cold_us = cold_started.elapsed().as_micros() as u64;

    let mut indexed_micros = Vec::new();
    for _ in 0..9 {
        let started = Instant::now();
        let result = ids(&db, std::hint::black_box(query), 1);
        std::hint::black_box(result);
        indexed_micros.push(started.elapsed().as_micros() as u64);
    }
    indexed_micros.sort_unstable();

    checkpoint(&db);
    let indexed_db_bytes = db_bytes(&db);
    let index_bytes = fts_bytes(&db);
    let indexed_rss_bytes = rss_bytes();
    replace_benchmark_projection(&db);
    let indexed_sync_wal_bytes = wal_bytes(&db_path);

    let fallback_p95_us = fallback_micros[fallback_micros.len() - 1];
    let indexed_p95_us = indexed_micros[indexed_micros.len() - 1];
    let p95_speedup = fallback_p95_us as f64 / indexed_p95_us as f64;
    let index_to_lexical_ratio = index_bytes as f64 / payload_bytes as f64;
    let write_amplification = indexed_sync_wal_bytes as f64 / fallback_sync_wal_bytes as f64;
    let rss_increase_bytes = indexed_rss_bytes.saturating_sub(base_rss_bytes);
    let budget_pass = p95_speedup >= MIN_P95_SPEEDUP
        && index_to_lexical_ratio <= MAX_INDEX_TO_LEXICAL_RATIO
        && write_amplification <= MAX_WRITE_AMPLIFICATION
        && rss_increase_bytes <= MAX_RSS_INCREASE_BYTES
        && rebuild_us <= MAX_REBUILD_US;
    println!(
        "{}",
        serde_json::json!({
            "schema_version": 1,
            "corpus_projections": 12_009,
            "query": query,
            "samples_per_lane": indexed_micros.len(),
            "ordered_result_parity": true,
            "unavailable_fts_fallback": "guide_doc_projection_fts absent -> deterministic full scan",
            "fallback_cold_us": fallback_cold_us,
            "fallback_p50_us": fallback_micros[fallback_micros.len() / 2],
            "fallback_p95_us": fallback_p95_us,
            "indexed_cold_us": indexed_cold_us,
            "indexed_p50_us": indexed_micros[indexed_micros.len() / 2],
            "indexed_p95_us": indexed_p95_us,
            "p95_speedup": p95_speedup,
            "base_db_bytes": base_db_bytes,
            "indexed_db_bytes": indexed_db_bytes,
            "lexical_payload_bytes": payload_bytes,
            "fts_index_bytes": index_bytes,
            "index_to_lexical_ratio": index_to_lexical_ratio,
            "rebuild_us": rebuild_us,
            "fallback_sync_wal_bytes": fallback_sync_wal_bytes,
            "indexed_sync_wal_bytes": indexed_sync_wal_bytes,
            "write_amplification": write_amplification,
            "base_rss_bytes": base_rss_bytes,
            "indexed_rss_bytes": indexed_rss_bytes,
            "rss_increase_bytes": rss_increase_bytes,
            "budget": {
                "min_p95_speedup": MIN_P95_SPEEDUP,
                "max_index_to_lexical_ratio": MAX_INDEX_TO_LEXICAL_RATIO,
                "max_write_amplification": MAX_WRITE_AMPLIFICATION,
                "max_rss_increase_bytes": MAX_RSS_INCREASE_BYTES,
                "max_rebuild_us": MAX_REBUILD_US,
            },
            "budget_pass": budget_pass,
            "result_ids": ["doc-benchmark"],
        })
    );
}
