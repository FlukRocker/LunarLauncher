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
    /// Set while a browser sign-in is pending, so it can be cancelled.
    pub login_cancel: std::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    /// True while a game process is alive.
    pub game_running: std::sync::atomic::AtomicBool,
    /// Ring buffer of the game's output, so the log tab can show history
    /// rather than only lines that arrive after it is opened.
    pub game_log: std::sync::Mutex<std::collections::VecDeque<String>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            config: ConfigManager::new(),
            distro: DistributionApi::new(),
            distribution: std::sync::Mutex::new(None),
            login_cancel: std::sync::Mutex::new(None),
            game_running: std::sync::atomic::AtomicBool::new(false),
            game_log: std::sync::Mutex::new(std::collections::VecDeque::new()),
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

    let (mc_version, java_supported, declared_loader) = {
        let guard = state.distribution.lock().unwrap();
        let distro = guard.as_ref().ok_or(Error::NoDistribution)?;
        let server = distro
            .server_by_id(&server_id)
            .ok_or_else(|| Error::UnknownServer(server_id.clone()))?;
        // A loader module's id is a maven coordinate, so the version is its
        // third segment. An absent version resolves to the newest release.
        let declared = server.modules.iter().find_map(|m| {
            let version = m.id.split(':').nth(2).unwrap_or("").to_string();
            match m.module_type {
                ModuleType::Fabric => Some(DeclaredLoader::Fabric(version)),
                // ForgeHosted ships the installer's output, so the expensive
                // part is already done; the id names the version manifest to
                // read once the modules are on disk.
                ModuleType::ForgeHosted => Some(DeclaredLoader::ForgeHosted(m.id.clone())),
                ModuleType::Forge | ModuleType::LiteLoader => Some(DeclaredLoader::Forge),
                _ => None,
            }
        });
        (
            server.minecraft_version.clone(),
            server.effective_java_options().supported,
            declared,
        )
    };

    // Fabric and ForgeHosted both resolve from metadata that already exists.
    // Plain Forge does not: the distribution gives only a version number, and
    // turning that into a launchable game means running Forge's installer
    // locally. Refused with a message that says which alternative works,
    // since the fix is on the distribution's side, not the player's.
    if matches!(declared_loader, Some(DeclaredLoader::Forge)) {
        return Err(Error::Other(
            "This server declares Forge as a plain version number, which requires running \
             Forge's installer locally to patch the game jar. That is not supported. A \
             distribution can ship the installer's output instead (a ForgeHosted module \
             with a version manifest), which does work — as does Fabric."
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
    let mut java_config = java_config
        .ok_or_else(|| Error::Other(format!("No java config for {server_id}.")))?;

    // Attach the OpenTelemetry Java agent, if the user configured one. Appended
    // to the user's own JVM options so their flags still win.
    let telemetry = state.config.with(|c| c.settings.telemetry.clone())?;
    let agent_args = crate::telemetry::java_agent_args(&telemetry, &server_id);
    if !agent_args.is_empty() {
        tracing::info!("Attaching the OpenTelemetry Java agent to the game");
        java_config.jvm_options.extend(agent_args);
    }

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

    // Download the distribution's own modules — the mods, configs and files
    // the server declares. Without this the game starts with an empty mods/
    // directory, which for Fabric is nearly indistinguishable from vanilla at
    // the main menu, so the failure is silent.
    {
        let saved: std::collections::HashMap<String, bool> = state.config.with(|c| {
            c.mod_configurations
                .iter()
                .find(|m| m.id == server_id)
                .and_then(|m| m.mods.as_object().cloned())
                .map(|o| {
                    o.into_iter()
                        .filter_map(|(k, v)| v.as_bool().map(|b| (k, b)))
                        .collect()
                })
                .unwrap_or_default()
        })?;

        let modules = {
            let guard = state.distribution.lock().unwrap();
            guard
                .as_ref()
                .and_then(|d| d.server_by_id(&server_id).map(|s| s.modules.clone()))
                .unwrap_or_default()
        };

        let mut wanted = Vec::new();
        crate::modules::collect_downloads(
            &modules,
            &saved,
            &game_dir,
            &common_dir,
            0,
            &mut wanted,
        );

        // Only fetch what is missing or fails its digest, so a relaunch does
        // not re-download a 300-mod pack.
        let mut needed = Vec::new();
        for asset in wanted {
            if !crate::dl::validate_by_digest_length(&asset.path, &asset.hash).await {
                needed.push(asset);
            }
        }

        if !needed.is_empty() {
            let mb = needed.iter().map(|a| a.size).sum::<u64>() as f64 / 1_048_576.0;
            emit_progress(
                &app,
                "modules",
                &format!("Downloading {} mods and files ({mb:.0} MB)", needed.len()),
                92.0,
            );
            let handle = app.clone();
            let list = crate::dl::RepairList {
                assets: needed,
                ..Default::default()
            };
            crate::dl::download_all(&list, 16, move |p| {
                emit_progress(
                    &handle,
                    "modules",
                    &format!("{}/{}", p.completed, p.total),
                    92.0 + p.percent * 0.04,
                );
            })
            .await?;
        }
    }


    // Resolve the loader and fetch its libraries. Done after the vanilla
    // validation pass so a loader failure cannot leave a half-downloaded game.
    let loader_profile = match &declared_loader {
        Some(DeclaredLoader::Fabric(version)) => {
            emit_progress(&app, "loader", "Resolving Fabric", 90.0);
            let http = reqwest::Client::new();
            let profile = crate::loader::resolve_fabric(&http, &mc_version, version).await?;

            let lib_dir = crate::dl::library_dir(&common_dir);
            let mut needed = Vec::new();
            for lib in &profile.libraries {
                let rel = lib.maven_path()?;
                let path = crate::paths::safe_join(&lib_dir, &rel)?;
                if !crate::dl::validate_local_file(&path, None).await {
                    needed.push(crate::dl::Asset {
                        id: lib.name.clone(),
                        // Fabric's metadata carries no hashes, so these cannot
                        // be checksum-validated the way Mojang's are. Recorded
                        // rather than silently skipped.
                        hash: String::new(),
                        size: 0,
                        url: lib.download_url()?,
                        path,
                    });
                }
            }
            if !needed.is_empty() {
                emit_progress(
                    &app,
                    "loader",
                    &format!("Downloading {} Fabric libraries", needed.len()),
                    91.0,
                );
                crate::dl::download_unverified(&needed, 8).await?;
            }
            Some(profile)
        }
        Some(DeclaredLoader::ForgeHosted(module_id)) => {
            emit_progress(&app, "loader", "Reading the Forge manifest", 93.0);

            // Pre-1.13 ForgeHosted distributions ship their libraries as
            // `.jar.pack.xz` — XZ-compressed pack200. Java removed pack200 in
            // 14 and there is no Rust implementation; the Electron original
            // shipped a Java helper jar to unpack them. Refused explicitly,
            // because the alternative is a classpath of files the JVM cannot
            // read and a crash far from the cause.
            if !crate::distribution::mc_version_at_least("1.13", &mc_version) {
                return Err(Error::Other(format!(
                    "This server uses Forge for Minecraft {mc_version}. Versions before 1.13 \
                     ship their libraries as pack200 archives, a format Java removed in \
                     version 14 and which this launcher cannot unpack. Forge on 1.13 and \
                     newer works."
                )));
            }

            // The manifest's id is not derivable from the module id by any
            // rule worth trusting, so find the VersionManifest module that
            // declares it.
            let manifest_id = {
                let guard = state.distribution.lock().unwrap();
                guard
                    .as_ref()
                    .and_then(|d| d.server_by_id(&server_id))
                    .and_then(|srv| srv.modules.iter().find(|m| &m.id == module_id))
                    .and_then(|forge| {
                        forge
                            .sub_modules
                            .iter()
                            .find(|sm| sm.module_type == ModuleType::VersionManifest)
                            .map(|sm| sm.id.clone())
                    })
            }
            .ok_or_else(|| {
                Error::Other(
                    "This server declares Forge but ships no version manifest. A ForgeHosted \
                     distribution must include the installer's output as a VersionManifest \
                     module; without it there is nothing to launch."
                        .into(),
                )
            })?;

            let path = common_dir
                .join("versions")
                .join(&manifest_id)
                .join(format!("{manifest_id}.json"));
            let json = tokio::fs::read_to_string(&path).await.map_err(|e| {
                Error::Other(format!(
                    "Could not read the Forge manifest at {}: {e}",
                    path.display()
                ))
            })?;

            Some(crate::loader::profile_from_forge_json(
                &json,
                &common_dir,
                &manifest_id,
            )?)
        }
        _ => None,
    };

    emit_progress(&app, "java", "Locating a compatible Java runtime", 96.0);
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
        loader: loader_profile,
    };

    let exec = crate::java::java_exec_from_root(&jvm.path);
    let child = crate::process_builder::launch(&exec, &ctx, &version_json).await?;
    let pid = child.id().unwrap_or(0);

    emit_progress(&app, "done", "Game launched", 100.0);
    tracing::info!(pid, server = %server_id, "Game process started");

    state.game_running.store(true, std::sync::atomic::Ordering::SeqCst);
    state.game_log.lock().unwrap().clear();
    let _ = tauri::Emitter::emit(&app, "game://started", pid);

    // Pump the game's output into the ring buffer and out as events, so the
    // log tab can show it live. Without this the pipes fill and the game
    // eventually blocks on its own stdout.
    tokio::spawn(async move {
        use tokio::io::{AsyncBufReadExt, BufReader};
        let mut child = child;
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        // Generic over the reader: stdout and stderr are distinct types, so a
        // closure would bind to whichever was passed first.
        async fn pump<R>(reader: Option<R>, app: tauri::AppHandle)
        where
            R: tokio::io::AsyncRead + Unpin,
        {
            let Some(r) = reader else { return };
            let mut lines = BufReader::new(r).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if let Some(state) = tauri::Manager::try_state::<AppState>(&app) {
                    let mut log = state.game_log.lock().unwrap();
                    if log.len() >= GAME_LOG_CAPACITY {
                        log.pop_front();
                    }
                    log.push_back(line.clone());
                }
                let _ = tauri::Emitter::emit(&app, "game://log", line);
            }
        }

        tokio::join!(pump(stdout, app.clone()), pump(stderr, app.clone()));

        let status = child.wait().await;
        match &status {
            Ok(s) => tracing::info!(?s, "Game process exited"),
            Err(err) => tracing::error!(%err, "Failed waiting on game process"),
        }
        if let Some(state) = tauri::Manager::try_state::<AppState>(&app) {
            state
                .game_running
                .store(false, std::sync::atomic::Ordering::SeqCst);
        }
        let code = status.ok().and_then(|s| s.code()).unwrap_or(-1);
        let _ = tauri::Emitter::emit(&app, "game://exited", code);
    });

    Ok(pid)
}

