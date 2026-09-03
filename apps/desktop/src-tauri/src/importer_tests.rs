use super::*;
use extractor::{Account, AccountExtraction, ExtractionResult, Summary, Transaction, TransactionType};

#[test]
fn normalize_tx_date_passes_through_iso() {
    assert_eq!(normalize_tx_date("2025-04-15", "2025-04"), "2025-04-15");
    assert_eq!(normalize_tx_date("", "2025-04"), "");
}

#[test]
fn normalize_tx_date_anchors_same_year() {
    assert_eq!(normalize_tx_date("04/15", "2025-04"), "2025-04-15");
    assert_eq!(normalize_tx_date("03/02", "2025-04"), "2025-03-02");
}

#[test]
fn normalize_tx_date_wraps_to_prior_year() {
    assert_eq!(normalize_tx_date("12/28", "2026-01"), "2025-12-28");
    assert_eq!(normalize_tx_date("11/05", "2025-01"), "2024-11-05");
}

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
            summary: Summary { total_debits: 87.32, total_credits: 2500.0, transaction_count: 2 },
        }],
    }
}

fn fixture() -> ExtractionResult {
    make_extraction("4242", "2024-12", "checking")
}

fn count(conn: &Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |r| r.get(0)).unwrap()
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
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM transactions"), 2);
    let source: String = conn.query_row("SELECT source FROM statements", [], |r| r.get(0)).unwrap();
    assert_eq!(source, SOURCE_PDF);
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
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM accounts"), 2);
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM transactions"), 4);
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

    assert_eq!(count(&conn, "SELECT COUNT(*) FROM accounts"), 1);
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM statements"), 2);
}

#[test]
fn duplicate_statement_skips_transactions() {
    let mut conn = open_test_db();
    for _ in 0..2 {
        let tx = conn.transaction().unwrap();
        let summaries = write_to_tx(&tx, "test.pdf", &fixture(), false).unwrap();
        tx.commit().unwrap();
        assert!(summaries[0].transaction_count <= 2);
    }
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM accounts"), 1);
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM transactions"), 2);
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM statements"), 1);
}

#[test]
fn transfer_description_written_with_flag() {
    let mut result = fixture();
    result.accounts[0].transactions[1] = Transaction {
        date: "2024-12-02".into(),
        description: "PAYMENT - THANK YOU".into(),
        category: "Transfer".into(),
        amount: 500.0,
        transaction_type: TransactionType::Credit,
    };
    let mut conn = open_test_db();
    let tx = conn.transaction().unwrap();
    write_to_tx(&tx, "test.pdf", &result, false).unwrap();
    tx.commit().unwrap();

    let rows: Vec<(String, i64)> = conn
        .prepare("SELECT type, is_transfer FROM transactions ORDER BY date")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(rows, vec![("debit".to_string(), 0), ("credit".to_string(), 1)]);
}

#[test]
fn detect_conflicts_returns_duplicate_only_when_present() {
    let mut conn = open_test_db();
    let tx = conn.transaction().unwrap();
    assert!(detect_conflicts(&tx, &fixture()).unwrap().is_empty());
    write_to_tx(&tx, "dec.pdf", &fixture(), false).unwrap();
    tx.commit().unwrap();

    let tx = conn.transaction().unwrap();
    let conflicts = detect_conflicts(&tx, &fixture()).unwrap();
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].account_number_last4, "4242");
    assert_eq!(conflicts[0].statement_period, "2024-12");
}

#[test]
fn overwrite_replaces_only_the_conflicting_statement() {
    let mut conn = open_test_db();
    let tx = conn.transaction().unwrap();
    write_to_tx(&tx, "dec.pdf", &fixture(), false).unwrap();
    write_to_tx(&tx, "jan.pdf", &make_extraction("4242", "2025-01", "checking"), false).unwrap();
    tx.commit().unwrap();

    let mut replacement = fixture();
    replacement.accounts[0].transactions = vec![Transaction {
        date: "2024-12-20".into(),
        description: "ONLY NEW".into(),
        category: "Other".into(),
        amount: 10.0,
        transaction_type: TransactionType::Debit,
    }];
    let tx = conn.transaction().unwrap();
    let summaries = write_to_tx(&tx, "dec.pdf", &replacement, true).unwrap();
    tx.commit().unwrap();

    assert_eq!(summaries[0].transaction_count, 1);
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM transactions"), 3);
    let desc: String = conn
        .query_row(
            "SELECT t.description FROM transactions t JOIN statements s ON s.id = t.statement_id \
             WHERE s.statement_period = '2024-12'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(desc, "ONLY NEW");
}

#[test]
fn pdf_import_refuses_a_simplefin_period() {
    let mut conn = open_test_db();
    let acct = store::upsert_account(&conn, "First National Bank", "4242", None).unwrap();
    store::find_or_create_statement(&conn, acct, "2024-12", SOURCE_SIMPLEFIN, "simplefin", None, None).unwrap();

    let tx = conn.transaction().unwrap();
    let err = detect_conflicts(&tx, &fixture()).unwrap_err().to_string();
    assert!(err.contains("synced from SimpleFIN"), "{err}");
    let err = write_to_tx(&tx, "dec.pdf", &fixture(), true).unwrap_err().to_string();
    assert!(err.contains("synced from SimpleFIN"), "{err}");
    drop(tx);
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM transactions"), 0);
}
