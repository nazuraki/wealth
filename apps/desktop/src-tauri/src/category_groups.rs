use anyhow::Result;
use rusqlite::{params, Connection};
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use tauri::{AppHandle, State};

#[derive(Debug, Serialize)]
pub struct CategoryGroup {
    pub id: i64,
    pub name: String,
    pub color: Option<String>,
    pub sort_order: i64,
    pub categories: Vec<String>,
}

fn open_conn(db_path: &Path) -> Result<Connection> {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(db_path)?;
    db::run_migrations(&conn)?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    Ok(conn)
}

fn query_groups(conn: &Connection) -> Result<Vec<CategoryGroup>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, color, sort_order FROM category_groups ORDER BY sort_order, name COLLATE NOCASE",
    )?;
    let mut groups: Vec<CategoryGroup> = stmt
        .query_map([], |r| {
            Ok(CategoryGroup {
                id: r.get(0)?,
                name: r.get(1)?,
                color: r.get(2)?,
                sort_order: r.get(3)?,
                categories: Vec::new(),
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    let mut members: HashMap<i64, Vec<String>> = HashMap::new();
    let mut mstmt = conn.prepare(
        "SELECT group_id, category FROM category_group_members ORDER BY category COLLATE NOCASE",
    )?;
    for row in mstmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))? {
        if let Ok((gid, cat)) = row {
            members.entry(gid).or_default().push(cat);
        }
    }
    for g in groups.iter_mut() {
        if let Some(cats) = members.remove(&g.id) {
            g.categories = cats;
        }
    }
    Ok(groups)
}

fn do_create(conn: &Connection, name: &str, color: Option<&str>) -> Result<i64> {
    conn.execute(
        "INSERT INTO category_groups (name, color) VALUES (?1, ?2)",
        params![name, color],
    )?;
    Ok(conn.last_insert_rowid())
}

fn do_update(conn: &Connection, id: i64, name: &str, color: Option<&str>) -> Result<()> {
    conn.execute(
        "UPDATE category_groups SET name = ?1, color = ?2 WHERE id = ?3",
        params![name, color, id],
    )?;
    Ok(())
}

fn do_delete(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM category_groups WHERE id = ?1", params![id])?;
    Ok(())
}

fn do_set_members(conn: &mut Connection, group_id: i64, categories: &[String]) -> Result<()> {
    let tx = conn.transaction()?;
    // Remove these categories from any other group (PK enforces uniqueness).
    for cat in categories {
        tx.execute(
            "DELETE FROM category_group_members WHERE category = ?1",
            params![cat],
        )?;
    }
    // Remove existing members of this group that aren't in the new list.
    {
        let current: Vec<String> = tx
            .prepare("SELECT category FROM category_group_members WHERE group_id = ?1")?
            .query_map(params![group_id], |r| r.get(0))?
            .filter_map(|r| r.ok())
            .collect();
        for cat in current {
            if !categories.iter().any(|c| c == &cat) {
                tx.execute(
                    "DELETE FROM category_group_members WHERE category = ?1",
                    params![cat],
                )?;
            }
        }
    }
    for cat in categories {
        tx.execute(
            "INSERT OR REPLACE INTO category_group_members (category, group_id) VALUES (?1, ?2)",
            params![cat, group_id],
        )?;
    }
    tx.commit()?;
    Ok(())
}

#[tauri::command]
pub async fn list_category_groups(
    _app: AppHandle,
    db: State<'_, crate::DbPath>,
) -> Result<Vec<CategoryGroup>, String> {
    let path = db.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let conn = open_conn(&path)?;
        query_groups(&conn)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn create_category_group(
    _app: AppHandle,
    db: State<'_, crate::DbPath>,
    name: String,
    color: Option<String>,
) -> Result<i64, String> {
    let path = db.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let conn = open_conn(&path)?;
        do_create(&conn, &name, color.as_deref())
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_category_group(
    _app: AppHandle,
    db: State<'_, crate::DbPath>,
    id: i64,
    name: String,
    color: Option<String>,
) -> Result<(), String> {
    let path = db.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let conn = open_conn(&path)?;
        do_update(&conn, id, &name, color.as_deref())
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_category_group(
    _app: AppHandle,
    db: State<'_, crate::DbPath>,
    id: i64,
) -> Result<(), String> {
    let path = db.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let conn = open_conn(&path)?;
        do_delete(&conn, id)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_category_group_members(
    _app: AppHandle,
    db: State<'_, crate::DbPath>,
    group_id: i64,
    categories: Vec<String>,
) -> Result<(), String> {
    let path = db.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut conn = open_conn(&path)?;
        do_set_members(&mut conn, group_id, &categories)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        db::run_migrations(&conn).unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        conn
    }

    #[test]
    fn create_and_list_groups() {
        let conn = open_test_db();
        let food = do_create(&conn, "Food", Some("#ff0000")).unwrap();
        do_create(&conn, "Transport", None).unwrap();
        let groups = query_groups(&conn).unwrap();
        assert_eq!(groups.len(), 2);
        let f = groups.iter().find(|g| g.id == food).unwrap();
        assert_eq!(f.name, "Food");
        assert_eq!(f.color.as_deref(), Some("#ff0000"));
    }

    #[test]
    fn set_members_reassigns_category() {
        let mut conn = open_test_db();
        let food = do_create(&conn, "Food", None).unwrap();
        let dining = do_create(&conn, "Dining-out", None).unwrap();
        do_set_members(&mut conn, food, &["Dining".into(), "Groceries".into()]).unwrap();
        do_set_members(&mut conn, dining, &["Dining".into()]).unwrap();

        let groups = query_groups(&conn).unwrap();
        let f = groups.iter().find(|g| g.id == food).unwrap();
        let d = groups.iter().find(|g| g.id == dining).unwrap();
        assert_eq!(f.categories, vec!["Groceries"]);
        assert_eq!(d.categories, vec!["Dining"]);
    }

    #[test]
    fn set_members_removes_omitted() {
        let mut conn = open_test_db();
        let food = do_create(&conn, "Food", None).unwrap();
        do_set_members(&mut conn, food, &["A".into(), "B".into(), "C".into()]).unwrap();
        do_set_members(&mut conn, food, &["A".into()]).unwrap();
        let groups = query_groups(&conn).unwrap();
        assert_eq!(groups[0].categories, vec!["A"]);
    }

    #[test]
    fn delete_cascades_members() {
        let mut conn = open_test_db();
        let food = do_create(&conn, "Food", None).unwrap();
        do_set_members(&mut conn, food, &["Dining".into()]).unwrap();
        do_delete(&conn, food).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM category_group_members", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }
}
