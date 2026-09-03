# SimpleFIN sync

Accounts linked through [SimpleFIN Bridge](https://beta-bridge.simplefin.org/)
sync automatically instead of being imported from PDFs. This page describes how
the sync behaves and how to run it on a schedule.

## Setup

1. Create a SimpleFIN Bridge account and connect your institutions there.
2. Generate a **setup token** in SimpleFIN and paste it into **Settings →
   SimpleFIN** in the desktop app. Saving exchanges the token for a permanent
   access URL. Tokens are single-use; if the exchange fails, generate another.
3. Open **Import Log** and press **Sync now**. The first sync lists every feed
   account it found. Map each one to an existing app account or create a new
   one, then sync again.

The same setup works headless: `wealth-cli map` and `wealth-cli sync` share the
app's database and settings (see [CLI](#cli)).

## How transactions land

- Feed transactions go into a synthesized statement per **calendar month**
  (`statements.source = 'simplefin'`). PDF statements keep their own
  bank-defined periods (`source = 'pdf'`).
- An account's feed starts **strictly after its latest PDF statement**. Bank
  statement periods end mid-month, so mixing the two for one month would
  double-count. The PDF importer refuses a period that is already synced, and
  the sync never writes into a month that has a PDF statement.
- Each feed transaction carries a stable id (`transactions.external_id`), so
  re-running is idempotent. Existing rows are never modified: edits you make to
  a description or category survive every sync.
- Pending transactions are skipped; they get a new id when they post.
- Outflows become `debit`, inflows `credit`. Transfers between your own
  accounts are flagged by description, as with PDF imports.
- The current month's `closing_balance` is updated from the feed balance on
  every sync.

## Categorization

Feed transactions have no category. Each new row is categorized in two passes:

1. **Rules** (`category_rules` table). A rule is a lowercased pattern that
   matches a description by prefix or substring; prefix matches win, then the
   longest pattern. User rules always beat learned rules.
2. **Model**. Descriptions with no matching rule are sent to Claude in one
   batch, along with the categories already in use so it reuses them. Each
   answer is stored as a *learned* rule, so a merchant is only ever asked about
   once. Without an API key, rows stay `Uncategorized`.

Seed rules for recurring merchants with the CLI:

```sh
wealth-cli rules add github Software
wealth-cli rules add anthropic "AI APIs"
wealth-cli rules list
```

## Business expenses

Put the relevant categories (for example `Software`, `Cloud`, `AI APIs`,
`Domains`) into a category group named `Business` under **Settings → Category
Groups**. Export the group whenever you need it:

```sh
wealth-cli export --group Business --from 2026-01-01 --to 2026-12-31 --out business-2026.csv
```

Transfers are excluded unless `--include-transfers` is given.

## CLI

`wealth-cli` reads the same database and `settings.json` as the desktop app.
Override the database with `--db PATH` or `WEALTH_DB`, the access URL with
`--access-url` or `SIMPLEFIN_ACCESS_URL`, and the model key with
`ANTHROPIC_API_KEY`.

```sh
wealth-cli sync                 # pull new transactions, categorize, print a summary
wealth-cli sync --json          # machine-readable report on stdout
wealth-cli accounts             # list accounts and their feed mapping
wealth-cli map <simplefin_id> --account 3
wealth-cli map <simplefin_id> --institution Ally --last4 1234 --type savings
```

`sync` exits 0 on success, 2 when the feed reported connection errors or there
are unmapped accounts, and 1 on a fatal error, so a scheduler can alert on
non-zero status.

## Scheduling on macOS

Build the CLI once (`cargo build --release -p wealth-cli`), then register a
`launchd` agent that runs it every morning. Save this as
`~/Library/LaunchAgents/com.nazuraki.wealth.sync.plist`, replacing the
binary path:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>com.nazuraki.wealth.sync</string>
  <key>ProgramArguments</key>
  <array>
    <string>/path/to/wealth/target/release/wealth-cli</string>
    <string>sync</string>
  </array>
  <key>StartCalendarInterval</key>
  <dict><key>Hour</key><integer>6</integer><key>Minute</key><integer>0</integer></dict>
  <key>StandardOutPath</key><string>/tmp/wealth-sync.log</string>
  <key>StandardErrorPath</key><string>/tmp/wealth-sync.log</string>
</dict>
</plist>
```

```sh
launchctl load ~/Library/LaunchAgents/com.nazuraki.wealth.sync.plist
```

SimpleFIN refreshes bank data roughly once a day, so a daily run is enough.
