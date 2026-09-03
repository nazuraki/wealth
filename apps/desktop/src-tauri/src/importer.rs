use anyhow::Result;
use db::store::{self, SOURCE_PDF, SOURCE_SIMPLEFIN};
use extractor::{
    extract_text, is_transfer, parse_line_items, AccountExtraction, AnthropicClient,
    ExtractionResult, TransactionType,
};
use rusqlite::{params, Connection, Transaction};
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager, State};

#[derive(Debug, Serialize)]
pub struct ImportSummary {
    pub institution: String,
    pub account_number_last4: String,
    pub statement_period: String,
    pub transaction_count: usize,
}

#[derive(Debug, Serialize)]
pub struct DuplicateConflict {
    pub institution: String,
    pub account_number_last4: String,
    pub statement_period: String,
}

#[derive(Debug, Serialize)]
pub struct ImportResponse {
    pub summaries: Vec<ImportSummary>,
    pub conflicts: Vec<DuplicateConflict>,
}

/// Normalize a transaction date to YYYY-MM-DD. Extractors sometimes return
/// MM/DD (e.g. for credit-card statements that print no year). We anchor the
/// year to the statement_period (YYYY-MM): if the tx month is greater than
/// the period month, the tx is in the prior calendar year.
fn normalize_tx_date(date: &str, statement_period: &str) -> String {
    let is_mm_dd = date.len() == 5
        && date.as_bytes()[2] == b'/'
        && date.as_bytes()[..2].iter().chain(&date.as_bytes()[3..]).all(|b| b.is_ascii_digit());
    if !is_mm_dd { return date.to_string(); }
    let period_year: i32 = match statement_period.get(..4).and_then(|s| s.parse().ok()) {
        Some(y) => y,
        None => return date.to_string(),
    };
    let period_month: u32 = match statement_period.get(5..7).and_then(|s| s.parse().ok()) {
        Some(m) => m,
        None => return date.to_string(),
    };
    let tx_month: u32 = date[..2].parse().unwrap_or(0);
    let tx_day: u32 = date[3..].parse().unwrap_or(0);
    let year = if tx_month <= period_month { period_year } else { period_year - 1 };
    format!("{:04}-{:02}-{:02}", year, tx_month, tx_day)
}

// ── DB write (testable inner fn) ──────────────────────────────────────────────

fn synced_period_error(acct: &extractor::Account) -> anyhow::Error {
    anyhow::anyhow!(
        "{} ···{} {} is synced from SimpleFIN; PDF import for that period is not allowed",
        acct.institution,
        acct.account_number_last4,
        acct.statement_period
    )
}

fn check_account_conflict(
    tx: &Transaction<'_>,
    extraction: &AccountExtraction,
) -> Result<Option<DuplicateConflict>> {
    let acct = &extraction.account;
    let source: Option<String> = tx
        .query_row(
            "SELECT s.source FROM statements s \
             JOIN accounts a ON a.id = s.account_id \
             WHERE a.account_number_last4 = ?1 AND s.statement_period = ?2",
            params![acct.account_number_last4, acct.statement_period],
            |row| row.get(0),
        )
        .ok();
    match source.as_deref() {
        Some(SOURCE_SIMPLEFIN) => Err(synced_period_error(acct)),
        Some(_) => Ok(Some(DuplicateConflict {
            institution: acct.institution.clone(),
            account_number_last4: acct.account_number_last4.clone(),
            statement_period: acct.statement_period.clone(),
        })),
        None => Ok(None),
    }
}

fn detect_conflicts(
    tx: &Transaction<'_>,
    result: &ExtractionResult,
) -> Result<Vec<DuplicateConflict>> {
    result
        .accounts
        .iter()
        .filter_map(|a| check_account_conflict(tx, a).transpose())
        .collect()
}

