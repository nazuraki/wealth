-- Replace type='transfer' with is_transfer flag so direction (debit/credit) is preserved.
-- Existing transfer rows default to type='debit' since direction was previously discarded.
CREATE TABLE IF NOT EXISTS transactions_v3 (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  statement_id INTEGER NOT NULL REFERENCES statements(id),
  date TEXT NOT NULL,
  description TEXT NOT NULL,
  category TEXT NOT NULL,
  amount REAL NOT NULL,
  type TEXT NOT NULL CHECK(type IN ('debit', 'credit')),
  is_transfer INTEGER NOT NULL DEFAULT 0
);
INSERT OR IGNORE INTO transactions_v3
  SELECT id, statement_id, date, description, category, amount,
         CASE WHEN type = 'transfer' THEN 'debit' ELSE type END,
         CASE WHEN type = 'transfer' THEN 1 ELSE 0 END
  FROM transactions;
DROP TABLE transactions;
ALTER TABLE transactions_v3 RENAME TO transactions;
CREATE INDEX IF NOT EXISTS idx_transactions_statement ON transactions(statement_id);
CREATE INDEX IF NOT EXISTS idx_transactions_date ON transactions(date);
CREATE INDEX IF NOT EXISTS idx_transactions_category ON transactions(category);
