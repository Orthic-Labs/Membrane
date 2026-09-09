//! LDG-028 public format matrix: conversion is deterministic, bounded, and source-bound.
use membrane_runtime::ledger::{doc_spine, document_conversion::*, LedgerDb};

fn grant(format: DocumentInputFormatV1, size: usize) -> DocumentConversionGrantV1 {
    DocumentConversionGrantV1::new([format], size)
}

fn stored_zip(name: &str, data: &[u8]) -> Vec<u8> {
    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = 0xffff_ffffu32;
        for byte in bytes { crc ^= u32::from(*byte); for _ in 0..8 { crc = (crc >> 1) ^ (0xedb8_8320 & (0u32.wrapping_sub(crc & 1))); } }
        !crc
    }
    let crc = crc32(data); let mut out = Vec::new();
    out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
    out.extend_from_slice(&20u16.to_le_bytes()); out.extend_from_slice(&[0; 8]);
    out.extend_from_slice(&crc.to_le_bytes()); out.extend_from_slice(&(data.len() as u32).to_le_bytes()); out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&(name.len() as u16).to_le_bytes()); out.extend_from_slice(&0u16.to_le_bytes()); out.extend_from_slice(name.as_bytes()); out.extend_from_slice(data);
    let central_offset = out.len() as u32; out.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
    out.extend_from_slice(&20u16.to_le_bytes()); out.extend_from_slice(&20u16.to_le_bytes()); out.extend_from_slice(&[0; 8]); out.extend_from_slice(&crc.to_le_bytes());
    out.extend_from_slice(&(data.len() as u32).to_le_bytes()); out.extend_from_slice(&(data.len() as u32).to_le_bytes()); out.extend_from_slice(&(name.len() as u16).to_le_bytes()); out.extend_from_slice(&[0; 12]); out.extend_from_slice(&central_offset.saturating_sub(central_offset).to_le_bytes()); out.extend_from_slice(name.as_bytes());
    let central_size = (out.len() as u32).saturating_sub(central_offset); out.extend_from_slice(&0x0605_4b50u32.to_le_bytes()); out.extend_from_slice(&[0; 4]); out.extend_from_slice(&1u16.to_le_bytes()); out.extend_from_slice(&1u16.to_le_bytes()); out.extend_from_slice(&central_size.to_le_bytes()); out.extend_from_slice(&central_offset.to_le_bytes()); out.extend_from_slice(&0u16.to_le_bytes()); out
}

