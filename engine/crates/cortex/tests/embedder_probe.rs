#[cfg(feature = "fastembed")]
#[test]
fn probe_fastembedder_init_and_dim() {
    use cortex_core::Embedder;
    match cortex_core::FastEmbedder::new() {
        Ok(e) => {
            let v = e.embed("probe sentence for dimension check");
            let nonzero = v.iter().filter(|x| **x != 0.0).count();
            println!(
                "INIT OK dim={} len={} nonzero={}",
                e.dim(),
                v.len(),
                nonzero
            );
            assert!(
                nonzero > 0,
                "embed returned all zeros — runtime embed failure"
            );
        }
        Err(e) => panic!("INIT FAILED: {e}"),
    }
}
