//! Launcher self-update: holding a discovered update, and installing it.
//!
//! The check runs at startup and deliberately does not install. This module is
//! the offer it defers to — the update is parked here, announced to the
//! frontend, and only acted on when the user says so.
//!
//! Two hazards shape everything below.
//!
//! **A running game.** Installing replaces the launcher binary. The game is a
//! child process of the launcher, and on Windows the NSIS installer will
//! happily overwrite a running executable's file while a session is live. That
//! is the one genuinely destructive thing this feature can do, so it is
//! refused rather than warned about.
//!
//! **The log.** On Windows there is no console — release builds set
//! `windows_subsystem = "windows"` — so the rolling file is the only record of
//! what happened. The installer terminates this process without unwinding, so
//! anything still buffered at that moment is lost, which is precisely the
//! evidence needed when an update fails. The writer is flushed at the last
//! moment before control passes to the installer.

use serde::Serialize;
use tauri::{Emitter, Manager};

use crate::error::{Error, Result};

/// The handle to a discovered update, whatever produced it.
///
/// Windows is migrating to Velopack — the Discord/Slack install model, where a
/// single Setup.exe writes to %LOCALAPPDATA% and updates arrive as deltas.
/// macOS and Linux stay on the Tauri updater until that is proven on Windows,
/// because migrating three packaging pipelines at once would leave none of
/// them verifiable.
///
/// Everything above this line is unchanged by that split: the frontend sees
/// the same `UpdateInfo`, the same `update://available` event and the same
/// three commands whichever backend answered.
#[cfg(target_os = "windows")]
pub type PendingUpdate = velopack::UpdateInfo;
#[cfg(not(target_os = "windows"))]
pub type PendingUpdate = tauri_plugin_updater::Update;

/// What the frontend needs to describe an update. The `Update` handle itself
/// stays in Rust; nothing the webview sends can influence what gets installed.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub current_version: String,
    /// Absent rather than null when the release carries none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pub_date: Option<String>,
}

#[cfg(not(target_os = "windows"))]
impl From<&tauri_plugin_updater::Update> for UpdateInfo {
    fn from(u: &tauri_plugin_updater::Update) -> Self {
        Self {
            version: u.version.clone(),
            current_version: u.current_version.clone(),
            notes: u.body.clone(),
            pub_date: u.date.map(|d| d.to_string()),
        }
    }
}

#[cfg(target_os = "windows")]
impl From<&velopack::UpdateInfo> for UpdateInfo {
    fn from(u: &velopack::UpdateInfo) -> Self {
        let rel = &u.TargetFullRelease;
        Self {
            version: rel.Version.clone(),
            current_version: env!("CARGO_PKG_VERSION").to_string(),
            notes: Some(rel.NotesMarkdown.clone()).filter(|n| !n.trim().is_empty()),
            pub_date: None,
        }
    }
}

/// Download progress, pushed as `update://progress`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProgress {
    pub downloaded: u64,
    /// Absent when the server sends no content-length, which is why the
    /// frontend must handle an indeterminate bar rather than assuming a total.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub percent: Option<f64>,
}

/// Install a previously discovered update.
///
/// Takes the update out of state: an install is one-shot, and leaving the
/// handle in place would let a second click start a concurrent download of the
/// same bytes.
pub async fn install(app: tauri::AppHandle, state: &crate::commands::AppState) -> Result<()> {
    if state.game_running.load(std::sync::atomic::Ordering::SeqCst) {
        return Err(Error::Other(
            "Minecraft is running. Close the game before updating the launcher — installing now \
             would replace the launcher underneath the running session."
                .into(),
        ));
    }

    // Claim the install before taking the update, so two rapid clicks cannot
    // both get past this point.
    if state
        .update_installing
        .swap(true, std::sync::atomic::Ordering::SeqCst)
    {
        return Err(Error::Other("An update is already installing.".into()));
    }

    let update = state.pending_update.lock().unwrap().take();
    let Some(update) = update else {
        state
            .update_installing
            .store(false, std::sync::atomic::Ordering::SeqCst);
        return Err(Error::Other("No update is pending.".into()));
    };

    let info = UpdateInfo::from(&update);
    tracing::info!(version = %info.version, "Installing launcher update");

    match apply(&app, update, state).await {
        Ok(()) => {
            // Reached only where the installer does not replace the process
            // itself. On Windows neither backend returns here.
            app.restart();
        }
        Err(err) => {
            state
                .update_installing
                .store(false, std::sync::atomic::Ordering::SeqCst);
            // A verification failure is not a transport failure, and the
            // difference decides whether the user should retry or stop.
            let lower = err.to_lowercase();
            let message = if lower.contains("signature") || lower.contains("verif") {
                format!(
                    "The update could not be verified and was not installed. Its signature does \
                     not match this launcher's key, so it may have been tampered with or built \
                     by someone else. ({err})"
                )
            } else {
                format!("The update could not be installed: {err}")
            };
            tracing::error!(%err, "Launcher update failed");
            Err(Error::Other(message))
        }
    }
}

