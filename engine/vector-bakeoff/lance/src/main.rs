use arrow_array::types::Float32Type;
use arrow_array::{
    BooleanArray, FixedSizeListArray, Int64Array, RecordBatch, StringArray, UInt32Array,
    UInt64Array, UInt8Array,
};
use arrow_schema::{DataType, Field, Schema};
use futures::TryStreamExt;
use lancedb::index::vector::{IvfHnswFlatIndexBuilder, IvfPqIndexBuilder};
use lancedb::index::Index;
use lancedb::query::{ExecutableQuery, QueryBase};
use lancedb::{DistanceType, Table};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use vector_bakeoff_common::{
    eligible, exact_candidates, generate_fixture, hex, load_config, measure, parse_runner_args,
    selected_cell, write_bundle_atomic, Candidate, Fixture, Query, RunBundle, GENERATOR_ID,
};

fn batch(fixture: &Fixture, dimension: usize) -> Result<RecordBatch, String> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("id", DataType::UInt64, false),
        Field::new(
            "vector",
            DataType::FixedSizeList(
                Arc::new(Field::new("item", DataType::Float32, true)),
                dimension as i32,
            ),
            false,
        ),
        Field::new("scope_id", DataType::UInt32, false),
        Field::new("authority", DataType::UInt8, false),
        Field::new("active", DataType::Boolean, false),
        Field::new("effective_from_ms", DataType::Int64, false),
        Field::new("effective_until_ms", DataType::Int64, false),
        Field::new("content_hash", DataType::Utf8, false),
    ]));
    let vectors = FixedSizeListArray::from_iter_primitive::<Float32Type, _, _>(
        fixture
            .rows
            .iter()
            .map(|row| Some(row.embedding.iter().copied().map(Some).collect::<Vec<_>>())),
        dimension as i32,
    );
    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(UInt64Array::from_iter_values(
                fixture.rows.iter().map(|row| row.id),
            )),
            Arc::new(vectors),
            Arc::new(UInt32Array::from_iter_values(
                fixture.rows.iter().map(|row| row.scope_id),
            )),
            Arc::new(UInt8Array::from_iter_values(
                fixture.rows.iter().map(|row| row.authority),
            )),
            Arc::new(BooleanArray::from(
                fixture
                    .rows
                    .iter()
                    .map(|row| row.active)
                    .collect::<Vec<_>>(),
            )),
            Arc::new(Int64Array::from_iter_values(
                fixture.rows.iter().map(|row| row.effective_from_ms),
            )),
            Arc::new(Int64Array::from_iter_values(
                fixture.rows.iter().map(|row| row.effective_until_ms),
            )),
            Arc::new(StringArray::from_iter_values(
                fixture.rows.iter().map(|row| hex(&row.content_hash)),
            )),
        ],
    )
    .map_err(|error| format!("build Arrow batch: {error}"))
}

fn integer_sqrt(value: usize) -> u32 {
    let mut root = 1usize;
    while (root + 1).saturating_mul(root + 1) <= value {
        root += 1;
    }
    root.max(1).min(u32::MAX as usize) as u32
}

