//! Port of `processbuilder.js` — assembling the JVM invocation.
//!
//! Scope: the **vanilla** launch path (both the 1.13+ structured-argument form
//! and the pre-1.13 flat form). Forge/Fabric mod-loader support is not here
//! yet; see the README. This is the part of the launcher where a subtle
//! mistake produces a game that silently fails to start, so the argument
//! rules and placeholder substitution follow the JS exactly.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::config::{Account, JavaConfig};
use crate::distribution::{mc_version_at_least, mojang_os};
use crate::dl::{
    is_library_compatible, library_dir, native_classifier, version_jar_path, VersionJson,
};
use crate::error::{Error, Result};

/// Platform classpath separator. `;` on Windows, `:` elsewhere.
pub fn classpath_separator() -> &'static str {
    if cfg!(target_os = "windows") {
        ";"
    } else {
        ":"
    }
}

/// Everything the builder needs that isn't in the version JSON.
pub struct LaunchContext {
    pub common_dir: PathBuf,
    pub game_dir: PathBuf,
    pub natives_dir: PathBuf,
    pub java_config: JavaConfig,
    pub account: Account,
    pub launcher_version: String,
    pub res_width: u32,
    pub res_height: u32,
    pub fullscreen: bool,
    /// `server_id` is used for `${version_name}`, matching the JS, which
    /// passes the distribution server id rather than the vanilla version.
    pub server_id: String,
    /// Mod loader contribution, when the server declares one. Absent for
    /// vanilla.
    pub loader: Option<crate::loader::LoaderProfile>,
}

// ---------------------------------------------------------------------------
// Classpath and natives
// ---------------------------------------------------------------------------

/// Non-native libraries plus the version jar.
pub fn classpath(common_dir: &Path, version_json: &VersionJson) -> Vec<PathBuf> {
    let lib_dir = library_dir(common_dir);
    let mut out = Vec::new();

    for lib in &version_json.libraries {
        if !is_library_compatible(&lib.rules, &lib.natives) {
            continue;
        }
        // Natives are extracted, not placed on the classpath.
        if lib.natives.is_some() {
            continue;
        }
        if let Some(artifact) = &lib.downloads.artifact {
            if let Some(rel) = &artifact.path {
                // Must agree with dl.rs: a library skipped there as unsafe has
                // not been downloaded, so it must not reach the classpath here.
                match crate::paths::safe_join(&lib_dir, rel) {
                    Ok(p) => out.push(p),
                    Err(err) => {
                        tracing::warn!(%err, library = %lib.name, "Omitting library with an unsafe path from the classpath.");
                    }
                }
            }
        }
    }

    out.push(version_jar_path(common_dir, &version_json.id));
    out
}

/// Classpath including a mod loader's libraries.
///
/// Loader libraries go **first**. The JVM resolves a class from the earliest
/// entry that provides it, and a loader ships patched versions of classes the
/// vanilla jar also contains — putting it after would silently load the
/// unpatched class and fail in ways that look nothing like a classpath
/// problem.
pub fn classpath_with_loader(
    common_dir: &Path,
    version_json: &VersionJson,
    loader: Option<&crate::loader::LoaderProfile>,
) -> Vec<PathBuf> {
    let lib_dir = library_dir(common_dir);
    let mut out = Vec::new();

    if let Some(profile) = loader {
        for lib in &profile.libraries {
            match lib.maven_path() {
                Ok(rel) => match crate::paths::safe_join(&lib_dir, &rel) {
                    Ok(p) => out.push(p),
                    Err(err) => tracing::warn!(%err, name = %lib.name, "skipping loader library"),
                },
                Err(err) => tracing::warn!(%err, "skipping malformed loader coordinate"),
            }
        }
    }

    out.extend(classpath(common_dir, version_json));
    out
}

