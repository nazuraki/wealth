CREATE TABLE IF NOT EXISTS accounts (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  institution TEXT NOT NULL,
  account_number_last4 TEXT NOT NULL,
  UNIQUE(institution, account_number_last4)
);

CREATE TABLE IF NOT EXISTS statements (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  account_id INTEGER NOT NULL REFERENCES accounts(id),
  statement_period TEXT NOT NULL,
  opening_balance REAL,
  closing_balance REAL,
  source_file TEXT,
  imported_at TEXT NOT NULL DEFAULT (datetime('now')),
  UNIQUE(account_id, statement_period)
);

CREATE TABLE IF NOT EXISTS transactions (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  statement_id INTEGER NOT NULL REFERENCES statements(id),
  date TEXT NOT NULL,
  description TEXT NOT NULL,
  category TEXT NOT NULL,
  amount REAL NOT NULL,
  type TEXT NOT NULL CHECK(type IN ('debit', 'credit'))
);

CREATE INDEX IF NOT EXISTS idx_transactions_statement ON transactions(statement_id);
CREATE INDEX IF NOT EXISTS idx_transactions_date ON transactions(date);
CREATE INDEX IF NOT EXISTS idx_transactions_category ON transactions(category);
