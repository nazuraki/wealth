CREATE TABLE IF NOT EXISTS category_groups (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT NOT NULL UNIQUE,
  color TEXT,
  sort_order INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS category_group_members (
  category TEXT PRIMARY KEY,
  group_id INTEGER NOT NULL REFERENCES category_groups(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_category_group_members_group
  ON category_group_members(group_id);