// ---------------------------------------------------------------------------
// Microsoft authentication
// ---------------------------------------------------------------------------

/// Open the Microsoft consent page in a dedicated window and complete the
/// login when it redirects back with an authorization code.
///
/// Replaces the `MSFT_AUTH_OPEN_LOGIN` BrowserWindow dance in index.js. The
/// window is watched for navigation to the redirect URI; the code is then
/// exchanged through the full token chain and the account is persisted.
#[tauri::command]
pub async fn microsoft_login(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<Account> {
    use crate::microsoft;
    use tauri::{WebviewUrl, WebviewWindowBuilder};

    const WINDOW_LABEL: &str = "msft-login";

    // Reusing a stale window leaves the user staring at a dead page.
    if let Some(existing) = tauri::Manager::get_webview_window(&app, WINDOW_LABEL) {
        let _ = existing.close();
    }

    let url = microsoft::authorize_url()
        .parse()
        .map_err(|e| Error::Other(format!("Bad authorize URL: {e}")))?;

    let (tx, rx) = tokio::sync::oneshot::channel::<std::result::Result<String, String>>();
    let tx = std::sync::Arc::new(std::sync::Mutex::new(Some(tx)));

    let window = WebviewWindowBuilder::new(&app, WINDOW_LABEL, WebviewUrl::External(url))
        .title("Microsoft Login")
        .inner_size(520.0, 600.0)
        .center()
        .on_navigation({
            let tx = std::sync::Arc::clone(&tx);
            move |url| {
                let s = url.to_string();
                if s.starts_with(microsoft::REDIRECT_URI) {
                    let outcome = match microsoft::extract_code_from_redirect(&s) {
                        Some(code) => Ok(code),
                        None => Err(microsoft::extract_error_from_redirect(&s)
                            .unwrap_or_else(|| "Login was not completed.".into())),
                    };
                    if let Some(tx) = tx.lock().unwrap().take() {
                        let _ = tx.send(outcome);
                    }
                    // Stop the webview actually loading the redirect page.
                    return false;
                }
                true
            }
        })
        .build()
        .map_err(|e| Error::Other(format!("Failed to open login window: {e}")))?;

    // A closed window must resolve the wait, or this command hangs forever.
    window.on_window_event({
        let tx = std::sync::Arc::clone(&tx);
        move |event| {
            if matches!(event, tauri::WindowEvent::Destroyed) {
                if let Some(tx) = tx.lock().unwrap().take() {
                    let _ = tx.send(Err("Login window was closed.".into()));
                }
            }
        }
    });

    let outcome = rx
        .await
        .map_err(|_| Error::Other("Login window closed unexpectedly.".into()))?;

    if let Some(w) = tauri::Manager::get_webview_window(&app, WINDOW_LABEL) {
        let _ = w.close();
    }

    let code = outcome.map_err(Error::Other)?;
    let auth = microsoft::full_auth_flow(&code, false).await?;

    let account = Account::Microsoft {
        access_token: auth.mc_access_token,
        username: auth.profile.name.clone(),
        uuid: auth.profile.id.clone(),
        display_name: auth.profile.name,
        expires_at: microsoft::expiry_from_now(auth.mc_expires_in),
        microsoft: crate::config::MicrosoftTokens {
            access_token: auth.ms_access_token,
            refresh_token: auth.ms_refresh_token,
            expires_at: microsoft::expiry_from_now(auth.ms_expires_in),
        },
    };

    state.config.with_mut(|c| {
        c.selected_account = Some(auth.profile.id.clone());
        c.authentication_database
            .insert(auth.profile.id.clone(), account.clone());
    })?;
    state.config.save()?;

    tracing::info!(user = %account.display_name(), "Microsoft login complete");
    Ok(account)
}

/// Refresh the selected account's tokens if they have expired.
///
/// Port of `validateSelected`. Offline accounts never need refreshing;
/// Microsoft accounts refresh through the same chain using the stored refresh
/// token. Returns true when the account is usable.
#[tauri::command]
pub async fn validate_selected_account(state: State<'_, AppState>) -> Result<bool> {
    use crate::microsoft;

    let current = state.config.with(|c| {
        c.selected_account
            .as_ref()
            .and_then(|u| c.authentication_database.get(u))
            .cloned()
    })?;

    let Some(account) = current else { return Ok(false) };

    let Account::Microsoft { uuid, expires_at, microsoft: tokens, .. } = &account else {
        // Offline accounts are always valid.
        return Ok(true);
    };

    if !microsoft::is_expired(expires_at) {
        return Ok(true);
    }

    tracing::info!("Minecraft token expired; refreshing");
    let auth = match microsoft::full_auth_flow(&tokens.refresh_token, true).await {
        Ok(a) => a,
        Err(err) => {
            tracing::error!(%err, "Token refresh failed; user must sign in again");
            return Ok(false);
        }
    };

    let uuid = uuid.clone();
    state.config.with_mut(|c| {
        if let Some(Account::Microsoft {
            access_token,
            expires_at,
            microsoft,
            ..
        }) = c.authentication_database.get_mut(&uuid)
        {
            *access_token = auth.mc_access_token.clone();
            *expires_at = microsoft::expiry_from_now(auth.mc_expires_in);
            microsoft.access_token = auth.ms_access_token.clone();
            microsoft.refresh_token = auth.ms_refresh_token.clone();
            microsoft.expires_at = microsoft::expiry_from_now(auth.ms_expires_in);
        }
    })?;
    state.config.save()?;
    Ok(true)
}

/// Open Microsoft's logout page so the next login can pick a different
/// account, then drop the local record.
#[tauri::command]
pub async fn microsoft_logout(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    uuid: String,
) -> Result<bool> {
    use tauri::{WebviewUrl, WebviewWindowBuilder};

    let url = crate::microsoft::LOGOUT_ENDPOINT
        .parse()
        .map_err(|e| Error::Other(format!("Bad logout URL: {e}")))?;

    // Best-effort: clearing the Microsoft session is a convenience, so a
    // failure here should not block removing the local account.
    if let Ok(w) = WebviewWindowBuilder::new(&app, "msft-logout", WebviewUrl::External(url))
        .title("Microsoft Logout")
        .inner_size(520.0, 600.0)
        .center()
        .build()
    {
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        let _ = w.close();
    }

    state.config.remove_account(&uuid)
}

// ---------------------------------------------------------------------------
// Java configuration (settings view)
// ---------------------------------------------------------------------------

/// Per-server java settings shown in the settings view.
#[derive(Debug, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaSettings {
    pub min_ram: String,
    pub max_ram: String,
    pub executable: Option<std::path::PathBuf>,
    pub jvm_options: Vec<String>,
}

