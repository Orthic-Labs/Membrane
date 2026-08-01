use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};
use std::time::Instant;
use vector_bakeoff_common::{
    eligible, exact_candidates, generate_fixture, hex, load_config, measure, parse_runner_args,
    selected_cell, write_bundle_atomic, Candidate, Fixture, Query, RunBundle, GENERATOR_ID,
};

type ExtensionEntry = unsafe extern "C" fn(
    *mut rusqlite::ffi::sqlite3,
    *mut *mut std::ffi::c_char,
    *const rusqlite::ffi::sqlite3_api_routines,
) -> std::ffi::c_int;

fn register_extension() {
    unsafe {
        rusqlite::ffi::sqlite3_auto_extension(Some(
            std::mem::transmute::<*const (), ExtensionEntry>(
                sqlite_vec_stable::sqlite3_vec_init as *const (),
            ),
        ));
    }
}

fn vector_blob(values: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(values.len() * 4);
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

fn create_projection(
    path: &Path,
    fixture: &Fixture,
    dimension: usize,
) -> Result<(Connection, String, u64), String> {
    if path.exists() {
        std::fs::remove_file(path).map_err(|e| format!("remove stale projection: {e}"))?;
    }
    let started = Instant::now();
    let mut conn = Connection::open(path).map_err(|e| format!("open sqlite projection: {e}"))?;
    let version: String = conn
        .query_row("SELECT vec_version()", [], |row| row.get(0))
        .map_err(|e| format!("sqlite-vec registration failed: {e}"))?;
    if version.trim_start_matches('v') != "0.1.9" {
        return Err(format!("sqlite-vec drift: expected 0.1.9, got {version}"));
    }
    conn.execute_batch(&format!(
        "PRAGMA journal_mode=WAL;
         CREATE VIRTUAL TABLE c1 USING vec0(
           memory_id INTEGER PRIMARY KEY,
           embedding float[{dimension}] distance_metric=cosine,
           scope_id INTEGER,
           authority INTEGER,
           active BOOLEAN,
           effective_from_ms INTEGER,
           effective_until_ms INTEGER
         );
         CREATE VIRTUAL TABLE c2 USING vec0(
           memory_id INTEGER PRIMARY KEY,
           embedding float[{dimension}] distance_metric=cosine,
           scope_id INTEGER partition key,
           authority INTEGER,
           active BOOLEAN,
           effective_from_ms INTEGER,
           effective_until_ms INTEGER
         );"
    ))
    .map_err(|e| format!("create vec0 projections: {e}"))?;
    let tx = conn
        .transaction()
        .map_err(|e| format!("begin projection: {e}"))?;
    for table in ["c1", "c2"] {
        let sql = format!("INSERT INTO {table}(memory_id, embedding, scope_id, authority, active, effective_from_ms, effective_until_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)");
        let mut statement = tx
            .prepare(&sql)
            .map_err(|e| format!("prepare projection insert: {e}"))?;
        for row in &fixture.rows {
            statement
                .execute(params![
                    row.id as i64,
                    vector_blob(&row.embedding),
                    row.scope_id as i64,
                    row.authority as i64,
                    row.active,
                    row.effective_from_ms,
                    row.effective_until_ms
                ])
                .map_err(|e| format!("insert {table} row {}: {e}", row.id))?;
        }
    }
    tx.commit().map_err(|e| format!("commit projection: {e}"))?;
    let build_ns = started.elapsed().as_nanos().min(u64::MAX as u128) as u64;
    Ok((conn, version, build_ns))
}

fn query_projection(
    conn: &Connection,
    table: &str,
    fixture: &Fixture,
    query: &Query,
    limit: usize,
) -> Result<Vec<Candidate>, String> {
    let sql = format!(
        "SELECT memory_id FROM {table}
         WHERE embedding MATCH ?1 AND k = ?2
           AND scope_id < ?3 AND authority <= ?4 AND active = 1
           AND effective_from_ms <= ?5 AND effective_until_ms > ?5"
    );
    let pool = limit.max(128).min(fixture.rows.len());
    let mut statement = conn
        .prepare_cached(&sql)
        .map_err(|e| format!("prepare {table} query: {e}"))?;
    let ids = statement
        .query_map(
            params![
                vector_blob(&query.embedding),
                pool as i64,
                query.allowed_scope_exclusive as i64,
                query.max_authority as i64,
                query.effective_at_ms
            ],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|e| format!("run {table} query: {e}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("collect {table} query: {e}"))?;
    let mut candidates = ids
        .into_iter()
        .filter_map(|id| fixture.rows.get(id as usize))
        .filter(|row| eligible(row, query))
        .map(|row| Candidate {
            id: row.id,
            cosine: memright_core::cosine(&row.embedding, &query.embedding),
            content_hash: hex(&row.content_hash),
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|a, b| b.cosine.total_cmp(&a.cosine).then_with(|| a.id.cmp(&b.id)));
    candidates.truncate(limit);
    Ok(candidates)
}

fn projection_path(output: &str) -> PathBuf {
    let output = Path::new(output);
    output.with_extension("sqlite")
}

fn main() -> Result<(), String> {
    register_extension();
    let args = parse_runner_args()?;
    let config = load_config(&args.config)?;
    if config.dependencies.sqlite_vec_stable != "0.1.9" {
        return Err("config sqlite-vec stable version drift".into());
    }
    let cell = selected_cell(&config, &args.cell)?;
    let fixture = generate_fixture(&config, cell)?;
    let path = projection_path(&args.output);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create projection dir: {e}"))?;
    }
    let (conn, version, build_ns) = create_projection(&path, &fixture, config.dimension)?;

    let mut c1_error = None;
    let mut arm_c1 = measure(&fixture, "C1", |query, limit| {
        match query_projection(&conn, "c1", &fixture, query, limit) {
            Ok(value) => value,
            Err(error) => {
                c1_error = Some(error);
                Vec::new()
            }
        }
    });
    if let Some(error) = c1_error {
        return Err(error);
    }
    let mut c2_error = None;
    let mut arm_c2 = measure(&fixture, "C2", |query, limit| {
        match query_projection(&conn, "c2", &fixture, query, limit) {
            Ok(value) => value,
            Err(error) => {
                c2_error = Some(error);
                Vec::new()
            }
        }
    });
    if let Some(error) = c2_error {
        return Err(error);
    }
    for arm in [&arm_c1, &arm_c2] {
        for measurement in &arm.measurements {
            let query = &fixture.queries[measurement.query_id as usize];
            let expected = exact_candidates(&fixture.rows, query, cell.top_k)
                .into_iter()
                .map(|candidate| candidate.id)
                .collect::<Vec<_>>();
            if measurement.candidate_ids != expected {
                return Err(format!("{} parity failure at query {}", arm.arm, query.id));
            }
        }
    }
    let db_bytes = std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
    arm_c1.backend = "sqlite-vec-stable-metadata-filtered".into();
    arm_c1.build_ns = build_ns;
    arm_c1.config = serde_json::json!({"version":version,"partitioned":false,"dbBytes":db_bytes});
    arm_c2.backend = "sqlite-vec-stable-scope-partitioned".into();
    arm_c2.build_ns = build_ns;
    arm_c2.config = serde_json::json!({"version":version,"partitioned":true,"dbBytes":db_bytes});
    let bundle = RunBundle {
        schema_version: 1,
        generator_id: GENERATOR_ID.into(),
        runner: "sqlite-stable".into(),
        runner_version: env!("CARGO_PKG_VERSION").into(),
        cell_id: cell.id.clone(),
        fixture_sha256: fixture.sha256,
        rows: fixture.rows.len(),
        queries: fixture.queries.len(),
        dimension: config.dimension,
        arms: vec![arm_c1, arm_c2],
    };
    write_bundle_atomic(&args.output, &bundle)?;
    println!(
        "PASS runner=sqlite-stable cell={} fixture={}",
        cell.id, bundle.fixture_sha256
    );
    Ok(())
}