async fn create_tables(
    path: &Path,
    fixture: &Fixture,
    dimension: usize,
) -> Result<(Table, Table, Table, [u64; 3], u32), String> {
    if path.exists() {
        std::fs::remove_dir_all(path).map_err(|error| format!("remove stale Lance DB: {error}"))?;
    }
    let db = lancedb::connect(path.to_string_lossy().as_ref())
        .execute()
        .await
        .map_err(|error| format!("connect LanceDB: {error}"))?;
    let source = batch(fixture, dimension)?;
    let partitions = integer_sqrt(fixture.rows.len());

    let started = Instant::now();
    let exact = db
        .create_table("exact", source.clone())
        .execute()
        .await
        .map_err(|error| format!("create exact Lance table: {error}"))?;
    let exact_ns = started.elapsed().as_nanos().min(u64::MAX as u128) as u64;

    let started = Instant::now();
    let hnsw = db
        .create_table("hnsw", source.clone())
        .execute()
        .await
        .map_err(|error| format!("create HNSW Lance table: {error}"))?;
    hnsw.create_index(
        &["vector"],
        Index::IvfHnswFlat(
            IvfHnswFlatIndexBuilder::default()
                .distance_type(DistanceType::Cosine)
                .num_partitions(partitions),
        ),
    )
    .execute()
    .await
    .map_err(|error| format!("create IVF-HNSW-flat index: {error}"))?;
    let hnsw_ns = started.elapsed().as_nanos().min(u64::MAX as u128) as u64;

    let started = Instant::now();
    let pq = db
        .create_table("pq", source)
        .execute()
        .await
        .map_err(|error| format!("create PQ Lance table: {error}"))?;
    pq.create_index(
        &["vector"],
        Index::IvfPq(
            IvfPqIndexBuilder::default()
                .distance_type(DistanceType::Cosine)
                .num_partitions(partitions)
                .num_sub_vectors((dimension / 16).max(1) as u32)
                .num_bits(8),
        ),
    )
    .execute()
    .await
    .map_err(|error| format!("create IVF-PQ index: {error}"))?;
    for column in ["scope_id", "authority", "active"] {
        pq.create_index(&[column], Index::Bitmap(Default::default()))
            .execute()
            .await
            .map_err(|error| format!("create {column} bitmap index: {error}"))?;
    }
    for column in ["effective_from_ms", "effective_until_ms"] {
        pq.create_index(&[column], Index::BTree(Default::default()))
            .execute()
            .await
            .map_err(|error| format!("create {column} BTree index: {error}"))?;
    }
    let pq_ns = started.elapsed().as_nanos().min(u64::MAX as u128) as u64;
    Ok((exact, hnsw, pq, [exact_ns, hnsw_ns, pq_ns], partitions))
}

async fn query_table(
    table: &Table,
    fixture: &Fixture,
    query: &Query,
    limit: usize,
    exact: bool,
    partitions: u32,
) -> Result<Vec<Candidate>, String> {
    let pool = if exact {
        limit
    } else {
        limit.saturating_mul(4).max(128).min(fixture.rows.len())
    };
    let filter = format!(
        "scope_id < {} AND authority <= {} AND active = true AND effective_from_ms <= {} AND effective_until_ms > {}",
        query.allowed_scope_exclusive,
        query.max_authority,
        query.effective_at_ms,
        query.effective_at_ms
    );
    let mut builder = table
        .vector_search(query.embedding.clone())
        .map_err(|error| format!("prepare Lance vector query: {error}"))?
        .distance_type(DistanceType::Cosine)
        .only_if(filter)
        .limit(pool);
    if exact {
        builder = builder.bypass_vector_index();
    } else {
        builder = builder
            .nprobes(partitions.min(64) as usize)
            .refine_factor(4);
    }
    let batches = builder
        .execute()
        .await
        .map_err(|error| format!("run Lance vector query: {error}"))?
        .try_collect::<Vec<_>>()
        .await
        .map_err(|error| format!("collect Lance results: {error}"))?;
    let mut candidates = Vec::new();
    for batch in batches {
        let ids = batch
            .column_by_name("id")
            .ok_or("Lance result missing id")?
            .as_any()
            .downcast_ref::<UInt64Array>()
            .ok_or("Lance id type drift")?;
        for id in ids.values() {
            if let Some(row) = fixture
                .rows
                .get(*id as usize)
                .filter(|row| eligible(row, query))
            {
                candidates.push(Candidate {
                    id: row.id,
                    cosine: memright_core::cosine(&row.embedding, &query.embedding),
                    content_hash: hex(&row.content_hash),
                });
            }
        }
    }
    candidates.sort_by(|a, b| b.cosine.total_cmp(&a.cosine).then_with(|| a.id.cmp(&b.id)));
    candidates.dedup_by_key(|candidate| candidate.id);
    candidates.truncate(limit);
    Ok(candidates)
}

fn directory_bytes(path: &Path) -> u64 {
    std::fs::read_dir(path)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| {
            entry
                .metadata()
                .map(|metadata| {
                    if metadata.is_dir() {
                        directory_bytes(&entry.path())
                    } else {
                        metadata.len()
                    }
                })
                .unwrap_or(0)
        })
        .sum()
}

fn database_path(output: &str) -> PathBuf {
    Path::new(output).with_extension("lancedb")
}

