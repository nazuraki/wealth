use anyhow::Result;
use extractor::{
    extract_text, parse_line_items, AccountExtraction, AnthropicClient, ExtractionResult,
    TransactionType,
};
use rusqlite::{params, Connection, Transaction};
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager, State};

#[derive(Debug, Serialize)]
pub struct ImportSummary {
    pub institution: String,
    pub account_number_last4: String,
    pub statement_period: String,
    pub transaction_count: usize,
}

#[derive(Debug, Serialize)]
pub struct DuplicateConflict {
    pub institution: String,
    pub account_number_last4: String,
    pub statement_period: String,
}

#[derive(Debug, Serialize)]
pub struct ImportResponse {
    pub summaries: Vec<ImportSummary>,
    pub conflicts: Vec<DuplicateConflict>,
}

// ── Transfer detection ────────────────────────────────────────────────────────

const TRANSFER_PATTERNS: &[&str] = &[
    "payment - thank you",
    "autopay",
    "online pmt",
    "ach payment",
    "online payment",
    "online transfer",
];

fn is_transfer(description: &str) -> bool {
    let lower = description.to_lowercase();
    TRANSFER_PATTERNS.iter().any(|p| lower.contains(p))
}

// ── DB write (testable inner fn) ──────────────────────────────────────────────

fn upsert_account(tx: &Transaction<'_>, extraction: &AccountExtraction) -> Result<i64> {
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
    let id: i64 = tx.query_row(
        "SELECT id FROM accounts WHERE account_number_last4 = ?1",
        params![acct.account_number_last4],
        |row| row.get(0),
    )?;
    Ok(id)
}

fn check_account_conflict(
    tx: &Transaction<'_>,
    extraction: &AccountExtraction,
) -> Result<Option<DuplicateConflict>> {
    let acct = &extraction.account;
    let exists: bool = tx.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM statements s
            JOIN accounts a ON a.id = s.account_id
            WHERE a.account_number_last4 = ?1 AND s.statement_period = ?2
         )",
        params![acct.account_number_last4, acct.statement_period],
        |row| row.get(0),
    )?;
    if exists {
        Ok(Some(DuplicateConflict {
            institution: acct.institution.clone(),
            account_number_last4: acct.account_number_last4.clone(),
            statement_period: acct.statement_period.clone(),
        }))
    } else {
        Ok(None)
    }
}

fn detect_conflicts(
    tx: &Transaction<'_>,
    result: &ExtractionResult,
) -> Result<Vec<DuplicateConflict>> {
    result
        .accounts
        .iter()
        .filter_map(|a| check_account_conflict(tx, a).transpose())
        .collect()
}

