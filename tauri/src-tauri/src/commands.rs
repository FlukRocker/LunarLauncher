//! The Rust -> JS command surface.
//!
//! Everything the renderer used to do with `require('fs')`, `require('os')`
//! and helios-core now goes through these. Keep them coarse-grained: each
//! command should be one meaningful operation, not a getter, because every
//! call crosses an IPC boundary and is async on the JS side.

use serde::Serialize;
use tauri::State;

use crate::config::{self, Account, ConfigManager};
use crate::distribution::{Distribution, DistributionApi, EffectiveJavaOptions, Server};
use crate::error::{Error, Result};

pub struct AppState {
    pub config: ConfigManager,
    pub distro: DistributionApi,
    /// Cached distribution index for the session, populated by `load_distribution`.
    pub distribution: std::sync::Mutex<Option<Distribution>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            config: ConfigManager::new(),
            distro: DistributionApi::new(),
            distribution: std::sync::Mutex::new(None),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

/// What the frontend needs to render the initial screen.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bootstrap {
    pub first_launch: bool,
    pub selected_account: Option<Account>,
    pub accounts: Vec<Account>,
    pub distribution_loaded: bool,
}

/// Startup sequence, replacing `preloader.js`.
///
/// Loads config, fetches the distribution (falling back to the disk cache),
/// resolves the selected server the way the preloader did, and ensures a java
/// config exists for it.
#[tauri::command]
pub async fn bootstrap(state: State<'_, AppState>) -> Result<Bootstrap> {
    state.config.load()?;

    let distribution = match state.distro.get().await {
        Ok(d) => Some(d),
        Err(err) => {
            tracing::error!(%err, "Unable to load distribution from remote or local disk.");
            None
        }
    };

    if let Some(distro) = &distribution {
        // Resolve the selected server if unset or no longer present, matching
        // `onDistroLoad` in preloader.js.
        let current = state.config.with(|c| c.selected_server.clone())?;
        let needs_default = match &current {
            Some(id) => distro.server_by_id(id).is_none(),
            None => true,
        };
        if needs_default {
            if let Some(main) = distro.main_server() {
                tracing::info!("Determining default selected server..");
                state
                    .config
                    .with_mut(|c| c.selected_server = Some(main.id.clone()))?;
                state.config.save()?;
            }
        }

        // Ensure java config for the selected server exists.
        if let Some(id) = state.config.with(|c| c.selected_server.clone())? {
            if let Some(server) = distro.server_by_id(&id) {
                let eff = server.effective_java_options();
                state
                    .config
                    .ensure_java_config(&id, eff.suggested_major, server.ram_hints())?;
                state.config.save()?;
            }
        }
    }

    let distribution_loaded = distribution.is_some();
    *state.distribution.lock().unwrap() = distribution;

    let (selected_account, accounts) = state.config.with(|c| {
        let selected = c
            .selected_account
            .as_ref()
            .and_then(|uuid| c.authentication_database.get(uuid))
            .cloned();
        let all = c.authentication_database.values().cloned().collect();
        (selected, all)
    })?;

    Ok(Bootstrap {
        first_launch: state.config.is_first_launch(),
        selected_account,
        accounts,
        distribution_loaded,
    })
}

/// The full distribution index for the session.
#[tauri::command]
pub fn get_distribution(state: State<'_, AppState>) -> Result<Distribution> {
    state
        .distribution
        .lock()
        .unwrap()
        .clone()
        .ok_or(Error::NoDistribution)
}

/// The currently selected server, if any.
#[tauri::command]
pub fn get_selected_server(state: State<'_, AppState>) -> Result<Option<Server>> {
    let id = match state.config.with(|c| c.selected_server.clone())? {
        Some(id) => id,
        None => return Ok(None),
    };
    Ok(state
        .distribution
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|d| d.server_by_id(&id).cloned()))
}

/// Switch servers, ensuring a java config exists for the new one.
#[tauri::command]
pub fn set_selected_server(state: State<'_, AppState>, server_id: String) -> Result<()> {
    let guard = state.distribution.lock().unwrap();
    let distro = guard.as_ref().ok_or(Error::NoDistribution)?;
    let server = distro
        .server_by_id(&server_id)
        .ok_or_else(|| Error::UnknownServer(server_id.clone()))?;
    let eff = server.effective_java_options();
    let ram = server.ram_hints();
    drop(guard);

    state
        .config
        .with_mut(|c| c.selected_server = Some(server_id.clone()))?;
    state
        .config
        .ensure_java_config(&server_id, eff.suggested_major, ram)?;
    state.config.save()
}

