pub mod commands;
pub mod config;
pub mod discord;
pub mod distribution;
pub mod dl;
pub mod java;
pub mod error;
pub mod microsoft;
pub mod mods;
pub mod mojang;
pub mod news;
pub mod paths;
pub mod server_status;
pub mod process_builder;

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
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(AppState::new())
        .manage(discord::DiscordState::default())
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
            commands::scan_java,
            commands::launch_game,
            commands::microsoft_login,
            commands::microsoft_login_browser,
            commands::cancel_microsoft_login,
            commands::mojang_login,
            commands::validate_selected_account,
            commands::microsoft_logout,
            commands::get_java_config,
            commands::save_java_config,
            commands::discord_connect,
            commands::discord_set_details,
            commands::discord_disconnect,
            commands::get_distribution_mods,
            commands::set_distribution_mod_enabled,
            commands::get_dropin_mods,
            commands::toggle_dropin_mod,
            commands::delete_dropin_mod,
            commands::add_dropin_mods,
            commands::open_mods_folder,
            commands::get_shaderpacks,
            commands::set_shaderpack,
            commands::get_server_status,
            commands::get_news,
        ])
        .run(tauri::generate_context!())
        .expect("error while running lunarlauncher");
}
