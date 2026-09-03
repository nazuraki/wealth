//! Account listing, feed-account mapping, and category rule maintenance.

use crate::{config, Args};
use anyhow::{anyhow, bail, Result};
use rusqlite::{params, Connection};

pub fn accounts(args: &Args) -> Result<()> {
    let conn = config::open(args)?;
    let mut stmt = conn.prepare(
        "SELECT id, institution, account_number_last4, account_type, simplefin_id FROM accounts ORDER BY id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, Option<String>>(3)?,
            r.get::<_, Option<String>>(4)?,
        ))
    })?;
    println!("{:>4}  {:<28} {:<6} {:<12} simplefin_id", "id", "institution", "last4", "type");
    for row in rows {
        let (id, inst, last4, kind, sf) = row?;
        println!(
            "{id:>4}  {inst:<28} {last4:<6} {:<12} {}",
            kind.unwrap_or_default(),
            sf.unwrap_or_else(|| "-".into())
        );
    }
    Ok(())
}

/// Attach a feed account to an existing account (`--account ID`) or a new one
/// (`--institution NAME --last4 NNNN [--type TYPE]`).
pub fn map(args: &Args) -> Result<()> {
    let simplefin_id = args
        .positional
        .get(1)
        .ok_or_else(|| anyhow!("usage: wealth-cli map SIMPLEFIN_ID (--account ID | --institution NAME --last4 NNNN)"))?;
    let conn = config::open(args)?;
    let id = match args.get("account") {
        Some(id) => id.parse::<i64>().map_err(|_| anyhow!("--account must be a numeric account id"))?,
        None => {
            let institution = args.get("institution").ok_or_else(|| anyhow!("--institution is required without --account"))?;
            let last4 = args.get("last4").ok_or_else(|| anyhow!("--last4 is required without --account"))?;
            db::store::upsert_account(&conn, institution, last4, args.get("type"))?
        }
    };
    do_map(&conn, simplefin_id, id)?;
    eprintln!("mapped feed account {simplefin_id} → account {id}");
    Ok(())
}

pub fn do_map(conn: &Connection, simplefin_id: &str, account_id: i64) -> Result<()> {
    conn.execute("UPDATE accounts SET simplefin_id = NULL WHERE simplefin_id = ?1", params![simplefin_id])?;
    let n = conn.execute("UPDATE accounts SET simplefin_id = ?1 WHERE id = ?2", params![simplefin_id, account_id])?;
    if n == 0 {
        bail!("account {account_id} not found");
    }
    Ok(())
}

pub fn unmap(args: &Args) -> Result<()> {
    let id: i64 = args
        .positional
        .get(1)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| anyhow!("usage: wealth-cli unmap ACCOUNT_ID"))?;
    let conn = config::open(args)?;
    let n = conn.execute("UPDATE accounts SET simplefin_id = NULL WHERE id = ?1", params![id])?;
    if n == 0 {
        bail!("account {id} not found");
    }
    eprintln!("account {id} is no longer synced");
    Ok(())
}

pub fn rules(args: &Args) -> Result<()> {
    let conn = config::open(args)?;
    let sub = args.positional.get(1).map(String::as_str).unwrap_or("list");
    match sub {
        "list" => {
            for r in db::rules::list(&conn)? {
                println!("{:<8} {:<40} {}", r.source, r.pattern, r.category);
            }
        }
        "add" => {
            let (pattern, category) = match (args.positional.get(2), args.positional.get(3)) {
                (Some(p), Some(c)) => (p, c),
                _ => bail!("usage: wealth-cli rules add PATTERN CATEGORY"),
            };
            db::rules::learn(&conn, pattern, category, db::rules::SOURCE_USER)?;
            eprintln!("rule: \"{}\" → {category}", db::rules::normalize(pattern));
        }
        "remove" => {
            let pattern = args.positional.get(2).ok_or_else(|| anyhow!("usage: wealth-cli rules remove PATTERN"))?;
            if !db::rules::delete(&conn, pattern)? {
                bail!("no rule matches \"{}\"", db::rules::normalize(pattern));
            }
            eprintln!("removed rule \"{}\"", db::rules::normalize(pattern));
        }
        other => bail!("unknown rules subcommand: {other} (expected list, add, remove)"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn do_map_moves_and_validates() {
        let conn = Connection::open_in_memory().unwrap();
        db::run_migrations(&conn).unwrap();
        let a = db::store::upsert_account(&conn, "Ally", "1111", None).unwrap();
        let b = db::store::upsert_account(&conn, "Ally", "2222", None).unwrap();
        do_map(&conn, "sf-1", a).unwrap();
        do_map(&conn, "sf-1", b).unwrap();
        let mapped: Vec<Option<String>> = conn
            .prepare("SELECT simplefin_id FROM accounts ORDER BY id")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(mapped, vec![None, Some("sf-1".to_string())]);
        assert!(do_map(&conn, "sf-2", 99).is_err());
    }
}
