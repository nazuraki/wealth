//! Locate the desktop app's database and settings so the CLI shares them.

use crate::Args;
use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::{Path, PathBuf};

/// Must match `identifier` in apps/desktop/src-tauri/tauri.conf.json.
const APP_IDENTIFIER: &str = "com.nazuraki.wealth";

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// The Tauri `app_data_dir` for this app on the current platform.
pub fn default_data_dir() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        home().map(|h| h.join("Library/Application Support").join(APP_IDENTIFIER))
    } else if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA").map(|p| PathBuf::from(p).join(APP_IDENTIFIER))
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| home().map(|h| h.join(".local/share")))
            .map(|p| p.join(APP_IDENTIFIER))
    }
}

/// `--db`, then `WEALTH_DB`, then the app's default location.
pub fn db_path(args: &Args) -> Result<PathBuf> {
    if let Some(p) = args.get("db") {
        return Ok(PathBuf::from(p));
    }
    if let Some(p) = std::env::var_os("WEALTH_DB") {
        return Ok(PathBuf::from(p));
    }
    default_data_dir()
        .map(|d| d.join("wealth.db"))
        .context("could not determine the default database path; pass --db or set WEALTH_DB")
}

pub fn open(args: &Args) -> Result<Connection> {
    let path = db_path(args)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(&path).with_context(|| format!("opening {}", path.display()))?;
    db::run_migrations(&conn)?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    Ok(conn)
}

/// The subset of the desktop app's settings.json the CLI cares about.
#[derive(Debug, Default, Clone)]
pub struct Settings {
    pub api_key: Option<String>,
    pub endpoint_url: Option<String>,
    pub simplefin_access_url: Option<String>,
}

fn non_empty(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(|s| s.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
}

pub fn load_settings(path: &Path) -> Settings {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Settings::default();
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Settings::default();
    };
    Settings {
        api_key: non_empty(&v, "api_key"),
        endpoint_url: non_empty(&v, "endpoint_url"),
        simplefin_access_url: non_empty(&v, "simplefin_access_url"),
    }
}

/// Settings from the same directory as the database (the app's data dir),
/// with environment variables taking precedence.
pub fn settings(args: &Args) -> Result<Settings> {
    let db = db_path(args)?;
    let mut s = db
        .parent()
        .map(|d| load_settings(&d.join("settings.json")))
        .unwrap_or_default();
    if let Some(url) = args.get("access-url").map(String::from).or_else(|| std::env::var("SIMPLEFIN_ACCESS_URL").ok()) {
        s.simplefin_access_url = Some(url);
    }
    if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
        if !key.is_empty() {
            s.api_key = Some(key);
        }
    }
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_settings_reads_present_fields_only() {
        let dir = std::env::temp_dir().join(format!("wealth-cli-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        std::fs::write(&path, r#"{"api_key":"k","endpoint_url":"","window_x":10}"#).unwrap();
        let s = load_settings(&path);
        assert_eq!(s.api_key.as_deref(), Some("k"));
        assert!(s.endpoint_url.is_none());
        assert!(s.simplefin_access_url.is_none());
        assert!(load_settings(&dir.join("missing.json")).api_key.is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn db_flag_wins_over_default() {
        let args = Args::parse(["sync", "--db", "/tmp/x.db"].into_iter().map(String::from)).unwrap();
        assert_eq!(db_path(&args).unwrap(), PathBuf::from("/tmp/x.db"));
    }
}
