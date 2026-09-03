//! Description → category rules. Applied to feed transactions before asking
//! the model, and learned from the model's answers so repeat merchants are free.

use rusqlite::{params, Connection, OptionalExtension};

pub const SOURCE_USER: &str = "user";
pub const SOURCE_LEARNED: &str = "learned";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub pattern: String,
    pub category: String,
    pub source: String,
}

/// Lowercase and collapse whitespace so rules match regardless of case or spacing.
pub fn normalize(description: &str) -> String {
    description
        .split_whitespace()
        .map(|w| w.to_lowercase())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Find the best rule for a description. Prefix matches win over substring
/// matches; among equals the longest pattern wins.
pub fn lookup(conn: &Connection, description: &str) -> rusqlite::Result<Option<String>> {
    let key = normalize(description);
    if key.is_empty() {
        return Ok(None);
    }
    conn.query_row(
        "SELECT category FROM category_rules \
         WHERE instr(?1, pattern) > 0 \
         ORDER BY (substr(?1, 1, length(pattern)) = pattern) DESC, length(pattern) DESC \
         LIMIT 1",
        params![key],
        |r| r.get(0),
    )
    .optional()
}

/// Insert or update a rule. A user rule always wins; a learned rule never
/// overwrites a user rule.
pub fn learn(conn: &Connection, pattern: &str, category: &str, source: &str) -> rusqlite::Result<()> {
    let key = normalize(pattern);
    if key.is_empty() || category.trim().is_empty() {
        return Ok(());
    }
    conn.execute(
        "INSERT INTO category_rules (pattern, category, source) VALUES (?1, ?2, ?3) \
         ON CONFLICT(pattern) DO UPDATE SET category = excluded.category, source = excluded.source \
         WHERE category_rules.source = 'learned' OR excluded.source = 'user'",
        params![key, category.trim(), source],
    )?;
    Ok(())
}

pub fn delete(conn: &Connection, pattern: &str) -> rusqlite::Result<bool> {
    let n = conn.execute("DELETE FROM category_rules WHERE pattern = ?1", params![normalize(pattern)])?;
    Ok(n > 0)
}

pub fn list(conn: &Connection) -> rusqlite::Result<Vec<Rule>> {
    let mut stmt = conn.prepare("SELECT pattern, category, source FROM category_rules ORDER BY source, pattern")?;
    let rows = stmt.query_map([], |r| {
        Ok(Rule { pattern: r.get(0)?, category: r.get(1)?, source: r.get(2)? })
    })?;
    rows.collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::run_migrations(&conn).unwrap();
        conn
    }

    #[test]
    fn normalize_collapses_case_and_whitespace() {
        assert_eq!(normalize("  GitHub,   Inc. "), "github, inc.");
        assert_eq!(normalize(""), "");
    }

    #[test]
    fn lookup_prefers_prefix_then_longest() {
        let conn = open();
        learn(&conn, "github", "Software", SOURCE_USER).unwrap();
        learn(&conn, "sq *", "Shopping", SOURCE_USER).unwrap();
        learn(&conn, "anthropic", "AI APIs", SOURCE_USER).unwrap();
        assert_eq!(lookup(&conn, "GITHUB INC").unwrap().as_deref(), Some("Software"));
        // Both rules match; "sq *" is the prefix so it wins over the substring hit.
        assert_eq!(lookup(&conn, "SQ *GITHUB SPONSORS").unwrap().as_deref(), Some("Shopping"));
        assert_eq!(lookup(&conn, "WHOLE FOODS").unwrap(), None);
        assert_eq!(lookup(&conn, "   ").unwrap(), None);
    }

    #[test]
    fn learned_rule_never_overrides_user_rule() {
        let conn = open();
        learn(&conn, "github", "Software", SOURCE_USER).unwrap();
        learn(&conn, "github", "Other", SOURCE_LEARNED).unwrap();
        assert_eq!(lookup(&conn, "github").unwrap().as_deref(), Some("Software"));

        learn(&conn, "openai", "Other", SOURCE_LEARNED).unwrap();
        learn(&conn, "openai", "AI APIs", SOURCE_LEARNED).unwrap();
        assert_eq!(lookup(&conn, "openai").unwrap().as_deref(), Some("AI APIs"));
        learn(&conn, "openai", "Cloud", SOURCE_USER).unwrap();
        assert_eq!(lookup(&conn, "openai").unwrap().as_deref(), Some("Cloud"));

        let rules = list(&conn).unwrap();
        assert_eq!(rules.len(), 2);
        assert!(delete(&conn, "OpenAI").unwrap());
        assert!(!delete(&conn, "nope").unwrap());
    }
}
