//! Reconciliation may retire only the source collection it actually enumerated.
use super::{limits::WorkBudget, policy::SourcePolicy};
use rusqlite::{params, Transaction};
use std::path::Path;

pub(crate) fn markdown_absences(
    tx: &Transaction<'_>, root: &str, generation: i64, now: i64,
    policy: &mut SourcePolicy, budget: &WorkBudget,
) -> Result<usize, String> {
    let rows = {
        let mut statement = tx.prepare(
            "SELECT doc_id,path FROM ledger_doc_artifacts a WHERE repository_root=?1
             AND index_generation<?2 AND lifecycle_state IN ('active','draft','retired')
             AND lower(path) LIKE '%.md'
             AND NOT EXISTS(SELECT 1 FROM ledger_document_conversions c WHERE c.doc_id=a.doc_id)")
            .map_err(|e| e.to_string())?;
        let result = statement.query_map(params![root, generation], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        }).map_err(|e| e.to_string())?.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
        result
    };
    let mut deleted = 0;
    for (id, path) in rows {
        budget.visit()?;
        let allowed = policy.allows(&path, false, budget)?;
        let state = if !allowed {
            "excluded"
        } else {
            match std::fs::symlink_metadata(Path::new(root).join(&path)) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => { deleted += 1; "tombstoned" }
                Err(_) => return Err("ledger_source_unavailable_during_reconcile".into()),
                Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => "excluded",
                // A permitted file that was absent from the completed walk appeared
                // concurrently. Retry instead of claiming it was deleted.
                Ok(_) => return Err("ledger_source_changed_during_scan".into()),
            }
        };
        tx.execute("UPDATE ledger_doc_artifacts SET lifecycle_state=?1,index_generation=?2,updated_at_ms=?3 WHERE repository_root=?4 AND doc_id=?5",
            params![state, generation, now, root, id]).map_err(|e| e.to_string())?;
    }
    Ok(deleted)
}