#[tauri::command]
pub fn get_java_config(state: State<'_, AppState>, server_id: String) -> Result<JavaSettings> {
    state
        .config
        .with(|c| c.java_config.get(&server_id).cloned())?
        .map(|j| JavaSettings {
            min_ram: j.min_ram,
            max_ram: j.max_ram,
            executable: j.executable,
            jvm_options: j.jvm_options,
        })
        .ok_or_else(|| Error::Other(format!("No java config for {server_id}.")))
}

#[tauri::command]
pub fn save_java_config(
    state: State<'_, AppState>,
    server_id: String,
    settings: JavaSettings,
) -> Result<()> {
    state.config.with_mut(|c| {
        if let Some(j) = c.java_config.get_mut(&server_id) {
            j.min_ram = settings.min_ram;
            j.max_ram = settings.max_ram;
            j.executable = settings.executable;
            j.jvm_options = settings.jvm_options;
        }
    })?;
    state.config.save()
}

// ---------------------------------------------------------------------------
// Discord
// ---------------------------------------------------------------------------

/// Connect Rich Presence for the selected server, using the ids the
/// distribution index supplies. Silently does nothing when the index carries
/// no discord block.
#[tauri::command]
pub fn discord_connect(
    state: State<'_, AppState>,
    discord: State<'_, crate::discord::DiscordState>,
) -> Result<bool> {
    let guard = state.distribution.lock().unwrap();
    let Some(distro) = guard.as_ref() else { return Ok(false) };
    let Some(global) = &distro.discord else { return Ok(false) };

    let server_id = state.config.with(|c| c.selected_server.clone())?;
    let server = server_id.as_ref().and_then(|id| distro.server_by_id(id));
    let Some(server_discord) = server.and_then(|s| s.discord.as_ref()) else {
        return Ok(false);
    };

    discord.initialize(
        &global.client_id,
        "Waiting for Client..",
        &format!("Server: {}", server_discord.short_id),
        &server_discord.large_image_key,
        &server_discord.large_image_text,
        &global.small_image_key,
        &global.small_image_text,
    );
    Ok(true)
}

