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

# Build desktop app
build:
    cd apps/desktop && npm run tauri build

# Run all checks (typecheck)
check: typecheck

# Type-check all packages
typecheck:
    cd apps/desktop && npm run check

# Remove build artifacts and node_modules
clean:
    rm -rf node_modules apps/desktop/node_modules apps/desktop/.svelte-kit apps/desktop/build

# Reinstall from scratch
fresh: clean install
