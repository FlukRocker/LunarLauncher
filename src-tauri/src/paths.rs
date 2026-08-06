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

use std::path::{Component, Path, PathBuf};

use crate::error::{Error, Result};

/// The product name Electron used for `app.getPath('userData')`.
/// Changing this orphans every existing user's config.json.
pub const PRODUCT_NAME: &str = "Lunar Launcher";

/// Join a distribution-supplied relative path onto a base directory, refusing
/// anything that would escape it.
///
/// `PathBuf::join` treats an argument beginning with `/` as **absolute** and
/// discards the base entirely:
///
/// ```text
/// Path::new("/instances/survival").join("/config/x.cfg")  ->  "/config/x.cfg"
/// ```
///
/// Node's `path.join` swallowed a leading slash harmlessly, so the Electron
/// code this was ported from was safe and a direct port is not. This is not
/// hypothetical: the reference `docs/sample_distribution.json` writes File
/// module paths as `"/config/dsurround/dsurround.cfg"` and friends.
///
/// The distribution is fetched over the network, so treat it as untrusted
/// input. Without this check a hostile or merely careless index could place a
/// file anywhere the launcher process can write.
///
/// This is a **lexical** check: it rejects absolute paths, drive prefixes and
/// `..` traversal, then confirms the result is still under `base`. It does not
/// resolve symlinks, because the target does not exist yet at download time —
/// a pre-existing symlink inside the instance directory could still redirect a
/// write, and guarding that belongs to whatever creates the directories.
pub fn safe_join(base: &Path, rel: &str) -> Result<PathBuf> {
    let trimmed = rel.trim();
    if trimmed.is_empty() {
        return Err(Error::UnsafePath("empty path".into()));
    }

    // Backslashes never appear in the spec, and leaving them alone would let
    // `..\..\x` walk out on Windows while a '/'-only check waved it through.
    let normalised = trimmed.replace('\\', "/");

    if normalised.starts_with('/') {
        return Err(Error::UnsafePath(format!("absolute path: {rel}")));
    }

    // `C:/x` is absolute on Windows and an ordinary relative name everywhere
    // else. Reject it on every platform so one index cannot behave differently
    // per OS.
    let drive_qualified = normalised.split('/').next().is_some_and(|c| {
        let b = c.as_bytes();
        b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
    });
    if drive_qualified {
        return Err(Error::UnsafePath(format!("drive-qualified path: {rel}")));
    }

    let mut out = base.to_path_buf();
    for comp in Path::new(&normalised).components() {
        match comp {
            Component::Normal(c) => out.push(c),
            Component::CurDir => {}
            Component::ParentDir => {
                return Err(Error::UnsafePath(format!("`..` traversal: {rel}")));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(Error::UnsafePath(format!("absolute path: {rel}")));
            }
        }
    }

    // Belt and braces. The component walk above should make this unreachable,
    // but the cost of being wrong here is a write outside the instance.
    if !out.starts_with(base) {
        return Err(Error::UnsafePath(format!(
            "path escapes the base directory: {rel}"
        )));
    }

    Ok(out)
}

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

/// Where the rolling launcher log is written. Beside the config rather than in
/// the instance tree, because it describes the launcher rather than any one
/// server and must survive a server being removed.
pub fn log_directory() -> PathBuf {
    launcher_directory().join("logs")
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

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> PathBuf {
        PathBuf::from("/instances/survival")
    }

    #[test]
    fn ordinary_relative_paths_land_under_the_base() {
        assert_eq!(
            safe_join(&base(), "config/dsurround/dsurround.cfg").unwrap(),
            PathBuf::from("/instances/survival/config/dsurround/dsurround.cfg")
        );
        assert_eq!(
            safe_join(&base(), "./mods/thing.jar").unwrap(),
            PathBuf::from("/instances/survival/mods/thing.jar")
        );
    }

    #[test]
    fn leading_slash_is_refused_not_silently_absolute() {
        // The exact shape in docs/sample_distribution.json. PathBuf::join would
        // discard the base and hand back "/config/dsurround/dsurround.cfg".
        for rel in [
            "/config/dsurround/dsurround.cfg",
            "/config/dsurround/westeros.json",
            "/config/betterfoliage.cfg",
        ] {
            let err = safe_join(&base(), rel).unwrap_err();
            assert!(
                matches!(err, Error::UnsafePath(_)),
                "{rel} must be rejected, got {err:?}"
            );
        }
    }

    #[test]
    fn join_really_would_have_escaped() {
        // Guards the premise of this whole helper, so the test fails loudly if
        // std's behaviour ever changes underneath us.
        assert_eq!(
            base().join("/config/x.cfg"),
            PathBuf::from("/config/x.cfg"),
            "PathBuf::join still treats a leading slash as absolute"
        );
    }

    #[test]
    fn parent_traversal_is_refused() {
        for rel in [
            "../../etc/passwd",
            "config/../../../etc/passwd",
            "mods/../../outside.jar",
            "..",
        ] {
            assert!(
                safe_join(&base(), rel).is_err(),
                "{rel} must be rejected"
            );
        }
    }

    #[test]
    fn backslash_traversal_is_refused_on_every_platform() {
        // A '/'-only check would wave these through on a Unix build, and the
        // resulting index would then behave differently on Windows.
        for rel in ["..\\..\\windows\\system32\\x.dll", "config\\..\\..\\x.cfg"] {
            assert!(safe_join(&base(), rel).is_err(), "{rel} must be rejected");
        }
    }

    #[test]
    fn drive_qualified_paths_are_refused() {
        for rel in ["C:/Windows/System32/x.dll", "c:x.dll", "D:\\x.dll"] {
            assert!(safe_join(&base(), rel).is_err(), "{rel} must be rejected");
        }
    }

    #[test]
    fn empty_and_whitespace_paths_are_refused() {
        assert!(safe_join(&base(), "").is_err());
        assert!(safe_join(&base(), "   ").is_err());
    }

    #[test]
    fn a_name_containing_a_colon_later_is_still_allowed() {
        // Only a leading drive letter is suspicious; a colon inside a filename
        // is legal on Unix and appears in some maven-ish artifact names.
        assert!(safe_join(&base(), "mods/some:mod.jar").is_ok());
    }

    #[test]
    fn every_result_stays_inside_the_base() {
        for rel in [
            "a/b/c.jar",
            "./a/./b.jar",
            "deeply/nested/path/to/a/file.cfg",
        ] {
            let joined = safe_join(&base(), rel).unwrap();
            assert!(joined.starts_with(base()), "{rel} escaped to {joined:?}");
        }
    }
}
