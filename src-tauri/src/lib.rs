pub mod commands;
pub mod config;
pub mod discord;
pub mod distribution;
/// `.env` parsing and application.
///
/// Shared verbatim with `build.rs` via `include!`, so that a variable behaves
/// the same whether it is baked in at build time or read at startup.
pub mod env_file;
pub mod dl;
pub mod java;
pub mod loader;
pub mod error;
pub mod microsoft;
pub mod modules;
pub mod mods;
pub mod mojang;
pub mod news;
pub mod paths;
pub mod server_status;
pub mod telemetry;
pub mod process_builder;

use commands::AppState;

/// A daily-rotated log file under the launcher directory, capped so it cannot
/// grow without bound on a machine nobody ever cleans up.
///
/// Returns the guard alongside the layer: it flushes the non-blocking writer on
/// drop, so whatever holds the layer must hold this for exactly as long.
///
/// Generic over the subscriber because the layer is added to a stack that
/// already has the filter on it — a box typed against bare `Registry` does not
/// match `Layered<EnvFilter, Registry>` and will not compose.
///
/// Takes the directory rather than reading `paths::log_directory()` itself, so
/// a test can exercise it without writing into the real launcher directory.
#[allow(clippy::type_complexity)]
fn file_log_layer<S>(
    dir: &std::path::Path,
) -> std::io::Result<(
    Box<dyn tracing_subscriber::Layer<S> + Send + Sync>,
    tracing_appender::non_blocking::WorkerGuard,
)>
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    use tracing_subscriber::Layer;

    std::fs::create_dir_all(dir)?;

    let appender = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("launcher")
        .filename_suffix("log")
        .max_log_files(7)
        .build(dir)
        .map_err(std::io::Error::other)?;

    let (writer, guard) = tracing_appender::non_blocking(appender);
    // No ANSI: colour escapes are noise in a file somebody opens in Notepad to
    // paste into a bug report.
    let layer = tracing_subscriber::fmt::layer()
        .with_writer(writer)
        .with_ansi(false)
        .boxed();

    Ok((layer, guard))
}

/// Apply a `.env` before anything reads the environment.
///
/// Debug builds pick up `.env.local` then `.env` from the working directory,
/// which is what makes `npm run app:dev` usable without exporting
/// `LUNAR_DISTRO_URL` by hand every time.
///
/// Release builds do **not**, unless `LUNAR_ENV_FILE` names a file
/// explicitly. A shipped launcher that silently honoured a `.env` sitting
/// next to it could be repointed at another distribution index — that is a
/// download-and-execute path, so it takes a deliberate act, not a dropped
/// file.
fn load_env_file() {
    let explicit = std::env::var_os("LUNAR_ENV_FILE").map(std::path::PathBuf::from);

    let candidates: Vec<std::path::PathBuf> = match explicit {
        Some(path) => vec![path],
        None if cfg!(debug_assertions) => {
            vec![".env.local".into(), ".env".into()]
        }
        None => return,
    };

    for path in candidates {
        let applied = env_file::apply(&path);
        if !applied.is_empty() {
            // Printed rather than traced: the subscriber is not installed yet,
            // and knowing which file moved a setting is exactly what saves an
            // hour when a build points somewhere unexpected.
            println!("[env] {} applied {}", path.display(), applied.join(", "));
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    load_env_file();

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

    // Log to a file as well as stdout, because on Windows stdout does not
    // exist: release builds set `windows_subsystem = "windows"` (main.rs) so
    // the process has no console, and every line written here goes nowhere.
    // That made Windows-only failures undiagnosable — a refused sign-in
    // redirect emitted both the port it was waiting on and a warning that the
    // IPv6 bind had failed, and neither was recoverable after the fact.
    //
    // The guard must outlive the app: dropping it flushes and stops the
    // writer thread, so binding it to `_` (rather than a name) would discard
    // every buffered line at the end of this statement.
    let (file_layer, _log_guard) = match file_log_layer(&paths::log_directory()) {
        Ok((layer, guard)) => (Some(layer), Some(guard)),
        Err(err) => {
            eprintln!("File logging unavailable, continuing with stdout only: {err}");
            (None, None)
        }
    };

    // `build_layer` is fallible and its error is worth seeing, but nothing can
    // be logged until the subscriber is installed — so the outcome is held and
    // reported afterwards rather than swallowed.
    let (otel_layer, otel_err) = match telemetry::build_layer(&telemetry) {
        Ok(layer) => (layer, None),
        Err(err) => (None, Some(err)),
    };

    {
        use tracing_subscriber::layer::SubscriberExt;
        use tracing_subscriber::util::SubscriberInitExt;
        tracing_subscriber::registry()
            .with(filter)
            .with(tracing_subscriber::fmt::layer())
            .with(file_layer)
            .with(otel_layer)
            .init();
    }

    if let Some(err) = otel_err {
        tracing::error!(%err, "Telemetry setup failed; continuing without it");
    } else if !telemetry.endpoint.is_empty() {
        tracing::info!(endpoint = %telemetry.endpoint, "OpenTelemetry enabled");
    }

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        log_dir = %paths::log_directory().display(),
        "Lunar Launcher starting"
    );

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
/// The key is populated, but `active` is back to false until a real endpoint
/// exists. Pointing an enabled updater at a placeholder host bought nothing —
/// every launch made a request that could only fail — while adding startup
/// work on a path that has to be reliable. Flip it on together with the
/// endpoint, not before.
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole point of the file layer is that it works where stdout does
    /// not, so "it compiles" is not evidence. This writes through a real
    /// subscriber and reads the bytes back off disk.
    #[test]
    fn a_logged_line_reaches_a_file_on_disk() {
        use tracing_subscriber::layer::SubscriberExt;

        let dir = std::env::temp_dir().join("lunar-log-layer-test");
        let _ = std::fs::remove_dir_all(&dir);

        let (layer, guard) = file_log_layer(&dir).expect("layer");
        let subscriber = tracing_subscriber::registry().with(layer);

        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(port = 51234, "waiting for the browser");
        });

        // The writer is non-blocking, so the line is still in the channel until
        // the guard flushes it. Dropping it here is the flush.
        drop(guard);

        let written = std::fs::read_dir(&dir)
            .expect("log dir")
            .filter_map(|e| e.ok())
            .map(|e| std::fs::read_to_string(e.path()).unwrap_or_default())
            .collect::<String>();

        assert!(
            written.contains("waiting for the browser") && written.contains("51234"),
            "log file did not contain the line: {written:?}"
        );
        // A file somebody pastes into a bug report must not be full of escapes.
        assert!(!written.contains('\u{1b}'), "ANSI escapes leaked into the file");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
