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
    assert!(tables.contains(&"category_groups".to_string()));
    assert!(tables.contains(&"category_group_members".to_string()));
}

#[test]
fn category_group_cascade_delete() {
    let conn = open();
    conn.execute_batch(
        "INSERT INTO category_groups (name) VALUES ('Food');
         INSERT INTO category_group_members (category, group_id) VALUES ('Dining', 1);
         INSERT INTO category_group_members (category, group_id) VALUES ('Groceries', 1);
         DELETE FROM category_groups WHERE id = 1;",
    ).unwrap();
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM category_group_members", [], |r| r.get(0)).unwrap();
    assert_eq!(count, 0, "members should be cascade-deleted with the group");
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

#[test]
fn migration_008_backfills_mm_dd_dates() {
    let conn = open();
    conn.execute_batch(
        "INSERT INTO accounts (institution, account_number_last4) VALUES ('Chase', '7793');
         INSERT INTO statements (account_id, statement_period, imported_at) VALUES (1, '2026-01', datetime('now'));
         INSERT INTO statements (account_id, statement_period, imported_at) VALUES (1, '2025-04', datetime('now'));
         INSERT INTO transactions (statement_id, date, description, category, amount, type) VALUES (1, '01/15', 'A', 'X', 1.0, 'debit');
         INSERT INTO transactions (statement_id, date, description, category, amount, type) VALUES (1, '12/28', 'B', 'X', 1.0, 'debit');
         INSERT INTO transactions (statement_id, date, description, category, amount, type) VALUES (2, '04/10', 'C', 'X', 1.0, 'debit');
         INSERT INTO transactions (statement_id, date, description, category, amount, type) VALUES (2, '03/02', 'D', 'X', 1.0, 'debit');
         INSERT INTO transactions (statement_id, date, description, category, amount, type) VALUES (2, '2025-04-20', 'E', 'X', 1.0, 'debit');",
    ).unwrap();

    conn.execute_batch(db::MIGRATION_008).unwrap();

    let dates: Vec<String> = conn
        .prepare("SELECT date FROM transactions ORDER BY id")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(dates, vec!["2026-01-15", "2025-12-28", "2025-04-10", "2025-03-02", "2025-04-20"]);

    // Idempotent — second run is a no-op.
    conn.execute_batch(db::MIGRATION_008).unwrap();
    let dates2: Vec<String> = conn
        .prepare("SELECT date FROM transactions ORDER BY id")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(dates, dates2);
}
