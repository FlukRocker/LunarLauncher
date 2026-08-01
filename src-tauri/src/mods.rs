//! Port of `dropinmodutil.js` — managing mods the user drops into an
//! instance's `mods` directory by hand, plus the shaderpack helpers.
//!
//! The enable/disable mechanism is the same one the Electron build used and
//! that mod loaders themselves understand: a disabled mod keeps its jar but
//! gains a `.disabled` suffix, so nothing is ever deleted to turn a mod off.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

const DISABLED_EXT: &str = ".disabled";
const SHADER_DIR: &str = "shaderpacks";
const SHADER_CONFIG: &str = "optionsshaders.txt";

/// Recognised mod containers. Matches the JS `MOD_REGEX`.
fn parse_mod_name(file: &str) -> Option<(String, String, bool)> {
    let (stem, disabled) = match file.strip_suffix(DISABLED_EXT) {
        Some(s) => (s, true),
        None => (file, false),
    };
    let ext = stem.rsplit_once('.')?.1.to_ascii_lowercase();
    if !matches!(ext.as_str(), "jar" | "zip" | "litemod") {
        return None;
    }
    Some((stem.to_string(), ext, disabled))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DropinMod {
    /// Path relative to the mods directory, including any version subfolder
    /// and the `.disabled` suffix. This is the handle for toggle/delete.
    pub full_name: String,
    /// Display name, without the `.disabled` suffix.
    pub name: String,
    pub ext: String,
    pub disabled: bool,
}

pub fn mods_dir(instance_dir: &Path) -> PathBuf {
    instance_dir.join("mods")
}

/// Version-specific mods live in `mods/<version>`, which some loaders read in
/// addition to the top-level directory.
fn version_dir(instance_dir: &Path, version: &str) -> PathBuf {
    mods_dir(instance_dir).join(version)
}

/// `scanForDropinMods`. Scans the mods directory and its version subfolder.
pub fn scan(instance_dir: &Path, version: &str) -> Vec<DropinMod> {
    let mut found = Vec::new();

    let mut collect = |dir: PathBuf, prefix: Option<&str>| {
        let Ok(entries) = std::fs::read_dir(&dir) else { return };
        for entry in entries.flatten() {
            let file = entry.file_name().to_string_lossy().to_string();
            let Some((name, ext, disabled)) = parse_mod_name(&file) else { continue };
            let full_name = match prefix {
                Some(p) => format!("{p}/{file}"),
                None => file.clone(),
            };
            found.push(DropinMod { full_name, name, ext, disabled });
        }
    };

    collect(mods_dir(instance_dir), None);
    collect(version_dir(instance_dir, version), Some(version));

    found.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    found
}

/// Reject a handle that could escape the mods directory. `full_name` comes
/// from the frontend, so it is treated as untrusted.
fn resolve_in_mods(instance_dir: &Path, full_name: &str) -> Result<PathBuf> {
    if full_name.contains("..") || full_name.starts_with('/') || full_name.contains('\\') {
        return Err(Error::Other(format!("Invalid mod path: {full_name}")));
    }
    let base = mods_dir(instance_dir);
    let candidate = base.join(full_name);
    // Only allow single-level nesting (the version subfolder).
    if candidate.components().count() > base.components().count() + 2 {
        return Err(Error::Other(format!("Invalid mod path: {full_name}")));
    }
    Ok(candidate)
}

/// `toggleDropinMod` — rename to add or drop the `.disabled` suffix.
pub fn toggle(instance_dir: &Path, full_name: &str, enable: bool) -> Result<String> {
    let from = resolve_in_mods(instance_dir, full_name)?;
    if !from.exists() {
        return Err(Error::Other(format!("Mod not found: {full_name}")));
    }

    let new_full = if enable {
        full_name
            .strip_suffix(DISABLED_EXT)
            .unwrap_or(full_name)
            .to_string()
    } else if full_name.ends_with(DISABLED_EXT) {
        full_name.to_string()
    } else {
        format!("{full_name}{DISABLED_EXT}")
    };

    if new_full == full_name {
        return Ok(new_full); // already in the requested state
    }

    let to = resolve_in_mods(instance_dir, &new_full)?;
    std::fs::rename(&from, &to)?;
    tracing::info!(mod_name = %full_name, enable, "Toggled drop-in mod");
    Ok(new_full)
}

/// `deleteDropinMod`. The Electron build moved the file to the OS trash so a
/// mistake was recoverable; that is preserved here rather than unlinking.
pub fn delete(instance_dir: &Path, full_name: &str) -> Result<()> {
    let path = resolve_in_mods(instance_dir, full_name)?;
    if !path.exists() {
        return Err(Error::Other(format!("Mod not found: {full_name}")));
    }
    match trash::delete(&path) {
        Ok(()) => {
            tracing::info!(mod_name = %full_name, "Moved drop-in mod to trash");
            Ok(())
        }
        Err(err) => Err(Error::Other(format!(
            "Could not move {full_name} to the trash: {err}"
        ))),
    }
}

/// `addDropinMods` — copy chosen files in, skipping anything that is not a
/// recognised mod container.
pub fn add(instance_dir: &Path, files: &[PathBuf]) -> Result<usize> {
    let dir = mods_dir(instance_dir);
    std::fs::create_dir_all(&dir)?;

    let mut added = 0;
    for src in files {
        let Some(name) = src.file_name().map(|n| n.to_string_lossy().to_string()) else {
            continue;
        };
        if parse_mod_name(&name).is_none() {
            tracing::warn!(file = %name, "Skipping unrecognised mod file");
            continue;
        }
        std::fs::copy(src, dir.join(&name))?;
        added += 1;
    }
    Ok(added)
}

// ---------------------------------------------------------------------------
// Shaderpacks
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct Shaderpack {
    pub fullname: String,
    pub name: String,
}

pub fn scan_shaderpacks(instance_dir: &Path) -> Vec<Shaderpack> {
    let dir = instance_dir.join(SHADER_DIR);
    let mut out = vec![Shaderpack {
        fullname: "OFF".into(),
        name: "Off (Default)".into(),
    }];
    let Ok(entries) = std::fs::read_dir(&dir) else { return out };
    for e in entries.flatten() {
        let f = e.file_name().to_string_lossy().to_string();
        if let Some(stem) = f.strip_suffix(".zip") {
            out.push(Shaderpack { fullname: f.clone(), name: stem.to_string() });
        }
    }
    out
}

/// Reads the selected pack out of `optionsshaders.txt`.
pub fn enabled_shaderpack(instance_dir: &Path) -> String {
    let path = instance_dir.join(SHADER_CONFIG);
    let Ok(buf) = std::fs::read_to_string(&path) else { return "OFF".into() };
    for line in buf.lines() {
        if let Some(v) = line.trim().strip_prefix("shaderPack=") {
            return v.trim().to_string();
        }
    }
    "OFF".into()
}

pub fn set_enabled_shaderpack(instance_dir: &Path, pack: &str) -> Result<()> {
    std::fs::create_dir_all(instance_dir)?;
    let path = instance_dir.join(SHADER_CONFIG);
    let existing = std::fs::read_to_string(&path).unwrap_or_default();

    let mut lines: Vec<String> = existing.lines().map(str::to_string).collect();
    let mut replaced = false;
    for line in lines.iter_mut() {
        if line.trim_start().starts_with("shaderPack=") {
            *line = format!("shaderPack={pack}");
            replaced = true;
        }
    }
    if !replaced {
        lines.push(format!("shaderPack={pack}"));
    }
    std::fs::write(&path, lines.join("\n") + "\n")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_mod_containers() {
        assert_eq!(
            parse_mod_name("JEI-1.20.1.jar"),
            Some(("JEI-1.20.1.jar".into(), "jar".into(), false))
        );
        assert_eq!(
            parse_mod_name("JEI-1.20.1.jar.disabled"),
            Some(("JEI-1.20.1.jar".into(), "jar".into(), true))
        );
        assert_eq!(
            parse_mod_name("pack.zip"),
            Some(("pack.zip".into(), "zip".into(), false))
        );
        assert_eq!(
            parse_mod_name("old.litemod"),
            Some(("old.litemod".into(), "litemod".into(), false))
        );
        assert!(parse_mod_name("notes.txt").is_none());
        assert!(parse_mod_name("README").is_none());
    }

    #[test]
    fn path_traversal_is_rejected() {
        let inst = Path::new("/data/instances/Test");
        assert!(resolve_in_mods(inst, "../../etc/passwd").is_err());
        assert!(resolve_in_mods(inst, "/etc/passwd").is_err());
        assert!(resolve_in_mods(inst, "a/b/c/deep.jar").is_err());
        assert!(resolve_in_mods(inst, "JEI.jar").is_ok());
        assert!(resolve_in_mods(inst, "1.20.1/JEI.jar").is_ok());
    }

    #[test]
    fn scan_and_toggle_round_trip() {
        let inst = std::env::temp_dir().join("lunar-mods-test");
        let _ = std::fs::remove_dir_all(&inst);
        let md = mods_dir(&inst);
        std::fs::create_dir_all(md.join("1.20.1")).unwrap();
        std::fs::write(md.join("JEI.jar"), b"x").unwrap();
        std::fs::write(md.join("Old.jar.disabled"), b"x").unwrap();
        std::fs::write(md.join("notes.txt"), b"x").unwrap();
        std::fs::write(md.join("1.20.1").join("Versioned.jar"), b"x").unwrap();

        let found = scan(&inst, "1.20.1");
        assert_eq!(found.len(), 3, "non-mod files must be ignored: {found:?}");
        assert!(found.iter().any(|m| m.full_name == "1.20.1/Versioned.jar"));
        let old = found.iter().find(|m| m.name == "Old.jar").unwrap();
        assert!(old.disabled);

        // Disable, then re-enable, and confirm the file follows.
        let new_name = toggle(&inst, "JEI.jar", false).unwrap();
        assert_eq!(new_name, "JEI.jar.disabled");
        assert!(md.join("JEI.jar.disabled").exists());
        assert!(!md.join("JEI.jar").exists());

        let back = toggle(&inst, "JEI.jar.disabled", true).unwrap();
        assert_eq!(back, "JEI.jar");
        assert!(md.join("JEI.jar").exists());

        let _ = std::fs::remove_dir_all(&inst);
    }

    #[test]
    fn shaderpack_config_round_trip() {
        let inst = std::env::temp_dir().join("lunar-shader-test");
        let _ = std::fs::remove_dir_all(&inst);
        std::fs::create_dir_all(inst.join(SHADER_DIR)).unwrap();
        std::fs::write(inst.join(SHADER_DIR).join("BSL.zip"), b"x").unwrap();

        assert_eq!(enabled_shaderpack(&inst), "OFF");
        let packs = scan_shaderpacks(&inst);
        assert_eq!(packs.len(), 2);
        assert_eq!(packs[0].fullname, "OFF");

        set_enabled_shaderpack(&inst, "BSL.zip").unwrap();
        assert_eq!(enabled_shaderpack(&inst), "BSL.zip");

        // Setting again must replace, not append a second line.
        set_enabled_shaderpack(&inst, "OFF").unwrap();
        assert_eq!(enabled_shaderpack(&inst), "OFF");
        let raw = std::fs::read_to_string(inst.join(SHADER_CONFIG)).unwrap();
        assert_eq!(raw.matches("shaderPack=").count(), 1);

        let _ = std::fs::remove_dir_all(&inst);
    }
}
