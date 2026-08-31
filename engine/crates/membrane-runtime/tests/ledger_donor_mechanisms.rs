use flate2::write::DeflateEncoder;
use flate2::Compression;
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
use std::io::Write as _;

fn fixture() -> (tempfile::TempDir, LedgerDb, String, String) {
    let root = tempfile::tempdir().unwrap();
    let docs = root.path().join("docs");
    std::fs::create_dir_all(&docs).unwrap();
    std::fs::write(
        docs.join("a.md"),
        "# Root\n\nKey rotation is performed with the key tool.\n\nHow do operators rotate keys?\n\n[Unicode](目标.md#section)\n\n[Encoded](%E7%9B%AE%E6%A0%87.md#section)\n\n[Missing][missing]\n\n[missing]: missing.md\n\n<https://example.com>\n\n![diagram](image.png)\n\n[Self](#root)\n",
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
fn production_recall_expands_only_from_strong_bounded_seeds() {
    let (_root, db, source, target) = fixture();
    let receipt = doc_spine::recall_with_graph(
        &db,
        "key rotation",
        5,
        &doc_spine::LedgerRecallGraphPolicyV1::default(),
    )
    .unwrap();
    assert!(receipt.hits.iter().any(|hit| hit.doc_id == source));
    assert!(receipt
        .hits
        .iter()
        .any(|hit| hit.doc_id == target && hit.lane == "ledger_graph"));
    assert!(receipt.graph.edges.len() <= 6);

    let abstained = doc_spine::recall_with_graph(
        &db,
        "root absent",
        5,
        &doc_spine::LedgerRecallGraphPolicyV1::default(),
    )
    .unwrap();
    assert!(abstained
        .graph
        .abstentions
        .contains(&GraphExpansionAbstentionV1::NoStrongSeed));
}

#[test]
fn future_question_aliases_are_separate_weighted_and_fail_closed_on_drift() {
    let (root, db, source, _target) = fixture();
    let hits = recall_query_aliases_shadow(&db, "what is key rotation", 5).unwrap();
    let hit = hits.iter().find(|hit| hit.doc_id == source).unwrap();
    assert_eq!(hit.alias, "What is Key rotation?");
    assert_eq!(hit.weight, 1.0);
    assert_eq!(hit.derivation, "declarative_copula_is");
    assert_eq!(
        hit.evidence_quote,
        "Key rotation is performed with the key tool."
    );
    assert_eq!(hit.evidence_sha256.len(), 64);
    assert!(hit.evidence_end_byte > hit.evidence_start_byte);
    assert!(!hits
        .iter()
        .any(|candidate| candidate.alias == "How do operators rotate keys?"));
    assert_eq!(hit.span_hash.len(), 64);
    assert!(doc_spine::recall_shadow(&db, "what is key rotation", 5)
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
    assert!(!recall_query_aliases_shadow(&db, "what is key rotation", 5)
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
    assert!(!recall_query_aliases_shadow(&db, "what is key rotation", 5)
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

#[test]
fn pdf_and_docx_conversion_feed_granted_ingest_with_raw_provenance() {
    let pdf = b"%PDF-1.4\nBT (PDF guidance is preserved.) Tj ET\n/Subtype /Image\n/FlateDecode\n%%EOF".to_vec();
    let docx = stored_zip(&[
        (
            "word/document.xml",
            br#"<?xml version="1.0"?><w:document xmlns:w="x"><w:body><w:p><w:r><w:t>DOCX guidance is retained.</w:t></w:r></w:p></w:body></w:document>"# as &[u8],
        ),
        ("word/media/image1.png", b"not-admitted-media" as &[u8]),
    ]);
    let grant = DocumentConversionGrantV1::new(
        [DocumentInputFormatV1::Pdf, DocumentInputFormatV1::Docx],
        64 * 1024,
    );
    let converted_pdf = convert_granted_document(
        &grant,
        DocumentConversionInputV1 {
            source_ref: "doc://grant/import/guide.pdf".to_owned(),
            format: DocumentInputFormatV1::Pdf,
            raw_input: pdf.clone(),
        },
    )
    .unwrap();
    assert!(converted_pdf.markdown.contains("PDF guidance is preserved."));
    assert!(converted_pdf
        .omissions
        .contains(&ConversionOmissionV1::EmbeddedMediaExcluded { count: 1 }));
    assert!(converted_pdf.omissions.contains(
        &ConversionOmissionV1::CompressedPdfStreamExcluded { count: 1 }
    ));

    let converted_docx = convert_granted_document(
        &grant,
        DocumentConversionInputV1 {
            source_ref: "doc://grant/import/guide.docx".to_owned(),
            format: DocumentInputFormatV1::Docx,
            raw_input: docx.clone(),
        },
    )
    .unwrap();
    assert!(converted_docx.markdown.contains("DOCX guidance is retained."));
    assert_eq!(converted_docx.raw_input, docx);
    assert!(!converted_docx.converter.version.is_empty());
    assert!(converted_docx
        .omissions
        .iter()
        .any(|omission| matches!(omission, ConversionOmissionV1::EmbeddedMediaExcluded { count } if *count >= 1)));

    let db = LedgerDb::open_in_memory();
    let artifact = doc_spine::ingest_granted_document(
        &db,
        &grant,
        doc_spine::GrantedDocumentIngestV1 {
            repository_root: "grant://documents".to_owned(),
            repository_id: "repo-grant".to_owned(),
            revision: "revision-1".to_owned(),
            path: "imports/guide.docx".to_owned(),
            title: "Imported guide".to_owned(),
            document: DocumentConversionInputV1 {
                source_ref: "doc://grant/import/guide.docx".to_owned(),
                format: DocumentInputFormatV1::Docx,
                raw_input: converted_docx.raw_input.clone(),
            },
        },
    )
    .unwrap();
    let (stored_raw, converter, converter_version, config_digest): (Vec<u8>, String, String, String) = db
        .lock()
        .query_row(
            "SELECT raw_input,converter,converter_version,config_digest
             FROM ledger_document_conversions WHERE doc_id=?1",
            [&artifact.doc_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(stored_raw, converted_docx.raw_input);
    assert_eq!(converter, "ledger.docx-wordprocessingml");
    assert!(!converter_version.is_empty());
    assert_eq!(config_digest, grant.config_digest());
    let aliases = recall_query_aliases_shadow(&db, "what is DOCX guidance", 5).unwrap();
    assert!(aliases.iter().any(|alias| {
        alias.doc_id == artifact.doc_id && alias.evidence_quote == "DOCX guidance is retained."
    }));
    let alias = aliases
        .iter()
        .find(|alias| alias.doc_id == artifact.doc_id)
        .unwrap();
    let read = doc_spine::read_registered_section(&db, &artifact.doc_id, &alias.anchor_id, 4096)
        .unwrap();
    assert_eq!(read.raw_content_hash, artifact.content_hash);
    assert!(read.read.content.contains("DOCX guidance is retained."));
    assert!(doc_spine::recall(&db, "DOCX guidance", 5)
        .unwrap()
        .iter()
        .any(|hit| hit.doc_id == artifact.doc_id));
}

#[test]
fn docx_zip_parser_accepts_deflate_descriptors_and_rejects_malformed_archives() {
    let xml = br#"<?xml version="1.0"?><w:document xmlns:w="x"><w:body><w:p><w:r><w:t>Deflated DOCX guidance is retained.</w:t></w:r></w:p></w:body></w:document>"#;
    let docx = deflated_zip_with_descriptors(&[("word/document.xml", xml)]);
    let grant = DocumentConversionGrantV1::new(
        [DocumentInputFormatV1::Docx],
        docx.len(),
    );
    let converted = convert_granted_document(
        &grant,
        DocumentConversionInputV1 {
            source_ref: "doc://grant/import/deflated.docx".to_owned(),
            format: DocumentInputFormatV1::Docx,
            raw_input: docx.clone(),
        },
    )
    .unwrap();
    assert!(converted
        .markdown
        .contains("Deflated DOCX guidance is retained."));
    assert_eq!(converted.converter.version, "zip-wordprocessingml-v1");

    let central = docx
        .windows(4)
        .position(|window| window == b"PK\x01\x02")
        .unwrap();
    let mut method_mismatch = docx.clone();
    method_mismatch[central + 10] = 0;
    assert!(matches!(
        convert_granted_document(
            &grant,
            DocumentConversionInputV1 {
                source_ref: "doc://grant/import/method-mismatch.docx".to_owned(),
                format: DocumentInputFormatV1::Docx,
                raw_input: method_mismatch,
            },
        ),
        Err(DocumentConversionErrorV1::InvalidInput { .. })
    ));

    let eocd = docx
        .windows(4)
        .rposition(|window| window == b"PK\x05\x06")
        .unwrap();
    let mut bad_central_offset = docx.clone();
    bad_central_offset[eocd + 16] = bad_central_offset[eocd + 16].wrapping_add(1);
    assert!(matches!(
        convert_granted_document(
            &grant,
            DocumentConversionInputV1 {
                source_ref: "doc://grant/import/bad-offset.docx".to_owned(),
                format: DocumentInputFormatV1::Docx,
                raw_input: bad_central_offset,
            },
        ),
        Err(DocumentConversionErrorV1::InvalidInput { .. })
    ));

    let descriptor = docx
        .windows(4)
        .position(|window| window == b"PK\x07\x08")
        .unwrap();
    let mut trailing_data = docx.clone();
    const TRAILING_LEN: usize = 64;
    trailing_data.splice(descriptor..descriptor, [0xaa; TRAILING_LEN]);
    let shifted_descriptor = descriptor + TRAILING_LEN;
    let shifted_central = central + TRAILING_LEN;
    let shifted_eocd = eocd + TRAILING_LEN;
    let compressed_size = u32::from_le_bytes(
        trailing_data[shifted_central + 20..shifted_central + 24]
            .try_into()
            .unwrap(),
    ) + TRAILING_LEN as u32;
    trailing_data[shifted_descriptor + 8..shifted_descriptor + 12]
        .copy_from_slice(&compressed_size.to_le_bytes());
    trailing_data[shifted_central + 20..shifted_central + 24]
        .copy_from_slice(&compressed_size.to_le_bytes());
    let central_offset = u32::from_le_bytes(
        trailing_data[shifted_eocd + 16..shifted_eocd + 20]
            .try_into()
            .unwrap(),
    ) + TRAILING_LEN as u32;
    trailing_data[shifted_eocd + 16..shifted_eocd + 20]
        .copy_from_slice(&central_offset.to_le_bytes());
    let trailing_grant =
        DocumentConversionGrantV1::new([DocumentInputFormatV1::Docx], trailing_data.len());
    assert!(matches!(
        convert_granted_document(
            &trailing_grant,
            DocumentConversionInputV1 {
                source_ref: "doc://grant/import/trailing-deflate.docx".to_owned(),
                format: DocumentInputFormatV1::Docx,
                raw_input: trailing_data,
            },
        ),
        Err(DocumentConversionErrorV1::InvalidInput { .. })
    ));

    let mut forged_small_size = docx.clone();
    forged_small_size[descriptor + 12..descriptor + 16].copy_from_slice(&1u32.to_le_bytes());
    forged_small_size[central + 24..central + 28].copy_from_slice(&1u32.to_le_bytes());
    assert!(matches!(
        convert_granted_document(
            &grant,
            DocumentConversionInputV1 {
                source_ref: "doc://grant/import/forged-small-size.docx".to_owned(),
                format: DocumentInputFormatV1::Docx,
                raw_input: forged_small_size,
            },
        ),
        Err(DocumentConversionErrorV1::InvalidInput { .. })
    ));

    let mut corrupt_crc = docx.clone();
    corrupt_crc[central + 16] ^= 0xff;
    assert!(matches!(
        convert_granted_document(
            &grant,
            DocumentConversionInputV1 {
                source_ref: "doc://grant/import/corrupt-crc.docx".to_owned(),
                format: DocumentInputFormatV1::Docx,
                raw_input: corrupt_crc,
            },
        ),
        Err(DocumentConversionErrorV1::InvalidInput { .. })
    ));

    let bomb_xml = format!(
        "<?xml version=\"1.0\"?><w:document xmlns:w=\"x\"><w:body><w:p><w:r><w:t>{}</w:t></w:r></w:p></w:body></w:document>",
        "x".repeat(256 * 1024)
    );
    let bomb = deflated_zip_with_descriptors(&[("word/document.xml", bomb_xml.as_bytes())]);
    let bomb_grant = DocumentConversionGrantV1::new(
        [DocumentInputFormatV1::Docx],
        bomb.len(),
    );
    assert!(matches!(
        convert_granted_document(
            &bomb_grant,
            DocumentConversionInputV1 {
                source_ref: "doc://grant/import/bomb.docx".to_owned(),
                format: DocumentInputFormatV1::Docx,
                raw_input: bomb,
            },
        ),
        Err(DocumentConversionErrorV1::OutputTooLarge { .. })
    ));
}

fn stored_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut output = Vec::new();
    let mut central = Vec::new();
    for (name, data) in entries {
        let offset = output.len() as u32;
        let crc = crc32(data);
        output.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        output.extend_from_slice(&20u16.to_le_bytes());
        output.extend_from_slice(&0u16.to_le_bytes());
        output.extend_from_slice(&0u16.to_le_bytes());
        output.extend_from_slice(&0u16.to_le_bytes());
        output.extend_from_slice(&0u16.to_le_bytes());
        output.extend_from_slice(&crc.to_le_bytes());
        output.extend_from_slice(&(data.len() as u32).to_le_bytes());
        output.extend_from_slice(&(data.len() as u32).to_le_bytes());
        output.extend_from_slice(&(name.len() as u16).to_le_bytes());
        output.extend_from_slice(&0u16.to_le_bytes());
        output.extend_from_slice(name.as_bytes());
        output.extend_from_slice(data);

        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&(data.len() as u32).to_le_bytes());
        central.extend_from_slice(&(data.len() as u32).to_le_bytes());
        central.extend_from_slice(&(name.len() as u16).to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name.as_bytes());
    }
    let central_offset = output.len() as u32;
    output.extend_from_slice(&central);
    output.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    output.extend_from_slice(&0u16.to_le_bytes());
    output.extend_from_slice(&0u16.to_le_bytes());
    output.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    output.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    output.extend_from_slice(&(central.len() as u32).to_le_bytes());
    output.extend_from_slice(&central_offset.to_le_bytes());
    output.extend_from_slice(&0u16.to_le_bytes());
    output
}

fn deflated_zip_with_descriptors(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut output = Vec::new();
    let mut central = Vec::new();
    for (name, data) in entries {
        let offset = output.len() as u32;
        let crc = crc32(data);
        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(data).unwrap();
        let compressed = encoder.finish().unwrap();
        let flags = 0x0808u16;
        output.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        output.extend_from_slice(&20u16.to_le_bytes());
        output.extend_from_slice(&flags.to_le_bytes());
        output.extend_from_slice(&8u16.to_le_bytes());
        output.extend_from_slice(&0u16.to_le_bytes());
        output.extend_from_slice(&0u16.to_le_bytes());
        output.extend_from_slice(&0u32.to_le_bytes());
        output.extend_from_slice(&0u32.to_le_bytes());
        output.extend_from_slice(&0u32.to_le_bytes());
        output.extend_from_slice(&(name.len() as u16).to_le_bytes());
        output.extend_from_slice(&0u16.to_le_bytes());
        output.extend_from_slice(name.as_bytes());
        output.extend_from_slice(&compressed);
        output.extend_from_slice(&0x0807_4b50u32.to_le_bytes());
        output.extend_from_slice(&crc.to_le_bytes());
        output.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
        output.extend_from_slice(&(data.len() as u32).to_le_bytes());

        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&flags.to_le_bytes());
        central.extend_from_slice(&8u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
        central.extend_from_slice(&(data.len() as u32).to_le_bytes());
        central.extend_from_slice(&(name.len() as u16).to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name.as_bytes());
    }
    let central_offset = output.len() as u32;
    output.extend_from_slice(&central);
    output.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    output.extend_from_slice(&0u16.to_le_bytes());
    output.extend_from_slice(&0u16.to_le_bytes());
    output.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    output.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    output.extend_from_slice(&(central.len() as u32).to_le_bytes());
    output.extend_from_slice(&central_offset.to_le_bytes());
    output.extend_from_slice(&0u16.to_le_bytes());
    output
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320u32 & (0u32.wrapping_sub(crc & 1)));
        }
    }
    !crc
}
