#!/usr/bin/env python3
"""One-time reviewed source repair. Refuse drift before changing source."""
from pathlib import Path
import hashlib
R=Path(__file__).resolve().parents[2]
p=R/'engine/crates/membrane-runtime/src/push/recovery.rs'
if hashlib.sha256(p.read_bytes()).hexdigest()!='9cf37491ce0d1a0d09e46683b2034159a30d67a2c9fe6a72d2e00489323985b1': raise SystemExit('recovery source drift')
s=p.read_text().replace('fn db_error(_: rusqlite::Error) -> RecoveryError { RecoveryError::Unavailable }','''fn db_error(error: rusqlite::Error) -> RecoveryError {
    match error {
        rusqlite::Error::IntegralValueOutOfRange(..) => RecoveryError::Corrupt,
        rusqlite::Error::SqliteFailure(ref e, _) if e.code == rusqlite::ErrorCode::DiskFull => RecoveryError::Limit,
        _ => RecoveryError::Unavailable,
    }
}
fn sql_u64(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<u64> {
    let value: i64 = row.get(index)?;
    u64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(index, value))
}
fn sql_size(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<usize> {
    let value: i64 = row.get(index)?;
    usize::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(index, value))
}''')
s=s.replace('Ok((r.get(0)?,r.get(1)?,r.get(2)?))','Ok((sql_size(r,0)?,sql_size(r,1)?,sql_u64(r,2)?))')
s=s.replace('Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).optional().map_err(db_error)?;', 'Ok((r.get(0)?, sql_size(r,1)?, sql_u64(r,2)?, r.get(3)?))).optional().map_err(db_error)?;',1)
s=s.replace('Ok((r.get(0)?, r.get(1)?, r.get(2)?))).map_err(db_error)?;', 'Ok((sql_u64(r,0)?, sql_u64(r,1)?, sql_u64(r,2)?))).map_err(db_error)?;')
s=s.replace('params![scope.id, hash, bytes, bytes.len(), now, expires]', 'params![scope.id, hash, bytes, bytes.len() as i64, now as i64, expires as i64]')
s=s.replace('Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).optional().map_err(db_error)?;', 'Ok((sql_size(r,0)?, sql_size(r,1)?, sql_u64(r,2)?, r.get(3)?))).optional().map_err(db_error)?;')
s=s.replace('let connection = Connection::open(path).map_err(db_error)?;', '''let connection = Connection::open(&path).map_err(db_error)?;
        #[cfg(unix)] {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
                .map_err(|_| RecoveryError::Unavailable)?;
        }
        // Physical pages are independently bounded. SQLite reuses free pages;
        // storage pressure is a refusal, never implicit eviction of a live lease.
        let page_size: i64 = connection.query_row("PRAGMA page_size", [], |r| r.get(0)).map_err(db_error)?;
        if page_size <= 0 { return Err(RecoveryError::Corrupt); }
        connection.pragma_update(None, "max_page_count", 512_i64 * 1024 * 1024 / page_size).map_err(db_error)?;''')
s=s.replace('    pub fn invalidate(&self, scope:', '''    /// Explicit compare-and-swap renewal. Reads and duplicate publication never
    /// call this operation; expired/invalidated handles cannot be resurrected.
    pub fn renew(&self, scope: &RecoveryScope, handle: &str, expected_expiry: u64, ttl_ms: u64, now: u64) -> Result<RecoveryReference, RecoveryError> {
        if ttl_ms == 0 || ttl_ms > MAX_TTL_MS || now > i64::MAX as u64 - ttl_ms { return Err(RecoveryError::Limit); }
        let resolved = self.resolve(scope, handle, &Selector::Bytes {start:0,end:0}, 1, now)?;
        if resolved.reference.expires_at != expected_expiry { return Err(RecoveryError::Denied); }
        let hash = crate::ledger::identifier::AnchorRef::parse(handle).map_err(|_| RecoveryError::InvalidAnchor)?.digest();
        let expires = (now + ttl_ms).max(expected_expiry);
        let connection = self.connection()?;
        let changed = connection.execute("UPDATE push_originals SET expires=?1 WHERE scope=?2 AND digest=?3 AND expires=?4 AND invalidated=0",
            params![expires as i64, scope.id, hash, expected_expiry as i64]).map_err(db_error)?;
        if changed != 1 { return Err(RecoveryError::Denied); }
        Self::reference(&connection, &hash, resolved.reference.size_bytes, expires, now)
    }
    pub fn invalidate(&self, scope:''')
s=s.replace('UPDATE push_originals SET invalidated=1 WHERE', "UPDATE push_originals SET invalidated=1,content=x'',size=0 WHERE")
s=s.replace('// Expiry frees logical quota, not a live promise. No access extends expiry.','// No access extends expiry; explicit invalidation releases payload quota.')
s=s.replace('    #[test]\n    fn exact_json_selectors_preserve_spelling_and_reject_ambiguity()', '''    #[test]
    fn explicit_renewal_is_cas_and_corrupt_sizes_fail_before_allocation() {
        let temp = tempfile::tempdir().unwrap();
        let store = RecoveryStore::at(temp.path());
        let s = scope(&temp, "a");
        let reference = store.publish(&s, b"example", 1000, 100).unwrap();
        assert_eq!(store.renew(&s, &reference.handle, 1100, 2000, 500).unwrap().expires_at, 2500);
        assert!(store.renew(&s, &reference.handle, 1100, 2000, 600).is_err());
        store.connection().unwrap().execute("UPDATE push_originals SET size=-1", []).unwrap();
        assert!(matches!(store.resolve(&s, &reference.handle, &Selector::Whole, 100, 600), Err(RecoveryError::Corrupt)));
    }
    #[test]
    fn exact_json_selectors_preserve_spelling_and_reject_ambiguity()''')
f=R/'engine/crates/cortex-core/src/accounting.rs'
if hashlib.sha256(f.read_bytes()).hexdigest()!='2eea7955d64c6fbd55ae318f98058d7d4b49800388074c161b23d9f0ea9ce239': raise SystemExit('accounting source drift')
t=f.read_text(); pos=t.index('/// Count the tokens in `text`.')
t=t[:pos]+'''/// Exact o200k_base count for literal data. Unlike estimate_tokens this never
/// substitutes a ratio and never treats token-looking source strings as control.
pub fn count_o200k_exact(text: &str) -> Result<usize, String> {
    let bpe = tokenizer().ok_or_else(|| "o200k_base tokenizer unavailable".to_string())?;
    Ok(bpe.encode_ordinary(text).len())
}

'''+t[pos:]
p.write_text(s);f.write_text(t)
Path(__file__).unlink()
