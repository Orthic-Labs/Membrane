// Schema 19: permit categorical facts to store confidence=NULL (INV-004).
// Called inside migrateDb's existing transaction/backup boundary. Do not
// reinterpret old confidence values or reseal generations during a schema change.
export function migrateNullableFactConfidence(db) {
  for (const table of ["symbols", "edges"]) {
    const definition = db.prepare("SELECT sql FROM sqlite_master WHERE type='table' AND name=?").get(table);
    const columns = db.prepare(`PRAGMA table_info("${table}")`).all();
    const confidence = columns.find((column) => column.name === "confidence");
    if (!definition?.sql || !confidence) {
      throw Object.assign(new Error(`confidence migration: missing ${table}.confidence`), { code: "confidence_migration_schema_mismatch" });
    }
    if (!confidence.notnull) continue;
    const nullable = definition.sql.replace(/(\bconfidence\s+REAL)\s+NOT\s+NULL\b/i, "$1");
    if (nullable === definition.sql) {
      throw Object.assign(new Error(`confidence migration: unrecognized ${table} definition`), { code: "confidence_migration_schema_mismatch" });
    }
    const temporary = `${table}_nullable_confidence_v19`;
    const create = nullable.replace(/^CREATE TABLE\s+(?:IF NOT EXISTS\s+)?(?:"[^\"]+"|\w+)/i, `CREATE TABLE "${temporary}"`);
    const dependents = db.prepare("SELECT sql FROM sqlite_master WHERE tbl_name=? AND type IN ('index','trigger') AND sql IS NOT NULL ORDER BY type,name").all(table);
    const names = columns.map((column) => `"${column.name.replaceAll('"', '""')}"`).join(", ");
    db.exec(create);
    // Edge rowid and node_ordinal order are observable in deterministic graph
    // hydration. Never let a schema-only upgrade silently re-number either.
    db.exec(`INSERT INTO "${temporary}" (rowid, ${names}) SELECT rowid, ${names} FROM "${table}" ORDER BY rowid`);
    db.exec(`DROP TABLE "${table}"`);
    db.exec(`ALTER TABLE "${temporary}" RENAME TO "${table}"`);
    for (const dependent of dependents) db.exec(dependent.sql);
  }
}
