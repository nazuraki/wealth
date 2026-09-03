//! CSV export of transactions, filtered by category group, category, account and date.

use crate::{config, Args};
use anyhow::Result;
use rusqlite::{types::ToSql, Connection};
use std::io::Write;

pub struct Filters<'a> {
    pub group: Option<&'a str>,
    pub category: Option<&'a str>,
    pub account_last4: Option<&'a str>,
    pub from: Option<&'a str>,
    pub to: Option<&'a str>,
    pub include_transfers: bool,
}

impl<'a> Filters<'a> {
    pub fn from_args(args: &'a Args) -> Self {
        Self {
            group: args.get("group"),
            category: args.get("category"),
            account_last4: args.get("account"),
            from: args.get("from"),
            to: args.get("to"),
            include_transfers: args.flag("include-transfers"),
        }
    }
}

pub const HEADER: &str = "date,institution,account,description,category,group,amount,type,is_transfer";

pub fn run(args: &Args) -> Result<()> {
    let conn = config::open(args)?;
    let rows = query(&conn, &Filters::from_args(args))?;
    match args.get("out") {
        Some(path) => {
            let mut f = std::fs::File::create(path)?;
            write_csv(&mut f, &rows)?;
            eprintln!("wrote {} rows to {path}", rows.len());
        }
        None => {
            let stdout = std::io::stdout();
            write_csv(&mut stdout.lock(), &rows)?;
        }
    }
    Ok(())
}

/// One exported row: date, institution, last4, description, category, group, amount, type, is_transfer.
pub type Row = (String, String, String, String, String, Option<String>, f64, String, bool);

pub fn query(conn: &Connection, f: &Filters<'_>) -> Result<Vec<Row>> {
    let mut conditions: Vec<String> = vec![];
    let mut params: Vec<Box<dyn ToSql>> = vec![];
    if let Some(g) = f.group {
        conditions.push("g.name = ?".into());
        params.push(Box::new(g.to_string()));
    }
    if let Some(c) = f.category {
        conditions.push("LOWER(t.category) = LOWER(?)".into());
        params.push(Box::new(c.to_string()));
    }
    if let Some(a) = f.account_last4 {
        conditions.push("a.account_number_last4 = ?".into());
        params.push(Box::new(a.to_string()));
    }
    if let Some(d) = f.from {
        conditions.push("t.date >= ?".into());
        params.push(Box::new(d.to_string()));
    }
    if let Some(d) = f.to {
        conditions.push("t.date <= ?".into());
        params.push(Box::new(d.to_string()));
    }
    if !f.include_transfers {
        conditions.push("t.is_transfer = 0".into());
    }
    let where_clause = if conditions.is_empty() { String::new() } else { format!("WHERE {}", conditions.join(" AND ")) };
    let sql = format!(
        "SELECT t.date, a.institution, a.account_number_last4, t.description, t.category, g.name, \
                t.amount, t.type, t.is_transfer \
         FROM transactions t \
         JOIN statements s ON s.id = t.statement_id \
         JOIN accounts a ON a.id = s.account_id \
         LEFT JOIN category_group_members m ON m.category = t.category \
         LEFT JOIN category_groups g ON g.id = m.group_id \
         {where_clause} \
         ORDER BY t.date, t.id"
    );
    let refs: Vec<&dyn ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(refs.as_slice(), |r| {
        Ok((
            r.get(0)?,
            r.get(1)?,
            r.get(2)?,
            r.get(3)?,
            r.get(4)?,
            r.get(5)?,
            r.get(6)?,
            r.get(7)?,
            r.get::<_, i64>(8)? != 0,
        ))
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn csv_field(s: &str) -> String {
    if s.contains(['"', ',', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

pub fn write_csv(out: &mut impl Write, rows: &[Row]) -> Result<()> {
    writeln!(out, "{HEADER}")?;
    for (date, inst, last4, desc, cat, group, amount, kind, xfer) in rows {
        writeln!(
            out,
            "{},{},{},{},{},{},{:.2},{},{}",
            date,
            csv_field(inst),
            last4,
            csv_field(desc),
            csv_field(cat),
            csv_field(group.as_deref().unwrap_or("")),
            amount,
            kind,
            xfer
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        db::run_migrations(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO accounts (institution, account_number_last4) VALUES ('Chase', '7793');
             INSERT INTO statements (account_id, statement_period) VALUES (1, '2026-02');
             INSERT INTO transactions (statement_id, date, description, category, amount, type, is_transfer) VALUES
               (1, '2026-02-01', 'GITHUB, INC', 'Software', 4.00, 'debit', 0),
               (1, '2026-02-02', 'ANTHROPIC', 'AI APIs', 40.00, 'debit', 0),
               (1, '2026-02-03', 'WHOLE FOODS', 'Groceries', 80.00, 'debit', 0),
               (1, '2026-02-04', 'Payment', 'Transfer', 300.00, 'credit', 1);
             INSERT INTO category_groups (name) VALUES ('Business');
             INSERT INTO category_group_members (category, group_id) VALUES ('Software', 1), ('AI APIs', 1);",
        )
        .unwrap();
        conn
    }

    fn filters<'a>() -> Filters<'a> {
        Filters { group: None, category: None, account_last4: None, from: None, to: None, include_transfers: false }
    }

    #[test]
    fn group_filter_selects_only_member_categories() {
        let conn = seed();
        let rows = query(&conn, &Filters { group: Some("Business"), ..filters() }).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].3, "GITHUB, INC");
        assert_eq!(rows[1].5.as_deref(), Some("Business"));
    }

    #[test]
    fn transfers_excluded_unless_requested() {
        let conn = seed();
        assert_eq!(query(&conn, &filters()).unwrap().len(), 3);
        assert_eq!(query(&conn, &Filters { include_transfers: true, ..filters() }).unwrap().len(), 4);
        let dated = query(&conn, &Filters { from: Some("2026-02-02"), to: Some("2026-02-02"), ..filters() }).unwrap();
        assert_eq!(dated.len(), 1);
        assert_eq!(dated[0].4, "AI APIs");
    }

    #[test]
    fn csv_quotes_fields_with_commas_and_quotes() {
        let conn = seed();
        let rows = query(&conn, &Filters { category: Some("software"), ..filters() }).unwrap();
        let mut buf = Vec::new();
        write_csv(&mut buf, &rows).unwrap();
        let text = String::from_utf8(buf).unwrap();
        assert_eq!(text, format!("{HEADER}\n2026-02-01,Chase,7793,\"GITHUB, INC\",Software,Business,4.00,debit,false\n"));
        assert_eq!(csv_field("say \"hi\""), "\"say \"\"hi\"\"\"");
    }
}
