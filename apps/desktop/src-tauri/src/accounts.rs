use anyhow::Result;
use rusqlite::{params, Connection};
use serde::Serialize;
use tauri::{AppHandle, Manager};

#[derive(Debug, Serialize)]
pub struct Account {
    pub id: i64,
    pub institution: String,
    pub account_number_last4: String,
    pub account_type: Option<String>,
    pub display_name: Option<String>,
    pub color: Option<String>,
    pub closing_balance: Option<f64>,
    pub statement_period: Option<String>,
}

fn open_conn(app: &AppHandle) -> Result<Connection> {
    let data_dir = app.path().app_data_dir()?;
    std::fs::create_dir_all(&data_dir)?;
    let conn = Connection::open(data_dir.join("wealth.db"))?;
    db::run_migrations(&conn)?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    Ok(conn)
}

fn query_accounts(conn: &Connection) -> Result<Vec<Account>> {
    let mut stmt = conn.prepare(
        "SELECT a.id, a.institution, a.account_number_last4, a.account_type, \
                a.display_name, a.color, \
                s.closing_balance, s.statement_period \
         FROM accounts a \
         LEFT JOIN statements s ON s.account_id = a.id \
           AND s.imported_at = ( \
               SELECT MAX(s2.imported_at) FROM statements s2 WHERE s2.account_id = a.id \
           ) \
         ORDER BY a.institution, a.account_number_last4",
    )?;
    let accounts = stmt
        .query_map([], |r| {
            Ok(Account {
                id: r.get(0)?,
                institution: r.get(1)?,
                account_number_last4: r.get(2)?,
                account_type: r.get(3)?,
                display_name: r.get(4)?,
                color: r.get(5)?,
                closing_balance: r.get(6)?,
                statement_period: r.get(7)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(accounts)
}

fn do_update_account(
    conn: &Connection,
    id: i64,
    display_name: Option<String>,
    color: Option<String>,
) -> Result<()> {
    conn.execute(
        "UPDATE accounts SET display_name = ?1, color = ?2 WHERE id = ?3",
        params![display_name, color, id],
    )?;
    Ok(())
}

#[tauri::command]
pub async fn get_accounts(app: AppHandle) -> Result<Vec<Account>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let conn = open_conn(&app)?;
        query_accounts(&conn)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_account(
    app: AppHandle,
    id: i64,
    display_name: Option<String>,
    color: Option<String>,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let conn = open_conn(&app)?;
        do_update_account(&conn, id, display_name, color)
    })
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

    fn seed_statement(conn: &Connection, account_id: i64, period: &str, closing: Option<f64>, imported_at: &str) {
        conn.execute(
            "INSERT INTO statements (account_id, statement_period, closing_balance, imported_at) \
             VALUES (?1, ?2, ?3, ?4)",
            params![account_id, period, closing, imported_at],
        )
        .unwrap();
    }

    #[test]
    fn get_accounts_returns_all_with_latest_balance() {
        let conn = open_test_db();
        let id = seed_account(&conn, "First Bank", "1234");
        seed_statement(&conn, id, "2025-01", Some(1000.0), "2025-01-01T00:00:00");
        seed_statement(&conn, id, "2025-02", Some(1200.0), "2025-02-01T00:00:00");

        let accounts = query_accounts(&conn).unwrap();
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].institution, "First Bank");
        assert_eq!(accounts[0].closing_balance, Some(1200.0));
        assert_eq!(accounts[0].statement_period, Some("2025-02".to_string()));
        assert!(accounts[0].display_name.is_none());
        assert!(accounts[0].color.is_none());
    }

    #[test]
    fn update_account_persists_name_and_color() {
        let conn = open_test_db();
        let id = seed_account(&conn, "Big Bank", "5678");

        do_update_account(&conn, id, Some("My Checking".to_string()), Some("#ff0000".to_string())).unwrap();

        let accounts = query_accounts(&conn).unwrap();
        assert_eq!(accounts[0].display_name, Some("My Checking".to_string()));
        assert_eq!(accounts[0].color, Some("#ff0000".to_string()));
    }

    #[test]
    fn update_account_can_clear_fields() {
        let conn = open_test_db();
        let id = seed_account(&conn, "Clear Bank", "0000");
        do_update_account(&conn, id, Some("Name".to_string()), Some("#abc".to_string())).unwrap();
        do_update_account(&conn, id, None, None).unwrap();

        let accounts = query_accounts(&conn).unwrap();
        assert!(accounts[0].display_name.is_none());
        assert!(accounts[0].color.is_none());
    }

    #[test]
    fn get_accounts_no_statements_returns_null_balance() {
        let conn = open_test_db();
        seed_account(&conn, "Empty Bank", "9999");

        let accounts = query_accounts(&conn).unwrap();
        assert_eq!(accounts.len(), 1);
        assert!(accounts[0].closing_balance.is_none());
        assert!(accounts[0].statement_period.is_none());
    }
}
