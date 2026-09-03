pub mod rules;
pub mod store;

pub const MIGRATION_001: &str = include_str!("../migrations/001_initial.sql");
pub const MIGRATION_002: &str = include_str!("../migrations/002_account_type.sql");
pub const MIGRATION_003: &str = include_str!("../migrations/003_normalize_periods.sql");
pub const MIGRATION_004: &str = include_str!("../migrations/004_dedupe_accounts.sql");
pub const MIGRATION_005: &str = include_str!("../migrations/005_account_display.sql");
pub const MIGRATION_006: &str = include_str!("../migrations/006_transfer_type.sql");
pub const MIGRATION_007: &str = include_str!("../migrations/007_is_transfer.sql");
pub const MIGRATION_008: &str = include_str!("../migrations/008_normalize_tx_dates.sql");
pub const MIGRATION_009: &str = include_str!("../migrations/009_category_groups.sql");
pub const MIGRATION_010: &str = include_str!("../migrations/010_simplefin.sql");

/// Run each `;`-separated statement, ignoring "duplicate column name" errors so
/// `ALTER TABLE ADD COLUMN` migrations can be re-run on SQLite versions without
/// `IF NOT EXISTS` support. Comment lines are stripped; keep `;` out of comments.
fn run_guarded(conn: &rusqlite::Connection, sql: &str) -> rusqlite::Result<()> {
    let body: String = sql
        .lines()
        .filter(|l| !l.trim_start().starts_with("--"))
        .collect::<Vec<_>>()
        .join("\n");
    for stmt in body.split(';').map(str::trim).filter(|s| !s.is_empty()) {
        match conn.execute_batch(&format!("{stmt};")) {
            Ok(_) => {}
            Err(e) if e.to_string().contains("duplicate column name") => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Apply all migrations in order. Idempotent: safe to call on every connection open.
pub fn run_migrations(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
    conn.execute_batch(MIGRATION_001)?;
    // Migration 002 uses ALTER TABLE ADD COLUMN, which errors if the column already exists
    // on older SQLite versions that don't support IF NOT EXISTS.
    run_guarded(conn, MIGRATION_002)?;
    // Migrations 003 and 004 are data UPDATEs/DELETEs — safe to re-run (no rows match after first run).
    conn.execute_batch(MIGRATION_003)?;
    conn.execute_batch(MIGRATION_004)?;
    // Migration 005 uses ALTER TABLE ADD COLUMN — guard against duplicate column on re-run.
    run_guarded(conn, MIGRATION_005)?;
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
    // Migration 008: backfill MM/DD transaction dates to YYYY-MM-DD. Idempotent
    // — only matches rows that still look like MM/DD.
    conn.execute_batch(MIGRATION_008)?;
    // Migration 009: category groups + members. Idempotent (IF NOT EXISTS).
    conn.execute_batch(MIGRATION_009)?;
    // Migration 010: SimpleFIN columns, external_id index, category_rules.
    // ALTER TABLE statements are guarded; the rest use IF NOT EXISTS.
    run_guarded(conn, MIGRATION_010)?;
    Ok(())
}
