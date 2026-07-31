pub mod commands;
pub mod config;
pub mod distribution;
pub mod error;
pub mod paths;

use commands::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    tracing::info!("Lunar Launcher starting");

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            commands::bootstrap,
            commands::get_distribution,
            commands::get_selected_server,
            commands::set_selected_server,
            commands::get_effective_java_options,
            commands::get_accounts,
            commands::add_lunar_account,
            commands::remove_account,
            commands::select_account,
            commands::get_memory_info,
            commands::get_config,
            commands::save_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running lunarlauncher");
}
