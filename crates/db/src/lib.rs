pub const MIGRATION_001: &str = include_str!("../migrations/001_initial.sql");
pub const MIGRATION_002: &str = include_str!("../migrations/002_account_type.sql");
pub const MIGRATION_003: &str = include_str!("../migrations/003_normalize_periods.sql");
pub const MIGRATION_004: &str = include_str!("../migrations/004_dedupe_accounts.sql");
pub const MIGRATION_005: &str = include_str!("../migrations/005_account_display.sql");

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
    Ok(())
}
