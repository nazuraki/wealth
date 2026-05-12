use anyhow::Result;
use rusqlite::{Connection, params, types::ToSql};
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

#[derive(Debug, Serialize)]
pub struct TransactionPage {
    pub rows: Vec<TransactionRow>,
    pub total: i64,
}

#[derive(Debug, Deserialize)]
pub struct TransactionFilters {
    pub account_id: Option<i64>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub category: Option<String>,
    pub kinds: Option<Vec<String>>,
    pub offset: i64,
    pub limit: i64,
}

/// Build the WHERE clause and its parameter list from the filter.
/// Called twice per query (once for COUNT, once for rows), so returns owned params each time.
fn build_where(filters: &TransactionFilters) -> (String, Vec<Box<dyn ToSql>>) {
    let mut conditions: Vec<String> = vec![];
    let mut params: Vec<Box<dyn ToSql>> = vec![];

    if let Some(acct_id) = filters.account_id {
        conditions.push("a.id = ?".to_string());
        params.push(Box::new(acct_id));
    }
    if let Some(ref df) = filters.date_from {
        conditions.push("t.date >= ?".to_string());
        params.push(Box::new(df.clone()));
    }
    if let Some(ref dt) = filters.date_to {
        conditions.push("t.date <= ?".to_string());
        params.push(Box::new(dt.clone()));
    }
    if let Some(ref cat) = filters.category {
        if !cat.is_empty() {
            conditions.push("LOWER(t.category) LIKE LOWER(?)".to_string());
            params.push(Box::new(format!("%{cat}%")));
        }
    }
    if let Some(ref kinds) = filters.kinds {
        if !kinds.is_empty() {
            let placeholders = kinds.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
            conditions.push(format!("t.type IN ({placeholders})"));
            for k in kinds {
                params.push(Box::new(k.clone()));
            }
        }
    }

    let clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };
    (clause, params)
}

