use crate::MemoryEntry;
#[cfg(test)]
use crate::MemoryTier;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DreamAgentPolicy {
    pub model: String,
    pub allowed_tools: Vec<String>,
    pub shell_allowed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DreamConsolidatedMemory {
    /// The PRIMARY entry's own id — consolidation updates it in place. (The old
    /// generated `dream-<slug>` ids churned stable ids on every date rewrite,
    /// could collide across distinct memories sharing a 12-word prefix, and
    /// dropped the primary's scope. Removed 2026-07-04.)
    pub id: String,
    pub content: String,
    pub keywords: Vec<String>,
    pub source_ids: Vec<String>,
    /// Preserved from the primary so an in-place write never re-scopes the row.
    pub scope_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DreamPlan {
    pub consolidated: Vec<DreamConsolidatedMemory>,
    pub pruned_ids: Vec<String>,
    pub quarantined_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DreamStatus {
    pub agent_id: String,
    pub status: String,
    pub model: String,
    pub shell_allowed: bool,
    pub read_count: usize,
    pub consolidated_count: usize,
    pub pruned_count: usize,
    pub quarantined_count: usize,
}

impl DreamAgentPolicy {
    pub fn restricted() -> Self {
        Self {
            // Consolidation is deterministic local Rust code. Do not attribute it to an LLM.
            model: String::new(),
            allowed_tools: vec![
                "memory.read".to_string(),
                "memory.write".to_string(),
                "remember_consolidated".to_string(),
            ],
            shell_allowed: false,
        }
    }
}

pub fn consolidate_dream_memories(entries: &[MemoryEntry], today: &str) -> DreamPlan {
    let mut groups: BTreeMap<String, Vec<&MemoryEntry>> = BTreeMap::new();
    for entry in entries {
        let source_date = entry.created_at.get(..10).unwrap_or(today);
        let normalized_content = normalize_relative_dates(&entry.content, source_date);
        groups
            .entry(format!(
                "{}\u{0}{}",
                entry.scope_id,
                dedup_key(&normalized_content)
            ))
            .or_default()
            .push(entry);
    }

    let mut consolidated = Vec::new();
    let mut pruned_ids = BTreeSet::new();
    let mut quarantined_ids = BTreeSet::new();
    for (_key, mut group) in groups {
        group.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.access_count.cmp(&a.access_count))
                .then_with(|| a.id.cmp(&b.id))
        });

        let primary = group[0];
        let source_date = primary.created_at.get(..10).unwrap_or(today);
        let normalized_content = normalize_relative_dates(&primary.content, source_date);
        let has_relative_dates = normalized_content != primary.content;
        let is_duplicate = group.len() > 1;
        if is_duplicate || has_relative_dates {
            let source_ids = group
                .iter()
                .map(|entry| entry.id.clone())
                .collect::<Vec<_>>();
            consolidated.push(DreamConsolidatedMemory {
                id: primary.id.clone(),
                content: normalized_content.trim().to_string(),
                keywords: merged_keywords(&group),
                source_ids,
                scope_id: primary.scope_id.clone(),
            });
            // Only the non-primary duplicates are pruned; the primary is updated
            // in place under its own id.
            for source in group.iter().skip(1) {
                pruned_ids.insert(source.id.clone());
            }
        }
    }

    // Low-value prune — but never prune an id that consolidation just chose as
    // a write target (a low-score primary is being refreshed, not discarded).
    let targets: BTreeSet<&str> = consolidated.iter().map(|c| c.id.as_str()).collect();
    for entry in entries {
        if entry.score < 0.2 && entry.access_count == 0 && !targets.contains(entry.id.as_str()) {
            quarantined_ids.insert(entry.id.clone());
        }
    }

    DreamPlan {
        consolidated,
        pruned_ids: pruned_ids.into_iter().collect(),
        quarantined_ids: quarantined_ids.into_iter().collect(),
    }
}

fn merged_keywords(entries: &[&MemoryEntry]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    for entry in entries {
        for keyword in &entry.keywords {
            let keyword = keyword.trim().to_lowercase();
            if !keyword.is_empty() {
                seen.insert(keyword);
            }
        }
    }
    seen.into_iter().take(12).collect()
}

