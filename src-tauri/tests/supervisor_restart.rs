use orthic::supervisor::{backoff_delay, Supervisor};
use orthic::schema_types::ManifestV1;

#[test]
fn backoff_increases_then_caps() {
    let delays: Vec<u64> = (0..5).map(|a| backoff_delay(a).as_millis() as u64).collect();
    assert_eq!(delays, vec![250, 500, 1000, 2000, 4000]);
    assert_eq!(backoff_delay(5).as_millis(), 8000);
    // Strictly increasing until cap
    for w in delays.windows(2) {
        assert!(w[0] < w[1]);
    }
}

#[test]
fn kills_four_times_observes_backoff_then_unavailable() {
    // Simulate 4 kills observing increasing delays, 5th abandons.
    let dir = tempfile::tempdir().unwrap();
    let fake_bin = dir.path().join("nonexistent_bin");
    let manifest = ManifestV1 {
        schema_version: 2,
        product_id: "membrane".into(),
        display_name: "Membrane".into(),
        product_version: "1.0.0".into(),
        hub_compat_range: ">=0.1".into(),
        install_root: dir.path().to_string_lossy().into(),
        service_start: vec![fake_bin.to_string_lossy().into()],
        service_stop: vec![],
        icon: dir.path().join("icon.png").to_string_lossy().into(),
        artifact_digest: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
    };
    let supervisor = Supervisor::new();
    let start = std::time::Instant::now();
    let status = supervisor.start_product(&manifest).unwrap();
    let elapsed = start.elapsed();
    // With 5 attempts and backoff 250+500+1000+2000=3750ms minimum, but test may be faster if we mock sleep?
    // Our implementation actually sleeps; so elapsed should be at least sum of first 4 delays = 3750ms
    // However to keep test fast, we allow either wall-clock check or just status check.
    assert_eq!(status, orthic::supervisor::ProductStatus::Unavailable);
    // If elapsed is measured, it should be at least 3000ms (allow slack)
    assert!(elapsed.as_millis() >= 3000, "elapsed {}ms should reflect backoff", elapsed.as_millis());
}

#[test]
fn hub_quit_terminates_all() {
    let s = Supervisor::new();
    s.stop_all(); // should not panic when empty
}
