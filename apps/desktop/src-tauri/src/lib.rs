mod chart;
mod dashboard;
mod importer;
mod settings;

use tauri_plugin_sql::{Migration, MigrationKind};

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
            chart::get_chart_data,
            settings::get_settings,
            settings::save_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
