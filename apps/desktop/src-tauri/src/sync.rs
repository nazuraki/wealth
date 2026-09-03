//! SimpleFIN sync commands: run a sync, map feed accounts to app accounts,
//! and turn a one-time setup token into an access URL.

use anyhow::{anyhow, bail, Result};
use extractor::{AnthropicClient, ClaudeClient};
use rusqlite::{params, Connection};
use simplefin::{HttpFeed, SyncReport};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager, State};

fn open_conn(db_path: &Path) -> Result<Connection> {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(db_path)?;
    db::run_migrations(&conn)?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    Ok(conn)
}

fn do_sync(app: &AppHandle, db_path: PathBuf) -> Result<SyncReport> {
    let data_dir = app.path().app_data_dir()?;
    let s = crate::settings::load(&data_dir);
    let access_url = s
        .simplefin_access_url
        .filter(|u| !u.trim().is_empty())
        .ok_or_else(|| anyhow!("No SimpleFIN access URL configured. Paste a setup token or access URL in Settings."))?;
    let feed = HttpFeed::new(&access_url)?;

    let client = s
        .api_key
        .filter(|k| !k.is_empty())
        .or_else(|| std::env::var("ANTHROPIC_API_KEY").ok())
        .map(|key| {
            let base_url = s
                .endpoint_url
                .clone()
                .filter(|u| !u.is_empty())
                .unwrap_or_else(|| AnthropicClient::DEFAULT_ENDPOINT.to_string());
            AnthropicClient::with_config(key, base_url)
        });

    let mut conn = open_conn(&db_path)?;
    simplefin::run_sync(&mut conn, &feed, client.as_ref().map(|c| c as &dyn ClaudeClient))
}

#[tauri::command]
pub async fn sync_simplefin(app: AppHandle, db: State<'_, crate::DbPath>) -> Result<SyncReport, String> {
    let db_path = db.0.clone();
    tauri::async_runtime::spawn_blocking(move || do_sync(&app, db_path))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// Attach a feed account to an app account. With `account_id` the existing
/// account is used; otherwise one is created from `institution` + `last4`.
pub(crate) fn do_map(
    conn: &Connection,
    simplefin_id: &str,
    account_id: Option<i64>,
    institution: Option<&str>,
    last4: Option<&str>,
    account_type: Option<&str>,
) -> Result<i64> {
    if simplefin_id.trim().is_empty() {
        bail!("simplefin_id is required");
    }
    let id = match account_id {
        Some(id) => id,
        None => {
            let institution = institution.map(str::trim).filter(|s| !s.is_empty()).ok_or_else(|| anyhow!("institution is required"))?;
            let last4 = last4.map(str::trim).filter(|s| !s.is_empty()).ok_or_else(|| anyhow!("account number (last 4) is required"))?;
            db::store::upsert_account(conn, institution, last4, account_type.filter(|s| !s.is_empty()))?
        }
    };
    // A feed account maps to exactly one app account; move it if it was elsewhere.
    conn.execute("UPDATE accounts SET simplefin_id = NULL WHERE simplefin_id = ?1", params![simplefin_id])?;
    let n = conn.execute("UPDATE accounts SET simplefin_id = ?1 WHERE id = ?2", params![simplefin_id, id])?;
    if n == 0 {
        bail!("account {id} not found");
    }
    Ok(id)
}

#[tauri::command]
pub fn map_simplefin_account(
    db: State<'_, crate::DbPath>,
    simplefin_id: String,
    account_id: Option<i64>,
    institution: Option<String>,
    account_number_last4: Option<String>,
    account_type: Option<String>,
) -> Result<i64, String> {
    let conn = open_conn(&db.0).map_err(|e| e.to_string())?;
    do_map(
        &conn,
        &simplefin_id,
        account_id,
        institution.as_deref(),
        account_number_last4.as_deref(),
        account_type.as_deref(),
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn unmap_simplefin_account(db: State<'_, crate::DbPath>, account_id: i64) -> Result<(), String> {
    let conn = open_conn(&db.0).map_err(|e| e.to_string())?;
    conn.execute("UPDATE accounts SET simplefin_id = NULL WHERE id = ?1", params![account_id])
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Accepts either an access URL (returned as-is) or a one-time setup token,
/// which is exchanged for an access URL.
#[tauri::command]
pub async fn resolve_simplefin_access(value: String) -> Result<String, String> {
    let trimmed = value.trim().to_string();
    if trimmed.is_empty() {
        return Ok(String::new());
    }
    if simplefin::is_access_url(&trimmed) {
        return Ok(trimmed);
    }
    tauri::async_runtime::spawn_blocking(move || simplefin::claim_access_url(&trimmed))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        db::run_migrations(&conn).unwrap();
        conn
    }

    fn mapping(conn: &Connection, id: i64) -> Option<String> {
        conn.query_row("SELECT simplefin_id FROM accounts WHERE id = ?1", [id], |r| r.get(0)).unwrap()
    }

    #[test]
    fn map_creates_account_when_none_given() {
        let conn = open();
        let id = do_map(&conn, "sf-1", None, Some("Ally"), Some("1234"), Some("savings")).unwrap();
        assert_eq!(mapping(&conn, id).as_deref(), Some("sf-1"));
        let kind: Option<String> = conn.query_row("SELECT account_type FROM accounts WHERE id = ?1", [id], |r| r.get(0)).unwrap();
        assert_eq!(kind.as_deref(), Some("savings"));
    }

    #[test]
    fn map_moves_feed_id_between_accounts() {
        let conn = open();
        let a = db::store::upsert_account(&conn, "Chase", "7793", None).unwrap();
        let b = db::store::upsert_account(&conn, "Chase", "0001", None).unwrap();
        do_map(&conn, "sf-1", Some(a), None, None, None).unwrap();
        do_map(&conn, "sf-1", Some(b), None, None, None).unwrap();
        assert_eq!(mapping(&conn, a), None);
        assert_eq!(mapping(&conn, b).as_deref(), Some("sf-1"));
    }

    #[test]
    fn map_validates_input() {
        let conn = open();
        assert!(do_map(&conn, "", Some(1), None, None, None).is_err());
        assert!(do_map(&conn, "sf-1", Some(999), None, None, None).is_err());
        assert!(do_map(&conn, "sf-1", None, Some("Ally"), None, None).is_err());
        assert!(do_map(&conn, "sf-1", None, None, Some("1234"), None).is_err());
    }
}