/// Where Velopack reads its release feed from.
///
/// A plain directory served over HTTP — Velopack reads `releases.win.json` and
/// the packages beside it. That is simpler than the endpoint the Tauri updater
/// needed, which had to answer per-target and per-version.
#[cfg(target_os = "windows")]
pub fn feed_url() -> String {
    std::env::var("LUNAR_UPDATE_FEED")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .or_else(|| option_env!("LUNAR_UPDATE_FEED").map(str::to_string))
        .unwrap_or_else(|| "http://192.168.1.115:8080/releases".to_string())
}

/// Windows: Velopack.
///
/// Its calls are blocking and the apply step replaces the process, so the whole
/// sequence runs on a blocking thread rather than on a Tokio worker, where it
/// would stall every other task in the runtime for the length of a download.
#[cfg(target_os = "windows")]
async fn apply(
    _app: &tauri::AppHandle,
    update: PendingUpdate,
    state: &crate::commands::AppState,
) -> std::result::Result<(), String> {
    let log_guard = state.log_guard.clone();
    tokio::task::spawn_blocking(move || {
        let source = velopack::sources::HttpSource::new(feed_url());
        let um = velopack::UpdateManager::new(source, None, None).map_err(|e| e.to_string())?;
        um.download_updates(&update, None).map_err(|e| e.to_string())?;

        // The point of no return: apply replaces this process. Flush the log
        // now, because nothing after this is guaranteed to reach disk and it is
        // exactly the evidence needed when an update fails.
        tracing::info!("Download complete; handing over to Velopack");
        drop(log_guard.lock().unwrap().take());

        um.apply_updates_and_restart(&update).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Everything else: the Tauri updater, unchanged.
#[cfg(not(target_os = "windows"))]
async fn apply(
    app: &tauri::AppHandle,
    update: PendingUpdate,
    state: &crate::commands::AppState,
) -> std::result::Result<(), String> {
    let handle = app.clone();
    let mut downloaded: u64 = 0;
    let log_guard = state.log_guard.clone();

    update
        .download_and_install(
            move |chunk, total| {
                downloaded += chunk as u64;
                let percent = total.map(|t| {
                    if t == 0 {
                        0.0
                    } else {
                        (downloaded as f64 / t as f64) * 100.0
                    }
                });
                let _ = handle.emit(
                    "update://progress",
                    UpdateProgress { downloaded, total, percent },
                );
            },
            move || {
                tracing::info!("Download complete; handing over to the installer");
                drop(log_guard.lock().unwrap().take());
            },
        )
        .await
        .map_err(|e| e.to_string())
}


/// Record a discovered update and tell the frontend.
pub fn announce(app: &tauri::AppHandle, update: tauri_plugin_updater::Update) {
    let info = UpdateInfo::from(&update);
    let state = app.state::<crate::commands::AppState>();
    *state.pending_update.lock().unwrap() = Some(update);
    // An event as well as the stored value: a launcher already open when the
    // check finishes should not have to poll to find out.
    let _ = app.emit("update://available", info);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_notes_are_omitted_rather_than_null() {
        let info = UpdateInfo {
            version: "2.3.0".into(),
            current_version: "2.2.1".into(),
            notes: None,
            pub_date: None,
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(!json.contains("null"), "{json}");
        assert!(!json.contains("notes"), "{json}");
        assert!(json.contains("\"currentVersion\":\"2.2.1\""), "{json}");
    }

    #[test]
    fn progress_without_a_total_stays_indeterminate() {
        let json = serde_json::to_string(&UpdateProgress {
            downloaded: 1024,
            total: None,
            percent: None,
        })
        .unwrap();
        assert_eq!(json, r#"{"downloaded":1024}"#);
    }

    #[test]
    fn progress_percent_is_a_percentage_not_a_fraction() {
        let json = serde_json::to_string(&UpdateProgress {
            downloaded: 50,
            total: Some(200),
            percent: Some(25.0),
        })
        .unwrap();
        assert!(json.contains("\"percent\":25.0"), "{json}");
    }
}
