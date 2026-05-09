use anyhow::Result;
use rusqlite::Connection;
use serde::Serialize;
use tauri::{AppHandle, Manager};

#[derive(Debug, Serialize)]
pub struct AccountBalance {
    pub institution: String,
    pub account_number_last4: String,
    pub closing_balance: Option<f64>,
    pub statement_period: String,
}

#[derive(Debug, Serialize)]
pub struct RecentImport {
    pub institution: String,
    pub account_number_last4: String,
    pub statement_period: String,
    pub transaction_count: i64,
    pub imported_at: String,
}

#[derive(Debug, Serialize)]
pub struct DashboardData {
    pub spend_30d: f64,
    pub spend_90d: f64,
    pub account_balances: Vec<AccountBalance>,
    pub recent_imports: Vec<RecentImport>,
}

fn query_dashboard(conn: &Connection) -> Result<Option<DashboardData>> {
    let account_count: i64 =
        conn.query_row("SELECT COUNT(*) FROM accounts", [], |r| r.get(0))?;
    if account_count == 0 {
        return Ok(None);
    }

    let spend_30d: f64 = conn
        .query_row(
            "SELECT COALESCE(SUM(amount), 0.0) \
             FROM transactions \
             WHERE type = 'debit' AND date >= date('now', '-30 days')",
            [],
            |r| r.get(0),
        )?;

    let spend_90d: f64 = conn
        .query_row(
            "SELECT COALESCE(SUM(amount), 0.0) \
             FROM transactions \
             WHERE type = 'debit' AND date >= date('now', '-90 days')",
            [],
            |r| r.get(0),
        )?;

    let mut stmt = conn.prepare(
        "SELECT a.institution, a.account_number_last4, s.closing_balance, s.statement_period \
         FROM accounts a \
         JOIN statements s ON s.account_id = a.id \
         WHERE s.imported_at = ( \
             SELECT MAX(s2.imported_at) FROM statements s2 WHERE s2.account_id = a.id \
         ) \
         ORDER BY a.institution, a.account_number_last4",
    )?;
    let account_balances: Vec<AccountBalance> = stmt
        .query_map([], |r| {
            Ok(AccountBalance {
                institution: r.get(0)?,
                account_number_last4: r.get(1)?,
                closing_balance: r.get(2)?,
                statement_period: r.get(3)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    let mut stmt = conn.prepare(
        "SELECT a.institution, a.account_number_last4, s.statement_period, \
                COUNT(t.id) as tx_count, s.imported_at \
         FROM statements s \
         JOIN accounts a ON s.account_id = a.id \
         LEFT JOIN transactions t ON t.statement_id = s.id \
         GROUP BY s.id \
         ORDER BY s.imported_at DESC \
         LIMIT 5",
    )?;
    let recent_imports: Vec<RecentImport> = stmt
        .query_map([], |r| {
            Ok(RecentImport {
                institution: r.get(0)?,
                account_number_last4: r.get(1)?,
                statement_period: r.get(2)?,
                transaction_count: r.get(3)?,
                imported_at: r.get(4)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(Some(DashboardData {
        spend_30d,
        spend_90d,
        account_balances,
        recent_imports,
    }))
}

fn do_get_dashboard(app: &AppHandle) -> Result<Option<DashboardData>> {
    let data_dir = app.path().app_data_dir()?;
    std::fs::create_dir_all(&data_dir)?;
    let db_path = data_dir.join("wealth.db");
    let conn = Connection::open(&db_path)?;
    conn.execute_batch(db::MIGRATION_001)?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    query_dashboard(&conn)
}

#[tauri::command]
pub async fn get_dashboard(app: AppHandle) -> Result<Option<DashboardData>, String> {
    tauri::async_runtime::spawn_blocking(move || do_get_dashboard(&app))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    fn open_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(db::MIGRATION_001).unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        conn
    }

    fn seed_account(conn: &Connection, institution: &str, last4: &str) -> i64 {
        conn.execute(
            "INSERT OR IGNORE INTO accounts (institution, account_number_last4) VALUES (?1, ?2)",
            params![institution, last4],
        )
        .unwrap();
        conn.query_row(
            "SELECT id FROM accounts WHERE institution = ?1 AND account_number_last4 = ?2",
            params![institution, last4],
            |r| r.get(0),
        )
        .unwrap()
    }

    fn seed_statement(
        conn: &Connection,
        account_id: i64,
        period: &str,
        closing_balance: Option<f64>,
        imported_at: &str,
    ) -> i64 {
        conn.execute(
            "INSERT INTO statements (account_id, statement_period, closing_balance, imported_at) \
             VALUES (?1, ?2, ?3, ?4)",
            params![account_id, period, closing_balance, imported_at],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    fn seed_transaction(conn: &Connection, stmt_id: i64, date: &str, amount: f64, kind: &str) {
        conn.execute(
            "INSERT INTO transactions (statement_id, date, description, category, amount, type) \
             VALUES (?1, ?2, 'test', 'test', ?3, ?4)",
            params![stmt_id, date, amount, kind],
        )
        .unwrap();
    }

    fn sqlite_date(conn: &Connection, modifier: &str) -> String {
        conn.query_row(
            &format!("SELECT date('now', '{modifier}')"),
            [],
            |r| r.get(0),
        )
        .unwrap()
    }

    #[test]
    fn empty_db_returns_none() {
        let conn = open_test_db();
        assert!(query_dashboard(&conn).unwrap().is_none());
    }

    #[test]
    fn spend_30d_counts_only_debits_in_window() {
        let conn = open_test_db();
        let acct = seed_account(&conn, "Test Bank", "1234");
        let stmt = seed_statement(&conn, acct, "2025-05", None, "2025-05-01T00:00:00");

        let recent = sqlite_date(&conn, "-5 days");
        let old = sqlite_date(&conn, "-100 days");

        // recent debit — should count
        seed_transaction(&conn, stmt, &recent, 50.0, "debit");
        // recent credit — should not count
        seed_transaction(&conn, stmt, &recent, 200.0, "credit");
        // old debit (>30 days) — should not count toward 30d
        seed_transaction(&conn, stmt, &old, 99.0, "debit");

        let data = query_dashboard(&conn).unwrap().unwrap();
        assert_eq!(data.spend_30d, 50.0);
    }

    #[test]
    fn spend_90d_includes_transactions_outside_30d_window() {
        let conn = open_test_db();
        let acct = seed_account(&conn, "Test Bank", "1234");
        let stmt = seed_statement(&conn, acct, "2025-02", None, "2025-02-01T00:00:00");

        let within_90d_not_30d = sqlite_date(&conn, "-60 days");
        let within_30d = sqlite_date(&conn, "-5 days");

        seed_transaction(&conn, stmt, &within_90d_not_30d, 75.0, "debit");
        seed_transaction(&conn, stmt, &within_30d, 25.0, "debit");

        let data = query_dashboard(&conn).unwrap().unwrap();
        assert_eq!(data.spend_30d, 25.0);
        assert_eq!(data.spend_90d, 100.0);
    }

    #[test]
    fn account_balance_reflects_most_recent_statement() {
        let conn = open_test_db();
        let acct = seed_account(&conn, "Big Bank", "9999");
        seed_statement(&conn, acct, "2024-11", Some(1000.0), "2024-11-01T00:00:00");
        seed_statement(&conn, acct, "2024-12", Some(850.5), "2024-12-01T00:00:00");

        let data = query_dashboard(&conn).unwrap().unwrap();
        assert_eq!(data.account_balances.len(), 1);
        assert_eq!(data.account_balances[0].closing_balance, Some(850.5));
        assert_eq!(data.account_balances[0].statement_period, "2024-12");
    }

    #[test]
    fn recent_imports_capped_at_5_newest_first() {
        let conn = open_test_db();
        let acct = seed_account(&conn, "My Bank", "0001");
        for i in 1..=7 {
            let ts = format!("2025-0{i}-01T00:00:00");
            let period = format!("2025-0{i}");
            seed_statement(&conn, acct, &period, None, &ts);
        }

        let data = query_dashboard(&conn).unwrap().unwrap();
        assert_eq!(data.recent_imports.len(), 5);
        assert_eq!(data.recent_imports[0].statement_period, "2025-07");
        assert_eq!(data.recent_imports[4].statement_period, "2025-03");
    }

    #[test]
    fn recent_import_transaction_count_correct() {
        let conn = open_test_db();
        let acct = seed_account(&conn, "Test Bank", "1234");
        let stmt = seed_statement(&conn, acct, "2025-05", None, "2025-05-01T00:00:00");
        seed_transaction(&conn, stmt, "2025-05-01", 10.0, "debit");
        seed_transaction(&conn, stmt, "2025-05-02", 20.0, "debit");
        seed_transaction(&conn, stmt, "2025-05-03", 100.0, "credit");

        let data = query_dashboard(&conn).unwrap().unwrap();
        assert_eq!(data.recent_imports[0].transaction_count, 3);
    }
}