#[tauri::command]
pub fn discord_set_details(
    discord: State<'_, crate::discord::DiscordState>,
    details: String,
    state_line: String,
) {
    discord.set_details(&details, &state_line);
}

#[tauri::command]
pub fn discord_disconnect(discord: State<'_, crate::discord::DiscordState>) {
    discord.shutdown();
}

// ---------------------------------------------------------------------------
// Mod manager
// ---------------------------------------------------------------------------

/// An optional module the distribution declares, which the user may turn off.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OptionalMod {
    pub id: String,
    pub name: String,
    pub required: bool,
    /// The *effective* state: this module's own preference AND every ancestor
    /// being on. A download path must read this rather than recomputing from
    /// the saved preference, or it will install children of a disabled parent.
    ///
    /// A required child under a switched-off optional parent therefore reports
    /// `required: true, enabled: false` — it is required *within its group*,
    /// and the group is off.
    pub enabled: bool,
    /// Nearest toggleable ancestor, or `None` at the top level. Non-mod parents
    /// (a loader carrying its libraries, say) are not listed and so are never
    /// named here, but they still gate their children.
    pub parent: Option<String>,
}

/// How deep `subModules` nesting is followed before giving up.
///
/// The distribution is untrusted network input, so the walk is bounded rather
/// than trusting the document to terminate.
const MAX_MODULE_DEPTH: usize = 16;

/// Walk a module tree, collecting the mods the user can toggle.
///
/// The spec models an optional mod group as a parent module with children in
/// `subModules` — the reference sample nests config files under `dynsurround`
/// and a `LiteMod` under `liteloader`. Before this walked recursively, a child
/// mod never appeared in the toggle list at all.
///
/// `parent_enabled` gates the whole subtree: a child is only on when its parent
/// is, which is the entire point of nesting. A child's own saved preference is
/// kept in config regardless, so switching a parent back on restores whatever
/// the user had chosen underneath it.
fn collect_distribution_mods(
    modules: &[crate::distribution::Module],
    saved: &std::collections::HashMap<String, bool>,
    parent: Option<&str>,
    parent_enabled: bool,
    depth: usize,
    out: &mut Vec<OptionalMod>,
) {
    use crate::distribution::ModuleType;

    if depth >= MAX_MODULE_DEPTH {
        tracing::warn!(
            depth,
            "subModules nested deeper than {MAX_MODULE_DEPTH}; ignoring the rest of this branch."
        );
        return;
    }

    for m in modules {
        let is_mod = matches!(
            m.module_type,
            ModuleType::ForgeMod | ModuleType::LiteMod | ModuleType::FabricMod
        );
        let required = m.required.as_ref().map(|r| r.value()).unwrap_or(true);
        let default_on = m.required.as_ref().map(|r| r.default_on()).unwrap_or(true);

        let own_on = if required {
            true
        } else {
            saved.get(&m.id).copied().unwrap_or(default_on)
        };
        let effective = parent_enabled && own_on;

        if is_mod {
            out.push(OptionalMod {
                id: m.id.clone(),
                name: m.name.clone(),
                required,
                enabled: effective,
                parent: parent.map(str::to_string),
            });
        }

        if !m.sub_modules.is_empty() {
            // Only a toggleable module becomes the reported parent; a Library or
            // loader passes its own gate straight through to its children.
            let (child_parent, gate) = if is_mod {
                (Some(m.id.as_str()), effective)
            } else {
                (parent, parent_enabled)
            };
            collect_distribution_mods(&m.sub_modules, saved, child_parent, gate, depth + 1, out);
        }
    }
}

