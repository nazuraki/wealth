//! Write feed accounts into the wealth database.
//!
//! Feed transactions land in synthesized calendar-month statements
//! (`source = 'simplefin'`). An account's feed starts strictly after its latest
//! PDF statement so the two never overlap. Re-running is idempotent thanks to
//! `transactions.external_id`; existing rows are never modified.

use crate::client::{Feed, FeedAccount};
use crate::mapping::{self, Skip};
use anyhow::Result;
use db::rules;
use db::store::{self, SOURCE_PDF, SOURCE_SIMPLEFIN};
use extractor::{is_transfer, ClaudeClient};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::collections::{BTreeSet, HashMap};

pub const UNCATEGORIZED: &str = "Uncategorized";
const LOOKBACK_SECS: i64 = 7 * 86_400;
const MODEL_BATCH: usize = 150;

#[derive(Debug, Clone, Serialize)]
pub struct MappedAccount {
    pub id: i64,
    pub institution: String,
    pub account_number_last4: String,
    pub simplefin_id: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct AccountSyncResult {
    pub account_id: i64,
    pub institution: String,
    pub account_number_last4: String,
    pub inserted: usize,
    pub already_present: usize,
    pub skipped_before_cutover: usize,
    pub skipped_pending: usize,
    pub periods: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UnmappedAccount {
    pub simplefin_id: String,
    pub institution: String,
    pub name: String,
    pub balance: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct SyncReport {
    pub accounts: Vec<AccountSyncResult>,
    pub unmapped: Vec<UnmappedAccount>,
    pub errors: Vec<String>,
    pub categorized_by_rule: usize,
    pub categorized_by_model: usize,
    pub uncategorized: usize,
}

pub fn mapped_accounts(conn: &Connection) -> Result<Vec<MappedAccount>> {
    let mut stmt = conn.prepare(
        "SELECT id, institution, account_number_last4, simplefin_id FROM accounts \
         WHERE simplefin_id IS NOT NULL ORDER BY id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(MappedAccount {
            id: r.get(0)?,
            institution: r.get(1)?,
            account_number_last4: r.get(2)?,
            simplefin_id: r.get(3)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Earliest `start-date` needed across mapped accounts: a week before the newest
/// synced row, or the month after the last PDF statement. `None` lets the feed
/// use its default window (no anchor on any account).
pub fn start_date(conn: &Connection, mapped: &[MappedAccount]) -> Result<Option<i64>> {
    let mut anchors: Vec<i64> = vec![];
    for m in mapped {
        let newest: Option<String> = conn
            .query_row(
                "SELECT MAX(t.date) FROM transactions t JOIN statements s ON s.id = t.statement_id \
                 WHERE s.account_id = ?1 AND t.external_id IS NOT NULL",
                params![m.id],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        if let Some(ts) = newest.as_deref().and_then(mapping::date_to_unix) {
            anchors.push(ts - LOOKBACK_SECS);
        } else if let Some(period) = store::latest_period(conn, m.id, SOURCE_PDF)? {
            if let Some(ts) = mapping::month_after_start(&period) {
                anchors.push(ts);
            }
        }
    }
    Ok(anchors.into_iter().min())
}

pub fn run_sync(conn: &mut Connection, feed: &dyn Feed, model: Option<&dyn ClaudeClient>) -> Result<SyncReport> {
    let mapped = mapped_accounts(conn)?;
    let start = start_date(conn, &mapped)?;
    let set = feed.fetch(start)?;

    let mut report = SyncReport { errors: set.errors.clone(), ..Default::default() };
    let tx = conn.transaction()?;
    let mut pending_model: Vec<(i64, String)> = vec![];
    for fa in &set.accounts {
        match mapped.iter().find(|m| m.simplefin_id == fa.id) {
            Some(m) => {
                let r = sync_account(&tx, m, fa, &mut pending_model, &mut report)?;
                report.accounts.push(r);
            }
            None => report.unmapped.push(UnmappedAccount {
                simplefin_id: fa.id.clone(),
                institution: fa.institution(),
                name: fa.name.clone(),
                balance: fa.balance_f64(),
            }),
        }
    }
    categorize_pending(&tx, pending_model, model, &mut report)?;
    tx.commit()?;
    Ok(report)
}

fn sync_account(
    tx: &Connection,
    m: &MappedAccount,
    fa: &FeedAccount,
    pending_model: &mut Vec<(i64, String)>,
    report: &mut SyncReport,
) -> Result<AccountSyncResult> {
    let cutover = store::latest_period(tx, m.id, SOURCE_PDF)?;
    let mut res = AccountSyncResult {
        account_id: m.id,
        institution: m.institution.clone(),
        account_number_last4: m.account_number_last4.clone(),
        ..Default::default()
    };
    let mut periods = BTreeSet::new();

    for ft in &fa.transactions {
        let n = match mapping::normalize(ft) {
            Ok(n) => n,
            Err(Skip::Pending) => {
                res.skipped_pending += 1;
                continue;
            }
            Err(other) => {
                report.errors.push(format!("{}: skipped transaction {}: {other:?}", fa.name, ft.id));
                continue;
            }
        };
        if cutover.as_deref().is_some_and(|c| n.period.as_str() <= c) {
            res.skipped_before_cutover += 1;
            continue;
        }
        let stmt = store::find_or_create_statement(tx, m.id, &n.period, SOURCE_SIMPLEFIN, "simplefin", None, None)?;
        if stmt.source != SOURCE_SIMPLEFIN {
            // A PDF statement was imported for a month after the cutover; never mix sources.
            res.skipped_before_cutover += 1;
            continue;
        }
        let (category, by_rule) = match rules::lookup(tx, &n.description)? {
            Some(c) => (c, true),
            None => (UNCATEGORIZED.to_string(), false),
        };
        tx.execute(
            "INSERT OR IGNORE INTO transactions \
             (statement_id, date, description, category, amount, type, is_transfer, external_id) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                stmt.id,
                n.date,
                n.description,
                category,
                n.amount,
                n.kind,
                is_transfer(&n.description) as i64,
                n.external_id
            ],
        )?;
        if tx.changes() == 0 {
            res.already_present += 1;
            continue;
        }
        res.inserted += 1;
        periods.insert(n.period.clone());
        if by_rule {
            report.categorized_by_rule += 1;
        } else {
            pending_model.push((tx.last_insert_rowid(), n.description));
        }
    }

    if let (Some(balance), Some(ts)) = (fa.balance_f64(), fa.balance_date) {
        let period = mapping::period_of(&mapping::iso_date(ts)).to_string();
        tx.execute(
            "UPDATE statements SET closing_balance = ?1 \
             WHERE account_id = ?2 AND statement_period = ?3 AND source = ?4",
            params![balance, m.id, period, SOURCE_SIMPLEFIN],
        )?;
    }
    res.periods = periods.into_iter().collect();
    Ok(res)
}

fn categorize_pending(
    tx: &Connection,
    pending: Vec<(i64, String)>,
    model: Option<&dyn ClaudeClient>,
    report: &mut SyncReport,
) -> Result<()> {
    if pending.is_empty() {
        return Ok(());
    }
    let Some(model) = model else {
        report.uncategorized += pending.len();
        return Ok(());
    };
    let known: Vec<String> = store::known_categories(tx)?
        .into_iter()
        .filter(|c| c != UNCATEGORIZED)
        .collect();
    let mut unique: Vec<String> = vec![];
    for (_, d) in &pending {
        if !unique.contains(d) {
            unique.push(d.clone());
        }
    }
    let mut answers: HashMap<String, String> = HashMap::new();
    for chunk in unique.chunks(MODEL_BATCH) {
        match model.categorize(chunk, &known) {
            Ok(cats) => answers.extend(chunk.iter().cloned().zip(cats)),
            Err(e) => {
                report.errors.push(format!("categorization failed: {e}"));
                break;
            }
        }
    }
    for (id, d) in &pending {
        match answers.get(d).map(|c| c.trim()).filter(|c| !c.is_empty()) {
            Some(c) => {
                tx.execute("UPDATE transactions SET category = ?1 WHERE id = ?2", params![c, id])?;
                rules::learn(tx, d, c, rules::SOURCE_LEARNED)?;
                report.categorized_by_model += 1;
            }
            None => report.uncategorized += 1,
        }
    }
    Ok(())
}