fn mean_recall(fixture: &Fixture, arm: &vector_bakeoff_common::ArmResult) -> f64 {
    if arm.measurements.is_empty() {
        return 0.0;
    }
    let total = arm
        .measurements
        .iter()
        .map(|measurement| {
            let expected = exact_candidates(
                &fixture.rows,
                &fixture.queries[measurement.query_id as usize],
                fixture.cell.top_k,
            );
            let found = measurement
                .candidate_ids
                .iter()
                .filter(|id| expected.iter().any(|candidate| candidate.id == **id))
                .count();
            found as f64 / expected.len().max(1) as f64
        })
        .sum::<f64>();
    total / arm.measurements.len() as f64
}

fn main() -> Result<(), String> {
    let args = parse_runner_args()?;
    let config = load_config(&args.config)?;
    if config.dependencies.lancedb != "0.33.0" {
        return Err("config LanceDB version drift".into());
    }
    let cell = selected_cell(&config, &args.cell)?;
    let fixture = generate_fixture(&config, cell)?;
    let path = database_path(&args.output);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| format!("create output dir: {error}"))?;
    }
    let runtime =
        tokio::runtime::Runtime::new().map_err(|error| format!("start Tokio: {error}"))?;
    let (exact, hnsw, pq, build_ns, partitions) =
        runtime.block_on(create_tables(&path, &fixture, config.dimension))?;

    let run_arm = |name: &str, table: &Table, is_exact: bool| -> Result<_, String> {
        let mut query_error = None;
        let arm = measure(&fixture, name, |query, limit| {
            match runtime.block_on(query_table(
                table, &fixture, query, limit, is_exact, partitions,
            )) {
                Ok(candidates) => candidates,
                Err(error) => {
                    query_error = Some(error);
                    Vec::new()
                }
            }
        });
        query_error.map_or(Ok(arm), Err)
    };
    let mut arm_d = run_arm("D", &exact, true)?;
    let mut arm_e = run_arm("E", &hnsw, false)?;
    let mut arm_f = run_arm("F", &pq, false)?;
    for measurement in &arm_d.measurements {
        let expected = exact_candidates(
            &fixture.rows,
            &fixture.queries[measurement.query_id as usize],
            cell.top_k,
        )
        .into_iter()
        .map(|candidate| candidate.id)
        .collect::<Vec<_>>();
        if measurement.candidate_ids != expected {
            return Err(format!(
                "D parity failure at query {}",
                measurement.query_id
            ));
        }
    }
    let bytes = directory_bytes(&path);
    arm_d.backend = "lancedb-exact-flat".into();
    arm_d.build_ns = build_ns[0];
    arm_d.config = serde_json::json!({"version":"0.33.0","dbBytes":bytes,"prefilter":true});
    arm_e.backend = "lancedb-ivf-hnsw-flat".into();
    arm_e.exact = false;
    arm_e.build_ns = build_ns[1];
    arm_e.config = serde_json::json!({"version":"0.33.0","partitions":partitions,"m":20,"efConstruction":300,"prefilter":true,"meanRecallAtK":mean_recall(&fixture, &arm_e)});
    arm_f.backend = "lancedb-ivf-pq-scalar-prefilter".into();
    arm_f.exact = false;
    arm_f.build_ns = build_ns[2];
    arm_f.config = serde_json::json!({"version":"0.33.0","partitions":partitions,"subVectors":(config.dimension / 16).max(1),"bits":8,"nprobes":partitions.min(64),"refineFactor":4,"prefilter":true,"scalarIndexes":true,"meanRecallAtK":mean_recall(&fixture, &arm_f)});
    let bundle = RunBundle {
        schema_version: 1,
        generator_id: GENERATOR_ID.into(),
        runner: "lance".into(),
        runner_version: env!("CARGO_PKG_VERSION").into(),
        cell_id: cell.id.clone(),
        fixture_sha256: fixture.sha256,
        rows: fixture.rows.len(),
        queries: fixture.queries.len(),
        dimension: config.dimension,
        arms: vec![arm_d, arm_e, arm_f],
    };
    write_bundle_atomic(&args.output, &bundle)?;
    println!(
        "PASS runner=lance cell={} fixture={}",
        cell.id, bundle.fixture_sha256
    );
    Ok(())
}
