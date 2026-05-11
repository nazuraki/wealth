mod accounts;
mod chart;
mod dashboard;
mod importer;
mod settings;
mod transactions;

use std::path::PathBuf;
use tauri::Manager;
use tauri_plugin_sql::{Migration, MigrationKind};

/// Resolved path to the SQLite database file, stored as Tauri app state.
/// Populated during setup from `--db <path>`, `WEALTH_DB` env var, or the default app-data directory.
pub struct DbPath(pub PathBuf);

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let migrations = vec![
        Migration {
            version: 1,
            description: "initial schema",
            sql: db::MIGRATION_001,
            kind: MigrationKind::Up,
        },
        Migration {
            version: 2,
            description: "account type",
            sql: db::MIGRATION_002,
            kind: MigrationKind::Up,
        },
        Migration {
            version: 5,
            description: "account display name and color",
            sql: db::MIGRATION_005,
            kind: MigrationKind::Up,
        },
    ];

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_sql::Builder::new()
                .add_migrations("sqlite:wealth.db", migrations)
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            importer::import_statement,
            dashboard::get_dashboard,
            chart::get_available_periods,
            chart::get_chart_data,
            accounts::get_accounts,
            accounts::update_account,
            settings::get_settings,
            settings::save_settings,
            transactions::get_transactions,
        ])
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;

            let db_path = std::env::args()
                .position(|a| a == "--db")
                .and_then(|i| std::env::args().nth(i + 1))
                .map(PathBuf::from)
                .or_else(|| std::env::var("WEALTH_DB").ok().map(PathBuf::from))
                .unwrap_or_else(|| data_dir.join("wealth.db"));
            app.manage(DbPath(db_path));

            let s = settings::load(&data_dir);

            if let Some(win) = app.get_webview_window("main") {
                if let (Some(x), Some(y)) = (s.window_x, s.window_y) {
                    let _ = win.set_position(tauri::PhysicalPosition::new(x, y));
                }
                if let (Some(w), Some(h)) = (s.window_width, s.window_height) {
                    let _ = win.set_size(tauri::PhysicalSize::new(w, h));
                }

                let win2 = win.clone();
                win.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { .. } = event {
                        if let (Ok(pos), Ok(size)) = (win2.outer_position(), win2.outer_size()) {
                            let mut current = settings::load(&data_dir);
                            current.window_x = Some(pos.x);
                            current.window_y = Some(pos.y);
                            current.window_width = Some(size.width);
                            current.window_height = Some(size.height);
                            let _ = settings::save(&data_dir, &current);
                        }
                    }
                });
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
