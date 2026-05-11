use anyhow::Result;
use rusqlite::{Connection, types::ToSql};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tauri::{AppHandle, State};

#[derive(Debug, Serialize)]
pub struct TransactionRow {
    pub id: i64,
    pub date: String,
    pub description: String,
    pub category: String,
    pub amount: f64,
    pub kind: String,
    pub account_id: i64,
    pub institution: String,
    pub account_number_last4: String,
}

#[derive(Debug, Deserialize)]
pub struct TransactionFilters {
    pub account_id: Option<i64>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub category: Option<String>,
    pub kind: Option<String>,
}

fn query_transactions(conn: &Connection, filters: &TransactionFilters) -> Result<Vec<TransactionRow>> {
    let mut conditions: Vec<&str> = vec![];
    let mut params: Vec<Box<dyn ToSql>> = vec![];

    if let Some(acct_id) = filters.account_id {
        conditions.push("a.id = ?");
        params.push(Box::new(acct_id));
    }
    if let Some(ref df) = filters.date_from {
        conditions.push("t.date >= ?");
        params.push(Box::new(df.clone()));
    }
    if let Some(ref dt) = filters.date_to {
        conditions.push("t.date <= ?");
        params.push(Box::new(dt.clone()));
    }
    if let Some(cat) = &filters.category {
        if !cat.is_empty() {
            conditions.push("LOWER(t.category) LIKE LOWER(?)");
            params.push(Box::new(format!("%{cat}%")));
        }
    }
    if let Some(ref k) = filters.kind {
        conditions.push("t.type = ?");
        params.push(Box::new(k.clone()));
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    let sql = format!(
        "SELECT t.id, t.date, t.description, t.category, t.amount, t.type, \
                a.id, a.institution, a.account_number_last4 \
         FROM transactions t \
         JOIN statements s ON t.statement_id = s.id \
         JOIN accounts a ON s.account_id = a.id \
         {where_clause} \
         ORDER BY t.date DESC, t.id DESC"
    );

    let param_refs: Vec<&dyn ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let rows: Vec<TransactionRow> = stmt
        .query_map(param_refs.as_slice(), |r| {
            Ok(TransactionRow {
                id: r.get(0)?,
                date: r.get(1)?,
                description: r.get(2)?,
                category: r.get(3)?,
                amount: r.get(4)?,
                kind: r.get(5)?,
                account_id: r.get(6)?,
                institution: r.get(7)?,
                account_number_last4: r.get(8)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(rows)
}

fn do_get_transactions(db_path: &Path, filters: TransactionFilters) -> Result<Vec<TransactionRow>> {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(db_path)?;
    db::run_migrations(&conn)?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    query_transactions(&conn, &filters)
}

#[tauri::command]
pub async fn get_transactions(
    _app: AppHandle,
    db: State<'_, crate::DbPath>,
    filters: TransactionFilters,
) -> Result<Vec<TransactionRow>, String> {
    let path = db.0.clone();
    tauri::async_runtime::spawn_blocking(move || do_get_transactions(&path, filters))
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

    fn seed_tx(conn: &Connection, stmt_id: i64, date: &str, desc: &str, category: &str, amount: f64, kind: &str) -> i64 {
        conn.execute(
            "INSERT INTO transactions (statement_id, date, description, category, amount, type) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![stmt_id, date, desc, category, amount, kind],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    fn no_filters() -> TransactionFilters {
        TransactionFilters { account_id: None, date_from: None, date_to: None, category: None, kind: None }
    }

    #[test]
    fn empty_db_returns_empty() {
        let conn = open_test_db();
        let rows = query_transactions(&conn, &no_filters()).unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn all_transactions_returned_with_no_filters() {
        let conn = open_test_db();
        let acct = seed_account(&conn, "Bank A", "1111");
        let stmt = seed_statement(&conn, acct, "2025-01");
        seed_tx(&conn, stmt, "2025-01-10", "Coffee", "Food", 5.0, "debit");
        seed_tx(&conn, stmt, "2025-01-15", "Salary", "Income", 3000.0, "credit");

        let rows = query_transactions(&conn, &no_filters()).unwrap();
        assert_eq!(rows.len(), 2);
        // sorted by date desc
        assert_eq!(rows[0].date, "2025-01-15");
        assert_eq!(rows[1].date, "2025-01-10");
    }

    #[test]
    fn filter_by_account_id() {
        let conn = open_test_db();
        let acct_a = seed_account(&conn, "Bank A", "1111");
        let acct_b = seed_account(&conn, "Bank B", "2222");
        let stmt_a = seed_statement(&conn, acct_a, "2025-01");
        let stmt_b = seed_statement(&conn, acct_b, "2025-01");
        seed_tx(&conn, stmt_a, "2025-01-10", "Coffee", "Food", 5.0, "debit");
        seed_tx(&conn, stmt_b, "2025-01-11", "Gas", "Auto", 40.0, "debit");

        let filters = TransactionFilters { account_id: Some(acct_a), ..no_filters() };
        let rows = query_transactions(&conn, &filters).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].account_id, acct_a);
    }

    #[test]
    fn filter_by_date_range() {
        let conn = open_test_db();
        let acct = seed_account(&conn, "Bank A", "1111");
        let stmt = seed_statement(&conn, acct, "2025-01");
        seed_tx(&conn, stmt, "2025-01-01", "Old", "Misc", 1.0, "debit");
        seed_tx(&conn, stmt, "2025-01-15", "Mid", "Misc", 2.0, "debit");
        seed_tx(&conn, stmt, "2025-01-31", "New", "Misc", 3.0, "debit");

        let filters = TransactionFilters {
            date_from: Some("2025-01-10".to_string()),
            date_to: Some("2025-01-20".to_string()),
            ..no_filters()
        };
        let rows = query_transactions(&conn, &filters).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].description, "Mid");
    }

    #[test]
    fn filter_by_category_case_insensitive_substring() {
        let conn = open_test_db();
        let acct = seed_account(&conn, "Bank A", "1111");
        let stmt = seed_statement(&conn, acct, "2025-01");
        seed_tx(&conn, stmt, "2025-01-10", "McDonald's", "Fast Food", 12.0, "debit");
        seed_tx(&conn, stmt, "2025-01-11", "Amazon", "Shopping", 50.0, "debit");

        let filters = TransactionFilters { category: Some("food".to_string()), ..no_filters() };
        let rows = query_transactions(&conn, &filters).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].description, "McDonald's");
    }

    #[test]
    fn filter_by_type() {
        let conn = open_test_db();
        let acct = seed_account(&conn, "Bank A", "1111");
        let stmt = seed_statement(&conn, acct, "2025-01");
        seed_tx(&conn, stmt, "2025-01-10", "Coffee", "Food", 5.0, "debit");
        seed_tx(&conn, stmt, "2025-01-11", "Salary", "Income", 3000.0, "credit");

        let filters = TransactionFilters { kind: Some("credit".to_string()), ..no_filters() };
        let rows = query_transactions(&conn, &filters).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, "credit");
    }

    #[test]
    fn combined_filters() {
        let conn = open_test_db();
        let acct_a = seed_account(&conn, "Bank A", "1111");
        let acct_b = seed_account(&conn, "Bank B", "2222");
        let stmt_a = seed_statement(&conn, acct_a, "2025-01");
        let stmt_b = seed_statement(&conn, acct_b, "2025-01");
        seed_tx(&conn, stmt_a, "2025-01-10", "Coffee", "Food", 5.0, "debit");
        seed_tx(&conn, stmt_a, "2025-01-11", "Salary", "Income", 3000.0, "credit");
        seed_tx(&conn, stmt_b, "2025-01-12", "Coffee", "Food", 6.0, "debit");

        let filters = TransactionFilters {
            account_id: Some(acct_a),
            kind: Some("debit".to_string()),
            ..no_filters()
        };
        let rows = query_transactions(&conn, &filters).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].description, "Coffee");
        assert_eq!(rows[0].account_id, acct_a);
    }

    #[test]
    fn no_matching_filters_returns_empty() {
        let conn = open_test_db();
        let acct = seed_account(&conn, "Bank A", "1111");
        let stmt = seed_statement(&conn, acct, "2025-01");
        seed_tx(&conn, stmt, "2025-01-10", "Coffee", "Food", 5.0, "debit");

        let filters = TransactionFilters { kind: Some("transfer".to_string()), ..no_filters() };
        let rows = query_transactions(&conn, &filters).unwrap();
        assert!(rows.is_empty());
    }
}
