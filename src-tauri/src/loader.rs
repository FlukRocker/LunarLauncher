//! Mod loader support.
//!
//! A loader contributes three things to a launch: extra libraries on the
//! classpath, a different `mainClass`, and sometimes extra JVM or game
//! arguments. This module resolves those from each loader's own metadata and
//! hands back a `LoaderProfile` that `process_builder` merges over the vanilla
//! version JSON.
//!
//! Fabric and Forge differ enormously in what that costs:
//!
//! * **Fabric** publishes a ready-made profile. One request returns the
//!   libraries and main class, and every library is a plain maven artifact.
//!   Nothing has to be built locally.
//! * **Forge** (1.13+) ships an *installer* that must be executed to produce a
//!   patched client jar, running processors (binarypatcher, SpecialSource,
//!   ForgeAutoRenamingTool) against the vanilla jar. That pipeline is a
//!   separate, much larger piece of work; see `resolve_forge`.
//!
//! Fabric is implemented first because it proves the merge path end to end
//! without the installer pipeline in the way.

use serde::Deserialize;

use crate::error::{Error, Result};

pub const FABRIC_META: &str = "https://meta.fabricmc.net/v2";

/// What a loader adds to the launch.
#[derive(Debug, Clone, PartialEq)]
pub struct LoaderProfile {
    /// Replaces the vanilla main class.
    pub main_class: String,
    /// Maven coordinates, resolved to paths and appended to the classpath.
    /// Order matters: the loader must precede the vanilla jar so its classes
    /// win.
    pub libraries: Vec<Library>,
    /// Extra game arguments, appended after the vanilla ones.
    pub game_args: Vec<String>,
    /// Extra JVM arguments.
    pub jvm_args: Vec<String>,
    /// Minimum Java major the loader itself needs, which can exceed the game's.
    pub min_java: Option<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Library {
    /// `group:artifact:version`
    pub name: String,
    /// Base to download from; the maven path is appended.
    pub repo_url: String,
}

impl Library {
    /// `net.fabricmc:fabric-loader:0.19.3` ->
    /// `net/fabricmc/fabric-loader/0.19.3/fabric-loader-0.19.3.jar`
    ///
    /// Maven coordinates may carry a classifier as a fourth segment, which
    /// becomes a filename suffix rather than a path segment — getting that
    /// wrong yields a 404 that looks like a missing library.
    pub fn maven_path(&self) -> Result<String> {
        let parts: Vec<&str> = self.name.split(':').collect();
        if parts.len() < 3 {
            return Err(Error::Other(format!(
                "malformed maven coordinate: {}",
                self.name
            )));
        }
        let (group, artifact, version) = (parts[0], parts[1], parts[2]);
        let classifier = parts.get(3).map(|c| format!("-{c}")).unwrap_or_default();
        Ok(format!(
            "{}/{artifact}/{version}/{artifact}-{version}{classifier}.jar",
            group.replace('.', "/")
        ))
    }

    pub fn download_url(&self) -> Result<String> {
        Ok(format!(
            "{}/{}",
            self.repo_url.trim_end_matches('/'),
            self.maven_path()?
        ))
    }
}

// ---------------------------------------------------------------------------
// Fabric
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct FabricEntry {
    loader: FabricLoader,
    #[serde(rename = "launcherMeta")]
    launcher_meta: FabricLauncherMeta,
}

#[derive(Debug, Deserialize)]
struct FabricLoader {
    version: String,
    #[serde(default)]
    stable: bool,
}

#[derive(Debug, Deserialize)]
struct FabricLauncherMeta {
    #[serde(rename = "mainClass")]
    main_class: FabricMainClass,
    libraries: FabricLibraries,
    #[serde(default, rename = "min_java_version")]
    min_java_version: Option<u32>,
}

/// `mainClass` is an object in v2 metadata but was a bare string in v1, and
/// some mirrors still serve the old shape.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum FabricMainClass {
    Split { client: String },
    Flat(String),
}