#[tauri::command]
pub fn get_effective_java_options(
    state: State<'_, AppState>,
    server_id: String,
) -> Result<EffectiveJavaOptions> {
    let guard = state.distribution.lock().unwrap();
    let distro = guard.as_ref().ok_or(Error::NoDistribution)?;
    distro
        .server_by_id(&server_id)
        .map(|s| s.effective_java_options())
        .ok_or(Error::UnknownServer(server_id))
}

// ---------------------------------------------------------------------------
// Accounts
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_accounts(state: State<'_, AppState>) -> Result<Vec<Account>> {
    state.config.with(|c| c.authentication_database.values().cloned().collect())
}

/// Add an offline account. Port of `AuthManager.addLunarAccount`.
#[tauri::command]
pub fn add_lunar_account(state: State<'_, AppState>, username: String) -> Result<Account> {
    let username = username.trim().to_string();
    if username.is_empty() {
        return Err(Error::Other("Username must not be empty.".into()));
    }
    state.config.add_lunar_account(&username)
}

#[tauri::command]
pub fn remove_account(state: State<'_, AppState>, uuid: String) -> Result<bool> {
    state.config.remove_account(&uuid)
}

#[tauri::command]
pub fn select_account(state: State<'_, AppState>, uuid: String) -> Result<()> {
    state.config.with_mut(|c| {
        if c.authentication_database.contains_key(&uuid) {
            c.selected_account = Some(uuid.clone());
        }
    })?;
    state.config.save()
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryInfo {
    pub absolute_min: f64,
    pub absolute_max: f64,
}

#[tauri::command]
pub fn get_memory_info(state: State<'_, AppState>, server_id: Option<String>) -> Result<MemoryInfo> {
    let hints = server_id.and_then(|id| {
        state
            .distribution
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|d| d.server_by_id(&id).and_then(|s| s.ram_hints()))
    });
    Ok(MemoryInfo {
        absolute_min: config::absolute_min_ram(hints),
        absolute_max: config::absolute_max_ram(),
    })
}

#[tauri::command]
pub fn get_config(state: State<'_, AppState>) -> Result<config::Config> {
    state.config.with(|c| c.clone())
}

/// Replace the whole settings block. The frontend edits a local copy and
/// commits it, matching how the Electron settings view batched changes.
#[tauri::command]
pub fn save_settings(state: State<'_, AppState>, settings: config::Settings) -> Result<()> {
    state.config.with_mut(|c| c.settings = settings)?;
    state.config.save()
}

// ---------------------------------------------------------------------------
// Java
// ---------------------------------------------------------------------------

/// Discover JVMs and report which satisfy the given server's requirement.
#[tauri::command]
pub async fn scan_java(
    state: State<'_, AppState>,
    server_id: String,
) -> Result<Vec<crate::java::JvmDetails>> {
    let supported = {
        let guard = state.distribution.lock().unwrap();
        let distro = guard.as_ref().ok_or(Error::NoDistribution)?;
        distro
            .server_by_id(&server_id)
            .map(|s| s.effective_java_options().supported)
            .ok_or_else(|| Error::UnknownServer(server_id.clone()))?
    };
    let data_dir = state.config.with(|c| c.settings.launcher.data_directory.clone())?;
    Ok(crate::java::discover_jvms(&data_dir)
        .await
        .into_iter()
        .filter(|j| crate::java::version_satisfies(j.version, &supported))
        .collect())
}

// ---------------------------------------------------------------------------
// Launch
// ---------------------------------------------------------------------------

/// Progress payload emitted on the `launch://progress` event.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchProgress {
    pub stage: String,
    pub detail: String,
    pub percent: f64,
}

fn emit_progress(app: &tauri::AppHandle, stage: &str, detail: &str, percent: f64) {
    use tauri::Emitter;
    let _ = app.emit(
        "launch://progress",
        LaunchProgress {
            stage: stage.to_string(),
            detail: detail.to_string(),
            percent,
        },
    );
}

