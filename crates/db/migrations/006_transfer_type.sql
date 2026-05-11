-- Expand the transactions.type CHECK to allow 'transfer'.
-- SQLite cannot ALTER a CHECK constraint, so we rebuild the table.
-- This migration is idempotent: it copies live data through transactions_v2
-- before dropping the old table, so re-running preserves all rows.
CREATE TABLE IF NOT EXISTS transactions_v2 (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  statement_id INTEGER NOT NULL REFERENCES statements(id),
  date TEXT NOT NULL,
  description TEXT NOT NULL,
  category TEXT NOT NULL,
  amount REAL NOT NULL,
  type TEXT NOT NULL CHECK(type IN ('debit', 'credit', 'transfer'))
);
INSERT OR IGNORE INTO transactions_v2 SELECT * FROM transactions;
DROP TABLE transactions;
ALTER TABLE transactions_v2 RENAME TO transactions;
CREATE INDEX IF NOT EXISTS idx_transactions_statement ON transactions(statement_id);
CREATE INDEX IF NOT EXISTS idx_transactions_date ON transactions(date);
CREATE INDEX IF NOT EXISTS idx_transactions_category ON transactions(category);