fn instance_dir_for(state: &AppState, server_id: &str) -> Result<std::path::PathBuf> {
    let data = state.config.with(|c| c.settings.launcher.data_directory.clone())?;
    Ok(data.join("instances").join(server_id))
}

/// Mods the distribution declares for a server, with their on/off state.
///
/// Required modules are listed too, marked so the UI can show them as locked
/// — the Electron settings view did the same, since seeing what a server
/// forces on you is useful even when you cannot change it.
#[tauri::command]
pub fn get_distribution_mods(
    state: State<'_, AppState>,
    server_id: String,
) -> Result<Vec<OptionalMod>> {
    let guard = state.distribution.lock().unwrap();
    let distro = guard.as_ref().ok_or(Error::NoDistribution)?;
    let server = distro
        .server_by_id(&server_id)
        .ok_or_else(|| Error::UnknownServer(server_id.clone()))?;

    let saved: std::collections::HashMap<String, bool> = state.config.with(|c| {
        c.mod_configurations
            .iter()
            .find(|m| m.id == server_id)
            .and_then(|m| m.mods.as_object().cloned())
            .map(|o| {
                o.into_iter()
                    .filter_map(|(k, v)| v.as_bool().map(|b| (k, b)))
                    .collect()
            })
            .unwrap_or_default()
    })?;

    let mut out = Vec::new();
    collect_distribution_mods(&server.modules, &saved, None, true, 0, &mut out);
    Ok(out)
}

/// Persist an optional module's on/off state for a server.
#[tauri::command]
pub fn set_distribution_mod_enabled(
    state: State<'_, AppState>,
    server_id: String,
    mod_id: String,
    enabled: bool,
) -> Result<()> {
    state.config.with_mut(|c| {
        let entry = match c.mod_configurations.iter_mut().find(|m| m.id == server_id) {
            Some(e) => e,
            None => {
                c.mod_configurations.push(crate::config::ModConfiguration {
                    id: server_id.clone(),
                    mods: serde_json::json!({}),
                });
                c.mod_configurations.last_mut().unwrap()
            }
        };
        if !entry.mods.is_object() {
            entry.mods = serde_json::json!({});
        }
        entry.mods[&mod_id] = serde_json::Value::Bool(enabled);
    })?;
    state.config.save()
}

/// Mods the user dropped into the instance's mods folder.
#[tauri::command]
pub fn get_dropin_mods(
    state: State<'_, AppState>,
    server_id: String,
) -> Result<Vec<crate::mods::DropinMod>> {
    let version = {
        let guard = state.distribution.lock().unwrap();
        guard
            .as_ref()
            .and_then(|d| d.server_by_id(&server_id).map(|s| s.minecraft_version.clone()))
            .unwrap_or_default()
    };
    Ok(crate::mods::scan(&instance_dir_for(&state, &server_id)?, &version))
}

/// Enable or disable a drop-in mod. Returns its new handle, since the file
/// is renamed.
#[tauri::command]
pub fn toggle_dropin_mod(
    state: State<'_, AppState>,
    server_id: String,
    full_name: String,
    enable: bool,
) -> Result<String> {
    crate::mods::toggle(&instance_dir_for(&state, &server_id)?, &full_name, enable)
}

/// Move a drop-in mod to the trash.
#[tauri::command]
pub fn delete_dropin_mod(
    state: State<'_, AppState>,
    server_id: String,
    full_name: String,
) -> Result<()> {
    crate::mods::delete(&instance_dir_for(&state, &server_id)?, &full_name)
}

/// Copy chosen jars into the instance's mods folder. Returns how many were
/// accepted; unrecognised files are skipped rather than failing the batch.
#[tauri::command]
pub fn add_dropin_mods(
    state: State<'_, AppState>,
    server_id: String,
    paths: Vec<std::path::PathBuf>,
) -> Result<usize> {
    crate::mods::add(&instance_dir_for(&state, &server_id)?, &paths)
}

