-- SimpleFIN sync support.
-- accounts.simplefin_id: feed account id this app account is mapped to.
-- statements.source: 'pdf' (imported statement) or 'simplefin' (synthesized calendar month).
-- transactions.external_id: feed transaction id, makes re-sync idempotent.
-- category_rules: description pattern -> category, applied before asking the model.
ALTER TABLE accounts ADD COLUMN simplefin_id TEXT;
ALTER TABLE statements ADD COLUMN source TEXT NOT NULL DEFAULT 'pdf';
ALTER TABLE transactions ADD COLUMN external_id TEXT;
CREATE UNIQUE INDEX IF NOT EXISTS idx_transactions_external_id
  ON transactions(external_id) WHERE external_id IS NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS idx_accounts_simplefin_id
  ON accounts(simplefin_id) WHERE simplefin_id IS NOT NULL;
CREATE TABLE IF NOT EXISTS category_rules (
  pattern TEXT PRIMARY KEY,
  category TEXT NOT NULL,
  source TEXT NOT NULL DEFAULT 'user' CHECK(source IN ('user', 'learned')),
  created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
