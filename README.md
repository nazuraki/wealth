# wealth

Personal finance tool. Drag-and-drop bank/credit card statements (PDF) to import, categorize, and report on transactions.

## Stack

- **Desktop app** — Tauri 2 + SvelteKit (Svelte 5)
- **Storage** — SQLite via Tauri SQL plugin
- **Extraction** — Claude API (Anthropic) parses PDF text into structured transactions
- **Monorepo** — npm workspaces

## Structure

```
apps/
  desktop/          Tauri desktop app (SvelteKit frontend + Rust backend)
packages/
  extractor/        PDF → JSON via Claude API (runs as Tauri sidecar)
  db/               Schema migrations + shared TypeScript types
  reporter/         CLI reporting tool (WIP)
```

## Prerequisites

- [Node.js](https://nodejs.org) 20+
- [Rust](https://rustup.rs) (stable)
- Tauri prerequisites for your OS — see [tauri.app/start/prerequisites](https://tauri.app/start/prerequisites/)
- Anthropic API key

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
cd packages/extractor
node src/index.js path/to/statement.pdf
```

## Build

```sh
just build
```

## Demo mode

`demo.db` in the repo root contains pre-populated fake data (3 accounts, 13 months of statements, ~970 transactions). Launch the app against it with the `--db` flag:

```sh
# dev
cd apps/desktop && npm run tauri dev -- -- --db ../../demo.db

# production build
wealth --db /path/to/demo.db
```

The flag overrides the default database location (`~/.local/share/wealth/wealth.db` on Linux, `~/Library/Application Support/com.wealth.app/wealth.db` on macOS).

## Further reading

See [docs/PURPOSE.md](docs/PURPOSE.md) for goals, non-goals, and intended audience.

## License

MIT