impl FabricMainClass {
    fn client(&self) -> String {
        match self {
            FabricMainClass::Split { client } => client.clone(),
            FabricMainClass::Flat(s) => s.clone(),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
struct FabricLibraries {
    #[serde(default)]
    client: Vec<FabricLibrary>,
    #[serde(default)]
    common: Vec<FabricLibrary>,
}

#[derive(Debug, Deserialize)]
struct FabricLibrary {
    name: String,
    #[serde(default)]
    url: Option<String>,
}

const FABRIC_MAVEN: &str = "https://maven.fabricmc.net/";

/// Build a profile from Fabric's metadata for a game version.
///
/// `loader_version` empty selects the newest stable release, which is what a
/// distribution that pins only "Fabric" should get.
pub fn profile_from_fabric_json(json: &str, loader_version: &str) -> Result<LoaderProfile> {
    let entries: Vec<FabricEntry> = serde_json::from_str(json)?;

    let entry = if loader_version.trim().is_empty() {
        entries
            .iter()
            .find(|e| e.loader.stable)
            // Fall back to the newest of any stability rather than failing: a
            // brand-new game version may have no stable loader yet.
            .or_else(|| entries.first())
    } else {
        entries.iter().find(|e| e.loader.version == loader_version)
    }
    .ok_or_else(|| {
        Error::Other(if loader_version.is_empty() {
            "Fabric has no loader for this Minecraft version.".into()
        } else {
            format!("Fabric loader {loader_version} is not available for this Minecraft version.")
        })
    })?;

    let meta = &entry.launcher_meta;
    let mut libraries = Vec::new();
    for lib in meta.libraries.common.iter().chain(meta.libraries.client.iter()) {
        libraries.push(Library {
            name: lib.name.clone(),
            repo_url: lib.url.clone().unwrap_or_else(|| FABRIC_MAVEN.to_string()),
        });
    }

    // The loader and intermediary jars are not in the libraries list; they are
    // derived from the entry itself.
    libraries.push(Library {
        name: format!("net.fabricmc:fabric-loader:{}", entry.loader.version),
        repo_url: FABRIC_MAVEN.into(),
    });

    Ok(LoaderProfile {
        main_class: meta.main_class.client(),
        libraries,
        game_args: Vec::new(),
        jvm_args: Vec::new(),
        min_java: meta.min_java_version,
    })
}

/// Fetch Fabric metadata and resolve a profile.
pub async fn resolve_fabric(
    client: &reqwest::Client,
    game_version: &str,
    loader_version: &str,
) -> Result<LoaderProfile> {
    let url = format!("{FABRIC_META}/versions/loader/{game_version}");
    let body = client
        .get(&url)
        .send()
        .await?
        .error_for_status()
        .map_err(|e| {
            Error::Other(format!(
                "Fabric has no metadata for Minecraft {game_version} ({e})"
            ))
        })?
        .text()
        .await?;
    profile_from_fabric_json(&body, loader_version)
}

// ---------------------------------------------------------------------------
// Forge
// ---------------------------------------------------------------------------

/// Forge is not implemented.
///
/// This returns a clear error rather than a partial launch. Modern Forge
/// cannot be resolved from metadata alone: the installer must run locally,
/// applying binary patches to the vanilla jar and executing processors to
/// produce the client it then launches. Half of that pipeline would produce a
/// game that starts and then fails deep inside itself, which is worse than
/// refusing.
pub fn resolve_forge(_game_version: &str, _loader_version: &str) -> Result<LoaderProfile> {
    Err(Error::Other(
        "Forge support is not implemented yet. Unlike Fabric, Forge ships an installer \
         that has to run locally — patching the vanilla jar and executing its processors — \
         before the game can start. Fabric servers work today."
            .into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"[
      { "loader": { "version": "0.19.3", "stable": true },
        "intermediary": { "maven": "net.fabricmc:intermediary:1.20.1" },
        "launcherMeta": { "version": 2, "min_java_version": 17,
          "mainClass": { "client": "net.fabricmc.loader.impl.launch.knot.KnotClient",
                         "server": "net.fabricmc.loader.impl.launch.knot.KnotServer" },
          "libraries": {
            "common": [ { "name": "org.ow2.asm:asm:9.6", "url": "https://maven.fabricmc.net/" } ],
            "client": [ { "name": "net.fabricmc:tiny-mappings-parser:0.3.0" } ] } } },
      { "loader": { "version": "0.20.0-beta.1", "stable": false },
        "intermediary": { "maven": "net.fabricmc:intermediary:1.20.1" },
        "launcherMeta": { "version": 2,
          "mainClass": { "client": "net.fabricmc.loader.impl.launch.knot.KnotClient" },
          "libraries": { "common": [], "client": [] } } }
    ]"#;

    #[test]
    fn maven_coordinates_become_repository_paths() {
        let l = Library { name: "org.ow2.asm:asm:9.6".into(), repo_url: "https://m/".into() };
        assert_eq!(l.maven_path().unwrap(), "org/ow2/asm/asm/9.6/asm-9.6.jar");
        assert_eq!(l.download_url().unwrap(), "https://m/org/ow2/asm/asm/9.6/asm-9.6.jar");
    }

    #[test]
    fn a_classifier_suffixes_the_filename_rather_than_the_path() {
        // Getting this wrong yields a 404 that reads as a missing library.
        let l = Library {
            name: "net.minecraft:client:1.20.1:mapped".into(),
            repo_url: "https://m".into(),
        };
        assert_eq!(
            l.maven_path().unwrap(),
            "net/minecraft/client/1.20.1/client-1.20.1-mapped.jar"
        );
    }

    #[test]
    fn malformed_coordinates_are_rejected() {
        let l = Library { name: "not-a-coordinate".into(), repo_url: "https://m".into() };
        assert!(l.maven_path().is_err());
    }

    #[test]
    fn empty_loader_version_selects_the_newest_stable() {
        let p = profile_from_fabric_json(SAMPLE, "").unwrap();
        assert!(p.libraries.iter().any(|l| l.name.contains("fabric-loader:0.19.3")),
            "must pick the stable release, not the beta: {:?}", p.libraries);
        assert_eq!(p.main_class, "net.fabricmc.loader.impl.launch.knot.KnotClient");
        assert_eq!(p.min_java, Some(17));
    }

    #[test]
    fn an_explicit_loader_version_is_honoured_even_when_unstable() {
        let p = profile_from_fabric_json(SAMPLE, "0.20.0-beta.1").unwrap();
        assert!(p.libraries.iter().any(|l| l.name.contains("0.20.0-beta.1")));
    }

    #[test]
    fn an_unavailable_loader_version_names_itself_in_the_error() {
        let err = profile_from_fabric_json(SAMPLE, "9.9.9").unwrap_err().to_string();
        assert!(err.contains("9.9.9"), "{err}");
    }

    #[test]
    fn libraries_default_to_the_fabric_maven_when_no_url_is_given() {
        let p = profile_from_fabric_json(SAMPLE, "").unwrap();
        let lib = p.libraries.iter().find(|l| l.name.contains("tiny-mappings")).unwrap();
        assert_eq!(lib.repo_url, FABRIC_MAVEN);
    }

    #[test]
    fn client_and_common_libraries_are_both_included() {
        let p = profile_from_fabric_json(SAMPLE, "").unwrap();
        assert!(p.libraries.iter().any(|l| l.name.starts_with("org.ow2.asm")), "common");
        assert!(p.libraries.iter().any(|l| l.name.starts_with("net.fabricmc:tiny")), "client");
    }

    #[test]
    fn a_flat_main_class_from_older_metadata_still_parses() {
        let old = r#"[{ "loader": { "version": "0.14.0", "stable": true },
          "launcherMeta": { "version": 1, "mainClass": "net.fabricmc.loader.launch.knot.KnotClient",
            "libraries": { "common": [] } } }]"#;
        let p = profile_from_fabric_json(old, "").unwrap();
        assert_eq!(p.main_class, "net.fabricmc.loader.launch.knot.KnotClient");
    }

    #[test]
    fn forge_refuses_clearly_rather_than_half_working() {
        let err = resolve_forge("1.20.1", "47.2.0").unwrap_err().to_string();
        assert!(err.contains("installer"), "explains why, not just that: {err}");
        assert!(err.contains("Fabric servers work today"));
    }

    #[tokio::test]
    #[ignore]
    async fn resolves_against_the_live_fabric_meta() {
        let c = reqwest::Client::new();
        let p = resolve_fabric(&c, "1.20.1", "").await.unwrap();
        println!("mainClass {} · {} libraries · java {:?}",
                 p.main_class, p.libraries.len(), p.min_java);
        assert!(p.main_class.contains("Knot"));
        assert!(p.libraries.len() > 3);
        for l in &p.libraries {
            l.download_url().expect("every library resolves to a url");
        }
    }
}
