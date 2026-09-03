# wealth

Personal finance tool. Import bank and credit card statements by dropping PDFs
onto the app, or sync accounts automatically through SimpleFIN. Transactions
are categorized, stored locally in SQLite, and reported on.

## Stack

- **Desktop app** — Tauri 2 + SvelteKit (Svelte 5)
- **Storage** — SQLite via `rusqlite`; migrations live in `crates/db`
- **Extraction** — Claude API (Anthropic) parses PDF text into structured transactions
- **Sync** — SimpleFIN Bridge feed for supported banks; see [docs/simplefin-sync.md](docs/simplefin-sync.md)
- **Workspace** — Cargo workspace for Rust, npm workspaces for the frontend

## Structure

```
apps/
  desktop/          Tauri desktop app (SvelteKit frontend + Rust commands)
crates/
  db/               Schema migrations, shared account/statement helpers, category rules
  extractor/        PDF text extraction + Claude client (statement parsing, categorization)
  extractor-cli/    Run the PDF extractor standalone
  simplefin/        SimpleFIN client and sync engine
  wealth-cli/       Headless sync, CSV export, rules and account mapping
docs/               Purpose and sync documentation
```

## Prerequisites

- [Node.js](https://nodejs.org) 20+
- [Rust](https://rustup.rs) (stable)
- Tauri prerequisites for your OS — see [tauri.app/start/prerequisites](https://tauri.app/start/prerequisites/)
- Anthropic API key
- Optional: a [SimpleFIN Bridge](https://beta-bridge.simplefin.org/) account for automatic sync

## Setup

```sh
npm install
cp .env.example .env   # add ANTHROPIC_API_KEY
```

## Development

```sh
just dev
```

Run the extractor standalone:

```sh
cargo run -p extractor-cli -- path/to/statement.pdf
```

## Build

```sh
just build
```

## CLI

`wealth-cli` shares the desktop app's database and settings, so a scheduled
sync and the app stay in step.

```sh
just sync                                   # pull new SimpleFIN transactions
just cli export --group Business --out business.csv
just cli accounts
just cli rules add github Software
```

Run `just cli --help` for every command. Scheduling with `launchd` is covered
in [docs/simplefin-sync.md](docs/simplefin-sync.md).

## Demo mode

`demo.db` in the repo root contains pre-populated fake data (3 accounts, 13 months of statements, ~970 transactions). Launch the app against it:

```sh
# dev
just demo

# production build — pass --db or set WEALTH_DB
wealth --db /path/to/demo.db
WEALTH_DB=/path/to/demo.db wealth
```

The `--db` flag (or `WEALTH_DB` env var) overrides the default database
location, which is the platform app-data directory for `com.nazuraki.wealth`
(`~/Library/Application Support/com.nazuraki.wealth/wealth.db` on macOS,
`~/.local/share/com.nazuraki.wealth/wealth.db` on Linux). `settings.json`
lives alongside it.

## Further reading

- [docs/PURPOSE.md](docs/PURPOSE.md) — goals, non-goals, and intended audience
- [docs/simplefin-sync.md](docs/simplefin-sync.md) — how sync, categorization, and export work

## License

MIT
