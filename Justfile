# wealth — personal finance desktop app
# Requires: just, node, rust

default:
    @just --list

# Install dependencies
install:
    npm install

# Run desktop app in dev mode
dev:
    cd apps/desktop && npm run tauri dev

# Run desktop app in dev mode with demo database
demo:
    cd apps/desktop && WEALTH_DB={{justfile_directory()}}/demo.db npm run tauri dev

# Run desktop app (dev mode)
run: dev

# Build desktop app
build:
    cd apps/desktop && npm run tauri build

# Run all checks (lint + typecheck + test)
check: lint typecheck test

# Lint Rust code (read-only)
lint:
    cargo clippy --workspace

# Fix Rust lint issues (write mode)
fix:
    cargo clippy --workspace --fix --allow-dirty

# Run Rust tests
test:
    cargo test --workspace

# Type-check the desktop frontend
typecheck:
    cd apps/desktop && npm run check

# Remove build artifacts and node_modules
clean:
    rm -rf node_modules apps/desktop/node_modules apps/desktop/.svelte-kit apps/desktop/build target

# Reinstall from scratch
fresh: clean install

# Re-index this project in nazu's code graph
index:
    nazu-index --path . --graph code:wealth
