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
# Desktop app
cd apps/desktop
npm run tauri dev

# Run extractor standalone
cd packages/extractor
node src/index.js path/to/statement.pdf
```

## Build

```sh
cd apps/desktop
npm run tauri build
```
