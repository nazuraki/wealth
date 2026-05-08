import Database from "better-sqlite3";
import { readFileSync, readdirSync } from "fs";
import { resolve, join } from "path";
import { fileURLToPath } from "url";

const migrationsDir = resolve(
  fileURLToPath(import.meta.url),
  "../../../../packages/db/migrations"
);

/**
 * Creates an in-memory SQLite DB and runs all migrations in order.
 * Returns { db, teardown } — call teardown() in afterEach/afterAll.
 *
 * @returns {{ db: import('better-sqlite3').Database, teardown: () => void }}
 */
export function createTestDb() {
  const db = new Database(":memory:");
  db.pragma("foreign_keys = ON");

  const files = readdirSync(migrationsDir)
    .filter((f) => f.endsWith(".sql"))
    .sort();

  for (const file of files) {
    const sql = readFileSync(join(migrationsDir, file), "utf8");
    db.exec(sql);
  }

  return {
    db,
    teardown: () => db.close(),
  };
}
