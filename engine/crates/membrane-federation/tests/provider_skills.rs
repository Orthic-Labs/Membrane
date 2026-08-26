use membrane_federation::providers::skills::skill_ranker;
use membrane_provider_sdk::SkillCatalogEntry;

fn entry(id: &str, title: &str, keywords: &[&str], hash: char) -> SkillCatalogEntry {
    SkillCatalogEntry {
        id: id.into(),
        repository_id: "repo".into(),
        generation: "sha256:1111111111111111111111111111111111111111111111111111111111111111"
            .into(),
        source_hash: hash.to_string().repeat(64),
        title: title.into(),
        keywords: keywords.iter().map(|v| (*v).into()).collect(),
    }
}

#[test]
fn ranks_metadata_without_skill_bodies() {
    let entries = vec![
        entry("generic", "General notes", &[], 'a'),
        entry("rust-review", "Review Rust code", &["rust", "review"], 'b'),
    ];
    let ranked = skill_ranker::rank("rust review", &entries, 5);
    assert_eq!(ranked[0].entry.id, "rust-review");
    assert_eq!(ranked[0].entry.source_hash, "b".repeat(64));
}

#[test]
fn ties_use_canonical_skill_id() {
    let entries = vec![
        entry("zeta", "Shared", &[], 'a'),
        entry("alpha", "Shared", &[], 'b'),
    ];
    let ranked = skill_ranker::rank("unmatched", &entries, 5);
    assert_eq!(
        ranked
            .iter()
            .map(|item| item.entry.id.as_str())
            .collect::<Vec<_>>(),
        ["alpha", "zeta"]
    );
}

#[test]
fn rank_limit_is_bounded() {
    let entries = vec![entry("one", "One", &[], 'a'), entry("two", "Two", &[], 'b')];
    assert_eq!(skill_ranker::rank("", &entries, 1).len(), 1);
}
