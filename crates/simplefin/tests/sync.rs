use anyhow::Result;
use db::store::{self, SOURCE_PDF, SOURCE_SIMPLEFIN};
use extractor::{ClaudeClient, ExtractionResult};
use rusqlite::Connection;
use simplefin::client::parse_account_set;
use simplefin::{run_sync, AccountSet, Feed, UNCATEGORIZED};
use std::cell::RefCell;
use std::sync::Mutex;

// 2026-01-15 and 2026-02-03, both 12:00 UTC.
const JAN_15: i64 = 1_768_478_400;
const FEB_03: i64 = 1_770_120_000;

struct FixtureFeed {
    json: String,
    calls: RefCell<Vec<Option<i64>>>,
}

impl FixtureFeed {
    fn new(json: &str) -> Self {
        Self { json: json.to_string(), calls: RefCell::new(vec![]) }
    }
}

impl Feed for FixtureFeed {
    fn fetch(&self, start_date: Option<i64>) -> Result<AccountSet> {
        self.calls.borrow_mut().push(start_date);
        parse_account_set(&self.json)
    }
}

struct FixedModel {
    answers: Vec<&'static str>,
    seen: Mutex<Vec<Vec<String>>>,
}

impl ClaudeClient for FixedModel {
    fn extract_from_statement(&self, _: &str, _: &str) -> Result<ExtractionResult> {
        unreachable!()
    }
    fn categorize(&self, descriptions: &[String], _known: &[String]) -> Result<Vec<String>> {
        self.seen.lock().unwrap().push(descriptions.to_vec());
        Ok(descriptions.iter().enumerate().map(|(i, _)| self.answers[i % self.answers.len()].to_string()).collect())
    }
}

fn feed_json() -> String {
    format!(
        r#"{{"errors":[],"accounts":[
          {{"org":{{"name":"Chase"}},"id":"sf-chase","name":"Amazon Visa (...7793)","balance":"-250.10","balance-date":{FEB_03},
            "transactions":[
              {{"id":"c1","posted":{JAN_15},"amount":"-12.00","description":"GITHUB INC","pending":false}},
              {{"id":"c2","posted":{FEB_03},"amount":"-40.00","description":"ANTHROPIC","pending":false}},
              {{"id":"c3","posted":{FEB_03},"amount":"300.00","description":"Payment to Chase card ending in 7793","pending":false}},
              {{"id":"c4","posted":0,"amount":"-5.00","description":"PENDING COFFEE","pending":true}}
            ]}},
          {{"org":{{"name":"Ally"}},"id":"sf-ally","name":"Online Savings","balance":"1500.00","balance-date":{FEB_03},
            "transactions":[{{"id":"a1","posted":{FEB_03},"amount":"2.10","description":"Interest Paid","pending":false}}]}}
        ]}}"#
    )
}

fn open() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    db::run_migrations(&conn).unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    conn
}

fn map_chase(conn: &Connection) -> i64 {
    let id = store::upsert_account(conn, "Chase", "7793", Some("credit_card")).unwrap();
    conn.execute("UPDATE accounts SET simplefin_id = 'sf-chase' WHERE id = ?1", [id]).unwrap();
    id
}

fn count(conn: &Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |r| r.get(0)).unwrap()
}

