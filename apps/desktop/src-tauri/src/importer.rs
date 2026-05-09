use anyhow::Result;
use extractor::{extract_text, parse_line_items, AnthropicClient, ExtractionResult, TransactionType};
use rusqlite::{params, Connection, Transaction};
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

#[derive(Debug, Serialize)]
pub struct ImportSummary {
    pub institution: String,
    pub account_number_last4: String,
    pub statement_period: String,
    pub transaction_count: usize,
}

// ── DB write (testable inner fn) ──────────────────────────────────────────────

fn write_to_tx(tx: &Transaction<'_>, source_file: &str, result: &ExtractionResult) -> Result<ImportSummary> {
    let acct = &result.account;

    tx.execute(
        "INSERT OR IGNORE INTO accounts (institution, account_number_last4) VALUES (?1, ?2)",
        params![acct.institution, acct.account_number_last4],
    )?;

    let account_id: i64 = tx.query_row(
        "SELECT id FROM accounts WHERE institution = ?1 AND account_number_last4 = ?2",
        params![acct.institution, acct.account_number_last4],
        |row| row.get(0),
    )?;

    tx.execute(
        "INSERT OR IGNORE INTO statements \
         (account_id, statement_period, opening_balance, closing_balance, source_file) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            account_id,
            acct.statement_period,
            acct.opening_balance,
            acct.closing_balance,
            source_file,
        ],
    )?;
    let statement_inserted = tx.changes() > 0;

    let statement_id: i64 = tx.query_row(
        "SELECT id FROM statements WHERE account_id = ?1 AND statement_period = ?2",
        params![account_id, acct.statement_period],
        |row| row.get(0),
    )?;

    if statement_inserted {
        let mut stmt = tx.prepare(
            "INSERT INTO transactions (statement_id, date, description, category, amount, type) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?;
        for t in &result.transactions {
            let kind = match t.transaction_type {
                TransactionType::Debit => "debit",
                TransactionType::Credit => "credit",
            };
            stmt.execute(params![statement_id, t.date, t.description, t.category, t.amount, kind])?;
        }
    }

    Ok(ImportSummary {
        institution: acct.institution.clone(),
        account_number_last4: acct.account_number_last4.clone(),
        statement_period: acct.statement_period.clone(),
        transaction_count: if statement_inserted { result.transactions.len() } else { 0 },
    })
}

pub(crate) fn write_to_db(db_path: &Path, source_file: &str, result: &ExtractionResult) -> Result<ImportSummary> {
    let mut conn = Connection::open(db_path)?;
    conn.execute_batch(db::MIGRATION_001)?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    let tx = conn.transaction()?;
    let summary = write_to_tx(&tx, source_file, result)?;
    tx.commit()?;
    Ok(summary)
}

// ── Tauri command ─────────────────────────────────────────────────────────────

fn do_import(app: &AppHandle, path: &str) -> Result<ImportSummary> {
    let pdf_path = PathBuf::from(path);
    let data_dir = app.path().app_data_dir()?;
    std::fs::create_dir_all(&data_dir)?;
    let db_path = data_dir.join("wealth.db");
    let client = AnthropicClient::from_env()?;
    let text = extract_text(&pdf_path)?;
    let label = pdf_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(path);
    let result = parse_line_items(&text, label, &client)?;
    write_to_db(&db_path, path, &result)
}

#[tauri::command]
pub async fn import_statement(app: AppHandle, path: String) -> Result<ImportSummary, String> {
    tauri::async_runtime::spawn_blocking(move || do_import(&app, &path))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use extractor::{Account, ExtractionResult, Summary, Transaction, TransactionType};

    fn open_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(db::MIGRATION_001).unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        conn
    }

    fn fixture() -> ExtractionResult {
        ExtractionResult {
            account: Account {
                institution: "First National Bank".into(),
                account_number_last4: "4242".into(),
                statement_period: "2024-12".into(),
                opening_balance: Some(1000.0),
                closing_balance: Some(850.5),
            },
            transactions: vec![
                Transaction {
                    date: "2024-12-01".into(),
                    description: "WHOLE FOODS MARKET".into(),
                    category: "Groceries".into(),
                    amount: 87.32,
                    transaction_type: TransactionType::Debit,
                },
                Transaction {
                    date: "2024-12-03".into(),
                    description: "DIRECT DEPOSIT".into(),
                    category: "Income".into(),
                    amount: 2500.0,
                    transaction_type: TransactionType::Credit,
                },
            ],
            summary: Summary {
                total_debits: 87.32,
                total_credits: 2500.0,
                transaction_count: 2,
            },
        }
    }

    #[test]
    fn write_inserts_account_statement_transactions() {
        let mut conn = open_test_db();
        let tx = conn.transaction().unwrap();
        let summary = write_to_tx(&tx, "test.pdf", &fixture()).unwrap();
        tx.commit().unwrap();

        assert_eq!(summary.institution, "First National Bank");
        assert_eq!(summary.transaction_count, 2);

        let tx_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(tx_count, 2);
    }

    #[test]
    fn duplicate_account_is_not_duplicated() {
        let mut conn = open_test_db();

        for _ in 0..2 {
            let tx = conn.transaction().unwrap();
            write_to_tx(&tx, "test.pdf", &fixture()).unwrap();
            tx.commit().unwrap();
        }

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM accounts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn duplicate_statement_skips_transactions() {
        let mut conn = open_test_db();

        for _ in 0..2 {
            let tx = conn.transaction().unwrap();
            write_to_tx(&tx, "test.pdf", &fixture()).unwrap();
            tx.commit().unwrap();
        }

        // transactions inserted only once
        let tx_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(tx_count, 2);

        let stmt_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM statements", [], |r| r.get(0))
            .unwrap();
        assert_eq!(stmt_count, 1);
    }

    #[test]
    fn second_statement_same_account_inserts_correctly() {
        let mut conn = open_test_db();

        let tx = conn.transaction().unwrap();
        write_to_tx(&tx, "dec.pdf", &fixture()).unwrap();
        tx.commit().unwrap();

        let mut jan = fixture();
        jan.account.statement_period = "2025-01".into();

        let tx = conn.transaction().unwrap();
        let summary = write_to_tx(&tx, "jan.pdf", &jan).unwrap();
        tx.commit().unwrap();

        assert_eq!(summary.transaction_count, 2);

        let stmt_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM statements", [], |r| r.get(0))
            .unwrap();
        assert_eq!(stmt_count, 2);

        let tx_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(tx_count, 4);
    }
}
