//! Downloading the modules a distribution declares.
//!
//! Without this the launcher fetches Mojang's assets and the loader's
//! libraries, then starts a game with an empty `mods/` directory — which for
//! Fabric is nearly indistinguishable from vanilla at the main menu, so the
//! failure is silent.
//!
//! Two things here are easy to get wrong and invisible when wrong:
//!
//! * **Install path.** A mod written to the wrong directory exists on disk and
//!   is simply never loaded, presenting exactly like the bug this fixes.
//! * **Path safety.** `path` comes from a document fetched over the network.
//!   `PathBuf::join` discards the base on a leading slash, so an index could
//!   otherwise write anywhere the process can reach.

use std::path::{Path, PathBuf};

use crate::distribution::{Module, ModuleType};
use crate::dl::Asset;
use crate::error::Result;
use crate::paths::safe_join;

/// Where a module's file belongs, by type.
///
/// Mods go directly into the instance's `mods/`, which is what every loader
/// scans. `File` modules carry their own destination relative to the instance.
/// Libraries and manifests are shared, so they live under the common
/// directory.
fn destination(
    module: &Module,
    instance_dir: &Path,
    common_dir: &Path,
) -> Result<Option<PathBuf>> {
    // Resolved before the maven path, because a manifest id is not a
    // coordinate — `1.16.5-forge-36.2.34` has no group or artifact — and
    // deriving one would fail before this arm was ever reached.
    if module.module_type == ModuleType::VersionManifest {
        let id = &module.id;
        return Ok(Some(safe_join(
            &common_dir.join("versions"),
            &format!("{id}/{id}.json"),
        )?));
    }

    let rel = artifact_relative_path(module)?;

    Ok(match module.module_type {
        ModuleType::ForgeMod | ModuleType::FabricMod | ModuleType::LiteMod => {
            // Only the filename: loaders scan `mods/` non-recursively, so a
            // nested maven path would place the jar somewhere real that the
            // game never looks at.
            let name = Path::new(&rel)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or(rel);
            Some(safe_join(&instance_dir.join("mods"), &name)?)
        }
        ModuleType::File => Some(safe_join(instance_dir, &rel)?),
        // The ForgeHosted module is itself a maven artifact — the forge jar —
        // and its libraries sit beside it, so both resolve like any library.
        ModuleType::Library | ModuleType::ForgeHosted => {
            Some(safe_join(&common_dir.join("libraries"), &rel)?)
        }
        ModuleType::VersionManifest => unreachable!("handled above"),
        // Resolved from their own metadata by the loader path, not shipped as
        // artifacts here.
        ModuleType::Forge | ModuleType::Fabric | ModuleType::LiteLoader | ModuleType::Unknown => {
            None
        }
    })
}

/// The artifact's path within its base directory.
///
/// An explicit `path` wins. Otherwise it is derived from the maven-style id,
/// which is how the spec addresses artifacts that carry no path.
fn artifact_relative_path(module: &Module) -> Result<String> {
    if let Some(p) = &module.artifact.path {
        if !p.trim().is_empty() {
            return Ok(p.clone());
        }
    }
    maven_path(&module.id, default_extension(module.module_type))
}

fn default_extension(kind: ModuleType) -> &'static str {
    match kind {
        ModuleType::LiteMod => "litemod",
        _ => "jar",
    }
}

/// `group:artifact:version[:classifier][@ext]` -> repository path.
fn maven_path(id: &str, default_ext: &str) -> Result<String> {
    let (coords, ext) = match id.split_once('@') {
        Some((c, e)) => (c, e),
        None => (id, default_ext),
    };
    let parts: Vec<&str> = coords.split(':').collect();
    if parts.len() < 3 {
        return Err(crate::error::Error::Other(format!(
            "module id {id:?} is not a maven coordinate and carries no path"
        )));
    }
    let (group, artifact, version) = (parts[0], parts[1], parts[2]);
    let classifier = parts.get(3).map(|c| format!("-{c}")).unwrap_or_default();
    Ok(format!(
        "{}/{artifact}/{version}/{artifact}-{version}{classifier}.{ext}",
        group.replace('.', "/")
    ))
}

