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
