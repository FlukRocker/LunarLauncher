//! Launcher directory resolution.
//!
//! Mirrors the path logic in the Electron `configmanager.js`:
//!   - `sysRoot`     = %APPDATA% / ~/Library/Application Support / $HOME
//!   - `dataPath`    = <sysRoot>/.lunarlauncher   (default game data directory)
//!   - `launcherDir` = Electron's `app.getPath('userData')`, i.e. the per-app
//!                     config directory. Tauri resolves the same location from
//!                     the bundle identifier, so this must stay in sync with
//!                     `productName` in tauri.conf.json for existing installs
//!                     to keep their config.

use std::path::PathBuf;

/// The product name Electron used for `app.getPath('userData')`.
/// Changing this orphans every existing user's config.json.
pub const PRODUCT_NAME: &str = "Lunar Launcher";

/// Root of the platform's per-user application data area.
fn sys_root() -> PathBuf {
    if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .or_else(dirs::data_dir)
            .unwrap_or_else(|| PathBuf::from("."))
    } else if cfg!(target_os = "macos") {
        dirs::home_dir()
            .map(|h| h.join("Library").join("Application Support"))
            .unwrap_or_else(|| PathBuf::from("."))
    } else {
        dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
    }
}

/// Default game data directory: `<sysRoot>/.lunarlauncher`.
///
/// This is where common/, instances/ and java/ live. Note the leading dot is
/// kept on every platform, matching the Electron build.
pub fn default_data_directory() -> PathBuf {
    sys_root().join(".lunarlauncher")
}

/// The launcher's own directory, holding config.json and the cached
/// distribution index. Equivalent to Electron's `userData` path.
pub fn launcher_directory() -> PathBuf {
    sys_root().join(PRODUCT_NAME)
}

/// Current config location.
pub fn config_path() -> PathBuf {
    launcher_directory().join("config.json")
}

/// Pre-2.0 config location, migrated on first load if present.
pub fn legacy_config_path() -> PathBuf {
    default_data_directory().join("config.json")
}

/// On-disk cache of the distribution index, used when the remote is
/// unreachable.
pub fn distribution_path() -> PathBuf {
    launcher_directory().join("distribution.json")
}
