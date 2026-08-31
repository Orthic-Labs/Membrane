use membrane_runtime::ledger::{
    doc_spine,
    document_conversion::{
        convert_granted_document, ConversionLossV1, ConversionOmissionV1,
        DocumentConversionErrorV1, DocumentConversionGrantV1, DocumentConversionInputV1,
        DocumentInputFormatV1,
    },
    link_projection::{
        expand_strong_seed_graph, link_targets, GraphExpansionAbstentionV1,
        LedgerGraphExpansionPolicyV1, LedgerGraphSeedV1, LedgerLinkKindV1,
        LinkResolutionStateV1,
    },
    query_alias::recall_query_aliases_shadow,
    LedgerDb,
};

fn fixture() -> (tempfile::TempDir, LedgerDb, String, String) {
    let root = tempfile::tempdir().unwrap();
    let docs = root.path().join("docs");
    std::fs::create_dir_all(&docs).unwrap();
    std::fs::write(
        docs.join("a.md"),
        "# Root\n\nHow do operators rotate keys?\n\n[Unicode](目标.md#section)\n\n[Encoded](%E7%9B%AE%E6%A0%87.md#section)\n\n[Missing][missing]\n\n[missing]: missing.md\n\n<https://example.com>\n\n![diagram](image.png)\n\n[Self](#root)\n",
    )
    .unwrap();
    std::fs::write(
        docs.join("目标.md"),
        "# Section\n\n[Back](a.md#root)\n",
    )
    .unwrap();
    let db = LedgerDb::open_in_memory();
    doc_spine::sync(&db, root.path()).unwrap();
    let (source, target) = {
        let conn = db.lock();
        let source = conn
            .query_row(
                "SELECT doc_id FROM ledger_doc_artifacts WHERE path='docs/a.md'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap();
        let target = conn
            .query_row(
                "SELECT doc_id FROM ledger_doc_artifacts WHERE path='docs/目标.md'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap();
        (source, target)
    };
    (root, db, source, target)
}

#[test]
fn links_are_typed_normalized_and_provenance_bound() {
    let (_root, db, source, target) = fixture();
    let links = link_targets(&db, &source).unwrap();
    assert_eq!(links.len(), 6);

    let unicode = links
        .iter()
        .find(|link| link.raw_target.contains("目标"))
        .unwrap();
    assert_eq!(unicode.link_kind, LedgerLinkKindV1::Inline);
    assert_eq!(unicode.normalized_target, "docs/目标.md");
    assert_eq!(unicode.target_fragment.as_deref(), Some("section"));
    assert_eq!(unicode.resolution_state, LinkResolutionStateV1::Resolved);
    assert_eq!(unicode.target_doc_id.as_deref(), Some(target.as_str()));
    assert_eq!(unicode.source_span_hash.len(), 64);
    assert_eq!(unicode.target_span_hash.as_ref().unwrap().len(), 64);
    let encoded = links
        .iter()
        .find(|link| link.raw_target.starts_with("%E7"))
        .unwrap();
    assert_eq!(encoded.normalized_target, "docs/目标.md");
    assert_eq!(encoded.resolution_state, LinkResolutionStateV1::Resolved);

    let reference = links
        .iter()
        .find(|link| link.raw_target == "missing.md")
        .unwrap();
    assert_eq!(reference.link_kind, LedgerLinkKindV1::Reference);
    assert_eq!(reference.resolution_state, LinkResolutionStateV1::Broken);

    let external = links
        .iter()
        .find(|link| link.raw_target == "https://example.com")
        .unwrap();
    assert_eq!(external.link_kind, LedgerLinkKindV1::Autolink);
    assert_eq!(external.resolution_state, LinkResolutionStateV1::External);

    let image = links
        .iter()
        .find(|link| link.raw_target == "image.png")
        .unwrap();
    assert_eq!(image.link_kind, LedgerLinkKindV1::Image);
    assert_eq!(image.resolution_state, LinkResolutionStateV1::MediaExcluded);
}

#[test]
fn graph_expansion_requires_strong_seeds_and_observes_every_cap() {
    let (root, db, source, _target) = fixture();
    let policy = LedgerGraphExpansionPolicyV1 {
        min_seed_strength: 0.8,
        max_hops: 3,
        max_nodes: 8,
        max_edges: 8,
    };
    let weak = expand_strong_seed_graph(
        &db,
        &[LedgerGraphSeedV1 {
            doc_id: source.clone(),
            strength: 0.79,
        }],
        &policy,
    )
    .unwrap();
    assert_eq!(
        weak.abstentions,
        vec![GraphExpansionAbstentionV1::NoStrongSeed]
    );

    let strong_seed = [LedgerGraphSeedV1 {
        doc_id: source.clone(),
        strength: 0.95,
    }];
    let expanded = expand_strong_seed_graph(&db, &strong_seed, &policy).unwrap();
    assert_eq!(expanded.node_ids.len(), 2);
    assert!(expanded.edges.iter().all(|edge| {
        edge.source_span_hash.len() == 64
            && edge.target_content_hash.len() == 64
            && edge.target_span_hash.len() == 64
    }));
    assert!(expanded
        .abstentions
        .contains(&GraphExpansionAbstentionV1::Cycle));

    let hop = expand_strong_seed_graph(
        &db,
        &strong_seed,
        &LedgerGraphExpansionPolicyV1 {
            max_hops: 0,
            ..policy.clone()
        },
    )
    .unwrap();
    assert!(hop
        .abstentions
        .contains(&GraphExpansionAbstentionV1::HopCap));

    let nodes = expand_strong_seed_graph(
        &db,
        &strong_seed,
        &LedgerGraphExpansionPolicyV1 {
            max_nodes: 1,
            ..policy.clone()
        },
    )
    .unwrap();
    assert!(nodes
        .abstentions
        .contains(&GraphExpansionAbstentionV1::NodeCap));

    let edges = expand_strong_seed_graph(
        &db,
        &strong_seed,
        &LedgerGraphExpansionPolicyV1 {
            max_edges: 1,
            ..policy.clone()
        },
    )
    .unwrap();
    assert!(edges
        .abstentions
        .contains(&GraphExpansionAbstentionV1::EdgeCap));

    std::fs::write(root.path().join("docs/目标.md"), "# Drifted\n").unwrap();
    let drifted = expand_strong_seed_graph(&db, &strong_seed, &policy).unwrap();
    assert!(drifted
        .abstentions
        .contains(&GraphExpansionAbstentionV1::SourceDrift));
}

#[test]
fn future_question_aliases_are_separate_weighted_and_fail_closed_on_drift() {
    let (root, db, source, _target) = fixture();
    let hits = recall_query_aliases_shadow(&db, "operators rotate keys", 5).unwrap();
    let hit = hits.iter().find(|hit| hit.doc_id == source).unwrap();
    assert_eq!(hit.alias, "How do operators rotate keys?");
    assert_eq!(hit.weight, 1.0);
    assert_eq!(hit.derivation, "source_question");
    assert_eq!(hit.span_hash.len(), 64);
    assert!(doc_spine::recall_shadow(&db, "operators rotate keys", 5)
        .unwrap()
        .alias_hits
        .iter()
        .any(|candidate| candidate.doc_id == source));

    db.lock()
        .execute(
            "UPDATE ledger_nodes SET span_hash='drift' WHERE doc_id=?1",
            [&source],
        )
        .unwrap();
    assert!(!recall_query_aliases_shadow(&db, "operators rotate keys", 5)
        .unwrap()
        .iter()
        .any(|candidate| candidate.doc_id == source));
    db.lock()
        .execute(
            "UPDATE ledger_nodes SET span_hash=?1 WHERE doc_id=?2",
            rusqlite::params![hit.span_hash, source],
        )
        .unwrap();
    std::fs::write(root.path().join("docs/a.md"), "# Changed\n").unwrap();
    assert!(!recall_query_aliases_shadow(&db, "operators rotate keys", 5)
        .unwrap()
        .iter()
        .any(|candidate| candidate.doc_id == source));
}

#[test]
fn conversion_is_grant_gated_raw_retaining_hash_bound_and_media_excluding() {
    let raw = b"<h1>Guide</h1><p>Use &amp; verify.</p><img src='secret.png'>".to_vec();
    let input = || DocumentConversionInputV1 {
        source_ref: "doc://grant/import/guide.html".to_owned(),
        format: DocumentInputFormatV1::Html,
        raw_input: raw.clone(),
    };
    assert_eq!(
        convert_granted_document(&DocumentConversionGrantV1::denied(), input()).unwrap_err(),
        DocumentConversionErrorV1::NotGranted
    );

    let grant = DocumentConversionGrantV1::new(
        [
            DocumentInputFormatV1::Html,
            DocumentInputFormatV1::PlainText,
        ],
        4096,
    );
    let converted = convert_granted_document(&grant, input()).unwrap();
    assert_eq!(converted.raw_input, raw);
    assert_eq!(converted.raw_sha256.len(), 64);
    assert_eq!(converted.markdown_sha256.len(), 64);
    assert_eq!(converted.converter.converter, "ledger.html-text");
    assert_eq!(converted.converter.version, "1");
    assert_eq!(converted.converter.config_digest, grant.config_digest());
    assert!(converted.markdown.contains("Use & verify."));
    assert!(!converted.markdown.contains("secret.png"));
    assert!(converted
        .losses
        .contains(&ConversionLossV1::FormattingFlattened));
    assert!(converted
        .omissions
        .contains(&ConversionOmissionV1::EmbeddedMediaExcluded { count: 1 }));
    let lossy = convert_granted_document(
        &grant,
        DocumentConversionInputV1 {
            source_ref: "doc://grant/import/lossy.txt".to_owned(),
            format: DocumentInputFormatV1::PlainText,
            raw_input: vec![b'a', 0xff],
        },
    )
    .unwrap();
    assert_eq!(
        lossy.losses,
        vec![ConversionLossV1::Utf8Replacement {
            replacement_count: 1
        }]
    );

    let media = convert_granted_document(
        &grant,
        DocumentConversionInputV1 {
            source_ref: "doc://grant/import/audio.mp3".to_owned(),
            format: DocumentInputFormatV1::Media("audio/mpeg".to_owned()),
            raw_input: vec![0, 1, 2],
        },
    )
    .unwrap_err();
    assert_eq!(
        media,
        DocumentConversionErrorV1::MediaExcluded {
            media_type: "audio/mpeg".to_owned()
        }
    );
}
