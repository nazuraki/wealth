import { describe, it, expect, afterEach } from "vitest";
import { readFileSync, readdirSync } from "fs";
import { resolve, join } from "path";
import { fileURLToPath } from "url";
import Database from "better-sqlite3";

const migrationsDir = resolve(
  fileURLToPath(import.meta.url),
  "../../../migrations"
);

function createTestDb() {
  const db = new Database(":memory:");
  db.pragma("foreign_keys = ON");
  const files = readdirSync(migrationsDir)
    .filter((f) => f.endsWith(".sql"))
    .sort();
  for (const file of files) {
    db.exec(readFileSync(join(migrationsDir, file), "utf8"));
  }
  return { db, teardown: () => db.close() };
}

describe("DB schema", () => {
  let teardown;

  afterEach(() => teardown?.());

  it("creates accounts, statements, and transactions tables on fresh launch", () => {
    const { db, teardown: td } = createTestDb();
    teardown = td;

    const tables = db
      .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
      .all()
      .map((r) => r.name);

    expect(tables).toContain("accounts");
    expect(tables).toContain("statements");
    expect(tables).toContain("transactions");
  });

  it("creates all expected indexes", () => {
    const { db, teardown: td } = createTestDb();
    teardown = td;

    const indexes = db
      .prepare("SELECT name FROM sqlite_master WHERE type='index'")
      .all()
      .map((r) => r.name);

    expect(indexes).toContain("idx_transactions_statement");
    expect(indexes).toContain("idx_transactions_date");
    expect(indexes).toContain("idx_transactions_category");
  });

  it("migration is idempotent — re-running SQL does not error", () => {
    const { db, teardown: td } = createTestDb();
    teardown = td;

    const files = readdirSync(migrationsDir)
      .filter((f) => f.endsWith(".sql"))
      .sort();

    expect(() => {
      for (const file of files) {
        const sql = readFileSync(join(migrationsDir, file), "utf8");
        db.exec(sql);
      }
    }).not.toThrow();
  });
});
