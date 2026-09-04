//! Durable source exclusions outlive rebuildable Ledger projections.
//!
//! The Membrane catalog holds the policy record. The Ledger index contains
//! only a deny-only cache; deleting/rebuilding it cannot re-enroll an erased
//! source. A failed catalog read is unavailable, never an empty permission set.
use crate::catalog::ContextCatalog;
use super::{LedgerDb,limits::WorkBudget};
use rusqlite::params;

const SCHEMA:&str="CREATE TABLE IF NOT EXISTS ledger_source_exclusions (
 repository_root TEXT NOT NULL,path_digest TEXT NOT NULL,excluded_at_ms INTEGER NOT NULL,
 PRIMARY KEY(repository_root,path_digest));";

pub(crate) fn record(catalog:&ContextCatalog,root:&str,path_digest:&str)->Result<(),String> {
    let conn=catalog.lock();
    conn.execute_batch(SCHEMA).map_err(|_|"ledger_policy_store_unavailable")?;
    conn.execute("INSERT OR IGNORE INTO ledger_source_exclusions VALUES (?1,?2,?3)",
        params![root,path_digest,crate::time::now_millis() as i64]).map_err(|_|"ledger_policy_store_unavailable")?;
    Ok(())
}

pub(crate) fn synchronize(catalog:&ContextCatalog,db:&LedgerDb,root:&str,budget:&WorkBudget)->Result<(),String> {
    budget.check()?;
    // Migrate a pre-registry deny cache conservatively. It can only reduce
    // authority; this operation never turns a projection into a read grant.
    let cached={
        let conn=db.lock();
        let mut statement=conn.prepare("SELECT path_digest FROM ledger_erasure_fences WHERE repository_root=?1 LIMIT 16385")
            .map_err(|_|"ledger_policy_cache_unavailable")?;
        let values=statement.query_map([root],|r|r.get::<_,String>(0)).map_err(|_|"ledger_policy_cache_unavailable")?
            .collect::<Result<Vec<_>,_>>().map_err(|_|"ledger_policy_cache_unavailable")?;
        values
    };
    if cached.len()>16384 {return Err("ledger_policy_budget_exhausted".into());}
    for digest in cached {budget.visit()?;record(catalog,root,&digest)?;}
    let excluded={
        let conn=catalog.lock();
        conn.execute_batch(SCHEMA).map_err(|_|"ledger_policy_store_unavailable")?;
        let mut statement=conn.prepare("SELECT path_digest,excluded_at_ms FROM ledger_source_exclusions WHERE repository_root=?1 ORDER BY path_digest LIMIT 16385")
            .map_err(|_|"ledger_policy_store_unavailable")?;
        let values=statement.query_map([root],|r|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?)))
            .map_err(|_|"ledger_policy_store_unavailable")?.collect::<Result<Vec<_>,_>>().map_err(|_|"ledger_policy_store_unavailable")?;
        values
    };
    if excluded.len()>16384 {return Err("ledger_policy_budget_exhausted".into());}
    let conn=db.lock();
    for (digest,time) in excluded {
        budget.visit()?;
        conn.execute("INSERT OR IGNORE INTO ledger_erasure_fences VALUES (?1,?2,?3)",params![root,digest,time])
            .map_err(|_|"ledger_policy_cache_unavailable")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rebuilt_index_inherits_durable_source_exclusion() {
        let catalog=ContextCatalog::open_in_memory();
        let first=LedgerDb::open_in_memory();
        let budget=WorkBudget::bounded(std::time::Duration::from_secs(5));
        let digest=super::super::resolve::digest(b"docs/erased.md");
        record(&catalog,"/repo",&digest).unwrap();
        synchronize(&catalog,&first,"/repo",&budget).unwrap();
        drop(first);
        let rebuilt=LedgerDb::open_in_memory();
        synchronize(&catalog,&rebuilt,"/repo",&budget).unwrap();
        let count:i64=rebuilt.lock().query_row("SELECT COUNT(*) FROM ledger_erasure_fences WHERE repository_root='/repo'",[],|r|r.get(0)).unwrap();
        assert_eq!(count,1);
    }
}
