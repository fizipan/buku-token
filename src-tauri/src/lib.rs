pub mod aggregate;
pub mod notify;
pub mod commands;
pub mod engine;
pub mod fmt_id;
pub mod i18n;
pub mod ingest;
pub mod model;
pub mod pricing;
pub mod project;
pub mod quota;
pub mod rtk;
pub mod screen;
pub mod settings;
pub mod share;
pub mod store;
pub mod tray;
#[cfg(test)]
mod ts_fixtures;

use tauri::ActivationPolicy;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            app.set_activation_policy(ActivationPolicy::Accessory);
            commands::init(app)?;

            // Before the tray: its first redraw, and any poll, already read the screen state.
            screen::setup(app);
            tray::setup(app)?;
            notify::setup(app);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_overview,
            commands::get_sessions,
            commands::get_models,
            commands::get_projects,
            commands::get_history,
            commands::share_preview,
            commands::share_copy,
            commands::share_save,
            commands::refresh,
            commands::get_settings,
            commands::save_settings,
            commands::update_pricing,
            commands::resize_popover,
            commands::open_settings,
            commands::scan_status,
            commands::cursor_session_status,
            commands::cursor_last_reason,
            commands::connect_cursor,
            commands::disconnect_cursor,
            commands::openrouter_session_status,
            commands::connect_openrouter,
            commands::disconnect_openrouter,
            commands::send_test_notification,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    #[test]
    fn app_identifier_is_stable() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(conf["identifier"], "local.toolslab.bukutoken");
        assert_eq!(conf["app"]["macOSPrivateApi"], true);
    }

    #[test]
    fn settings_opened_event_name_matches_the_page() {
        assert_eq!(crate::commands::SETTINGS_OPENED, "settings-opened");
    }

    #[test]
    fn settings_window_is_declared_hidden_and_allowed_to_use_commands() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let win = conf["app"]["windows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|w| w["label"] == "settings")
            .expect("settings window declared, so open_settings can show it");
        assert_eq!(win["title"], "Pengaturan Buku Token");
        assert_eq!((win["width"].as_u64(), win["height"].as_u64()), (Some(520), Some(600)));
        assert_eq!(win["visible"], false);
        assert_eq!(win["decorations"], true);
        assert_eq!(win["resizable"], false);
        assert_eq!(win["url"], "settings.html");

        let cap: serde_json::Value =
            serde_json::from_str(include_str!("../capabilities/default.json")).unwrap();
        let windows: Vec<_> = cap["windows"].as_array().unwrap().iter().collect();
        assert!(windows.contains(&&serde_json::json!("settings")));
        let perms = cap["permissions"].to_string();
        for p in ["autostart:allow-enable", "autostart:allow-disable", "autostart:allow-is-enabled"] {
            assert!(perms.contains(p), "{p}");
        }
    }
}