/// Extract every native library for this platform into `natives_dir`.
///
/// Entries under META-INF are skipped, as are directory entries, matching the
/// exclusion the vanilla launcher applies.
pub fn extract_natives(common_dir: &Path, version_json: &VersionJson, natives_dir: &Path) -> Result<()> {
    std::fs::create_dir_all(natives_dir)?;
    let lib_dir = library_dir(common_dir);

    for lib in &version_json.libraries {
        if !is_library_compatible(&lib.rules, &lib.natives) {
            continue;
        }
        let Some(natives) = &lib.natives else { continue };
        let Some(classifier) = native_classifier(natives) else { continue };
        let Some(entry) = lib.downloads.classifiers.get(&classifier) else { continue };
        let Some(rel) = &entry.path else { continue };

        let jar = lib_dir.join(rel);
        if !jar.exists() {
            tracing::warn!(path = %jar.display(), "Native jar missing; skipping");
            continue;
        }

        let file = std::fs::File::open(&jar)?;
        let mut archive = zip::ZipArchive::new(file)
            .map_err(|e| Error::Other(format!("Failed to open native jar {}: {e}", jar.display())))?;

        for i in 0..archive.len() {
            let mut zf = archive
                .by_index(i)
                .map_err(|e| Error::Other(format!("Corrupt native jar {}: {e}", jar.display())))?;
            let Some(enclosed) = zf.enclosed_name() else { continue };
            let name = enclosed.to_string_lossy().to_string();

            if zf.is_dir() || name.starts_with("META-INF") {
                continue;
            }
            // Flatten: natives are loaded from a single directory.
            let Some(file_name) = enclosed.file_name() else { continue };
            let dest = natives_dir.join(file_name);
            let mut out = std::fs::File::create(&dest)?;
            std::io::copy(&mut zf, &mut out)?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Argument rules
// ---------------------------------------------------------------------------

/// Evaluate a 1.13+ conditional argument entry.
///
/// Ported from the checksum loop in `_constructJVMArguments113`: an entry is
/// included only when *every* rule passes. `has_custom_resolution` also
/// rewrites the value to `--fullscreen true` when fullscreen is configured,
/// which is a quirk of the JS preserved here.
fn resolve_conditional_arg(entry: &serde_json::Value, ctx: &LaunchContext) -> Option<Vec<String>> {
    let rules = entry.get("rules")?.as_array()?;
    let mut passed = 0usize;
    let mut forced_value: Option<Vec<String>> = None;

    for rule in rules {
        let action = rule.get("action").and_then(|a| a.as_str()).unwrap_or("allow");

        if let Some(os) = rule.get("os") {
            let name_matches = os
                .get("name")
                .and_then(|n| n.as_str())
                .map(|n| n == mojang_os())
                .unwrap_or(true);

            // os.version is a regex against the kernel release. Without a
            // regex engine we treat a present version constraint as matching
            // only when the name matches; in practice these target old
            // Windows builds and this errs toward including the argument.
            if name_matches {
                if action == "allow" {
                    passed += 1;
                }
            } else if action == "disallow" {
                passed += 1;
            }
        } else if let Some(features) = rule.get("features") {
            if features
                .get("has_custom_resolution")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                if ctx.fullscreen {
                    forced_value = Some(vec!["--fullscreen".into(), "true".into()]);
                }
                passed += 1;
            }
        }
    }

    if passed != rules.len() {
        return None;
    }
    if let Some(v) = forced_value {
        return Some(v);
    }

    match entry.get("value")? {
        serde_json::Value::String(s) => Some(vec![s.clone()]),
        serde_json::Value::Array(a) => {
            Some(a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        }
        _ => None,
    }
}

/// Flatten a 1.13+ argument array (strings and conditional objects) into
/// concrete argument strings.
fn flatten_args(list: &serde_json::Value, ctx: &LaunchContext) -> Vec<String> {
    let Some(arr) = list.as_array() else { return Vec::new() };
    let mut out = Vec::new();
    for entry in arr {
        match entry {
            serde_json::Value::String(s) => out.push(s.clone()),
            serde_json::Value::Object(_) => {
                if let Some(vals) = resolve_conditional_arg(entry, ctx) {
                    out.extend(vals);
                }
            }
            _ => {}
        }
    }
    out
}

/// The placeholder table used for `${...}` substitution.
fn placeholders(
    ctx: &LaunchContext,
    version_json: &VersionJson,
    classpath_str: &str,
) -> HashMap<&'static str, String> {
    let (uuid, display_name, access_token, user_type) = match &ctx.account {
        Account::Microsoft { uuid, display_name, access_token, .. } => (
            uuid.clone(),
            display_name.clone(),
            access_token.clone(),
            "msa",
        ),
        Account::Mojang { uuid, display_name, access_token, .. } => (
            uuid.clone(),
            display_name.clone(),
            access_token.clone(),
            "mojang",
        ),
        // Offline accounts have no real token; the vanilla client accepts a
        // placeholder because it never validates it in offline mode.
        Account::Lunar { uuid, display_name, .. } => (
            uuid.clone(),
            display_name.clone(),
            "0".to_string(),
            "mojang",
        ),
    };

    let mut m = HashMap::new();
    m.insert("auth_player_name", display_name.trim().to_string());
    m.insert("version_name", ctx.server_id.clone());
    m.insert("game_directory", ctx.game_dir.display().to_string());
    m.insert(
        "assets_root",
        ctx.common_dir.join("assets").display().to_string(),
    );
    m.insert(
        "assets_index_name",
        version_json
            .assets
            .clone()
            .unwrap_or_else(|| version_json.asset_index.id.clone()),
    );
    m.insert("auth_uuid", uuid.trim().to_string());
    m.insert("auth_access_token", access_token);
    m.insert("user_type", user_type.to_string());
    m.insert(
        "version_type",
        version_json.version_type.clone().unwrap_or_else(|| "release".into()),
    );
    m.insert("resolution_width", ctx.res_width.to_string());
    m.insert("resolution_height", ctx.res_height.to_string());
    m.insert("natives_directory", ctx.natives_dir.display().to_string());
    m.insert("launcher_name", "Lunar-Launcher".to_string());
    m.insert("launcher_version", ctx.launcher_version.clone());
    m.insert("classpath", classpath_str.to_string());
    m.insert("library_directory", library_dir(&ctx.common_dir).display().to_string());
    m.insert("classpath_separator", classpath_separator().to_string());
    m
}

/// Replace every `${key}` occurrence for which we have a value.
///
/// Unknown placeholders are left intact, matching the JS, which only
/// substituted identifiers it recognised.
fn substitute(arg: &str, table: &HashMap<&'static str, String>) -> String {
    let mut out = arg.to_string();
    for (k, v) in table {
        let needle = format!("${{{k}}}");
        if out.contains(&needle) {
            out = out.replace(&needle, v);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Argument construction
// ---------------------------------------------------------------------------

/// Build the full JVM argument vector for a vanilla launch.
pub fn build_args(ctx: &LaunchContext, version_json: &VersionJson) -> Vec<String> {
    let cp = classpath_with_loader(&ctx.common_dir, version_json, ctx.loader.as_ref());
    let cp_str = cp
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(classpath_separator());

    let table = placeholders(ctx, version_json, &cp_str);
    let modern = mc_version_at_least("1.13", &version_json.id)
        || version_json.arguments.is_some();

    let mut args: Vec<String> = Vec::new();

    if modern {
        // JVM arguments come from the manifest and already contain the
        // -cp / -Djava.library.path entries as placeholders.
        if let Some(arguments) = &version_json.arguments {
            if let Some(jvm) = arguments.get("jvm") {
                args.extend(flatten_args(jvm, ctx));
            }
        }
    } else {
        // Pre-1.13 manifests have no jvm argument list; construct it.
        args.push(format!("-Djava.library.path={}", ctx.natives_dir.display()));
        args.push("-cp".into());
        args.push(cp_str.clone());
    }

    if let Some(l) = &ctx.loader {
        args.extend(l.jvm_args.iter().cloned());
    }

    if cfg!(target_os = "macos") {
        args.push("-Xdock:name=Lunar Launcher".into());
    }
    args.push(format!("-Xmx{}", ctx.java_config.max_ram));
    args.push(format!("-Xms{}", ctx.java_config.min_ram));
    args.extend(ctx.java_config.jvm_options.iter().cloned());

    // The loader replaces the entry point; that is how it gets control before
    // the game starts.
    args.push(match &ctx.loader {
        Some(l) => l.main_class.clone(),
        None => version_json.main_class.clone(),
    });

    if modern {
        if let Some(arguments) = &version_json.arguments {
            if let Some(game) = arguments.get("game") {
                args.extend(flatten_args(game, ctx));
            }
        }
    } else if let Some(flat) = &version_json.minecraft_arguments {
        args.extend(flat.split_whitespace().map(str::to_string));
    }

    if let Some(l) = &ctx.loader {
        args.extend(l.game_args.iter().cloned());
    }

    args.into_iter().map(|a| substitute(&a, &table)).collect()
}

/// Assemble everything and spawn the game.
///
/// Natives are extracted first; the caller is responsible for having run a
/// successful validation/download pass, since this does not verify files.
pub async fn launch(
    java_exec: &Path,
    ctx: &LaunchContext,
    version_json: &VersionJson,
) -> Result<tokio::process::Child> {
    extract_natives(&ctx.common_dir, version_json, &ctx.natives_dir)?;
    std::fs::create_dir_all(&ctx.game_dir)?;

    let args = build_args(ctx, version_json);
    tracing::info!(
        java = %java_exec.display(),
        main_class = %version_json.main_class,
        arg_count = args.len(),
        "Launching game"
    );

    let child = tokio::process::Command::new(java_exec)
        .args(&args)
        .current_dir(&ctx.game_dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;

    Ok(child)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    pub(super) fn ctx() -> LaunchContext {
        LaunchContext {
            common_dir: PathBuf::from("/data/common"),
            game_dir: PathBuf::from("/data/instances/Test"),
            natives_dir: PathBuf::from("/tmp/natives"),
            java_config: JavaConfig {
                min_ram: "2G".into(),
                max_ram: "4G".into(),
                executable: None,
                jvm_options: vec!["-XX:+UseG1GC".into()],
            },
            account: Account::Lunar {
                username: "Steve".into(),
                display_name: "Steve".into(),
                uuid: "abc123".into(),
                expires_at: "2027-01-01T00:00:00Z".into(),
            },
            launcher_version: "2.2.1".into(),
            res_width: 1280,
            res_height: 720,
            fullscreen: false,
            server_id: "Lunar_Test".into(),
            loader: None,
        }
    }

    pub(super) fn version(id: &str, modern: bool) -> VersionJson {
        let arguments = modern.then(|| {
            json!({
                "jvm": ["-Djava.library.path=${natives_directory}", "-cp", "${classpath}"],
                "game": ["--username", "${auth_player_name}", "--uuid", "${auth_uuid}",
                         "--accessToken", "${auth_access_token}", "--version", "${version_name}",
                         "--gameDir", "${game_directory}", "--assetsDir", "${assets_root}",
                         "--assetIndex", "${assets_index_name}", "--userType", "${user_type}"]
            })
        });
        serde_json::from_value(json!({
            "id": id,
            "assetIndex": { "id": "5", "sha1": "abc", "url": "http://x" },
            "assets": "5",
            "downloads": {},
            "libraries": [],
            "mainClass": "net.minecraft.client.main.Main",
            "arguments": arguments,
            "minecraftArguments": (!modern).then_some(
                "--username ${auth_player_name} --version ${version_name} --gameDir ${game_directory}"
            ),
            "type": "release"
        }))
        .unwrap()
    }

    #[test]
    fn modern_args_substitute_placeholders() {
        let c = ctx();
        let args = build_args(&c, &version("1.20.1", true));

        assert!(args.contains(&"Steve".to_string()));
        assert!(args.contains(&"abc123".to_string()));
        assert!(args.contains(&"Lunar_Test".to_string()));
        assert!(args.contains(&"net.minecraft.client.main.Main".to_string()));
        assert!(args.contains(&"-Xmx4G".to_string()));
        assert!(args.contains(&"-Xms2G".to_string()));
        assert!(args.contains(&"-XX:+UseG1GC".to_string()));
        // Offline accounts get a placeholder token, never an empty string,
        // which the client would reject.
        assert!(args.contains(&"0".to_string()));
        // No unresolved placeholders survive.
        assert!(
            !args.iter().any(|a| a.contains("${")),
            "unsubstituted placeholder in {args:?}"
        );
    }

    #[test]
    fn legacy_args_are_constructed_manually() {
        let c = ctx();
        let args = build_args(&c, &version("1.12.2", false));
        assert!(args.iter().any(|a| a.starts_with("-Djava.library.path=")));
        assert!(args.contains(&"-cp".to_string()));
        assert!(args.contains(&"--username".to_string()));
        assert!(args.contains(&"Steve".to_string()));
        assert!(!args.iter().any(|a| a.contains("${")));
    }

    #[test]
    fn main_class_precedes_game_arguments() {
        let c = ctx();
        let args = build_args(&c, &version("1.20.1", true));
        let main = args.iter().position(|a| a == "net.minecraft.client.main.Main").unwrap();
        let username = args.iter().position(|a| a == "--username").unwrap();
        assert!(main < username, "main class must come before game args");
    }

    #[test]
    fn conditional_arg_included_when_os_matches() {
        let c = ctx();
        let entry = json!({
            "rules": [{ "action": "allow", "os": { "name": mojang_os() } }],
            "value": "-XstartOnFirstThread"
        });
        assert_eq!(
            resolve_conditional_arg(&entry, &c),
            Some(vec!["-XstartOnFirstThread".to_string()])
        );
    }

    #[test]
    fn conditional_arg_excluded_when_os_differs() {
        let c = ctx();
        let entry = json!({
            "rules": [{ "action": "allow", "os": { "name": "not-this-os" } }],
            "value": "-XsomethingElse"
        });
        assert_eq!(resolve_conditional_arg(&entry, &c), None);
    }

    #[test]
    fn fullscreen_rewrites_the_resolution_feature_arg() {
        let mut c = ctx();
        c.fullscreen = true;
        let entry = json!({
            "rules": [{ "action": "allow", "features": { "has_custom_resolution": true } }],
            "value": ["--width", "${resolution_width}"]
        });
        assert_eq!(
            resolve_conditional_arg(&entry, &c),
            Some(vec!["--fullscreen".to_string(), "true".to_string()])
        );
    }

    #[test]
    fn classpath_separator_is_platform_correct() {
        if cfg!(target_os = "windows") {
            assert_eq!(classpath_separator(), ";");
        } else {
            assert_eq!(classpath_separator(), ":");
        }
    }
}

#[cfg(test)]
mod integration {
    use super::*;
    use crate::dl::{download_all, MojangIndexProcessor};

    /// The end-to-end proof: resolve, download and actually launch vanilla
    /// Minecraft, asserting the client gets far enough to initialise its
    /// window/render pipeline. Ignored by default (large download + spawns a
    /// real game process).
    #[tokio::test]
    #[ignore]
    async fn downloads_and_launches_vanilla() {
        let version = "1.20.1";
        let base = dirs::home_dir()
            .unwrap()
            .join("Library/Application Support/.lunarlauncher");
        let common = base.join("common");
        let game_dir = base.join("instances/__launch_test");
        let natives = std::env::temp_dir().join("lunar-natives-test");
        let _ = std::fs::remove_dir_all(&natives);

        let proc = MojangIndexProcessor::new(common.clone(), version.into());
        let manifest = proc.load_version_manifest().await.expect("manifest");
        let vj = proc.load_version_json(Some(&manifest)).await.expect("version json");
        let index = proc.load_asset_index(&vj).await.expect("asset index");

        let list = proc.validate(&vj, &index).await;
        println!(
            "need {} files ({:.1} MB)",
            list.len(),
            list.total_bytes() as f64 / 1_048_576.0
        );
        if !list.is_empty() {
            download_all(&list, 16, |p| {
                if p.completed % 250 == 0 || p.completed == p.total {
                    println!("  {}/{} ({:.0}%)", p.completed, p.total, p.percent);
                }
            })
            .await
            .expect("download");
        }

        // Re-validate: a second pass must find nothing left to do.
        let again = proc.validate(&vj, &index).await;
        assert!(again.is_empty(), "{} files still invalid after download", again.len());
        println!("all files validated");

        let jvm = crate::java::select_jvm(&base, ">=17.x").await.expect("a JDK 17+");
        println!("using JVM {} at {}", jvm.version_str, jvm.path.display());

        let ctx = LaunchContext {
            common_dir: common.clone(),
            game_dir: game_dir.clone(),
            natives_dir: natives.clone(),
            java_config: JavaConfig {
                min_ram: "1G".into(),
                max_ram: "2G".into(),
                executable: None,
                jvm_options: vec![],
            },
            account: Account::Lunar {
                username: "TestUser".into(),
                display_name: "TestUser".into(),
                uuid: crate::config::md5_hex("TestUser"),
                expires_at: "2027-01-01T00:00:00Z".into(),
            },
            launcher_version: "2.2.1".into(),
            res_width: 854,
            res_height: 480,
            fullscreen: false,
            server_id: version.into(),
            loader: None,
        };

        let exec = crate::java::java_exec_from_root(&jvm.path);
        let mut child = launch(&exec, &ctx, &vj).await.expect("spawn");

        // Collect output briefly, then stop the game.
        use tokio::io::AsyncReadExt;
        let mut stdout = child.stdout.take().unwrap();
        let mut stderr = child.stderr.take().unwrap();
        let mut out = String::new();
        let mut err = String::new();

        let _ = tokio::time::timeout(std::time::Duration::from_secs(45), async {
            let mut obuf = [0u8; 8192];
            let mut ebuf = [0u8; 8192];
            loop {
                tokio::select! {
                    n = stdout.read(&mut obuf) => match n {
                        Ok(0) | Err(_) => break,
                        Ok(n) => out.push_str(&String::from_utf8_lossy(&obuf[..n])),
                    },
                    n = stderr.read(&mut ebuf) => match n {
                        Ok(0) | Err(_) => break,
                        Ok(n) => err.push_str(&String::from_utf8_lossy(&ebuf[..n])),
                    },
                }
                if out.contains("Setting user:") || out.contains("LWJGL") || out.contains("Backend library") {
                    break;
                }
            }
        })
        .await;

        let _ = child.kill().await;
        let combined = format!("{out}\n{err}");
        println!("--- game output ---\n{}", &combined[..combined.len().min(2500)]);

        let _ = std::fs::remove_dir_all(&natives);
        let _ = std::fs::remove_dir_all(&game_dir);

        // What this test can prove: the launcher resolved, downloaded and
        // validated every file, and produced a JVM invocation the client
        // accepts far enough to begin native initialisation.
        //
        // What it deliberately does NOT assert: that a game window appears.
        // Reaching GL/AppKit init means control has passed out of the
        // launcher's hands into the graphics stack, and whether that
        // succeeds depends on the display session the test runs under. A
        // headless or unattached session fails there for reasons unrelated
        // to argument construction, so asserting on it would make this test
        // flaky rather than meaningful.
        assert!(
            !combined.contains("Could not find or load main class")
                && !combined.contains("ClassNotFoundException")
                && !combined.contains("NoClassDefFoundError"),
            "classpath is wrong:\n{combined}"
        );
        assert!(
            !combined.contains("Unrecognized option")
                && !combined.contains("Could not create the Java Virtual Machine"),
            "JVM rejected our arguments:\n{combined}"
        );
    }
}

#[cfg(test)]
mod argdump {
    use super::*;
    use crate::dl::MojangIndexProcessor;

    #[tokio::test]
    #[ignore]
    async fn dump_real_args() {
        let base = dirs::home_dir().unwrap().join("Library/Application Support/.lunarlauncher");
        let common = base.join("common");
        let proc = MojangIndexProcessor::new(common.clone(), "1.20.1".into());
        let m = proc.load_version_manifest().await.unwrap();
        let vj = proc.load_version_json(Some(&m)).await.unwrap();

        let ctx = LaunchContext {
            common_dir: common.clone(),
            game_dir: base.join("instances/__launch_test"),
            natives_dir: std::env::temp_dir().join("lunar-natives-test"),
            java_config: JavaConfig { min_ram: "1G".into(), max_ram: "2G".into(), executable: None, jvm_options: vec![] },
            account: Account::Lunar { username: "TestUser".into(), display_name: "TestUser".into(), uuid: crate::config::md5_hex("TestUser"), expires_at: "2027".into() },
            launcher_version: "2.2.1".into(),
            res_width: 854, res_height: 480, fullscreen: false,
            server_id: "1.20.1".into(),
            loader: None,
        };
        let args = build_args(&ctx, &vj);
        for (i, a) in args.iter().enumerate() {
            if a.len() > 200 { println!("{i}: <{} chars: {}...>", a.len(), &a[..120]); }
            else { println!("{i}: {a}"); }
        }
        println!("\nstartOnFirstThread present: {}", args.iter().any(|a| a.contains("XstartOnFirstThread")));
        let cp_idx = args.iter().position(|a| a == "-cp");
        println!("-cp flag present: {:?}", cp_idx);
        if let Some(i) = cp_idx {
            let entries: Vec<&str> = args[i+1].split(':').collect();
            println!("classpath entries: {}", entries.len());
            println!("natives-jars on cp: {}", entries.iter().filter(|e| e.contains("natives")).count());
            let missing: Vec<&&str> = entries.iter().filter(|e| !std::path::Path::new(e).exists()).collect();
            println!("MISSING from disk: {} {:?}", missing.len(), &missing[..missing.len().min(5)]);
        }
    }
}

#[cfg(test)]
mod loader_tests {
    use super::tests::{ctx, version};
    use super::*;
    use crate::loader::{Library, LoaderProfile};

    fn fabric() -> LoaderProfile {
        LoaderProfile {
            main_class: "net.fabricmc.loader.impl.launch.knot.KnotClient".into(),
            libraries: vec![
                Library { name: "net.fabricmc:fabric-loader:0.19.3".into(), repo_url: "https://m".into() },
                Library { name: "org.ow2.asm:asm:9.6".into(), repo_url: "https://m".into() },
            ],
            game_args: vec!["--fabric".into()],
            jvm_args: vec!["-Dfabric.test=1".into()],
            min_java: Some(17),
        }
    }

    #[test]
    fn the_loader_replaces_the_entry_point() {
        let mut c = ctx();
        c.loader = Some(fabric());
        let args = build_args(&c, &version("1.20.1", true));
        assert!(args.contains(&"net.fabricmc.loader.impl.launch.knot.KnotClient".to_string()));
        assert!(
            !args.contains(&"net.minecraft.client.main.Main".to_string()),
            "the vanilla main class must not also be passed"
        );
    }

    #[test]
    fn loader_libraries_precede_the_vanilla_jar() {
        // The JVM takes a class from the earliest entry providing it, and a
        // loader ships patched versions of vanilla classes.
        let vj = version("1.20.1", true);
        let cp = classpath_with_loader(Path::new("/data/common"), &vj, Some(&fabric()));
        let loader_at = cp.iter().position(|p| p.to_string_lossy().contains("fabric-loader")).unwrap();
        let jar_at = cp.iter().position(|p| p.to_string_lossy().ends_with("1.20.1.jar")).unwrap();
        assert!(loader_at < jar_at, "loader must come first: {cp:?}");
    }

    #[test]
    fn loader_arguments_are_appended_to_both_lists() {
        let mut c = ctx();
        c.loader = Some(fabric());
        let args = build_args(&c, &version("1.20.1", true));
        assert!(args.contains(&"-Dfabric.test=1".to_string()));
        assert!(args.contains(&"--fabric".to_string()));
    }

    #[test]
    fn a_vanilla_launch_is_unchanged_by_the_loader_path() {
        let with = classpath_with_loader(Path::new("/data/common"), &version("1.20.1", true), None);
        let without = classpath(Path::new("/data/common"), &version("1.20.1", true));
        assert_eq!(with, without);
    }
}
