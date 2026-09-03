use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppSettings {
    pub api_key: Option<String>,
    pub endpoint_url: Option<String>,
    pub simplefin_access_url: Option<String>,
    pub window_x: Option<i32>,
    pub window_y: Option<i32>,
    pub window_width: Option<u32>,
    pub window_height: Option<u32>,
}

fn settings_path(data_dir: &Path) -> std::path::PathBuf {
    data_dir.join("settings.json")
}

pub fn load(data_dir: &Path) -> AppSettings {
    let path = settings_path(data_dir);
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(data_dir: &Path, settings: &AppSettings) -> Result<()> {
    std::fs::create_dir_all(data_dir)?;
    let json = serde_json::to_string_pretty(settings)?;
    std::fs::write(settings_path(data_dir), json)?;
    Ok(())
}

#[tauri::command]
pub fn get_settings(app: AppHandle) -> AppSettings {
    let data_dir = app.path().app_data_dir().unwrap_or_default();
    load(&data_dir)
}

/// Persist the user-editable fields, keeping window geometry the UI never sends.
#[tauri::command]
pub fn save_settings(app: AppHandle, settings: AppSettings) -> Result<(), String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let mut current = load(&data_dir);
    current.api_key = settings.api_key;
    current.endpoint_url = settings.endpoint_url;
    current.simplefin_access_url = settings.simplefin_access_url;
    save(&data_dir, &current).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn load_returns_default_when_no_file() {
        let dir = TempDir::new().unwrap();
        let settings = load(dir.path());
        assert!(settings.api_key.is_none());
        assert!(settings.endpoint_url.is_none());
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = TempDir::new().unwrap();
        let original = AppSettings {
            api_key: Some("sk-test-key".into()),
            endpoint_url: Some("https://example.com/v1/messages".into()),
            ..Default::default()
        };
        save(dir.path(), &original).unwrap();
        let loaded = load(dir.path());
        assert_eq!(loaded.api_key.as_deref(), Some("sk-test-key"));
        assert_eq!(
            loaded.endpoint_url.as_deref(),
            Some("https://example.com/v1/messages")
        );
    }

    #[test]
    fn save_partial_and_load() {
        let dir = TempDir::new().unwrap();
        let s = AppSettings { api_key: Some("key-only".into()), ..Default::default() };
        save(dir.path(), &s).unwrap();
        let loaded = load(dir.path());
        assert_eq!(loaded.api_key.as_deref(), Some("key-only"));
        assert!(loaded.endpoint_url.is_none());
        assert!(loaded.simplefin_access_url.is_none());
    }

    #[test]
    fn simplefin_url_round_trips_and_old_files_still_load() {
        let dir = TempDir::new().unwrap();
        std::fs::write(settings_path(dir.path()), r#"{"api_key":"k","endpoint_url":null}"#).unwrap();
        let loaded = load(dir.path());
        assert_eq!(loaded.api_key.as_deref(), Some("k"));
        assert!(loaded.simplefin_access_url.is_none());

        let s = AppSettings { simplefin_access_url: Some("https://u:p@bridge.example/simplefin".into()), ..Default::default() };
        save(dir.path(), &s).unwrap();
        assert_eq!(load(dir.path()).simplefin_access_url.as_deref(), Some("https://u:p@bridge.example/simplefin"));
    }
}
