use crate::{
    HostVectorPolicy, MemoryEntry, MemoryRegistry, MemoryTier, VectorIndex, VectorKernel,
    VectorSearchError,
};

fn entry(id: &str, scope: &str, embedding: Option<Vec<f32>>) -> MemoryEntry {
    MemoryEntry {
        id: id.to_string(),
        tier: MemoryTier::Working,
        content: id.to_string(),
        keywords: Vec::new(),
        score: 0.0,
        created_at: "2026-08-02T00:00:00Z".to_string(),
        access_count: 0,
        embedding,
        scope_id: scope.to_string(),
    }
}

#[test]
fn lifecycle_keeps_stable_ids_and_advances_generation() {
    let mut index = VectorIndex::new();
    assert_eq!(index.generation(), 0);

    index.upsert(&entry("a", "one", Some(vec![1.0, 0.0])));
    index.upsert(&entry("b", "two", Some(vec![0.0, 1.0])));
    assert_eq!(index.generation(), 2);
    assert_eq!(index.row_count(), 2);
    assert_eq!(index.dimension(), Some(2));
    assert_eq!(index.resident_bytes(), 16);

    index.upsert(&entry("a", "two", Some(vec![0.0, 2.0])));
    let hits = index.top_k(&[0.0, 1.0], None, 2).unwrap();
    assert_eq!(
        hits.iter().map(|hit| hit.id.as_str()).collect::<Vec<_>>(),
        ["a", "b"]
    );
    assert_eq!(index.generation(), 3);

    assert!(index.remove("a"));
    assert_eq!(index.row_count(), 1);
    assert_eq!(index.generation(), 4);
    assert!(!index.remove("missing"));
    assert_eq!(index.generation(), 4);
}

#[test]
fn concurrent_queries_share_one_immutable_projection() {
    let mut index = VectorIndex::new();
    for row in 0..4_096 {
        index.upsert(&entry(
            &format!("row-{row:04}"),
            "scope",
            Some(vec![row as f32 + 1.0, 1.0, -1.0, 0.5]),
        ));
    }
    let generation = index.generation();
    let resident_bytes = index.resident_bytes();

    std::thread::scope(|scope| {
        let handles = (0..8)
            .map(|_| scope.spawn(|| index.top_k(&[1.0, 0.0, 0.0, 0.0], None, 8).unwrap()))
            .collect::<Vec<_>>();
        for handle in handles {
            assert_eq!(handle.join().unwrap().len(), 8);
        }
    });

    assert_eq!(index.generation(), generation);
    assert_eq!(index.resident_bytes(), resident_bytes);
}

#[test]
fn scope_filter_never_returns_ineligible_rows() {
    let mut index = VectorIndex::new();
    index.upsert(&entry("a", "alpha", Some(vec![1.0, 0.0, 0.0])));
    index.upsert(&entry("b", "beta", Some(vec![0.9, 0.1, 0.0])));
    index.upsert(&entry("c", "alpha", Some(vec![0.8, 0.2, 0.0])));

    let scopes = ["beta"];
    let hits = index.top_k(&[1.0, 0.0, 0.0], Some(&scopes), 3).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, "b");
}

#[test]
fn indexed_order_matches_scalar_cosine_across_tail_dimensions() {
    for dimension in [1, 8, 13, 16, 37, 768] {
        let mut index = VectorIndex::new();
        let query = (0..dimension)
            .map(|i| ((i * 17 % 31) as f32 - 15.0) / 11.0)
            .collect::<Vec<_>>();
        let rows = (0..7)
            .map(|row| {
                (0..dimension)
                    .map(|i| (((row + 3) * (i + 5) % 43) as f32 - 21.0) / 13.0)
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        for (row, embedding) in rows.iter().enumerate() {
            index.upsert(&entry(
                &format!("row-{row}"),
                "scope",
                Some(embedding.clone()),
            ));
        }

        let mut expected = rows
            .iter()
            .enumerate()
            .map(|(row, embedding)| {
                let dot = embedding
                    .iter()
                    .zip(&query)
                    .map(|(a, b)| a * b)
                    .sum::<f32>();
                let left = embedding.iter().map(|v| v * v).sum::<f32>().sqrt();
                let right = query.iter().map(|v| v * v).sum::<f32>().sqrt();
                (format!("row-{row}"), dot / (left * right))
            })
            .collect::<Vec<_>>();
        expected.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

        let actual = index.top_k(&query, None, rows.len()).unwrap();
        assert_eq!(
            actual.iter().map(|hit| hit.id.as_str()).collect::<Vec<_>>(),
            expected
                .iter()
                .map(|(id, _)| id.as_str())
                .collect::<Vec<_>>(),
            "dimension {dimension}"
        );
    }
}

#[test]
fn dimension_mismatch_fails_closed() {
    let mut index = VectorIndex::new();
    index.upsert(&entry("a", "scope", Some(vec![1.0, 0.0])));
    assert_eq!(
        index.top_k(&[1.0, 0.0, 0.0], None, 1),
        Err(VectorSearchError::DimensionMismatch {
            expected: 2,
            actual: 3,
        })
    );
}

#[test]
fn heterogeneous_stored_dimensions_fail_closed() {
    let mut index = VectorIndex::new();
    index.upsert(&entry("a", "scope", Some(vec![1.0, 0.0])));
    index.upsert(&entry("b", "scope", Some(vec![1.0, 0.0, 0.0])));

    assert_eq!(
        index.top_k(&[1.0, 0.0], None, 1),
        Err(VectorSearchError::HeterogeneousDimensions { incompatible: 1 })
    );
}

#[test]
fn default_registry_keeps_projection_disabled() {
    let mut fallback = MemoryRegistry::new();
    fallback.insert(entry("a", "scope", Some(vec![1.0, 0.0])));
    assert!(fallback.vector_index().is_none());

    let mut indexed = MemoryRegistry::new_indexed();
    indexed.insert(entry("a", "scope", Some(vec![1.0, 0.0])));
    assert_eq!(indexed.vector_index().unwrap().row_count(), 1);
}

#[test]
fn dispatch_boundaries_match_measured_host_thresholds() {
    assert_eq!(
        HostVectorPolicy::Mac.kernel(4095, 4095),
        VectorKernel::ScalarFull
    );
    assert_eq!(
        HostVectorPolicy::Mac.kernel(4096, 1023),
        VectorKernel::Gather
    );
    assert_eq!(
        HostVectorPolicy::Mac.kernel(4096, 1024),
        VectorKernel::ParallelFull
    );
    assert_eq!(
        HostVectorPolicy::Windows.kernel(4096, 2047),
        VectorKernel::Gather
    );
    assert_eq!(
        HostVectorPolicy::Windows.kernel(4095, 4095),
        VectorKernel::Gather
    );
    assert_eq!(
        HostVectorPolicy::Windows.kernel(4096, 2048),
        VectorKernel::ParallelFull
    );
    assert_eq!(
        HostVectorPolicy::Portable.kernel(100_000, 100_000),
        VectorKernel::ScalarFull
    );
}