fn write_account_to_tx(
    tx: &Transaction<'_>,
    source_file: &str,
    extraction: &AccountExtraction,
    overwrite: bool,
) -> Result<ImportSummary> {
    let acct = &extraction.account;
    let account_id = store::upsert_account(
        tx,
        &acct.institution,
        &acct.account_number_last4,
        acct.account_type.as_deref(),
    )?;

    let stmt = store::find_or_create_statement(
        tx,
        account_id,
        &acct.statement_period,
        SOURCE_PDF,
        source_file,
        acct.opening_balance,
        acct.closing_balance,
    )?;
    if stmt.source == SOURCE_SIMPLEFIN {
        return Err(synced_period_error(acct));
    }

    let write_transactions = if stmt.inserted {
        true
    } else if overwrite {
        tx.execute(
            "DELETE FROM transactions WHERE statement_id = ?1",
            params![stmt.id],
        )?;
        // Update statement metadata in case it changed.
        tx.execute(
            "UPDATE statements SET opening_balance = ?1, closing_balance = ?2, source_file = ?3 \
             WHERE id = ?4",
            params![acct.opening_balance, acct.closing_balance, source_file, stmt.id],
        )?;
        true
    } else {
        false
    };

    if write_transactions {
        let mut ins = tx.prepare(
            "INSERT INTO transactions (statement_id, date, description, category, amount, type, is_transfer) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )?;
        for t in &extraction.transactions {
            let flagged = is_transfer(&t.description) || t.transaction_type == TransactionType::Transfer;
            let kind = match t.transaction_type {
                TransactionType::Credit => "credit",
                _ => "debit",
            };
            let normalized_date = normalize_tx_date(&t.date, &acct.statement_period);
            ins.execute(params![
                stmt.id,
                normalized_date,
                t.description,
                t.category,
                t.amount,
                kind,
                flagged as i64
            ])?;
        }
    }

    Ok(ImportSummary {
        institution: acct.institution.clone(),
        account_number_last4: acct.account_number_last4.clone(),
        statement_period: acct.statement_period.clone(),
        transaction_count: if write_transactions { extraction.transactions.len() } else { 0 },
    })
}

fn write_to_tx(
    tx: &Transaction<'_>,
    source_file: &str,
    result: &ExtractionResult,
    overwrite: bool,
) -> Result<Vec<ImportSummary>> {
    result
        .accounts
        .iter()
        .map(|a| write_account_to_tx(tx, source_file, a, overwrite))
        .collect()
}

pub(crate) fn write_to_db(
    db_path: &Path,
    source_file: &str,
    result: &ExtractionResult,
    overwrite: bool,
) -> Result<Vec<ImportSummary>> {
    let mut conn = Connection::open(db_path)?;
    db::run_migrations(&conn)?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    let tx = conn.transaction()?;
    let summaries = write_to_tx(&tx, source_file, result, overwrite)?;
    tx.commit()?;
    Ok(summaries)
}

pub(crate) fn check_conflicts_in_db(
    db_path: &Path,
    result: &ExtractionResult,
) -> Result<Vec<DuplicateConflict>> {
    let mut conn = Connection::open(db_path)?;
    db::run_migrations(&conn)?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    let tx = conn.transaction()?;
    let conflicts = detect_conflicts(&tx, result)?;
    // No commit needed — read-only.
    Ok(conflicts)
}

// ── Tauri command ─────────────────────────────────────────────────────────────

fn do_import(app: &AppHandle, db_path: PathBuf, path: &str, overwrite: bool) -> Result<ImportResponse> {
    let pdf_path = PathBuf::from(path);
    let data_dir = app.path().app_data_dir()?;

    let s = crate::settings::load(&data_dir);
    let api_key = s
        .api_key
        .filter(|k| !k.is_empty())
        .or_else(|| std::env::var("ANTHROPIC_API_KEY").ok())
        .ok_or_else(|| anyhow::anyhow!("No API key configured. Set one in Settings or set the ANTHROPIC_API_KEY environment variable."))?;
    let base_url = s
        .endpoint_url
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| AnthropicClient::DEFAULT_ENDPOINT.to_string());
    let client = AnthropicClient::with_config(api_key, base_url);

    let text = extract_text(&pdf_path)?;
    let label = pdf_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(path);
    let result = parse_line_items(&text, label, &client)?;

    if !overwrite {
        let conflicts = check_conflicts_in_db(&db_path, &result)?;
        if !conflicts.is_empty() {
            return Ok(ImportResponse { summaries: vec![], conflicts });
        }
    }

    let summaries = write_to_db(&db_path, path, &result, overwrite)?;
    Ok(ImportResponse { summaries, conflicts: vec![] })
}

#[tauri::command]
pub async fn import_statement(
    app: AppHandle,
    db: State<'_, crate::DbPath>,
    path: String,
    overwrite: bool,
) -> Result<ImportResponse, String> {
    let db_path = db.0.clone();
    tauri::async_runtime::spawn_blocking(move || do_import(&app, db_path, &path, overwrite))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[cfg(test)]
#[path = "importer_tests.rs"]
mod tests;