/// Validate, download and launch the selected server.
///
/// This is the Rust equivalent of `dlAsync()` in landing.js. Progress is
/// pushed to the frontend as events rather than returned, so the UI can show
/// a live bar during what is often a multi-gigabyte download.
///
/// Vanilla only for now — a server whose distribution declares Forge/Fabric
/// modules will download correctly but launch without the mod loader, so this
/// refuses rather than starting a broken game.
#[tauri::command]
pub async fn launch_game(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<u32> {
    use crate::distribution::ModuleType;

    let server_id = state
        .config
        .with(|c| c.selected_server.clone())?
        .ok_or_else(|| Error::Other("No server selected.".into()))?;

    let (mc_version, java_supported, has_mod_loader) = {
        let guard = state.distribution.lock().unwrap();
        let distro = guard.as_ref().ok_or(Error::NoDistribution)?;
        let server = distro
            .server_by_id(&server_id)
            .ok_or_else(|| Error::UnknownServer(server_id.clone()))?;
        let modded = server.modules.iter().any(|m| {
            matches!(
                m.module_type,
                ModuleType::Forge | ModuleType::ForgeHosted | ModuleType::Fabric | ModuleType::LiteLoader
            )
        });
        (
            server.minecraft_version.clone(),
            server.effective_java_options().supported,
            modded,
        )
    };

    if has_mod_loader {
        return Err(Error::Other(
            "This server uses a mod loader (Forge/Fabric), which the Tauri build does not \
             support yet. Use the Electron launcher for modded servers."
                .into(),
        ));
    }

    let account = state
        .config
        .with(|c| {
            c.selected_account
                .as_ref()
                .and_then(|u| c.authentication_database.get(u))
                .cloned()
        })?
        .ok_or_else(|| Error::Other("No account selected.".into()))?;

    let (data_dir, java_config, res_w, res_h, fullscreen) = state.config.with(|c| {
        (
            c.settings.launcher.data_directory.clone(),
            c.java_config.get(&server_id).cloned(),
            c.settings.game.res_width,
            c.settings.game.res_height,
            c.settings.game.fullscreen,
        )
    })?;
    let java_config = java_config
        .ok_or_else(|| Error::Other(format!("No java config for {server_id}.")))?;

    let common_dir = data_dir.join("common");
    let game_dir = data_dir.join("instances").join(&server_id);

    emit_progress(&app, "resolving", "Loading version information", 0.0);
    let proc = crate::dl::MojangIndexProcessor::new(common_dir.clone(), mc_version.clone());
    let manifest = proc.load_version_manifest().await;
    let version_json = proc.load_version_json(manifest.as_ref()).await?;
    let asset_index = proc.load_asset_index(&version_json).await?;

    emit_progress(&app, "validating", "Validating file integrity", 5.0);
    let list = proc.validate(&version_json, &asset_index).await;

    if !list.is_empty() {
        let total_mb = list.total_bytes() as f64 / 1_048_576.0;
        emit_progress(
            &app,
            "downloading",
            &format!("Downloading {} files ({total_mb:.0} MB)", list.len()),
            10.0,
        );
        let handle = app.clone();
        crate::dl::download_all(&list, 16, move |p| {
            // Map download progress onto the 10-90% band of the overall bar.
            emit_progress(
                &handle,
                "downloading",
                &format!("{}/{}", p.completed, p.total),
                10.0 + p.percent * 0.8,
            );
        })
        .await?;
    }

    emit_progress(&app, "java", "Locating a compatible Java runtime", 92.0);
    let jvm = match &java_config.executable {
        Some(exec) => crate::java::validate_jvm(exec, &java_supported).await?,
        None => crate::java::select_jvm(&data_dir, &java_supported).await,
    }
    .ok_or_else(|| {
        Error::Other(format!(
            "No compatible Java runtime found (need {java_supported}). \
             Automatic JDK download is not implemented yet — install one and retry."
        ))
    })?;

    emit_progress(&app, "launching", "Starting the game", 96.0);
    let ctx = crate::process_builder::LaunchContext {
        common_dir,
        game_dir,
        natives_dir: std::env::temp_dir().join(format!("lunar-natives-{server_id}")),
        java_config,
        account,
        launcher_version: env!("CARGO_PKG_VERSION").to_string(),
        res_width: res_w,
        res_height: res_h,
        fullscreen,
        server_id: server_id.clone(),
    };

    let exec = crate::java::java_exec_from_root(&jvm.path);
    let child = crate::process_builder::launch(&exec, &ctx, &version_json).await?;
    let pid = child.id().unwrap_or(0);

    emit_progress(&app, "done", "Game launched", 100.0);
    tracing::info!(pid, server = %server_id, "Game process started");

    // Detach: the game outlives this call.
    tokio::spawn(async move {
        let mut child = child;
        match child.wait().await {
            Ok(status) => tracing::info!(?status, "Game process exited"),
            Err(err) => tracing::error!(%err, "Failed waiting on game process"),
        }
    });

    Ok(pid)
}