/// Reveal the mods folder in the OS file manager.
#[tauri::command]
pub fn open_mods_folder(state: State<'_, AppState>, server_id: String) -> Result<()> {
    let dir = crate::mods::mods_dir(&instance_dir_for(&state, &server_id)?);
    std::fs::create_dir_all(&dir)?;
    open::that(&dir).map_err(|e| Error::Other(format!("Could not open {}: {e}", dir.display())))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShaderState {
    pub packs: Vec<crate::mods::Shaderpack>,
    pub selected: String,
}

#[tauri::command]
pub fn get_shaderpacks(state: State<'_, AppState>, server_id: String) -> Result<ShaderState> {
    let dir = instance_dir_for(&state, &server_id)?;
    Ok(ShaderState {
        packs: crate::mods::scan_shaderpacks(&dir),
        selected: crate::mods::enabled_shaderpack(&dir),
    })
}

#[tauri::command]
pub fn set_shaderpack(
    state: State<'_, AppState>,
    server_id: String,
    pack: String,
) -> Result<()> {
    crate::mods::set_enabled_shaderpack(&instance_dir_for(&state, &server_id)?, &pack)
}

/// Ping the selected server for its live player count.
///
/// Never fails on an unreachable host — an offline server is a normal state
/// the landing view has to render, not an error.
#[tauri::command]
pub async fn get_server_status(
    state: State<'_, AppState>,
    server_id: String,
) -> Result<crate::server_status::ServerStatus> {
    let address = {
        let guard = state.distribution.lock().unwrap();
        let distro = guard.as_ref().ok_or(Error::NoDistribution)?;
        distro
            .server_by_id(&server_id)
            .ok_or_else(|| Error::UnknownServer(server_id.clone()))?
            .parse_address()?
    };
    Ok(crate::server_status::ping(&address.hostname, address.port).await)
}

/// Microsoft sign-in via the user's default browser (RFC 8252 loopback).
///
/// Preferred over the embedded webview: the user can see the real address
/// bar, existing sessions and password managers work, and Microsoft has been
/// progressively restricting embedded webviews for OAuth.
///
/// Requires `http://127.0.0.1` to be registered as a redirect URI on the
/// Azure application; see the auth section of README.md.
#[tauri::command]
pub async fn microsoft_login_browser(state: State<'_, AppState>) -> Result<Account> {
    use crate::microsoft;

    let (redirect_uri, listener) = microsoft::start_loopback().await?;
    let url = microsoft::authorize_url_for(&redirect_uri);

    open::that(&url)
        .map_err(|e| Error::Other(format!("Could not open your browser: {e}")))?;
    tracing::info!(%redirect_uri, "Waiting for the browser to complete sign-in");

    // Race the redirect against an explicit cancel and a timeout, so an
    // abandoned tab or a user who changes their mind does not leave this
    // command pending forever.
    let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();
    *state.login_cancel.lock().unwrap() = Some(cancel_tx);

    let wait = microsoft::await_loopback_code(listener);
    tokio::pin!(wait);

    let code = tokio::select! {
        result = &mut wait => {
            state.login_cancel.lock().unwrap().take();
            result?
        }
        _ = cancel_rx => {
            tracing::info!("Browser sign-in cancelled by the user");
            return Err(Error::Other("Sign-in cancelled.".into()));
        }
        _ = tokio::time::sleep(std::time::Duration::from_secs(300)) => {
            state.login_cancel.lock().unwrap().take();
            return Err(Error::Other("Sign-in timed out after 5 minutes.".into()));
        }
    };

    let auth = microsoft::full_auth_flow_with_redirect(&code, false, &redirect_uri).await?;

    let account = Account::Microsoft {
        access_token: auth.mc_access_token,
        username: auth.profile.name.clone(),
        uuid: auth.profile.id.clone(),
        display_name: auth.profile.name,
        expires_at: microsoft::expiry_from_now(auth.mc_expires_in),
        microsoft: crate::config::MicrosoftTokens {
            access_token: auth.ms_access_token,
            refresh_token: auth.ms_refresh_token,
            expires_at: microsoft::expiry_from_now(auth.ms_expires_in),
        },
    };

    state.config.with_mut(|c| {
        c.selected_account = Some(auth.profile.id.clone());
        c.authentication_database.insert(auth.profile.id.clone(), account.clone());
    })?;
    state.config.save()?;

    tracing::info!(user = %account.display_name(), "Microsoft login complete (browser)");
    Ok(account)
}

/// Abort a pending browser sign-in.
///
/// Returns false when nothing was waiting, so the frontend can tell a real
/// cancellation from a stale click.
#[tauri::command]
pub fn cancel_microsoft_login(state: State<'_, AppState>) -> bool {
    match state.login_cancel.lock().unwrap().take() {
        Some(tx) => {
            let _ = tx.send(());
            true
        }
        None => false,
    }
}

/// Yggdrasil ("Mojang") sign-in.
///
/// Mojang's own auth server is shut down; this targets whatever endpoint
/// `LUNAR_AUTH_SERVER` names, which is how private servers running
/// authlib-injector and similar are reached.
#[tauri::command]
pub async fn mojang_login(
    state: State<'_, AppState>,
    username: String,
    password: String,
) -> Result<Account> {
    let username = username.trim().to_string();
    if username.is_empty() || password.is_empty() {
        return Err(Error::Other("Username and password are required.".into()));
    }

    // Yggdrasil ties a token to a client token; reuse the stored one so
    // existing sessions are not invalidated on every sign-in.
    let client_token = state
        .config
        .with(|c| c.client_token.clone())?
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let auth = crate::mojang::authenticate(&username, &password, &client_token).await?;
    let profile = auth
        .selected_profile
        .ok_or_else(|| Error::Other("No Minecraft profile on this account.".into()))?;

    let account = Account::Mojang {
        access_token: auth.access_token,
        username: username.clone(),
        uuid: profile.id.clone(),
        display_name: profile.name,
    };

    state.config.with_mut(|c| {
        c.client_token = Some(auth.client_token.clone());
        c.selected_account = Some(profile.id.clone());
        c.authentication_database.insert(profile.id.clone(), account.clone());
    })?;
    state.config.save()?;

    tracing::info!(user = %account.display_name(), "Mojang/Yggdrasil login complete");
    Ok(account)
}

/// Load the news feed named by the distribution's `rss` field.
///
/// Returns an empty list when no feed is configured, which the UI shows as a
/// disabled news button rather than an error.
#[tauri::command]
pub async fn get_news(state: State<'_, AppState>) -> Result<Vec<crate::news::Article>> {
    let rss = {
        let guard = state.distribution.lock().unwrap();
        guard.as_ref().and_then(|d| d.rss.clone()).unwrap_or_default()
    };
    crate::news::load(&rss).await
}

/// How many lines of game output to retain.
const GAME_LOG_CAPACITY: usize = 2000;

/// Is a game process alive right now?
#[tauri::command]
pub fn is_game_running(state: State<'_, AppState>) -> bool {
    state.game_running.load(std::sync::atomic::Ordering::SeqCst)
}

/// The retained game output, oldest first.
#[tauri::command]
pub fn get_game_log(state: State<'_, AppState>) -> Vec<String> {
    state.game_log.lock().unwrap().iter().cloned().collect()
}

#[tauri::command]
pub fn clear_game_log(state: State<'_, AppState>) {
    state.game_log.lock().unwrap().clear();
}

/// Current telemetry settings.
#[tauri::command]
pub fn get_telemetry(state: State<'_, AppState>) -> Result<crate::telemetry::TelemetryConfig> {
    state.config.with(|c| c.settings.telemetry.clone())
}

/// Update telemetry settings. Takes effect for the game immediately; the
/// launcher's own exporter is installed at startup, so that part needs a
/// restart, which the UI says.
#[tauri::command]
pub fn save_telemetry(
    state: State<'_, AppState>,
    telemetry: crate::telemetry::TelemetryConfig,
) -> Result<()> {
    state.config.with_mut(|c| c.settings.telemetry = telemetry)?;
    state.config.save()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::distribution::{Artifact, Module, ModuleType, RequiredSpec};
    use std::collections::HashMap;

    fn module(id: &str, ty: ModuleType, required: Option<(bool, bool)>) -> Module {
        Module {
            id: id.into(),
            name: id.into(),
            module_type: ty,
            classpath: None,
            required: required.map(|(value, def)| RequiredSpec {
                value: Some(value),
                def: Some(def),
            }),
            artifact: Artifact {
                size: 1,
                md5: None,
                url: "https://example.com/a.jar".into(),
                path: None,
            },
            sub_modules: Vec::new(),
        }
    }

    fn collect(modules: &[Module], saved: &HashMap<String, bool>) -> Vec<OptionalMod> {
        let mut out = Vec::new();
        collect_distribution_mods(modules, saved, None, true, 0, &mut out);
        out
    }

    #[test]
    fn nested_mods_are_listed_not_dropped() {
        // The regression: the walk used to be flat, so a child mod authored
        // under a parent never reached the toggle list at all.
        let mut parent = module("parent", ModuleType::ForgeMod, Some((false, true)));
        parent.sub_modules = vec![module("child", ModuleType::LiteMod, Some((false, true)))];

        let got = collect(&[parent], &HashMap::new());

        let ids: Vec<_> = got.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["parent", "child"]);
        assert_eq!(got[1].parent.as_deref(), Some("parent"));
    }

    #[test]
    fn a_child_is_off_when_its_parent_is_off() {
        // The point of nesting: turning a group off must take its children with
        // it, even though the child's own saved preference says on.
        let mut parent = module("parent", ModuleType::ForgeMod, Some((false, true)));
        parent.sub_modules = vec![module("child", ModuleType::ForgeMod, Some((false, true)))];

        let saved = HashMap::from([("parent".to_string(), false), ("child".to_string(), true)]);
        let got = collect(&[parent], &saved);

        assert!(!got[0].enabled, "parent was switched off");
        assert!(!got[1].enabled, "child must follow its parent off");
    }

    #[test]
    fn a_childs_preference_survives_the_parent_going_off_and_on() {
        let mut parent = module("parent", ModuleType::ForgeMod, Some((false, true)));
        parent.sub_modules = vec![module("child", ModuleType::ForgeMod, Some((false, true)))];

        // Child explicitly off, parent on: only the child is off.
        let saved = HashMap::from([("parent".to_string(), true), ("child".to_string(), false)]);
        let got = collect(&[parent.clone()], &saved);
        assert!(got[0].enabled);
        assert!(!got[1].enabled);

        // Parent off, then back on — the child's stored preference still rules.
        let off = HashMap::from([("parent".to_string(), false), ("child".to_string(), true)]);
        assert!(!collect(&[parent.clone()], &off)[1].enabled);
        let on = HashMap::from([("parent".to_string(), true), ("child".to_string(), true)]);
        assert!(collect(&[parent], &on)[1].enabled);
    }

    #[test]
    fn a_required_child_under_a_disabled_parent_is_still_off() {
        let mut parent = module("parent", ModuleType::ForgeMod, Some((false, true)));
        parent.sub_modules = vec![module("child", ModuleType::ForgeMod, None)]; // required

        let saved = HashMap::from([("parent".to_string(), false)]);
        let got = collect(&[parent], &saved);

        assert!(got[1].required, "the child is required within its group");
        assert!(!got[1].enabled, "but the group is switched off");
    }

    #[test]
    fn non_mod_parents_gate_children_without_being_listed() {
        // A loader carrying mods beneath it: the loader itself has no toggle,
        // so it must not appear, but its children still must.
        let mut loader = module("loader", ModuleType::Forge, None);
        loader.sub_modules = vec![
            module("lib", ModuleType::Library, None),
            module("bundled", ModuleType::ForgeMod, Some((false, true))),
        ];

        let got = collect(&[loader], &HashMap::new());

        let ids: Vec<_> = got.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["bundled"], "only the mod is toggleable");
        assert_eq!(
            got[0].parent, None,
            "a non-toggleable ancestor is not reported as the parent"
        );
        assert!(got[0].enabled);
    }

    #[test]
    fn unknown_module_types_are_never_listed_as_mods() {
        let got = collect(&[module("mystery", ModuleType::Unknown, None)], &HashMap::new());
        assert!(got.is_empty());
    }

    #[test]
    fn nesting_deeper_than_the_cap_is_ignored_rather_than_overflowing() {
        // The distribution is untrusted input; a pathological document must not
        // take the process out.
        let mut root = module("d0", ModuleType::ForgeMod, None);
        {
            let mut cursor = &mut root;
            for i in 1..(MAX_MODULE_DEPTH + 8) {
                cursor.sub_modules = vec![module(&format!("d{i}"), ModuleType::ForgeMod, None)];
                cursor = &mut cursor.sub_modules[0];
            }
        }

        let got = collect(&[root], &HashMap::new());
        assert_eq!(got.len(), MAX_MODULE_DEPTH, "walk stops at the cap");
    }
}