/// Walk the module tree, collecting what must be on disk.
///
/// A child is only included when its parent is included: that gating is the
/// whole point of nesting, and installing a disabled mod's dependencies would
/// defeat the toggle.
pub fn collect_downloads(
    modules: &[Module],
    saved: &std::collections::HashMap<String, bool>,
    instance_dir: &Path,
    common_dir: &Path,
    depth: usize,
    out: &mut Vec<Asset>,
) {
    // Matches the listing traversal's gate; a cyclic or absurdly deep index
    // must not hang the launch.
    const MAX_DEPTH: usize = 8;
    if depth > MAX_DEPTH {
        tracing::warn!(depth, "module tree deeper than expected; stopping");
        return;
    }

    for m in modules {
        let required = m.required.as_ref().map(|r| r.value()).unwrap_or(true);
        let default_on = m.required.as_ref().map(|r| r.default_on()).unwrap_or(true);
        let enabled = required || saved.get(&m.id).copied().unwrap_or(default_on);

        if !enabled {
            continue;
        }

        match destination(m, instance_dir, common_dir) {
            Ok(Some(path)) => out.push(Asset {
                id: m.id.clone(),
                // The spec field is `MD5`, but an index may carry a SHA1 there
                // in practice; the validator picks by digest length rather
                // than trusting the name.
                hash: m.artifact.md5.clone().unwrap_or_default(),
                size: m.artifact.size,
                url: m.artifact.url.clone(),
                path,
            }),
            Ok(None) => {}
            Err(err) => {
                // Refusing one bad module beats aborting the whole launch, but
                // it must be loud: the game will start without it.
                tracing::error!(module = %m.id, %err, "skipping module with an unusable path");
            }
        }

        collect_downloads(&m.sub_modules, saved, instance_dir, common_dir, depth + 1, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::distribution::{Artifact, RequiredSpec};

    fn module(id: &str, kind: ModuleType, path: Option<&str>, req: Option<(bool, bool)>) -> Module {
        Module {
            id: id.into(),
            name: id.into(),
            module_type: kind,
            classpath: None,
            required: req.map(|(value, def)| RequiredSpec {
                value: Some(value),
                def: Some(def),
            }),
            artifact: Artifact {
                size: 10,
                md5: Some("d41d8cd98f00b204e9800998ecf8427e".into()),
                url: "https://cdn.test/a.jar".into(),
                path: path.map(str::to_string),
            },
            sub_modules: Vec::new(),
        }
    }

    fn dirs() -> (PathBuf, PathBuf) {
        (PathBuf::from("/data/instances/srv"), PathBuf::from("/data/common"))
    }

    #[test]
    fn mods_land_flat_in_the_instance_mods_directory() {
        // The trap this guards: a synthetic coordinate with no path would
        // otherwise nest the jar under mods/<group>/<artifact>/…, where it
        // exists on disk and is never loaded.
        let (inst, common) = dirs();
        let m = module("mrpack.mods-voidz-1.0.11.jar:voidz-1.0.11:v1", ModuleType::FabricMod, None, None);
        let dest = destination(&m, &inst, &common).unwrap().unwrap();
        assert_eq!(dest.parent().unwrap(), inst.join("mods"), "must be flat: {dest:?}");
        assert!(dest.to_string_lossy().ends_with(".jar"));
    }

    #[test]
    fn an_explicit_path_is_honoured_for_file_modules() {
        let (inst, common) = dirs();
        let m = module("cfg:betterfoliage:1", ModuleType::File, Some("config/betterfoliage.cfg"), None);
        assert_eq!(
            destination(&m, &inst, &common).unwrap().unwrap(),
            inst.join("config/betterfoliage.cfg")
        );
    }

    #[test]
    fn an_absolute_path_from_the_index_is_refused() {
        // The reference sample really ships these.
        let (inst, common) = dirs();
        let m = module("cfg:x:1", ModuleType::File, Some("/config/dsurround.cfg"), None);
        assert!(destination(&m, &inst, &common).is_err());
    }

    #[test]
    fn traversal_out_of_the_instance_is_refused() {
        let (inst, common) = dirs();
        let m = module("cfg:x:1", ModuleType::File, Some("../../evil.sh"), None);
        assert!(destination(&m, &inst, &common).is_err());
    }

    #[test]
    fn libraries_go_to_the_shared_directory_not_the_instance() {
        let (inst, common) = dirs();
        let m = module("org.ow2.asm:asm:9.6", ModuleType::Library, None, None);
        let dest = destination(&m, &inst, &common).unwrap().unwrap();
        assert_eq!(dest, common.join("libraries/org/ow2/asm/asm/9.6/asm-9.6.jar"));
    }

    #[test]
    fn loaders_resolved_from_their_own_metadata_are_not_downloaded_here() {
        let (inst, common) = dirs();
        for kind in [ModuleType::Fabric, ModuleType::Forge, ModuleType::LiteLoader] {
            let m = module("net.fabricmc:fabric-loader:0.19.3", kind, None, None);
            assert!(destination(&m, &inst, &common).unwrap().is_none(), "{kind:?}");
        }
    }

    /// ForgeHosted is the opposite case: the distribution ships the installer's
    /// output, so its artifact and its version JSON are real files that must
    /// land on disk before the loader path can read them.
    #[test]
    fn forgehosted_artifacts_are_downloaded() {
        let (inst, common) = dirs();
        let m = module(
            "net.minecraftforge:forge:1.16.5-36.2.34",
            ModuleType::ForgeHosted,
            None,
            None,
        );
        assert_eq!(
            destination(&m, &inst, &common).unwrap().unwrap(),
            common.join("libraries/net/minecraftforge/forge/1.16.5-36.2.34/forge-1.16.5-36.2.34.jar")
        );
    }

    /// The version manifest is keyed by id, not by maven coordinate — its id is
    /// `1.16.5-forge-36.2.34`, which is not a coordinate at all.
    #[test]
    fn the_version_manifest_lands_where_the_loader_path_looks_for_it() {
        let (inst, common) = dirs();
        let m = module("1.16.5-forge-36.2.34", ModuleType::VersionManifest, None, None);
        assert_eq!(
            destination(&m, &inst, &common).unwrap().unwrap(),
            common.join("versions/1.16.5-forge-36.2.34/1.16.5-forge-36.2.34.json")
        );
    }

    #[test]
    fn litemods_keep_their_extension() {
        assert_eq!(
            maven_path("com.mumfrey:liteloader:1.12", "litemod").unwrap(),
            "com/mumfrey/liteloader/1.12/liteloader-1.12.litemod"
        );
    }

    #[test]
    fn an_explicit_extension_overrides_the_default() {
        assert_eq!(
            maven_path("org.scala-lang:scala-library:2.11.1@jar.pack.xz", "jar").unwrap(),
            "org/scala-lang/scala-library/2.11.1/scala-library-2.11.1.jar.pack.xz"
        );
    }

    #[test]
    fn a_disabled_optional_mod_is_not_downloaded() {
        let (inst, common) = dirs();
        let mods = vec![
            module("a:optional:1", ModuleType::FabricMod, None, Some((false, true))),
            module("a:required:1", ModuleType::FabricMod, None, None),
        ];
        let mut saved = std::collections::HashMap::new();
        saved.insert("a:optional:1".to_string(), false);

        let mut out = Vec::new();
        collect_downloads(&mods, &saved, &inst, &common, 0, &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, "a:required:1");
    }

    #[test]
    fn children_of_a_disabled_parent_are_skipped() {
        // Installing a disabled mod's dependencies would defeat the toggle.
        let (inst, common) = dirs();
        let mut parent = module("a:parent:1", ModuleType::FabricMod, None, Some((false, true)));
        parent.sub_modules = vec![module("a:child:1", ModuleType::File, Some("config/c.cfg"), None)];
        let mut saved = std::collections::HashMap::new();
        saved.insert("a:parent:1".to_string(), false);

        let mut out = Vec::new();
        collect_downloads(&[parent], &saved, &inst, &common, 0, &mut out);
        assert!(out.is_empty(), "{out:?}");
    }

    #[test]
    fn children_of_an_enabled_parent_are_included() {
        let (inst, common) = dirs();
        let mut parent = module("a:parent:1", ModuleType::FabricMod, None, None);
        parent.sub_modules = vec![module("a:child:1", ModuleType::File, Some("config/c.cfg"), None)];
        let mut out = Vec::new();
        collect_downloads(&[parent], &std::collections::HashMap::new(), &inst, &common, 0, &mut out);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn a_module_with_an_unusable_path_is_skipped_not_fatal() {
        let (inst, common) = dirs();
        let bad = module("not-a-coordinate", ModuleType::FabricMod, None, None);
        let good = module("a:good:1", ModuleType::FabricMod, None, None);
        let mut out = Vec::new();
        collect_downloads(&[bad, good], &std::collections::HashMap::new(), &inst, &common, 0, &mut out);
        assert_eq!(out.len(), 1, "one bad module must not abort the launch");
    }
}
