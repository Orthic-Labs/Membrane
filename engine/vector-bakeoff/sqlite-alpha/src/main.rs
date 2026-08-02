use rusqlite::{params, Connection};
use std::collections::HashSet;
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

extern "C" {
    fn sqlite3_vec_init();
}

fn register_extension() {
    unsafe {
        rusqlite::ffi::sqlite3_auto_extension(Some(
            std::mem::transmute::<*const (), ExtensionEntry>(sqlite3_vec_init as *const ()),
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

fn projection_path(output: &str) -> PathBuf {
    Path::new(output).with_extension("diskann.sqlite")
}

fn create_projection(
    path: &Path,
    fixture: &Fixture,
    dimension: usize,
) -> Result<(Connection, String, u64), String> {
    if path.exists() {
        std::fs::remove_file(path).map_err(|e| format!("remove stale DiskANN projection: {e}"))?;
    }
    let started = Instant::now();
    let mut conn = Connection::open(path).map_err(|e| format!("open DiskANN projection: {e}"))?;
    let version: String = conn
        .query_row("SELECT vec_version()", [], |row| row.get(0))
        .map_err(|e| format!("sqlite-vec alpha registration failed: {e}"))?;
    if version.trim_start_matches('v') != "0.1.10-alpha.4" {
        return Err(format!(
            "sqlite-vec alpha drift: expected 0.1.10-alpha.4, got {version}"
        ));
    }
    conn.execute_batch(&format!(
        "PRAGMA journal_mode=WAL;
         CREATE VIRTUAL TABLE g USING vec0(
           memory_id INTEGER PRIMARY KEY,
           embedding float[{dimension}] distance_metric=cosine indexed by diskann(
             neighbor_quantizer=binary,
             n_neighbors=48,
             search_list_size_insert=96,
             search_list_size_search=512
           )
         );"
    ))
    .map_err(|e| format!("create DiskANN projection: {e}"))?;
    let tx = conn
        .transaction()
        .map_err(|e| format!("begin DiskANN projection: {e}"))?;
    {
        let mut statement = tx
            .prepare("INSERT INTO g(memory_id, embedding) VALUES (?1, ?2)")
            .map_err(|e| format!("prepare DiskANN insert: {e}"))?;
        for row in &fixture.rows {
            statement
                .execute(params![row.id as i64, vector_blob(&row.embedding)])
                .map_err(|e| format!("insert DiskANN row {}: {e}", row.id))?;
        }
    }
    tx.commit()
        .map_err(|e| format!("commit DiskANN projection: {e}"))?;
    let build_ns = started.elapsed().as_nanos().min(u64::MAX as u128) as u64;
    Ok((conn, version, build_ns))
}

fn query_projection(
    conn: &Connection,
    fixture: &Fixture,
    query: &Query,
    limit: usize,
) -> Result<Vec<Candidate>, String> {
    let pool = limit.saturating_mul(4).max(limit).min(fixture.rows.len());
    let mut statement = conn
        .prepare_cached("SELECT memory_id FROM g WHERE embedding MATCH ?1 AND k = ?2")
        .map_err(|e| format!("prepare DiskANN query: {e}"))?;
    let ids = statement
        .query_map(params![vector_blob(&query.embedding), pool as i64], |row| {
            row.get::<_, i64>(0)
        })
        .map_err(|e| format!("run DiskANN query: {e}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("collect DiskANN query: {e}"))?;
    let mut candidates = ids
        .into_iter()
        .filter_map(|id| fixture.rows.get(id as usize))
        .filter(|row| eligible(row, query))
        .map(|row| Candidate {
            id: row.id,
            cosine: crypt_core::cosine(&row.embedding, &query.embedding),
            content_hash: hex(&row.content_hash),
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|a, b| b.cosine.total_cmp(&a.cosine).then_with(|| a.id.cmp(&b.id)));
    candidates.truncate(limit);
    Ok(candidates)
}

fn main() -> Result<(), String> {
    register_extension();
    let args = parse_runner_args()?;
    let config = load_config(&args.config)?;
    if config.dependencies.sqlite_vec_alpha != "0.1.10-alpha.4" {
        return Err("config sqlite-vec alpha version drift".into());
    }
    let cell = selected_cell(&config, &args.cell)?;
    let fixture = generate_fixture(&config, cell)?;
    let path = projection_path(&args.output);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create projection dir: {e}"))?;
    }
    let (conn, version, build_ns) = create_projection(&path, &fixture, config.dimension)?;
    let mut query_error = None;
    let mut arm = measure(&fixture, "G", |query, limit| {
        match query_projection(&conn, &fixture, query, limit) {
            Ok(value) => value,
            Err(error) => {
                query_error = Some(error);
                Vec::new()
            }
        }
    });
    if let Some(error) = query_error {
        return Err(error);
    }
    let mut recall_sum = 0.0_f64;
    for measurement in &arm.measurements {
        let query = &fixture.queries[measurement.query_id as usize];
        let expected = exact_candidates(&fixture.rows, query, cell.top_k)
            .into_iter()
            .map(|candidate| candidate.id)
            .collect::<HashSet<_>>();
        let hits = measurement
            .candidate_ids
            .iter()
            .filter(|id| expected.contains(id))
            .count();
        recall_sum += hits as f64 / expected.len().max(1) as f64;
    }
    let mean_recall = recall_sum / arm.measurements.len().max(1) as f64;
    let db_bytes = std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
    arm.backend = "sqlite-vec-alpha-diskann".into();
    arm.exact = false;
    arm.research_only = true;
    arm.build_ns = build_ns;
    arm.config = serde_json::json!({
        "version": version,
        "dispositionCeiling": "research-only; never production-eligible",
        "neighborQuantizer": "binary",
        "nNeighbors": 48,
        "searchListSizeInsert": 96,
        "searchListSizeSearch": 512,
        "prefilter": false,
        "meanRecallAtK": mean_recall,
        "dbBytes": db_bytes
    });
    let bundle = RunBundle {
        schema_version: 1,
        generator_id: GENERATOR_ID.into(),
        runner: "sqlite-alpha".into(),
        runner_version: env!("CARGO_PKG_VERSION").into(),
        cell_id: cell.id.clone(),
        fixture_sha256: fixture.sha256,
        rows: fixture.rows.len(),
        queries: fixture.queries.len(),
        dimension: config.dimension,
        arms: vec![arm],
    };
    write_bundle_atomic(&args.output, &bundle)?;
    println!(
        "PASS runner=sqlite-alpha cell={} fixture={}",
        cell.id, bundle.fixture_sha256
    );
    Ok(())
}
