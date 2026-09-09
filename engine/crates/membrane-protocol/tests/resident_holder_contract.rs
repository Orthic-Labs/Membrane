use membrane_protocol::{
    ResidentControllerIdentityV1, ResidentHolderOperationV1, ResidentHolderRequestV1,
    RESIDENT_HOLDER_SCHEMA_VERSION,
};

#[test]
fn holder_request_is_versioned_and_closed() {
    let request = ResidentHolderRequestV1 {
        schema_version: RESIDENT_HOLDER_SCHEMA_VERSION,
        operation: ResidentHolderOperationV1::Status,
        controller: ResidentControllerIdentityV1 {
            installation_id: "install-1".into(),
            cortex_store_id: "store-1".into(),
            release_generation: "release-1".into(),
            startup_generation: 1,
            stable_current: r"C:\Membrane\current".into(),
        },
        holder: None,
        expires_at_unix_ms: None,
        observed_at_unix_ms: 1,
        loss_cursor: None,
    };
    let json = serde_json::to_value(&request).unwrap();
    assert_eq!(json["operation"], "status");
    assert_eq!(serde_json::from_value::<ResidentHolderRequestV1>(json).unwrap(), request);
}