fn write_account_to_tx(
    tx: &Transaction<'_>,
    source_file: &str,
    extraction: &AccountExtraction,
    overwrite: bool,
) -> Result<ImportSummary> {
    let acct = &extraction.account;
    let account_id = upsert_account(tx, extraction)?;

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

    let write_transactions = if statement_inserted {
        true
    } else if overwrite {
        tx.execute(
            "DELETE FROM transactions WHERE statement_id = ?1",
            params![statement_id],
        )?;
        // Update statement metadata in case it changed.
        tx.execute(
            "UPDATE statements SET opening_balance = ?1, closing_balance = ?2, source_file = ?3 \
             WHERE id = ?4",
            params![
                acct.opening_balance,
                acct.closing_balance,
                source_file,
                statement_id
            ],
        )?;
        true
    } else {
        false
    };

    if write_transactions {
        let mut stmt = tx.prepare(
            "INSERT INTO transactions (statement_id, date, description, category, amount, type) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?;
        for t in &extraction.transactions {
            let kind = if is_transfer(&t.description) {
                "transfer"
            } else {
                match t.transaction_type {
                    TransactionType::Debit => "debit",
                    TransactionType::Credit => "credit",
                    TransactionType::Transfer => "transfer",
                }
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
        transaction_count: if write_transactions {
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
    overwrite: bool,
) -> Result<Vec<ImportSummary>> {
    result
        .accounts
        .iter()
        .map(|a| write_account_to_tx(tx, source_file, a, overwrite))
        .collect()
}

pub(crate) fn write_to_db(
    db_path: &Path,
    source_file: &str,
    result: &ExtractionResult,
    overwrite: bool,
) -> Result<Vec<ImportSummary>> {
    let mut conn = Connection::open(db_path)?;
    db::run_migrations(&conn)?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    let tx = conn.transaction()?;
    let summaries = write_to_tx(&tx, source_file, result, overwrite)?;
    tx.commit()?;
    Ok(summaries)
}

pub(crate) fn check_conflicts_in_db(
    db_path: &Path,
    result: &ExtractionResult,
) -> Result<Vec<DuplicateConflict>> {
    let mut conn = Connection::open(db_path)?;
    db::run_migrations(&conn)?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    let tx = conn.transaction()?;
    let conflicts = detect_conflicts(&tx, result)?;
    // No commit needed — read-only.
    Ok(conflicts)
}

// ── Tauri command ─────────────────────────────────────────────────────────────

fn do_import(app: &AppHandle, db_path: PathBuf, path: &str, overwrite: bool) -> Result<ImportResponse> {
    let pdf_path = PathBuf::from(path);
    let data_dir = app.path().app_data_dir()?;

    let s = crate::settings::load(&data_dir);
    let api_key = s
        .api_key
        .filter(|k| !k.is_empty())
        .or_else(|| std::env::var("ANTHROPIC_API_KEY").ok())
        .ok_or_else(|| anyhow::anyhow!("No API key configured. Set one in Settings or set the ANTHROPIC_API_KEY environment variable."))?;
    let base_url = s
        .endpoint_url
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| AnthropicClient::DEFAULT_ENDPOINT.to_string());
    let client = AnthropicClient::with_config(api_key, base_url);

    let text = extract_text(&pdf_path)?;
    let label = pdf_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(path);
    let result = parse_line_items(&text, label, &client)?;

    if !overwrite {
        let conflicts = check_conflicts_in_db(&db_path, &result)?;
        if !conflicts.is_empty() {
            return Ok(ImportResponse { summaries: vec![], conflicts });
        }
    }

    let summaries = write_to_db(&db_path, path, &result, overwrite)?;
    Ok(ImportResponse { summaries, conflicts: vec![] })
}

#[tauri::command]
pub async fn import_statement(
    app: AppHandle,
    db: State<'_, crate::DbPath>,
    path: String,
    overwrite: bool,
) -> Result<ImportResponse, String> {
    let db_path = db.0.clone();
    tauri::async_runtime::spawn_blocking(move || do_import(&app, db_path, &path, overwrite))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_transfer_matches_known_patterns() {
        assert!(is_transfer("PAYMENT - THANK YOU"));
        assert!(is_transfer("Payment - Thank You"));
        assert!(is_transfer("AUTOPAY"));
        assert!(is_transfer("Chase Autopay"));
        assert!(is_transfer("ONLINE PMT"));
        assert!(is_transfer("ACH PAYMENT"));
        assert!(is_transfer("ONLINE PAYMENT"));
        assert!(is_transfer("ONLINE TRANSFER"));
    }

    #[test]
    fn is_transfer_does_not_match_regular_transactions() {
        assert!(!is_transfer("WHOLE FOODS MARKET"));
        assert!(!is_transfer("DIRECT DEPOSIT"));
        assert!(!is_transfer("AMAZON.COM"));
        assert!(!is_transfer("TRANSFER FEE"));
    }
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
        let summaries = write_to_tx(&tx, "test.pdf", &fixture(), false).unwrap();
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
        let summaries = write_to_tx(&tx, "combined.pdf", &result, false).unwrap();
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
        write_to_tx(&tx, "dec.pdf", &fixture(), false).unwrap();
        tx.commit().unwrap();

        let mut variant = fixture();
        variant.accounts[0].account.institution = "First National Bank (Visa)".into();
        variant.accounts[0].account.statement_period = "2025-01".into();

        let tx = conn.transaction().unwrap();
        write_to_tx(&tx, "jan.pdf", &variant, false).unwrap();
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
            write_to_tx(&tx, "test.pdf", &fixture(), false).unwrap();
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
            write_to_tx(&tx, "test.pdf", &fixture(), false).unwrap();
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
        write_to_tx(&tx, "dec.pdf", &fixture(), false).unwrap();
        tx.commit().unwrap();

        let jan = make_extraction("4242", "2025-01", "checking");
        let tx = conn.transaction().unwrap();
        let summaries = write_to_tx(&tx, "jan.pdf", &jan, false).unwrap();
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

    #[test]
    fn transfer_description_written_as_transfer_type() {
        let result = ExtractionResult {
            accounts: vec![AccountExtraction {
                account: Account {
                    institution: "Big Bank".into(),
                    account_number_last4: "1111".into(),
                    account_type: Some("checking".into()),
                    statement_period: "2024-12".into(),
                    opening_balance: None,
                    closing_balance: None,
                },
                transactions: vec![
                    Transaction {
                        date: "2024-12-01".into(),
                        description: "PAYMENT - THANK YOU".into(),
                        category: "Transfer".into(),
                        amount: 500.0,
                        transaction_type: TransactionType::Credit,
                    },
                    Transaction {
                        date: "2024-12-02".into(),
                        description: "WHOLE FOODS MARKET".into(),
                        category: "Groceries".into(),
                        amount: 80.0,
                        transaction_type: TransactionType::Debit,
                    },
                ],
                summary: Summary { total_debits: 80.0, total_credits: 500.0, transaction_count: 2 },
            }],
        };

        let mut conn = open_test_db();
        let tx = conn.transaction().unwrap();
        write_to_tx(&tx, "test.pdf", &result, false).unwrap();
        tx.commit().unwrap();

        let types: Vec<String> = conn
            .prepare("SELECT type FROM transactions ORDER BY date")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(types, vec!["transfer", "debit"]);
    }

    // ── Conflict detection tests ──────────────────────────────────────────────

    #[test]
    fn detect_conflicts_returns_duplicate() {
        let mut conn = open_test_db();
        let tx = conn.transaction().unwrap();
        write_to_tx(&tx, "dec.pdf", &fixture(), false).unwrap();
        tx.commit().unwrap();

        let tx = conn.transaction().unwrap();
        let conflicts = detect_conflicts(&tx, &fixture()).unwrap();
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].account_number_last4, "4242");
        assert_eq!(conflicts[0].statement_period, "2024-12");
    }

    #[test]
    fn detect_conflicts_returns_empty_for_new_statement() {
        let mut conn = open_test_db();
        let tx = conn.transaction().unwrap();
        let conflicts = detect_conflicts(&tx, &fixture()).unwrap();
        assert!(conflicts.is_empty());
    }

    #[test]
    fn skip_makes_no_db_changes() {
        let mut conn = open_test_db();

        // First import.
        let tx = conn.transaction().unwrap();
        write_to_tx(&tx, "dec.pdf", &fixture(), false).unwrap();
        tx.commit().unwrap();

        // Second import with overwrite=false should skip (0 transactions written).
        let tx = conn.transaction().unwrap();
        let summaries = write_to_tx(&tx, "dec.pdf", &fixture(), false).unwrap();
        tx.commit().unwrap();

        assert_eq!(summaries[0].transaction_count, 0);
        let tx_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(tx_count, 2);
    }

    #[test]
    fn overwrite_replaces_transactions() {
        let mut conn = open_test_db();

        let tx = conn.transaction().unwrap();
        write_to_tx(&tx, "dec.pdf", &fixture(), false).unwrap();
        tx.commit().unwrap();

        // Build a replacement with a different transaction.
        let mut replacement = fixture();
        replacement.accounts[0].transactions = vec![extractor::Transaction {
            date: "2024-12-15".into(),
            description: "NEW TRANSACTION".into(),
            category: "Other".into(),
            amount: 42.0,
            transaction_type: TransactionType::Debit,
        }];

        let tx = conn.transaction().unwrap();
        let summaries = write_to_tx(&tx, "dec.pdf", &replacement, true).unwrap();
        tx.commit().unwrap();

        assert_eq!(summaries[0].transaction_count, 1);

        let tx_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(tx_count, 1);

        let desc: String = conn
            .query_row("SELECT description FROM transactions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(desc, "NEW TRANSACTION");
    }

    #[test]
    fn overwrite_only_affects_conflicting_statement() {
        let mut conn = open_test_db();

        // Import two statements for the same account.
        let tx = conn.transaction().unwrap();
        write_to_tx(&tx, "dec.pdf", &fixture(), false).unwrap();
        tx.commit().unwrap();

        let jan = make_extraction("4242", "2025-01", "checking");
        let tx = conn.transaction().unwrap();
        write_to_tx(&tx, "jan.pdf", &jan, false).unwrap();
        tx.commit().unwrap();

        // Overwrite only the December statement.
        let mut replacement = fixture();
        replacement.accounts[0].transactions = vec![extractor::Transaction {
            date: "2024-12-20".into(),
            description: "ONLY NEW".into(),
            category: "Other".into(),
            amount: 10.0,
            transaction_type: TransactionType::Debit,
        }];

        let tx = conn.transaction().unwrap();
        write_to_tx(&tx, "dec.pdf", &replacement, true).unwrap();
        tx.commit().unwrap();

        // Total: 1 (overwritten Dec) + 2 (untouched Jan) = 3.
        let tx_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(tx_count, 3);

        // The December statement has exactly the new transaction.
        let dec_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM transactions t \
                 JOIN statements s ON s.id = t.statement_id \
                 WHERE s.statement_period = '2024-12'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(dec_count, 1);
    }
}
