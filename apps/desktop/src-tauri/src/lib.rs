use extractor::{AnthropicClient, ExtractionResult};
use tauri_plugin_sql::{Migration, MigrationKind};

#[tauri::command]
async fn import_statement(path: String, label: String) -> Result<ExtractionResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let client = AnthropicClient::from_env().map_err(|e| e.to_string())?;
        let raw_text = extractor::extract_text(std::path::Path::new(&path))
            .map_err(|e| e.to_string())?;
        extractor::parse_line_items(&raw_text, &label, &client).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let migrations = vec![Migration {
        version: 1,
        description: "initial schema",
        sql: db::MIGRATION_001,
        kind: MigrationKind::Up,
    }];

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_sql::Builder::new()
                .add_migrations("sqlite:wealth.db", migrations)
                .build(),
        )
        .invoke_handler(tauri::generate_handler![import_statement])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
