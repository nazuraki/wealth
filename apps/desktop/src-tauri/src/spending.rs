use anyhow::Result;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tauri::{AppHandle, State};

#[derive(Debug, Deserialize)]
pub struct SpendingFilters {
    pub date_from: String,
    pub date_to: String,
    pub account_id: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct CategorySpend {
    pub category: String,
    pub total: f64,
    pub percentage: f64,
}

fn query_spending(conn: &Connection, filters: &SpendingFilters) -> Result<Vec<CategorySpend>> {
    let mut conditions = vec![
        "t.date >= ?1".to_string(),
        "t.date <= ?2".to_string(),
        "t.type = 'debit'".to_string(),
        "t.is_transfer = 0".to_string(),
    ];

    let account_id_param: Option<i64> = filters.account_id;
    if account_id_param.is_some() {
        conditions.push("a.id = ?3".to_string());
    }

    let where_clause = format!("WHERE {}", conditions.join(" AND "));

    let sql = format!(
        "SELECT t.category, SUM(t.amount) AS total \
         FROM transactions t \
         JOIN statements s ON t.statement_id = s.id \
         JOIN accounts a ON s.account_id = a.id \
         {where_clause} \
         GROUP BY t.category \
         ORDER BY total DESC"
    );

    let mut stmt = conn.prepare(&sql)?;

    let rows: Vec<(String, f64)> = if let Some(acct_id) = account_id_param {
        stmt.query_map(
            params![filters.date_from, filters.date_to, acct_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?
        .filter_map(|r| r.ok())
        .collect()
    } else {
        stmt.query_map(
            params![filters.date_from, filters.date_to],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?
        .filter_map(|r| r.ok())
        .collect()
    };

    let grand_total: f64 = rows.iter().map(|(_, t)| t).sum();

    Ok(rows
        .into_iter()
        .map(|(category, total)| CategorySpend {
            percentage: if grand_total > 0.0 {
                (total / grand_total * 100.0 * 10.0).round() / 10.0
            } else {
                0.0
            },
            category,
            total,
        })
        .collect())
}

fn do_get_spending_by_category(db_path: &Path, filters: SpendingFilters) -> Result<Vec<CategorySpend>> {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(db_path)?;
    db::run_migrations(&conn)?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    query_spending(&conn, &filters)
}

#[tauri::command]
pub async fn get_spending_by_category(
    _app: AppHandle,
    db: State<'_, crate::DbPath>,
    filters: SpendingFilters,
) -> Result<Vec<CategorySpend>, String> {
    let path = db.0.clone();
    tauri::async_runtime::spawn_blocking(move || do_get_spending_by_category(&path, filters))
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
        db::run_migrations(&conn).unwrap();
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

    fn seed_statement(conn: &Connection, account_id: i64, period: &str) -> i64 {
        conn.execute(
            "INSERT INTO statements (account_id, statement_period, imported_at) VALUES (?1, ?2, '2025-01-01T00:00:00')",
            params![account_id, period],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    fn seed_tx(conn: &Connection, stmt_id: i64, date: &str, category: &str, amount: f64, kind: &str, is_transfer: i64) {
        conn.execute(
            "INSERT INTO transactions (statement_id, date, description, category, amount, type, is_transfer) \
             VALUES (?1, ?2, '', ?3, ?4, ?5, ?6)",
            params![stmt_id, date, category, amount, kind, is_transfer],
        )
        .unwrap();
    }

    fn base_filters() -> SpendingFilters {
        SpendingFilters {
            date_from: "2025-01-01".to_string(),
            date_to: "2025-12-31".to_string(),
            account_id: None,
        }
    }

    #[test]
    fn empty_db_returns_empty() {
        let conn = open_test_db();
        let rows = query_spending(&conn, &base_filters()).unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn aggregates_by_category_ordered_by_total_desc() {
        let conn = open_test_db();
        let acct = seed_account(&conn, "Bank A", "1111");
        let stmt = seed_statement(&conn, acct, "2025-01");
        seed_tx(&conn, stmt, "2025-01-10", "Dining", 20.0, "debit", 0);
        seed_tx(&conn, stmt, "2025-01-11", "Dining", 30.0, "debit", 0);
        seed_tx(&conn, stmt, "2025-01-12", "Shopping", 100.0, "debit", 0);

        let rows = query_spending(&conn, &base_filters()).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].category, "Shopping");
        assert_eq!(rows[0].total, 100.0);
        assert_eq!(rows[1].category, "Dining");
        assert_eq!(rows[1].total, 50.0);
    }

    #[test]
    fn percentages_sum_to_100() {
        let conn = open_test_db();
        let acct = seed_account(&conn, "Bank A", "1111");
        let stmt = seed_statement(&conn, acct, "2025-01");
        seed_tx(&conn, stmt, "2025-01-10", "Dining", 25.0, "debit", 0);
        seed_tx(&conn, stmt, "2025-01-11", "Shopping", 75.0, "debit", 0);

        let rows = query_spending(&conn, &base_filters()).unwrap();
        let sum: f64 = rows.iter().map(|r| r.percentage).sum();
        assert!((sum - 100.0).abs() < 0.2, "percentages summed to {sum}");
    }

    #[test]
    fn excludes_credits() {
        let conn = open_test_db();
        let acct = seed_account(&conn, "Bank A", "1111");
        let stmt = seed_statement(&conn, acct, "2025-01");
        seed_tx(&conn, stmt, "2025-01-10", "Dining", 50.0, "debit", 0);
        seed_tx(&conn, stmt, "2025-01-11", "Income", 3000.0, "credit", 0);

        let rows = query_spending(&conn, &base_filters()).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].category, "Dining");
    }

    #[test]
    fn excludes_transfers() {
        let conn = open_test_db();
        let acct = seed_account(&conn, "Bank A", "1111");
        let stmt = seed_statement(&conn, acct, "2025-01");
        seed_tx(&conn, stmt, "2025-01-10", "Dining", 50.0, "debit", 0);
        seed_tx(&conn, stmt, "2025-01-11", "Transfer", 500.0, "debit", 1);

        let rows = query_spending(&conn, &base_filters()).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].category, "Dining");
    }

    #[test]
    fn date_range_filters_correctly() {
        let conn = open_test_db();
        let acct = seed_account(&conn, "Bank A", "1111");
        let stmt = seed_statement(&conn, acct, "2025-01");
        seed_tx(&conn, stmt, "2024-12-31", "OldYearDining", 10.0, "debit", 0);
        seed_tx(&conn, stmt, "2025-01-15", "Dining", 50.0, "debit", 0);
        seed_tx(&conn, stmt, "2025-02-01", "FutureGas", 40.0, "debit", 0);

        let filters = SpendingFilters {
            date_from: "2025-01-01".to_string(),
            date_to: "2025-01-31".to_string(),
            account_id: None,
        };
        let rows = query_spending(&conn, &filters).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].category, "Dining");
    }

    #[test]
    fn account_filter_narrows_results() {
        let conn = open_test_db();
        let acct_a = seed_account(&conn, "Bank A", "1111");
        let acct_b = seed_account(&conn, "Bank B", "2222");
        let stmt_a = seed_statement(&conn, acct_a, "2025-01");
        let stmt_b = seed_statement(&conn, acct_b, "2025-01");
        seed_tx(&conn, stmt_a, "2025-01-10", "Dining", 50.0, "debit", 0);
        seed_tx(&conn, stmt_b, "2025-01-10", "Gas", 40.0, "debit", 0);

        let filters = SpendingFilters { account_id: Some(acct_a), ..base_filters() };
        let rows = query_spending(&conn, &filters).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].category, "Dining");
    }

    #[test]
    fn custom_date_range_works() {
        let conn = open_test_db();
        let acct = seed_account(&conn, "Bank A", "1111");
        let stmt = seed_statement(&conn, acct, "2025-01");
        seed_tx(&conn, stmt, "2025-03-01", "Dining", 60.0, "debit", 0);
        seed_tx(&conn, stmt, "2025-06-15", "Gas", 40.0, "debit", 0);
        seed_tx(&conn, stmt, "2025-09-01", "Shopping", 80.0, "debit", 0);

        let filters = SpendingFilters {
            date_from: "2025-04-01".to_string(),
            date_to: "2025-07-31".to_string(),
            account_id: None,
        };
        let rows = query_spending(&conn, &filters).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].category, "Gas");
    }

    #[test]
    fn no_spend_returns_empty_percentages() {
        let conn = open_test_db();
        let rows = query_spending(&conn, &base_filters()).unwrap();
        assert!(rows.is_empty());
    }
}