#[test]
fn inserts_mapped_account_and_reports_unmapped() {
    let mut conn = open();
    let chase = map_chase(&conn);
    db::rules::learn(&conn, "github", "Software", db::rules::SOURCE_USER).unwrap();
    let feed = FixtureFeed::new(&feed_json());

    let report = run_sync(&mut conn, &feed, None).unwrap();

    assert_eq!(feed.calls.borrow().as_slice(), &[None], "no anchor → feed default window");
    assert_eq!(report.accounts.len(), 1);
    let r = &report.accounts[0];
    assert_eq!(r.account_id, chase);
    assert_eq!((r.inserted, r.skipped_pending, r.already_present), (3, 1, 0));
    assert_eq!(r.periods, vec!["2026-01", "2026-02"]);
    assert_eq!(report.unmapped.len(), 1);
    assert_eq!(report.unmapped[0].simplefin_id, "sf-ally");
    assert_eq!(report.unmapped[0].institution, "Ally");
    assert_eq!(report.categorized_by_rule, 1);
    assert_eq!(report.uncategorized, 2, "no model → rows stay uncategorized");

    assert_eq!(count(&conn, "SELECT COUNT(*) FROM statements WHERE source = 'simplefin'"), 2);
    let (cat, xfer): (String, i64) = conn
        .query_row("SELECT category, is_transfer FROM transactions WHERE external_id = 'c3'", [], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap();
    assert_eq!((cat.as_str(), xfer), (UNCATEGORIZED, 1));
    let github: String = conn
        .query_row("SELECT category FROM transactions WHERE external_id = 'c1'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(github, "Software");
    let balance: Option<f64> = conn
        .query_row("SELECT closing_balance FROM statements WHERE statement_period = '2026-02'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(balance, Some(-250.10));
}

#[test]
fn rerun_is_idempotent_and_keeps_user_edits() {
    let mut conn = open();
    map_chase(&conn);
    let feed = FixtureFeed::new(&feed_json());
    run_sync(&mut conn, &feed, None).unwrap();
    conn.execute("UPDATE transactions SET category = 'Dev Tools' WHERE external_id = 'c1'", []).unwrap();

    let report = run_sync(&mut conn, &feed, None).unwrap();

    assert_eq!((report.accounts[0].inserted, report.accounts[0].already_present), (0, 3));
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM transactions"), 3);
    let cat: String = conn
        .query_row("SELECT category FROM transactions WHERE external_id = 'c1'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(cat, "Dev Tools");
    // Second call anchors a week before the newest synced row (2026-02-03).
    let second = feed.calls.borrow()[1].unwrap();
    assert_eq!(simplefin::mapping::iso_date(second), "2026-01-27");
}

#[test]
fn feed_starts_after_last_pdf_statement() {
    let mut conn = open();
    let chase = map_chase(&conn);
    store::find_or_create_statement(&conn, chase, "2026-01", SOURCE_PDF, "jan.pdf", None, Some(1.0)).unwrap();
    let feed = FixtureFeed::new(&feed_json());

    let report = run_sync(&mut conn, &feed, None).unwrap();

    assert_eq!(simplefin::mapping::iso_date(feed.calls.borrow()[0].unwrap()), "2026-02-01");
    let r = &report.accounts[0];
    assert_eq!((r.inserted, r.skipped_before_cutover), (2, 1));
    assert_eq!(r.periods, vec!["2026-02"]);
    assert_eq!(store::statement_source(&conn, chase, "2026-01").unwrap().as_deref(), Some(SOURCE_PDF));
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM transactions WHERE external_id = 'c1'"), 0);
}

#[test]
fn never_writes_into_a_pdf_statement_month() {
    let mut conn = open();
    let chase = map_chase(&conn);
    // PDF for Feb exists but Jan does not: Jan syncs, Feb is left alone.
    store::find_or_create_statement(&conn, chase, "2025-12", SOURCE_PDF, "dec.pdf", None, None).unwrap();
    store::find_or_create_statement(&conn, chase, "2026-02", SOURCE_PDF, "feb.pdf", None, None).unwrap();
    let feed = FixtureFeed::new(&feed_json());

    let report = run_sync(&mut conn, &feed, None).unwrap();

    let r = &report.accounts[0];
    assert_eq!((r.inserted, r.skipped_before_cutover), (0, 3));
    assert_eq!(store::statement_source(&conn, chase, "2026-02").unwrap().as_deref(), Some(SOURCE_PDF));
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM transactions"), 0);
}

#[test]
fn model_categorizes_new_rows_and_learns_rules() {
    let mut conn = open();
    map_chase(&conn);
    let feed = FixtureFeed::new(&feed_json());
    let model = FixedModel { answers: vec!["Software", "AI APIs", "Transfer"], seen: Mutex::new(vec![]) };

    let report = run_sync(&mut conn, &feed, Some(&model)).unwrap();

    assert_eq!((report.categorized_by_model, report.uncategorized), (3, 0));
    let seen = model.seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0], vec!["GITHUB INC", "ANTHROPIC", "Payment to Chase card ending in 7793"]);
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM category_rules WHERE source = 'learned'"), 3);
    assert_eq!(db::rules::lookup(&conn, "anthropic").unwrap().as_deref(), Some("AI APIs"));
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM transactions WHERE category = 'Uncategorized'"), 0);
    assert_eq!(count(&conn, &format!("SELECT COUNT(*) FROM statements WHERE source = '{SOURCE_SIMPLEFIN}'")), 2);
}

#[test]
fn feed_errors_are_surfaced_without_aborting() {
    let mut conn = open();
    map_chase(&conn);
    let json = feed_json().replace(r#""errors":[]"#, r#""errors":["Ally: login needs attention"]"#);
    let feed = FixtureFeed::new(&json);

    let report = run_sync(&mut conn, &feed, None).unwrap();

    assert_eq!(report.errors, vec!["Ally: login needs attention"]);
    assert_eq!(report.accounts[0].inserted, 3);
}