fn dedup_key(content: &str) -> String {
    content
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalize_relative_dates(content: &str, today: &str) -> String {
    let Some(today) = parse_date(today) else {
        return content.to_string();
    };
    let replacements = [
        ("yesterday", format_date(add_days(today, -1))),
        ("today", format_date(today)),
        ("tomorrow", format_date(add_days(today, 1))),
        ("last week", format_date(add_days(today, -7))),
        ("next week", format_date(add_days(today, 7))),
    ];
    replace_case_insensitive(content, &replacements)
}

fn replace_case_insensitive(input: &str, replacements: &[(&str, String)]) -> String {
    let mut out = input.to_string();
    for (needle, replacement) in replacements {
        let mut cursor = 0usize;
        loop {
            let haystack = out[cursor..].to_lowercase();
            let Some(pos) = haystack.find(needle) else {
                break;
            };
            let start = cursor + pos;
            let end = start + needle.len();
            if is_word_boundary(&out, start, end) {
                out.replace_range(start..end, replacement);
                cursor = start + replacement.len();
            } else {
                cursor = end;
            }
            if cursor >= out.len() {
                break;
            }
        }
    }
    out
}

fn is_word_boundary(s: &str, start: usize, end: usize) -> bool {
    let before = s[..start].chars().next_back();
    let after = s[end..].chars().next();
    !before.map(|c| c.is_ascii_alphanumeric()).unwrap_or(false)
        && !after.map(|c| c.is_ascii_alphanumeric()).unwrap_or(false)
}

fn parse_date(s: &str) -> Option<(i32, u32, u32)> {
    let mut parts = s.split('-');
    let year = parts.next()?.parse().ok()?;
    let month = parts.next()?.parse().ok()?;
    let day = parts.next()?.parse().ok()?;
    if parts.next().is_some()
        || !(1..=12).contains(&month)
        || day == 0
        || day > days_in_month(year, month)
    {
        return None;
    }
    Some((year, month, day))
}

fn format_date((year, month, day): (i32, u32, u32)) -> String {
    format!("{year:04}-{month:02}-{day:02}")
}

fn add_days(mut date: (i32, u32, u32), days: i32) -> (i32, u32, u32) {
    if days >= 0 {
        for _ in 0..days {
            date.2 += 1;
            let dim = days_in_month(date.0, date.1);
            if date.2 > dim {
                date.2 = 1;
                date.1 += 1;
                if date.1 > 12 {
                    date.1 = 1;
                    date.0 += 1;
                }
            }
        }
    } else {
        for _ in days..0 {
            if date.2 > 1 {
                date.2 -= 1;
            } else if date.1 > 1 {
                date.1 -= 1;
                date.2 = days_in_month(date.0, date.1);
            } else {
                date.0 -= 1;
                date.1 = 12;
                date.2 = 31;
            }
        }
    }
    date
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 30,
    }
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, content: &str, score: f64, access_count: u32) -> MemoryEntry {
        MemoryEntry {
            id: id.to_string(),
            tier: MemoryTier::Episodic,
            content: content.to_string(),
            keywords: vec!["memory".into()],
            score,
            created_at: "2026-06-29T00:00:00Z".to_string(),
            access_count,
            embedding: None,
            scope_id: crate::default_scope(),
        }
    }

    fn scoped_entry(id: &str, scope: &str, created_at: &str, content: &str) -> MemoryEntry {
        MemoryEntry {
            scope_id: scope.to_string(),
            created_at: created_at.to_string(),
            ..entry(id, content, 0.8, 1)
        }
    }

    #[test]
    fn dream_agent_policy_is_restricted() {
        let policy = DreamAgentPolicy::restricted();
        assert!(policy.model.is_empty());
        assert_eq!(
            policy.allowed_tools,
            vec![
                "memory.read".to_string(),
                "memory.write".to_string(),
                "remember_consolidated".to_string()
            ]
        );
        assert!(!policy.shell_allowed);
    }

    #[test]
    fn dream_merges_duplicate_memories() {
        let plan = consolidate_dream_memories(
            &[
                entry("a", "Use cargo fmt before cargo test.", 0.8, 3),
                entry("b", " use cargo fmt before cargo test! ", 0.7, 2),
            ],
            "2026-06-29",
        );

        assert_eq!(plan.consolidated.len(), 1);
        assert_eq!(plan.consolidated[0].source_ids, vec!["a", "b"]);
        // The primary keeps its own id — no generated dream- ids.
        assert_eq!(plan.consolidated[0].id, "a");
        assert_eq!(plan.consolidated[0].scope_id, crate::default_scope());
        assert!(plan.pruned_ids.contains(&"b".to_string()));
        assert!(!plan.pruned_ids.contains(&"a".to_string()));
    }

    #[test]
    fn dream_dedup_never_crosses_scopes() {
        let plan = consolidate_dream_memories(
            &[
                scoped_entry("a", "project-a", "2026-06-01T10:00:00Z", "Same rule."),
                scoped_entry("b", "project-b", "2026-06-01T10:00:00Z", "Same rule."),
            ],
            "2026-07-10",
        );

        assert!(plan.consolidated.is_empty());
        assert!(plan.pruned_ids.is_empty());
    }

    #[test]
    fn dream_relative_dates_use_each_entry_created_at() {
        let plan = consolidate_dream_memories(
            &[scoped_entry(
                "old",
                "project-a",
                "2026-06-01T23:45:00Z",
                "Yesterday the release failed; tomorrow retry it.",
            )],
            "2026-07-10",
        );

        assert_eq!(plan.consolidated.len(), 1);
        assert!(plan.consolidated[0].content.contains("2026-05-31"));
        assert!(plan.consolidated[0].content.contains("2026-06-02"));
        assert!(!plan.consolidated[0].content.contains("2026-07-09"));
    }

    #[test]
    fn deterministic_dream_policy_does_not_claim_an_llm_model() {
        let policy = DreamAgentPolicy::restricted();
        assert!(policy.model.is_empty());
    }

    #[test]
    fn dream_converts_relative_dates_to_absolute_dates() {
        let plan = consolidate_dream_memories(
            &[entry(
                "a",
                "Yesterday the Windows build failed; today cargo test passed.",
                0.9,
                1,
            )],
            "2026-06-29",
        );

        assert_eq!(plan.consolidated.len(), 1);
        // A date rewrite is a content fix under the SAME id, never a new id.
        assert_eq!(plan.consolidated[0].id, "a");
        assert!(plan.pruned_ids.is_empty());
        assert!(plan.consolidated[0].content.contains("2026-06-28"));
        assert!(plan.consolidated[0].content.contains("2026-06-29"));
        assert!(!plan.consolidated[0].content.contains("Yesterday"));
    }

    #[test]
    fn dream_never_collides_distinct_memories_sharing_a_prefix() {
        // The old dream-<slug> id was a 12-word content prefix: these two DISTINCT
        // memories would have consolidated into the SAME id and overwritten each
        // other. In-place ids make collision structurally impossible.
        let shared = "today the build pipeline for the windows installer of the app failed";
        let plan = consolidate_dream_memories(
            &[
                entry("x", &format!("{shared} because signing timed out"), 0.8, 1),
                entry("y", &format!("{shared} because the disk filled up"), 0.8, 1),
            ],
            "2026-06-29",
        );

        assert_eq!(plan.consolidated.len(), 2);
        let mut ids: Vec<&str> = plan.consolidated.iter().map(|c| c.id.as_str()).collect();
        ids.sort();
        assert_eq!(ids, vec!["x", "y"]);
        assert!(plan.pruned_ids.is_empty());
    }

    #[test]
    fn dream_low_value_primary_is_refreshed_not_pruned() {
        // A low-score primary that consolidation rewrites (date fix) must not land
        // in the prune list — it is being refreshed, not discarded.
        let plan = consolidate_dream_memories(
            &[entry("frail", "today the cache warmed slowly", 0.1, 0)],
            "2026-06-29",
        );

        assert_eq!(plan.consolidated.len(), 1);
        assert_eq!(plan.consolidated[0].id, "frail");
        assert!(plan.pruned_ids.is_empty());
    }

    #[test]
    fn dream_quarantines_low_effectiveness_entries() {
        let plan = consolidate_dream_memories(
            &[
                entry("low", "Tiny low-value memory", 0.1, 0),
                entry("kept", "Useful durable memory", 0.6, 0),
            ],
            "2026-06-29",
        );

        assert!(plan.pruned_ids.is_empty());
        assert_eq!(plan.quarantined_ids, vec!["low"]);
    }
}
