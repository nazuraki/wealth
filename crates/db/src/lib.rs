pub const MIGRATION_001: &str = include_str!("../migrations/001_initial.sql");
pub const MIGRATION_002: &str = include_str!("../migrations/002_account_type.sql");
pub const MIGRATION_003: &str = include_str!("../migrations/003_normalize_periods.sql");
pub const MIGRATION_004: &str = include_str!("../migrations/004_dedupe_accounts.sql");
pub const MIGRATION_005: &str = include_str!("../migrations/005_account_display.sql");
pub const MIGRATION_006: &str = include_str!("../migrations/006_transfer_type.sql");
pub const MIGRATION_007: &str = include_str!("../migrations/007_is_transfer.sql");

/// Apply all migrations in order. Idempotent: safe to call on every connection open.
pub fn run_migrations(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
    conn.execute_batch(MIGRATION_001)?;
    // Migration 002 uses ALTER TABLE ADD COLUMN, which errors if the column already exists
    // on older SQLite versions that don't support IF NOT EXISTS.
    match conn.execute_batch(MIGRATION_002) {
        Ok(_) => {}
        Err(e) if e.to_string().contains("duplicate column name") => {}
        Err(e) => return Err(e),
    }
    // Migrations 003 and 004 are data UPDATEs/DELETEs — safe to re-run (no rows match after first run).
    conn.execute_batch(MIGRATION_003)?;
    conn.execute_batch(MIGRATION_004)?;
    // Migration 005 uses ALTER TABLE ADD COLUMN — guard against duplicate column on re-run.
    for sql in MIGRATION_005.lines().filter(|l| !l.trim().is_empty()) {
        match conn.execute_batch(sql) {
            Ok(_) => {}
            Err(e) if e.to_string().contains("duplicate column name") => {}
            Err(e) => return Err(e),
        }
    }
    // Check once whether migration 007 has been applied (is_transfer column present).
    // This guards both 006 and 007: running 006 after 007 corrupts the schema because
    // transactions now has 8 columns but transactions_v2 only expects 7.
    let migration_007_applied: bool = conn.query_row(
        "SELECT COUNT(*) > 0 FROM pragma_table_info('transactions') WHERE name = 'is_transfer'",
        [],
        |r| r.get(0),
    )?;
    if !migration_007_applied {
        conn.execute_batch(MIGRATION_006)?;
    }
    // Drop orphaned intermediate tables left by any partial migration run.
    conn.execute_batch("DROP TABLE IF EXISTS transactions_v2; DROP TABLE IF EXISTS transactions_v3;")?;
    // Migration 007: replace type='transfer' with is_transfer flag.
    if !migration_007_applied {
        conn.execute_batch(MIGRATION_007)?;
    }
    Ok(())
}
