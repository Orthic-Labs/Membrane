use cortex_core::{cosine, QuantizedVector};
use std::time::Instant;
use vector_bakeoff_common::{
    eligible, generate_fixture, hex, load_config, measure, parse_runner_args, quantized_candidates,
    selected_cell, write_bundle_atomic, Candidate, RunBundle, GENERATOR_ID,
};

fn main() -> Result<(), String> {
    let args = parse_runner_args()?;
    let config = load_config(&args.config)?;
    let cell = selected_cell(&config, &args.cell)?;
    let build_started = Instant::now();
    let fixture = generate_fixture(&config, cell)?;
    let hydrated = fixture
        .rows
        .iter()
        .map(|row| (row, QuantizedVector::quantize(&row.embedding).dequantize()))
        .collect::<Vec<_>>();
    let build_ns = build_started.elapsed().as_nanos().min(u64::MAX as u128) as u64;

    let mut arm_a = measure(&fixture, "A", |query, limit| {
        let mut scoped = hydrated
            .iter()
            .filter(|(row, _)| eligible(row, query))
            .map(|(row, embedding)| Candidate {
                id: row.id,
                cosine: cosine(embedding, &query.embedding),
                content_hash: hex(&row.content_hash),
            })
            .collect::<Vec<_>>();
        scoped.sort_by(|a, b| b.cosine.total_cmp(&a.cosine).then_with(|| a.id.cmp(&b.id)));
        scoped.truncate(limit);
        scoped
    });
    arm_a.backend = "current-rust-dequantized-scope-clone-full-sort".into();
    arm_a.build_ns = build_ns;
    arm_a.config = serde_json::json!({"resident":"f32","selection":"full-sort"});

    let mut arm_b = measure(&fixture, "B", |query, limit| {
        quantized_candidates(&fixture.rows, query, limit)
    });
    arm_b.backend = "optimized-rust-quantized-scope-index-bounded-topn".into();
    arm_b.build_ns = 0;
    arm_b.config =
        serde_json::json!({"resident":"i8-per-vector-scale","selection":"bounded-top-n"});

    for (a, b) in arm_a.measurements.iter().zip(&arm_b.measurements) {
        if a.query_id != b.query_id || a.candidate_ids != b.candidate_ids {
            return Err(format!("A/B parity failure at query {}", a.query_id));
        }
    }

    let bundle = RunBundle {
        schema_version: 1,
        generator_id: GENERATOR_ID.into(),
        runner: "control".into(),
        runner_version: env!("CARGO_PKG_VERSION").into(),
        cell_id: cell.id.clone(),
        fixture_sha256: fixture.sha256,
        rows: fixture.rows.len(),
        queries: fixture.queries.len(),
        dimension: config.dimension,
        arms: vec![arm_a, arm_b],
    };
    write_bundle_atomic(&args.output, &bundle)?;
    println!(
        "PASS runner=control cell={} fixture={}",
        cell.id, bundle.fixture_sha256
    );
    Ok(())
}