#[test]
fn advertised_formats_have_stable_hashes_and_provenance() {
    let cases = [
        (DocumentInputFormatV1::PlainText, b"plain text".to_vec()),
        (DocumentInputFormatV1::Json, br#"{"answer":42}"#.to_vec()),
        (DocumentInputFormatV1::Html, b"<h1>Heading</h1><p>body</p>".to_vec()),
        (DocumentInputFormatV1::Pdf, b"%PDF-1.4\nBT (PDF body) Tj ET\n%%EOF".to_vec()),
        (DocumentInputFormatV1::Docx, stored_zip("word/document.xml", br#"<w:document xmlns:w="x"><w:body><w:p><w:r><w:t>DOCX body</w:t></w:r></w:p></w:body></w:document>"#)),
    ];
    for (format, raw) in cases {
        let input = || DocumentConversionInputV1 {
            source_ref: "snapshot://ldg028".into(),
            format: format.clone(),
            raw_input: raw.clone(),
        };
        let first = convert_granted_document(&grant(format.clone(), 4096), input()).unwrap();
        let second = convert_granted_document(&grant(format.clone(), 4096), input()).unwrap();
        assert_eq!(first.raw_sha256, second.raw_sha256);
        assert_eq!(first.markdown_sha256, second.markdown_sha256);
        assert_eq!(first.converter, second.converter);
        assert_eq!(first.raw_input, raw);
    }
}

#[test]
fn media_other_malformed_oversized_inputs_are_typed_refusals() {
    let media = convert_granted_document(
        &grant(DocumentInputFormatV1::Media("image/png".into()), 32),
        DocumentConversionInputV1 { source_ref: "snapshot://media".into(), format: DocumentInputFormatV1::Media("image/png".into()), raw_input: vec![1] },
    );
    assert!(matches!(media, Err(DocumentConversionErrorV1::MediaExcluded { .. })));
    let other = convert_granted_document(
        &grant(DocumentInputFormatV1::Other("rtf".into()), 32),
        DocumentConversionInputV1 { source_ref: "snapshot://other".into(), format: DocumentInputFormatV1::Other("rtf".into()), raw_input: vec![1] },
    );
    assert!(matches!(other, Err(DocumentConversionErrorV1::UnsupportedFormat { .. })));
    let malformed = convert_granted_document(
        &grant(DocumentInputFormatV1::Json, 32),
        DocumentConversionInputV1 { source_ref: "snapshot://bad".into(), format: DocumentInputFormatV1::Json, raw_input: b"{bad".to_vec() },
    );
    assert!(matches!(malformed, Err(DocumentConversionErrorV1::InvalidInput { .. })));
    let oversized = convert_granted_document(
        &grant(DocumentInputFormatV1::PlainText, 2),
        DocumentConversionInputV1 { source_ref: "snapshot://large".into(), format: DocumentInputFormatV1::PlainText, raw_input: b"large".to_vec() },
    );
    assert!(matches!(oversized, Err(DocumentConversionErrorV1::InputTooLarge { .. })));
}

#[test]
fn imported_snapshot_is_distinct_from_worktree_resolution() {
    let root = tempfile::tempdir().unwrap();
    let db = LedgerDb::open_in_memory();
    let artifact = doc_spine::ingest_granted_document(&db, &grant(DocumentInputFormatV1::PlainText, 4096), doc_spine::GrantedDocumentIngestV1 {
        repository_root: root.path().to_string_lossy().replace('\\', "/"),
        repository_id: "ldg028".into(), revision: "snapshot-1".into(), path: "import.txt".into(), title: "Snapshot".into(),
        document: DocumentConversionInputV1 { source_ref: "snapshot://one".into(), format: DocumentInputFormatV1::PlainText, raw_input: b"immutable".to_vec() },
    }).unwrap();
    let row: (String, String, String) = db.lock().query_row("SELECT raw_sha256,markdown_sha256,source_ref FROM ledger_document_conversions WHERE doc_id=?1", [&artifact.doc_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
    assert_eq!(row.2, "snapshot://one");
    assert_eq!(row.0, row.1);
    assert_eq!(row.0.len(), 64);
}

// LDG-028 r5 fidelity repair: expected-content fixtures. Hash/repeatability equality
// alone (asserted above) never closes fidelity; these assert exact converted bytes.

#[test]
fn docx_contiguous_styled_runs_join_without_spurious_space() {
    // "wonderful" split across bold/italic run boundaries must remain one contiguous word.
    let xml = br#"<w:document xmlns:w="x"><w:body><w:p>
        <w:r><w:rPr><w:b/></w:rPr><w:t>won</w:t></w:r>
        <w:r><w:rPr><w:i/></w:rPr><w:t>der</w:t></w:r>
        <w:r><w:t>ful</w:t></w:r>
    </w:p></w:body></w:document>"#;
    let converted = convert_granted_document(
        &grant(DocumentInputFormatV1::Docx, 4096),
        DocumentConversionInputV1 {
            source_ref: "snapshot://ldg028-runs".into(),
            format: DocumentInputFormatV1::Docx,
            raw_input: stored_zip("word/document.xml", xml),
        },
    )
    .unwrap();
    assert_eq!(converted.markdown, "wonderful\n");
}

#[test]
fn docx_tabs_and_line_breaks_are_preserved() {
    let xml = br#"<w:document xmlns:w="x"><w:body><w:p><w:r><w:t>a</w:t></w:r><w:r><w:tab/></w:r><w:r><w:t>b</w:t></w:r><w:r><w:br/></w:r><w:r><w:t>c</w:t></w:r></w:p></w:body></w:document>"#;
    let converted = convert_granted_document(
        &grant(DocumentInputFormatV1::Docx, 4096),
        DocumentConversionInputV1 {
            source_ref: "snapshot://ldg028-tabs".into(),
            format: DocumentInputFormatV1::Docx,
            raw_input: stored_zip("word/document.xml", xml),
        },
    )
    .unwrap();
    assert_eq!(converted.markdown, "a\tb\nc\n");
}

#[test]
fn docx_table_rows_and_cells_are_rendered_with_delimiters() {
    let xml = br#"<w:document xmlns:w="x"><w:body><w:tbl>
        <w:tr><w:tc><w:p><w:r><w:t>Name</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>Score</w:t></w:r></w:p></w:tc></w:tr>
        <w:tr><w:tc><w:p><w:r><w:t>Ada</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>9</w:t></w:r></w:p></w:tc></w:tr>
    </w:tbl></w:body></w:document>"#;
    let converted = convert_granted_document(
        &grant(DocumentInputFormatV1::Docx, 4096),
        DocumentConversionInputV1 {
            source_ref: "snapshot://ldg028-table".into(),
            format: DocumentInputFormatV1::Docx,
            raw_input: stored_zip("word/document.xml", xml),
        },
    )
    .unwrap();
    assert!(converted.markdown.contains("Name | Score"));
    assert!(converted.markdown.contains("Ada | 9"));
}

#[test]
fn docx_entities_decode_in_one_pass_not_recursively() {
    // The literal text "&lt;tag&gt;" is XML-escaped as "&amp;lt;tag&amp;gt;"; a
    // recursive/multi-pass decoder would wrongly re-decode this into "<tag>".
    let xml = br#"<w:document xmlns:w="x"><w:body><w:p><w:r><w:t>&amp;lt;tag&amp;gt; and https://example.com/a&amp;b?x=1</w:t></w:r></w:p></w:body></w:document>"#;
    let converted = convert_granted_document(
        &grant(DocumentInputFormatV1::Docx, 4096),
        DocumentConversionInputV1 {
            source_ref: "snapshot://ldg028-entities".into(),
            format: DocumentInputFormatV1::Docx,
            raw_input: stored_zip("word/document.xml", xml),
        },
    )
    .unwrap();
    assert_eq!(
        converted.markdown,
        "&lt;tag&gt; and https://example.com/a&b?x=1\n"
    );
}

#[test]
fn docx_identifiers_and_urls_survive_run_boundaries_intact() {
    // "user_name" and a URL split across run boundaries must not gain an injected space.
    let xml = br#"<w:document xmlns:w="x"><w:body><w:p><w:r><w:t>user_</w:t></w:r><w:r><w:t>name sees https://example.com/</w:t></w:r><w:r><w:t>path?id=42</w:t></w:r></w:p></w:body></w:document>"#;
    let converted = convert_granted_document(
        &grant(DocumentInputFormatV1::Docx, 4096),
        DocumentConversionInputV1 {
            source_ref: "snapshot://ldg028-ids".into(),
            format: DocumentInputFormatV1::Docx,
            raw_input: stored_zip("word/document.xml", xml),
        },
    )
    .unwrap();
    assert_eq!(
        converted.markdown,
        "user_name sees https://example.com/path?id=42\n"
    );
}

#[test]
fn html_entities_decode_in_one_pass_not_recursively() {
    let (normalized, _media, _had_markup) = {
        // Access via the public conversion path rather than the private helper directly.
        let converted = convert_granted_document(
            &grant(DocumentInputFormatV1::Html, 4096),
            DocumentConversionInputV1 {
                source_ref: "snapshot://ldg028-html-entities".into(),
                format: DocumentInputFormatV1::Html,
                raw_input: b"<p>&amp;lt;script&amp;gt;</p>".to_vec(),
            },
        )
        .unwrap();
        (converted.markdown, 0usize, false)
    };
    assert_eq!(normalized, "&lt;script&gt;\n");
}
