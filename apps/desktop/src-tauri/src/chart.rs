use anyhow::Result;
use rusqlite::{params, Connection};
use serde::Serialize;
use tauri::{AppHandle, Manager};

#[derive(Debug, Serialize)]
pub struct BalancePoint {
    pub period: String,
    pub closing_balance: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct AccountSeries {
    pub institution: String,
    pub account_number_last4: String,
    pub account_type: Option<String>,
    pub points: Vec<BalancePoint>,
}

#[derive(Debug, Serialize)]
pub struct MonthlyFlow {
    pub period: String,
    pub income: f64,
    pub spend: f64,
}

#[derive(Debug, Serialize)]
pub struct ChartData {
    pub account_series: Vec<AccountSeries>,
    pub monthly_flows: Vec<MonthlyFlow>,
}

fn query_chart_data(conn: &Connection) -> Result<Option<ChartData>> {
    let account_count: i64 =
        conn.query_row("SELECT COUNT(*) FROM accounts", [], |r| r.get(0))?;
    if account_count == 0 {
        return Ok(None);
    }

    let mut acct_stmt = conn.prepare(
        "SELECT id, institution, account_number_last4, account_type \
         FROM accounts \
         ORDER BY institution, account_number_last4",
    )?;
    let accounts: Vec<(i64, String, String, Option<String>)> = acct_stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .filter_map(|r| r.ok())
        .collect();

    let mut account_series = Vec::new();
    for (account_id, institution, last4, account_type) in accounts {
        let mut pt_stmt = conn.prepare(
            "SELECT statement_period, closing_balance \
             FROM statements \
             WHERE account_id = ?1 \
               AND date(statement_period || '-01') >= date('now', '-12 months') \
             ORDER BY statement_period",
        )?;
        let points: Vec<BalancePoint> = pt_stmt
            .query_map(params![account_id], |r| {
                Ok(BalancePoint {
                    period: r.get(0)?,
                    closing_balance: r.get(1)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        if !points.is_empty() {
            account_series.push(AccountSeries {
                institution,
                account_number_last4: last4,
                account_type,
                points,
            });
        }
    }

    let mut flow_stmt = conn.prepare(
        "SELECT strftime('%Y-%m', t.date) AS month, \
                COALESCE(SUM(CASE WHEN t.type = 'credit' THEN t.amount ELSE 0.0 END), 0.0), \
                COALESCE(SUM(CASE WHEN t.type = 'debit'  THEN t.amount ELSE 0.0 END), 0.0) \
         FROM transactions t \
         WHERE date(t.date) >= date('now', '-12 months') \
         GROUP BY month \
         ORDER BY month",
    )?;
    let monthly_flows: Vec<MonthlyFlow> = flow_stmt
        .query_map([], |r| {
            Ok(MonthlyFlow {
                period: r.get(0)?,
                income: r.get(1)?,
                spend: r.get(2)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(Some(ChartData {
        account_series,
        monthly_flows,
    }))
}

fn do_get_chart_data(app: &AppHandle) -> Result<Option<ChartData>> {
    let data_dir = app.path().app_data_dir()?;
    std::fs::create_dir_all(&data_dir)?;
    let db_path = data_dir.join("wealth.db");
    let conn = Connection::open(&db_path)?;
    db::run_migrations(&conn)?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    query_chart_data(&conn)
}

#[tauri::command]
pub async fn get_chart_data(app: AppHandle) -> Result<Option<ChartData>, String> {
    tauri::async_runtime::spawn_blocking(move || do_get_chart_data(&app))
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

    fn seed_account(conn: &Connection, institution: &str, last4: &str, account_type: Option<&str>) -> i64 {
        conn.execute(
            "INSERT OR IGNORE INTO accounts (institution, account_number_last4, account_type) VALUES (?1, ?2, ?3)",
            params![institution, last4, account_type],
        )
        .unwrap();
        conn.query_row(
            "SELECT id FROM accounts WHERE institution = ?1 AND account_number_last4 = ?2",
            params![institution, last4],
            |r| r.get(0),
        )
        .unwrap()
    }

    fn seed_statement(conn: &Connection, account_id: i64, period: &str, closing: Option<f64>) -> i64 {
        conn.execute(
            "INSERT INTO statements (account_id, statement_period, closing_balance) VALUES (?1, ?2, ?3)",
            params![account_id, period, closing],
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

    fn sqlite_period(conn: &Connection, modifier: &str) -> String {
        conn.query_row(
            &format!("SELECT strftime('%Y-%m', date('now', '{modifier}'))"),
            [],
            |r| r.get(0),
        )
        .unwrap()
    }

    #[test]
    fn empty_db_returns_none() {
        let conn = open_test_db();
        assert!(query_chart_data(&conn).unwrap().is_none());
    }

    #[test]
    fn returns_account_series_with_balances() {
        let conn = open_test_db();
        let acct = seed_account(&conn, "Big Bank", "1234", Some("checking"));
        let period = sqlite_period(&conn, "-1 month");
        seed_statement(&conn, acct, &period, Some(5000.0));

        let data = query_chart_data(&conn).unwrap().unwrap();
        assert_eq!(data.account_series.len(), 1);
        assert_eq!(data.account_series[0].account_type, Some("checking".into()));
        assert_eq!(data.account_series[0].points[0].closing_balance, Some(5000.0));
    }

    #[test]
    fn excludes_statements_older_than_12_months() {
        let conn = open_test_db();
        let acct = seed_account(&conn, "Old Bank", "0000", Some("savings"));
        let old = sqlite_period(&conn, "-13 months");
        seed_statement(&conn, acct, &old, Some(9999.0));

        let data = query_chart_data(&conn).unwrap().unwrap();
        assert!(data.account_series.is_empty());
    }

    #[test]
    fn monthly_flows_aggregate_correctly() {
        let conn = open_test_db();
        let acct = seed_account(&conn, "Flow Bank", "5678", Some("checking"));
        let stmt = seed_statement(&conn, acct, &sqlite_period(&conn, "-1 month"), None);
        let recent = sqlite_date(&conn, "-20 days");
        seed_transaction(&conn, stmt, &recent, 3000.0, "credit");
        seed_transaction(&conn, stmt, &recent, 500.0, "debit");

        let data = query_chart_data(&conn).unwrap().unwrap();
        assert_eq!(data.monthly_flows.len(), 1);
        assert_eq!(data.monthly_flows[0].income, 3000.0);
        assert_eq!(data.monthly_flows[0].spend, 500.0);
    }
}
