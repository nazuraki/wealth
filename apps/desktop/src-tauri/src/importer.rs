use anyhow::Result;
use extractor::{
    extract_text, parse_line_items, AccountExtraction, AnthropicClient, ExtractionResult,
    TransactionType,
};
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

fn write_account_to_tx(
    tx: &Transaction<'_>,
    source_file: &str,
    extraction: &AccountExtraction,
) -> Result<ImportSummary> {
    let acct = &extraction.account;

    // Accounts are keyed on last4 only — institution name variants of the same card map to one record.
    tx.execute(
        "INSERT INTO accounts (institution, account_number_last4, account_type) \
         SELECT ?1, ?2, ?3 WHERE NOT EXISTS (SELECT 1 FROM accounts WHERE account_number_last4 = ?2)",
        params![acct.institution, acct.account_number_last4, acct.account_type],
    )?;
    if acct.account_type.is_some() {
        tx.execute(
            "UPDATE accounts SET account_type = ?1 WHERE account_number_last4 = ?2 AND account_type IS NULL",
            params![acct.account_type, acct.account_number_last4],
        )?;
    }

    let account_id: i64 = tx.query_row(
        "SELECT id FROM accounts WHERE account_number_last4 = ?1",
        params![acct.account_number_last4],
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
        for t in &extraction.transactions {
            let kind = match t.transaction_type {
                TransactionType::Debit => "debit",
                TransactionType::Credit => "credit",
            };
            stmt.execute(params![
                statement_id,
                t.date,
                t.description,
                t.category,
                t.amount,
                kind
            ])?;
        }
    }

    Ok(ImportSummary {
        institution: acct.institution.clone(),
        account_number_last4: acct.account_number_last4.clone(),
        statement_period: acct.statement_period.clone(),
        transaction_count: if statement_inserted {
            extraction.transactions.len()
        } else {
            0
        },
    })
}

fn write_to_tx(
    tx: &Transaction<'_>,
    source_file: &str,
    result: &ExtractionResult,
) -> Result<Vec<ImportSummary>> {
    result
        .accounts
        .iter()
        .map(|a| write_account_to_tx(tx, source_file, a))
        .collect()
}

pub(crate) fn write_to_db(
    db_path: &Path,
    source_file: &str,
    result: &ExtractionResult,
) -> Result<Vec<ImportSummary>> {
    let mut conn = Connection::open(db_path)?;
    db::run_migrations(&conn)?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    let tx = conn.transaction()?;
    let summaries = write_to_tx(&tx, source_file, result)?;
    tx.commit()?;
    Ok(summaries)
}

// ── Tauri command ─────────────────────────────────────────────────────────────

fn do_import(app: &AppHandle, path: &str) -> Result<Vec<ImportSummary>> {
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
pub async fn import_statement(app: AppHandle, path: String) -> Result<Vec<ImportSummary>, String> {
    tauri::async_runtime::spawn_blocking(move || do_import(&app, &path))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use extractor::{Account, AccountExtraction, ExtractionResult, Summary, Transaction, TransactionType};

    fn open_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        db::run_migrations(&conn).unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        conn
    }

    fn make_extraction(last4: &str, period: &str, account_type: &str) -> ExtractionResult {
        ExtractionResult {
            accounts: vec![AccountExtraction {
                account: Account {
                    institution: "First National Bank".into(),
                    account_number_last4: last4.into(),
                    account_type: Some(account_type.into()),
                    statement_period: period.into(),
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
            }],
        }
    }

    fn fixture() -> ExtractionResult {
        make_extraction("4242", "2024-12", "checking")
    }

    #[test]
    fn write_inserts_account_statement_transactions() {
        let mut conn = open_test_db();
        let tx = conn.transaction().unwrap();
        let summaries = write_to_tx(&tx, "test.pdf", &fixture()).unwrap();
        tx.commit().unwrap();

        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].institution, "First National Bank");
        assert_eq!(summaries[0].transaction_count, 2);

        let tx_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(tx_count, 2);
    }

    #[test]
    fn multi_account_pdf_inserts_all_accounts() {
        let result = ExtractionResult {
            accounts: vec![
                make_extraction("4242", "2024-12", "checking").accounts.remove(0),
                make_extraction("9999", "2024-12", "savings").accounts.remove(0),
            ],
        };

        let mut conn = open_test_db();
        let tx = conn.transaction().unwrap();
        let summaries = write_to_tx(&tx, "combined.pdf", &result).unwrap();
        tx.commit().unwrap();

        assert_eq!(summaries.len(), 2);

        let acct_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM accounts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(acct_count, 2);

        let tx_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(tx_count, 4);
    }

    #[test]
    fn same_last4_different_institution_maps_to_one_account() {
        let mut conn = open_test_db();

        let tx = conn.transaction().unwrap();
        write_to_tx(&tx, "dec.pdf", &fixture()).unwrap();
        tx.commit().unwrap();

        let mut variant = fixture();
        variant.accounts[0].account.institution = "First National Bank (Visa)".into();
        variant.accounts[0].account.statement_period = "2025-01".into();

        let tx = conn.transaction().unwrap();
        write_to_tx(&tx, "jan.pdf", &variant).unwrap();
        tx.commit().unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM accounts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);

        let stmt_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM statements", [], |r| r.get(0))
            .unwrap();
        assert_eq!(stmt_count, 2);
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

        let jan = make_extraction("4242", "2025-01", "checking");
        let tx = conn.transaction().unwrap();
        let summaries = write_to_tx(&tx, "jan.pdf", &jan).unwrap();
        tx.commit().unwrap();

        assert_eq!(summaries[0].transaction_count, 2);

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
