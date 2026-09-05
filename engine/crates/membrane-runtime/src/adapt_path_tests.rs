// Included inside serve's test module to exercise its verified Taste fixture.
#[test]
fn adapt_final_packet_round_trip_binds_host_loaded_representation() {
    let store = MemoryStore::new();
    let root = tempfile::tempdir().unwrap();
    put_taste(
        &store,
        "roundtrip",
        "Use focused verification.",
        "standing_preference",
        "global",
        &[],
        "behavioral_directive",
        "active",
    );
    let request = json!({"session":"session","hostContext":{"client":"test"}});
    let mut ccs = json!({"traceId":"adapt-roundtrip","candidates":[]});
    let selection =
        crate::adapt_service::prepare_packet(&store, root.path(), &request, &mut ccs).unwrap();
    assert_eq!(ccs["candidates"].as_array().unwrap().len(), 1);
    let emitted = crate::adapt_service::finalize_packet(
        &store,
        &selection,
        &json!({"blocks":ccs["candidates"]}),
        "task",
    )
    .unwrap();
    assert_eq!(emitted["emission"]["host_loaded"], Value::Null);
    let record = &emitted["emission"]["records"][0];
    let provenance = membrane_protocol::host_observation::HostObservationProvenanceV1::new(
        "host-receipt",
        "test",
        100,
        format!("sha256:{}", "a".repeat(64)),
    );
    let observed = |value: Value| {
        serde_json::to_value(membrane_protocol::host_observation::ObservedFieldV1::complete(value))
            .unwrap()
    };
    // Exercise the public wire contract rather than constructing test-local aliases.
    let submit = json!({
        "operation":"acknowledge", "scope":selection.scope,
        "emission_receipt_id":emitted["receipt"]["receipt_id"],
        "acknowledgement":{
            "schemaVersion":1, "acknowledgementId":"ack-roundtrip",
            "packetDigest":emitted["emission"]["packet_digest"],
            "hostSerializedDigest":format!("sha256:{}", "b".repeat(64)),
            "sessionId":"session", "taskId":observed(json!("task")),
            "status":"acknowledged", "serializedBytes":observed(json!(123)),
            "acknowledgedAtUnixMs":100, "provenanceReceipt":provenance
        },
        "loaded":{
            "schemaVersion":1, "snapshotId":"loaded-roundtrip", "sessionId":"session",
            "compactionGeneration":observed(json!(0)),
            "identities":observed(json!([{"identity":record["candidate_id"], "sourceRef":record["source_ref"], "sourceDigest":format!("sha256:{}",record["representation_sha256"].as_str().unwrap())}])),
            "observedAtUnixMs":100, "provenanceReceipt":provenance
        }
    });
    let call = |value: Value| {
        crate::adapt_observations::execute(
            &store,
            serde_json::from_value(value).expect("advertised host wire contract"),
        )
    };
    assert_eq!(call(submit.clone()).unwrap()["replayed"], false);
    assert_eq!(call(submit.clone()).unwrap()["replayed"], true);
    let mut bad = submit;
    bad["loaded"]["identities"]["value"][0]["sourceDigest"] =
        json!(format!("sha256:{}", "c".repeat(64)));
    assert!(call(bad).is_err());
}

#[test]
fn adapt_packet_never_publishes_reduced_qualifier_or_retired_preference() {
    let store = MemoryStore::new();
    let root = tempfile::tempdir().unwrap();
    let memory_id = put_taste(
        &store,
        "fidelity",
        "Use focused tests.",
        "standing_preference",
        "global",
        &[],
        "behavioral_directive",
        "active",
    );
    let mut ccs = json!({"traceId":"fidelity","candidates":[]});
    let selection = crate::adapt_service::prepare_packet(
        &store,
        root.path(),
        &json!({"session":"s"}),
        &mut ccs,
    )
    .unwrap();
    let mut bad = ccs["candidates"].clone();
    bad[0]["text"] = json!("Use focused tests.");
    assert!(crate::adapt_service::finalize_packet(
        &store,
        &selection,
        &json!({"blocks":bad}),
        "task"
    )
    .unwrap_err()
    .contains("qualifier"));
    store
        .db()
        .lock()
        .execute(
            "UPDATE memories SET lifecycle_state='retired' WHERE id=?1",
            [memory_id],
        )
        .unwrap();
    assert!(crate::adapt_service::finalize_packet(
        &store,
        &selection,
        &json!({"blocks":ccs["candidates"]}),
        "task"
    )
    .unwrap_err()
    .contains("lifecycle"));
}

#[test]
fn adapt_packet_budget_omission_is_not_exposure_or_retirement() {
    let store = MemoryStore::new();
    let root = tempfile::tempdir().unwrap();
    put_taste(
        &store,
        "omitted",
        "Use focused tests.",
        "standing_preference",
        "global",
        &[],
        "behavioral_directive",
        "active",
    );
    let mut ccs = json!({"traceId":"omitted","candidates":[]});
    let selection = crate::adapt_service::prepare_packet(
        &store,
        root.path(),
        &json!({"session":"s"}),
        &mut ccs,
    )
    .unwrap();
    let out =
        crate::adapt_service::finalize_packet(&store, &selection, &json!({"blocks":[]}), "task")
            .unwrap();
    assert!(out["emission"]["records"].as_array().unwrap().is_empty());
    assert_eq!(out["emission"]["decisions"][0]["selected"], false);
    assert!(store
        .taste_delivery_inventory()
        .unwrap()
        .candidates
        .iter()
        .any(|c| c.record_id == "omitted" && c.lifecycle_eligible));
}

#[tokio::test]
async fn adapt_http_requires_token_and_complete_resident_binding() {
    use axum::body::Body;
    use tower::ServiceExt;
    let store = MemoryStore::new();
    let app = router_for_tests_with_policy(
        store.clone(),
        8765,
        Some(TEST_API_TOKEN.into()),
        Duration::from_secs(5),
        MAX_CONCURRENT_REQUESTS,
    );
    let request = || {
        axum::http::Request::post(crate::adapt_service::OPERATOR_PATH)
            .header("content-type", "application/json")
    };
    let body = || Body::from(r#"{"command":"status","scope":"global"}"#);
    assert_eq!(
        app.clone()
            .oneshot(request().body(body()).unwrap())
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        app.clone()
            .oneshot(
                request()
                    .header("authorization", format!("Bearer {TEST_API_TOKEN}"))
                    .body(body())
                    .unwrap()
            )
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    let full = request()
        .header("authorization", format!("Bearer {TEST_API_TOKEN}"))
        .header("x-membrane-installation-id", store.installation_id())
        .header("x-membrane-cortex-store-id", store.cortex_store_id())
        .header(
            "x-membrane-release-generation",
            crate::release_identity::release_generation(),
        )
        .header("x-membrane-session", store.service_instance_id());
    assert_eq!(
        app.oneshot(full.body(body()).unwrap())
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
}
