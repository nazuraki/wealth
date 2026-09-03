//! Headless companion to the desktop app: scheduled SimpleFIN sync, CSV export,
//! category rules, and feed-account mapping. Shares the app's database and settings.

mod cmd_admin;
mod cmd_export;
mod cmd_sync;
mod config;

use anyhow::{bail, Result};
use std::collections::HashMap;

const USAGE: &str = "\
wealth-cli — headless sync, export and admin for the wealth database

USAGE
  wealth-cli sync    [--db PATH] [--access-url URL] [--no-model] [--json]
  wealth-cli export  [--db PATH] [--group NAME] [--category NAME] [--account LAST4]
                     [--from YYYY-MM-DD] [--to YYYY-MM-DD] [--include-transfers] [--out FILE]
  wealth-cli accounts [--db PATH]
  wealth-cli map     SIMPLEFIN_ID (--account ID | --institution NAME --last4 NNNN [--type TYPE]) [--db PATH]
  wealth-cli unmap   ACCOUNT_ID [--db PATH]
  wealth-cli rules   list | add PATTERN CATEGORY | remove PATTERN   [--db PATH]

ENVIRONMENT
  WEALTH_DB             database path (default: the desktop app's database)
  SIMPLEFIN_ACCESS_URL  overrides the access URL saved in the app's settings
  ANTHROPIC_API_KEY     enables model categorization when no key is saved in settings

Exit status: 0 ok, 1 usage or fatal error, 2 sync finished with feed errors or unmapped accounts.";

const BOOL_FLAGS: &[&str] = &["--no-model", "--json", "--include-transfers", "--help", "-h"];

/// Parsed command line: positionals plus `--key value` / `--flag` options.
#[derive(Debug, Default)]
pub struct Args {
    pub positional: Vec<String>,
    pub options: HashMap<String, String>,
}

impl Args {
    pub fn parse(raw: impl Iterator<Item = String>) -> Result<Self> {
        let mut out = Args::default();
        let mut iter = raw.peekable();
        while let Some(arg) = iter.next() {
            if let Some(key) = arg.strip_prefix("--").or_else(|| arg.strip_prefix('-')) {
                if BOOL_FLAGS.contains(&arg.as_str()) {
                    out.options.insert(key.to_string(), "true".into());
                } else if let Some((k, v)) = key.split_once('=') {
                    out.options.insert(k.to_string(), v.to_string());
                } else {
                    let value = iter.next().filter(|v| !v.starts_with("--"));
                    match value {
                        Some(v) => {
                            out.options.insert(key.to_string(), v);
                        }
                        None => bail!("option --{key} needs a value"),
                    }
                }
            } else {
                out.positional.push(arg);
            }
        }
        Ok(out)
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.options.get(key).map(String::as_str)
    }

    pub fn flag(&self, key: &str) -> bool {
        self.options.contains_key(key)
    }
}

fn main() {
    let code = match run() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e:#}");
            1
        }
    };
    std::process::exit(code);
}

fn run() -> Result<i32> {
    let args = Args::parse(std::env::args().skip(1))?;
    if args.flag("help") || args.flag("h") {
        println!("{USAGE}");
        return Ok(0);
    }
    let Some(command) = args.positional.first().map(String::as_str) else {
        eprintln!("{USAGE}");
        return Ok(1);
    };
    match command {
        "sync" => cmd_sync::run(&args),
        "export" => cmd_export::run(&args).map(|_| 0),
        "accounts" => cmd_admin::accounts(&args).map(|_| 0),
        "map" => cmd_admin::map(&args).map(|_| 0),
        "unmap" => cmd_admin::unmap(&args).map(|_| 0),
        "rules" => cmd_admin::rules(&args).map(|_| 0),
        other => {
            eprintln!("unknown command: {other}\n\n{USAGE}");
            Ok(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Args {
        Args::parse(s.split_whitespace().map(String::from)).unwrap()
    }

    #[test]
    fn parses_positionals_options_and_flags() {
        let a = parse("export --group Business --from 2026-01-01 --json --out=out.csv");
        assert_eq!(a.positional, vec!["export"]);
        assert_eq!(a.get("group"), Some("Business"));
        assert_eq!(a.get("from"), Some("2026-01-01"));
        assert_eq!(a.get("out"), Some("out.csv"));
        assert!(a.flag("json"));
        assert!(!a.flag("no-model"));
    }

    #[test]
    fn rules_add_keeps_pattern_and_category_positional() {
        let a = parse("rules add github Software --db x.db");
        assert_eq!(a.positional, vec!["rules", "add", "github", "Software"]);
        assert_eq!(a.get("db"), Some("x.db"));
    }

    #[test]
    fn value_option_without_value_is_an_error() {
        assert!(Args::parse(["sync", "--db"].into_iter().map(String::from)).is_err());
        assert!(Args::parse(["sync", "--db", "--json"].into_iter().map(String::from)).is_err());
    }
}
