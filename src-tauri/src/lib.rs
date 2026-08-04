pub mod commands;
pub mod config;
pub mod discord;
pub mod distribution;
pub mod dl;
pub mod java;
pub mod loader;
pub mod error;
pub mod microsoft;
pub mod mods;
pub mod mojang;
pub mod news;
pub mod paths;
pub mod server_status;
pub mod telemetry;
pub mod process_builder;

use commands::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Telemetry settings live in config.json, which the app has not loaded
    // yet, so read just that file here to decide whether to install the OTLP
    // layer. Any failure falls back to plain logging rather than blocking
    // startup — diagnostics must never stop the launcher running.
    let telemetry = std::fs::read_to_string(paths::config_path())
        .ok()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .and_then(|v| v.get("settings")?.get("telemetry").cloned())
        .and_then(|v| serde_json::from_value::<telemetry::TelemetryConfig>(v).ok())
        .unwrap_or_default();

    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "info".into());

    match telemetry::build_layer(&telemetry) {
        Ok(Some(otel)) => {
            use tracing_subscriber::layer::SubscriberExt;
            use tracing_subscriber::util::SubscriberInitExt;
            tracing_subscriber::registry()
                .with(filter)
                .with(tracing_subscriber::fmt::layer())
                .with(otel)
                .init();
            tracing::info!(endpoint = %telemetry.endpoint, "OpenTelemetry enabled");
        }
        Ok(None) => {
            tracing_subscriber::fmt().with_env_filter(filter).init();
        }
        Err(err) => {
            tracing_subscriber::fmt().with_env_filter(filter).init();
            tracing::error!(%err, "Telemetry setup failed; continuing without it");
        }
    }

    tracing::info!("Lunar Launcher starting");

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
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
            commands::is_game_running,
            commands::get_game_log,
            commands::clear_game_log,
            commands::export_diagnostics,
            commands::get_telemetry,
            commands::save_telemetry,
        ])
        .setup(|app| {
            check_for_updates(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running lunarlauncher");
}

/// Check for an update once at startup, in the background.
///
/// Registering `tauri_plugin_updater` is not enough on its own — nothing called
/// into it, so no shipped build could ever be fixed remotely and every bug in a
/// release was permanent for that install.
///
/// This is spawned rather than awaited: a slow or unreachable update endpoint
/// must not delay the window appearing, and a failure here is never fatal. The
/// launcher works fine without an update; it does not work at all if it cannot
/// start.
///
/// Gated on the updater being configured. `builder().build()` fails when the
/// public key is empty, in which case this logs once and stops rather than
/// retrying — an unverifiable update channel is worse than none, since it
/// would mean downloading and executing code nobody signed.
///
/// The key is now populated, so the remaining requirement is a reachable
/// endpoint publishing a signed `latest.json`. Until one exists the check
/// fails per launch and is logged at debug, which is deliberately quiet: a
/// user cannot act on it and it must never block startup.
fn check_for_updates(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        use tauri_plugin_updater::UpdaterExt;

        let updater = match app.updater_builder().build() {
            Ok(u) => u,
            Err(err) => {
                tracing::info!(%err, "Updater not configured; skipping the update check.");
                return;
            }
        };

        match updater.check().await {
            Ok(Some(update)) => {
                // Reported rather than installed. Silently replacing the binary
                // under a user who is mid-session is a decision for the UI to
                // offer, not for startup to take.
                tracing::info!(
                    version = %update.version,
                    current = %update.current_version,
                    "An update is available."
                );
            }
            Ok(None) => tracing::info!("Launcher is up to date."),
            Err(err) => tracing::warn!(%err, "Update check failed; continuing."),
        }
    });
}