fn query_transaction_page(conn: &Connection, filters: &TransactionFilters) -> Result<TransactionPage> {
    let (where_clause, count_params) = build_where(filters);
    let (_, row_params) = build_where(filters);

    let count_sql = format!(
        "SELECT COUNT(*) \
         FROM transactions t \
         JOIN statements s ON t.statement_id = s.id \
         JOIN accounts a ON s.account_id = a.id \
         {where_clause}"
    );
    let count_refs: Vec<&dyn ToSql> = count_params.iter().map(|p| p.as_ref()).collect();
    let total: i64 = conn.query_row(&count_sql, count_refs.as_slice(), |r| r.get(0))?;

    let rows_sql = format!(
        "SELECT t.id, t.date, t.description, t.category, t.amount, t.type, \
                a.id, a.institution, a.account_number_last4 \
         FROM transactions t \
         JOIN statements s ON t.statement_id = s.id \
         JOIN accounts a ON s.account_id = a.id \
         {where_clause} \
         ORDER BY t.date DESC, t.id DESC \
         LIMIT ? OFFSET ?"
    );
    let mut row_refs: Vec<&dyn ToSql> = row_params.iter().map(|p| p.as_ref()).collect();
    let limit = filters.limit;
    let offset = filters.offset;
    row_refs.push(&limit);
    row_refs.push(&offset);

    let mut stmt = conn.prepare(&rows_sql)?;
    let rows: Vec<TransactionRow> = stmt
        .query_map(row_refs.as_slice(), |r| {
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

    Ok(TransactionPage { rows, total })
}

fn do_get_transactions(db_path: &Path, filters: TransactionFilters) -> Result<TransactionPage> {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(db_path)?;
    db::run_migrations(&conn)?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    query_transaction_page(&conn, &filters)
}

#[tauri::command]
pub async fn get_transactions(
    _app: AppHandle,
    db: State<'_, crate::DbPath>,
    filters: TransactionFilters,
) -> Result<TransactionPage, String> {
    let path = db.0.clone();
    tauri::async_runtime::spawn_blocking(move || do_get_transactions(&path, filters))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

fn do_update_transaction(db_path: &Path, id: i64, description: String, category: String) -> Result<()> {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(db_path)?;
    db::run_migrations(&conn)?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    conn.execute(
        "UPDATE transactions SET description = ?1, category = ?2 WHERE id = ?3",
        params![description, category, id],
    )?;
    Ok(())
}

#[tauri::command]
pub async fn update_transaction(
    _app: AppHandle,
    db: State<'_, crate::DbPath>,
    id: i64,
    description: String,
    category: String,
) -> Result<(), String> {
    let path = db.0.clone();
    tauri::async_runtime::spawn_blocking(move || do_update_transaction(&path, id, description, category))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

fn do_get_categories(db_path: &Path) -> Result<Vec<String>> {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(db_path)?;
    db::run_migrations(&conn)?;
    let mut stmt = conn.prepare(
        "SELECT DISTINCT category FROM transactions WHERE category != '' ORDER BY category COLLATE NOCASE",
    )?;
    let cats: Vec<String> = stmt
        .query_map([], |r| r.get(0))?
        .filter_map(|r| r.ok())
        .collect();
    Ok(cats)
}

#[tauri::command]
pub async fn get_categories(
    _app: AppHandle,
    db: State<'_, crate::DbPath>,
) -> Result<Vec<String>, String> {
    let path = db.0.clone();
    tauri::async_runtime::spawn_blocking(move || do_get_categories(&path))
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
        TransactionFilters {
            account_id: None,
            date_from: None,
            date_to: None,
            category: None,
            kinds: None,
            offset: 0,
            limit: 1000,
        }
    }

    #[test]
    fn empty_db_returns_empty() {
        let conn = open_test_db();
        let page = query_transaction_page(&conn, &no_filters()).unwrap();
        assert!(page.rows.is_empty());
        assert_eq!(page.total, 0);
    }

    #[test]
    fn all_transactions_returned_with_no_filters() {
        let conn = open_test_db();
        let acct = seed_account(&conn, "Bank A", "1111");
        let stmt = seed_statement(&conn, acct, "2025-01");
        seed_tx(&conn, stmt, "2025-01-10", "Coffee", "Food", 5.0, "debit");
        seed_tx(&conn, stmt, "2025-01-15", "Salary", "Income", 3000.0, "credit");

        let page = query_transaction_page(&conn, &no_filters()).unwrap();
        assert_eq!(page.total, 2);
        assert_eq!(page.rows.len(), 2);
        assert_eq!(page.rows[0].date, "2025-01-15");
        assert_eq!(page.rows[1].date, "2025-01-10");
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
        let page = query_transaction_page(&conn, &filters).unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.rows[0].account_id, acct_a);
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
        let page = query_transaction_page(&conn, &filters).unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.rows[0].description, "Mid");
    }

    #[test]
    fn filter_by_category_case_insensitive_substring() {
        let conn = open_test_db();
        let acct = seed_account(&conn, "Bank A", "1111");
        let stmt = seed_statement(&conn, acct, "2025-01");
        seed_tx(&conn, stmt, "2025-01-10", "McDonald's", "Fast Food", 12.0, "debit");
        seed_tx(&conn, stmt, "2025-01-11", "Amazon", "Shopping", 50.0, "debit");

        let filters = TransactionFilters { category: Some("food".to_string()), ..no_filters() };
        let page = query_transaction_page(&conn, &filters).unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.rows[0].description, "McDonald's");
    }

    #[test]
    fn filter_by_kinds_single() {
        let conn = open_test_db();
        let acct = seed_account(&conn, "Bank A", "1111");
        let stmt = seed_statement(&conn, acct, "2025-01");
        seed_tx(&conn, stmt, "2025-01-10", "Coffee", "Food", 5.0, "debit");
        seed_tx(&conn, stmt, "2025-01-11", "Salary", "Income", 3000.0, "credit");

        let filters = TransactionFilters { kinds: Some(vec!["credit".to_string()]), ..no_filters() };
        let page = query_transaction_page(&conn, &filters).unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.rows[0].kind, "credit");
    }

    #[test]
    fn filter_by_kinds_multiple() {
        let conn = open_test_db();
        let acct = seed_account(&conn, "Bank A", "1111");
        let stmt = seed_statement(&conn, acct, "2025-01");
        seed_tx(&conn, stmt, "2025-01-10", "Coffee", "Food", 5.0, "debit");
        seed_tx(&conn, stmt, "2025-01-11", "Salary", "Income", 3000.0, "credit");
        seed_tx(&conn, stmt, "2025-01-12", "Transfer Out", "Transfer", 500.0, "transfer");

        let filters = TransactionFilters {
            kinds: Some(vec!["debit".to_string(), "credit".to_string()]),
            ..no_filters()
        };
        let page = query_transaction_page(&conn, &filters).unwrap();
        assert_eq!(page.total, 2);
        assert!(page.rows.iter().all(|r| r.kind != "transfer"));
    }

    #[test]
    fn pagination_offset_and_limit() {
        let conn = open_test_db();
        let acct = seed_account(&conn, "Bank A", "1111");
        let stmt = seed_statement(&conn, acct, "2025-01");
        for d in 1..=10u32 {
            seed_tx(&conn, stmt, &format!("2025-01-{d:02}"), "Tx", "Misc", d as f64, "debit");
        }

        let filters = TransactionFilters { limit: 3, offset: 2, ..no_filters() };
        let page = query_transaction_page(&conn, &filters).unwrap();
        assert_eq!(page.total, 10);  // total reflects all matching rows
        assert_eq!(page.rows.len(), 3);
        // sorted date DESC: day 10..1; offset 2 skips 10 and 9, so first row is day 8
        assert_eq!(page.rows[0].description, "Tx");
        assert_eq!(page.rows[0].amount, 8.0);
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
            kinds: Some(vec!["debit".to_string()]),
            ..no_filters()
        };
        let page = query_transaction_page(&conn, &filters).unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.rows[0].description, "Coffee");
        assert_eq!(page.rows[0].account_id, acct_a);
    }

    #[test]
    fn no_matching_filters_returns_empty() {
        let conn = open_test_db();
        let acct = seed_account(&conn, "Bank A", "1111");
        let stmt = seed_statement(&conn, acct, "2025-01");
        seed_tx(&conn, stmt, "2025-01-10", "Coffee", "Food", 5.0, "debit");

        let filters = TransactionFilters {
            kinds: Some(vec!["transfer".to_string()]),
            ..no_filters()
        };
        let page = query_transaction_page(&conn, &filters).unwrap();
        assert_eq!(page.total, 0);
        assert!(page.rows.is_empty());
    }

    fn query_tx(conn: &Connection, id: i64) -> (String, String) {
        conn.query_row(
            "SELECT description, category FROM transactions WHERE id = ?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
    }

    fn query_categories(conn: &Connection) -> Vec<String> {
        let mut stmt = conn
            .prepare("SELECT DISTINCT category FROM transactions WHERE category != '' ORDER BY category COLLATE NOCASE")
            .unwrap();
        stmt.query_map([], |r| r.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect()
    }

    #[test]
    fn update_transaction_persists_description_and_category() {
        let conn = open_test_db();
        let acct = seed_account(&conn, "Bank A", "1111");
        let stmt = seed_statement(&conn, acct, "2025-01");
        let tx_id = seed_tx(&conn, stmt, "2025-01-10", "Coffee", "Food", 5.0, "debit");

        conn.execute(
            "UPDATE transactions SET description = ?1, category = ?2 WHERE id = ?3",
            params!["Espresso", "Dining", tx_id],
        )
        .unwrap();

        let (desc, cat) = query_tx(&conn, tx_id);
        assert_eq!(desc, "Espresso");
        assert_eq!(cat, "Dining");
    }

    #[test]
    fn update_transaction_nonexistent_id_is_no_op() {
        let conn = open_test_db();
        // No rows — updating a nonexistent id should not error
        let affected = conn
            .execute(
                "UPDATE transactions SET description = ?1, category = ?2 WHERE id = ?3",
                params!["X", "Y", 9999i64],
            )
            .unwrap();
        assert_eq!(affected, 0);
    }

    #[test]
    fn get_categories_returns_distinct_sorted() {
        let conn = open_test_db();
        let acct = seed_account(&conn, "Bank A", "1111");
        let stmt = seed_statement(&conn, acct, "2025-01");
        seed_tx(&conn, stmt, "2025-01-10", "Coffee", "Food", 5.0, "debit");
        seed_tx(&conn, stmt, "2025-01-11", "Salary", "Income", 3000.0, "credit");
        seed_tx(&conn, stmt, "2025-01-12", "Burger", "food", 12.0, "debit"); // duplicate category different case

        let cats = query_categories(&conn);
        // COLLATE NOCASE deduplication: "Food" and "food" are considered equal by ORDER BY,
        // but DISTINCT still returns both since SQLite DISTINCT is case-sensitive.
        // The test just verifies they are returned in case-insensitive order.
        assert!(cats.contains(&"Food".to_string()) || cats.contains(&"food".to_string()));
        assert!(cats.contains(&"Income".to_string()));
        // "Food"/"food" should come before "Income" in case-insensitive order
        let food_pos = cats.iter().position(|c| c.eq_ignore_ascii_case("food")).unwrap();
        let income_pos = cats.iter().position(|c| c == "Income").unwrap();
        assert!(food_pos < income_pos);
    }
}
