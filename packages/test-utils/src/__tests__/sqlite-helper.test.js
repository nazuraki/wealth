import { describe, it, expect, afterEach } from "vitest";
import { createTestDb } from "../sqlite-helper.js";

describe("createTestDb", () => {
  let teardown;

  afterEach(() => teardown?.());

  it("creates all tables from migrations", () => {
    ({ teardown } = createTestDb());
    const { db } = createTestDb();
    const tables = db
      .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
      .all()
      .map((r) => r.name);
    expect(tables).toContain("accounts");
    expect(tables).toContain("statements");
    expect(tables).toContain("transactions");
  });

  it("creates expected indexes", () => {
    const { db, teardown: td } = createTestDb();
    teardown = td;
    const indexes = db
      .prepare("SELECT name FROM sqlite_master WHERE type='index'")
      .all()
      .map((r) => r.name);
    expect(indexes).toContain("idx_transactions_date");
    expect(indexes).toContain("idx_transactions_category");
    expect(indexes).toContain("idx_transactions_statement");
  });

  it("enforces foreign keys", () => {
    const { db, teardown: td } = createTestDb();
    teardown = td;
    expect(() =>
      db
        .prepare(
          "INSERT INTO statements (account_id, statement_period, imported_at) VALUES (999, '2024-01', datetime('now'))"
        )
        .run()
    ).toThrow();
  });

  it("teardown closes the db without throwing", () => {
    const { teardown: td } = createTestDb();
    expect(() => td()).not.toThrow();
  });
});
