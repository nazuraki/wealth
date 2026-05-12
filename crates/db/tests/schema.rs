use rusqlite::Connection;

fn open() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    db::run_migrations(&conn).unwrap();
    conn
}

#[test]
fn creates_expected_tables() {
    let conn = open();
    let mut tables: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    tables.sort();
    assert!(tables.contains(&"accounts".to_string()));
    assert!(tables.contains(&"statements".to_string()));
    assert!(tables.contains(&"transactions".to_string()));
}

#[test]
fn creates_expected_indexes() {
    let conn = open();
    let indexes: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='index'")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert!(indexes.contains(&"idx_transactions_statement".to_string()));
    assert!(indexes.contains(&"idx_transactions_date".to_string()));
    assert!(indexes.contains(&"idx_transactions_category".to_string()));
}

#[test]
fn migration_is_idempotent() {
    let conn = open();
    // Running the migration a second time must not error (all statements use IF NOT EXISTS)
    conn.execute_batch(db::MIGRATION_001).unwrap();
}

#[test]
fn enforces_transaction_type_check() {
    let conn = open();
    conn.execute_batch(
        "INSERT INTO accounts (institution, account_number_last4) VALUES ('Bank', '1234');
         INSERT INTO statements (account_id, statement_period, imported_at) VALUES (1, '2024-12', datetime('now'));",
    ).unwrap();

    let ok = conn.execute(
        "INSERT INTO transactions (statement_id, date, description, category, amount, type) VALUES (1, '2024-01-01', 'Test', 'Misc', 10.0, 'debit')",
        [],
    );
    assert!(ok.is_ok());

    let transfer = conn.execute(
        "INSERT INTO transactions (statement_id, date, description, category, amount, type) VALUES (1, '2024-01-01', 'Test', 'Misc', 10.0, 'transfer')",
        [],
    );
    assert!(transfer.is_err(), "type = 'transfer' should be rejected after migration 007");

    let xfer_with_flag = conn.execute(
        "INSERT INTO transactions (statement_id, date, description, category, amount, type, is_transfer) VALUES (1, '2024-01-01', 'Test', 'Misc', 10.0, 'debit', 1)",
        [],
    );
    assert!(xfer_with_flag.is_ok(), "debit with is_transfer=1 should be accepted");

    let bad = conn.execute(
        "INSERT INTO transactions (statement_id, date, description, category, amount, type) VALUES (1, '2024-01-01', 'Test', 'Misc', 10.0, 'invalid')",
        [],
    );
    assert!(bad.is_err(), "CHECK constraint on type should reject 'invalid'");
}