// ---------------------------------------------------------------------------
// Diagnostics
// ---------------------------------------------------------------------------

/// Build a support report for a failure the user hit.
///
/// Deliberately assembled here rather than in the frontend, because the useful
/// context — resolved paths, the JVM actually chosen, recent game output —
/// only exists on this side.
///
/// Nothing secret is included. The config carries Microsoft access and refresh
/// tokens; a report is meant to be pasted into a Discord thread, so accounts
/// are summarised by type and never by token or even by full UUID.
#[tauri::command]
pub fn export_diagnostics(
    state: State<'_, AppState>,
    error_context: Option<String>,
) -> Result<String> {
    use std::fmt::Write as _;

    let mut r = String::new();
    let _ = writeln!(r, "Lunar Launcher diagnostics");
    let _ = writeln!(r, "==========================");
    let _ = writeln!(r, "version    {}", env!("CARGO_PKG_VERSION"));
    let _ = writeln!(
        r,
        "platform   {} {}",
        std::env::consts::OS,
        std::env::consts::ARCH
    );

    if let Some(err) = &error_context {
        let _ = writeln!(r, "\n-- what went wrong --\n{err}");
    }

    let _ = writeln!(r, "\n-- distribution --");
    let _ = writeln!(r, "source     {}", state.distro.source_description());
    match &*state.distribution.lock().unwrap() {
        Some(d) => {
            let _ = writeln!(r, "loaded     yes ({} servers)", d.servers.len());
            for s in &d.servers {
                let _ = writeln!(
                    r,
                    "  - {} [{}] mc={} modules={}",
                    s.id,
                    if s.main_server { "main" } else { "alt" },
                    s.minecraft_version,
                    s.modules.len()
                );
            }
        }
        None => {
            let _ = writeln!(r, "loaded     NO — this alone prevents launching");
        }
    }

    let _ = writeln!(r, "\n-- configuration --");
    if let Ok(cfg) = state.config.with(|c| c.clone()) {
        let _ = writeln!(r, "data dir   {}", cfg.settings.launcher.data_directory.display());
        let _ = writeln!(r, "server     {}", cfg.selected_server.as_deref().unwrap_or("(none)"));
        let _ = writeln!(
            r,
            "resolution {}x{} fullscreen={}",
            cfg.settings.game.res_width, cfg.settings.game.res_height, cfg.settings.game.fullscreen
        );

        // Accounts by type only. Never the token, and only enough of the uuid
        // to correlate against a server log.
        let _ = writeln!(r, "accounts   {}", cfg.authentication_database.len());
        for a in cfg.authentication_database.values() {
            let kind = match a {
                Account::Microsoft { .. } => "microsoft",
                Account::Mojang { .. } => "mojang",
                Account::Lunar { .. } => "offline",
            };
            let uuid = a.uuid();
            let short = uuid.get(..8).unwrap_or(uuid);
            let selected = cfg.selected_account.as_deref() == Some(uuid);
            let _ = writeln!(
                r,
                "  - {kind} {short}…{}",
                if selected { " (selected)" } else { "" }
            );
        }

        for (server, j) in &cfg.java_config {
            let _ = writeln!(
                r,
                "java[{server}] {}..{} exec={} opts={}",
                j.min_ram,
                j.max_ram,
                j.executable
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "(auto)".into()),
                j.jvm_options.join(" ")
            );
        }
    } else {
        let _ = writeln!(r, "(configuration not loaded)");
    }

    let log = state.game_log.lock().unwrap();
    let _ = writeln!(r, "\n-- game output (last {} of {} lines) --", 200.min(log.len()), log.len());
    if log.is_empty() {
        let _ = writeln!(r, "(the game has not been launched this session)");
    } else {
        for line in log.iter().skip(log.len().saturating_sub(200)) {
            let _ = writeln!(r, "{line}");
        }
    }

    Ok(r)
}

/// Which loader a server's distribution declares.
#[derive(Debug, Clone, PartialEq)]
enum DeclaredLoader {
    /// Carries the pinned version; empty means newest.
    Fabric(String),
    /// Carries the ForgeHosted module's maven id, from which the game version
    /// and the id of the shipped version manifest are derived.
    ForgeHosted(String),
    /// Modern Forge and LiteLoader, which need the installer pipeline.
    Forge,
}
