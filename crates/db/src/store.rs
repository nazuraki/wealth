//! Account and statement helpers shared by the PDF importer and the SimpleFIN sync.
//!
//! Every function takes `&Connection`; a `rusqlite::Transaction` derefs to one,
//! so callers can pass either.

use rusqlite::{params, Connection, OptionalExtension};

pub const SOURCE_PDF: &str = "pdf";
pub const SOURCE_SIMPLEFIN: &str = "simplefin";

/// Find the account with this last4, creating it if missing. Accounts are keyed
/// on last4 only so institution-name variants of one card map to one record.
/// A known `account_type` fills in a NULL on an existing row but never overwrites.
pub fn upsert_account(
    conn: &Connection,
    institution: &str,
    last4: &str,
    account_type: Option<&str>,
) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO accounts (institution, account_number_last4, account_type) \
         SELECT ?1, ?2, ?3 WHERE NOT EXISTS (SELECT 1 FROM accounts WHERE account_number_last4 = ?2)",
        params![institution, last4, account_type],
    )?;
    if account_type.is_some() {
        conn.execute(
            "UPDATE accounts SET account_type = ?1 WHERE account_number_last4 = ?2 AND account_type IS NULL",
            params![account_type, last4],
        )?;
    }
    conn.query_row(
        "SELECT id FROM accounts WHERE account_number_last4 = ?1",
        params![last4],
        |r| r.get(0),
    )
}

/// Result of [`find_or_create_statement`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatementRef {
    pub id: i64,
    pub inserted: bool,
    /// Source of the row actually in the table, which may differ from the
    /// requested source when the statement already existed.
    pub source: String,
}

/// Find the statement for `(account_id, period)`, creating it with the given
/// source and balances if missing. Never modifies an existing row.
pub fn find_or_create_statement(
    conn: &Connection,
    account_id: i64,
    period: &str,
    source: &str,
    source_file: &str,
    opening_balance: Option<f64>,
    closing_balance: Option<f64>,
) -> rusqlite::Result<StatementRef> {
    conn.execute(
        "INSERT OR IGNORE INTO statements \
         (account_id, statement_period, opening_balance, closing_balance, source_file, source) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![account_id, period, opening_balance, closing_balance, source_file, source],
    )?;
    let inserted = conn.changes() > 0;
    conn.query_row(
        "SELECT id, source FROM statements WHERE account_id = ?1 AND statement_period = ?2",
        params![account_id, period],
        |r| Ok(StatementRef { id: r.get(0)?, inserted, source: r.get(1)? }),
    )
}

/// Latest `statement_period` (YYYY-MM) for an account with the given source.
pub fn latest_period(
    conn: &Connection,
    account_id: i64,
    source: &str,
) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT MAX(statement_period) FROM statements WHERE account_id = ?1 AND source = ?2",
        params![account_id, source],
        |r| r.get::<_, Option<String>>(0),
    )
}

/// Source of the statement at `(account_id, period)`, if one exists.
pub fn statement_source(
    conn: &Connection,
    account_id: i64,
    period: &str,
) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT source FROM statements WHERE account_id = ?1 AND statement_period = ?2",
        params![account_id, period],
        |r| r.get(0),
    )
    .optional()
}

/// Distinct categories in use, from transactions and group memberships.
pub fn known_categories(conn: &Connection) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT category FROM transactions \
         UNION SELECT category FROM category_group_members \
         UNION SELECT category FROM category_rules \
         ORDER BY 1",
    )?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    rows.filter_map(|r| r.ok())
        .filter(|c| !c.is_empty())
        .collect::<Vec<_>>()
        .pipe_ok()
}

trait PipeOk<T> {
    fn pipe_ok(self) -> rusqlite::Result<T>;
}
impl<T> PipeOk<T> for T {
    fn pipe_ok(self) -> rusqlite::Result<T> {
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::run_migrations(&conn).unwrap();
        conn
    }

    #[test]
    fn upsert_account_is_keyed_on_last4() {
        let conn = open();
        let a = upsert_account(&conn, "Bank", "1234", None).unwrap();
        let b = upsert_account(&conn, "Bank (Visa)", "1234", Some("credit_card")).unwrap();
        assert_eq!(a, b);
        let kind: Option<String> = conn
            .query_row("SELECT account_type FROM accounts WHERE id = ?1", [a], |r| r.get(0))
            .unwrap();
        assert_eq!(kind.as_deref(), Some("credit_card"));
    }

    #[test]
    fn statement_is_created_once_and_keeps_original_source() {
        let conn = open();
        let acct = upsert_account(&conn, "Bank", "1234", None).unwrap();
        let first = find_or_create_statement(&conn, acct, "2026-05", SOURCE_PDF, "may.pdf", None, Some(10.0)).unwrap();
        assert!(first.inserted);
        let second = find_or_create_statement(&conn, acct, "2026-05", SOURCE_SIMPLEFIN, "simplefin", None, None).unwrap();
        assert!(!second.inserted);
        assert_eq!(second.id, first.id);
        assert_eq!(second.source, SOURCE_PDF);
        assert_eq!(statement_source(&conn, acct, "2026-05").unwrap().as_deref(), Some(SOURCE_PDF));
        assert_eq!(statement_source(&conn, acct, "2026-06").unwrap(), None);
    }

    #[test]
    fn latest_period_filters_by_source() {
        let conn = open();
        let acct = upsert_account(&conn, "Bank", "1234", None).unwrap();
        find_or_create_statement(&conn, acct, "2026-03", SOURCE_PDF, "a.pdf", None, None).unwrap();
        find_or_create_statement(&conn, acct, "2026-05", SOURCE_PDF, "b.pdf", None, None).unwrap();
        find_or_create_statement(&conn, acct, "2026-07", SOURCE_SIMPLEFIN, "simplefin", None, None).unwrap();
        assert_eq!(latest_period(&conn, acct, SOURCE_PDF).unwrap().as_deref(), Some("2026-05"));
        assert_eq!(latest_period(&conn, acct, SOURCE_SIMPLEFIN).unwrap().as_deref(), Some("2026-07"));
        assert_eq!(latest_period(&conn, 999, SOURCE_PDF).unwrap(), None);
    }
}
