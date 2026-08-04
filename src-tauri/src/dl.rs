//! Port of `helios-core`'s download layer (`MojangIndexProcessor` + the
//! validation/download half of `FullRepair`).
//!
//! The shape follows the JS: resolve the version manifest, then the version
//! JSON, then the asset index; validate every asset/library/client jar against
//! its SHA1; and download only what is missing or corrupt. Validation is what
//! makes a launcher trustworthy, so a hash mismatch is always treated as
//! "redownload", never as "close enough".

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::distribution::mojang_os;
use crate::error::{Error, Result};

pub const VERSION_MANIFEST_ENDPOINT: &str =
    "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
pub const ASSET_RESOURCE_ENDPOINT: &str = "https://resources.download.minecraft.net";

// ---------------------------------------------------------------------------
// Paths (FileUtils.js)
// ---------------------------------------------------------------------------

pub fn version_dir(common_dir: &Path, version: &str) -> PathBuf {
    common_dir.join("versions").join(version)
}
pub fn version_json_path(common_dir: &Path, version: &str) -> PathBuf {
    version_dir(common_dir, version).join(format!("{version}.json"))
}
pub fn version_jar_path(common_dir: &Path, version: &str) -> PathBuf {
    version_dir(common_dir, version).join(format!("{version}.jar"))
}
pub fn library_dir(common_dir: &Path) -> PathBuf {
    common_dir.join("libraries")
}
pub fn asset_dir(common_dir: &Path) -> PathBuf {
    common_dir.join("assets")
}
pub fn asset_index_path(common_dir: &Path, id: &str) -> PathBuf {
    asset_dir(common_dir).join("indexes").join(format!("{id}.json"))
}

// ---------------------------------------------------------------------------
// Mojang types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct ManifestVersion {
    pub id: String,
    pub url: String,
    #[serde(default)]
    pub sha1: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VersionManifest {
    pub versions: Vec<ManifestVersion>,
}

impl VersionManifest {
    pub fn find(&self, id: &str) -> Option<&ManifestVersion> {
        self.versions.iter().find(|v| v.id == id)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct DownloadEntry {
    #[serde(default)]
    pub path: Option<String>,
    pub sha1: String,
    pub size: u64,
    pub url: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct LibraryDownloads {
    #[serde(default)]
    pub artifact: Option<DownloadEntry>,
    #[serde(default)]
    pub classifiers: std::collections::HashMap<String, DownloadEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RuleOs {
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Rule {
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub os: Option<RuleOs>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Library {
    pub name: String,
    #[serde(default)]
    pub downloads: LibraryDownloads,
    #[serde(default)]
    pub rules: Option<Vec<Rule>>,
    #[serde(default)]
    pub natives: Option<std::collections::HashMap<String, String>>,
    #[serde(default)]
    pub extract: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AssetIndexRef {
    pub id: String,
    pub sha1: String,
    pub url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LoggingFile {
    pub id: String,
    pub sha1: String,
    pub size: u64,
    pub url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LoggingClient {
    pub file: LoggingFile,
    #[serde(default)]
    pub argument: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Logging {
    #[serde(default)]
    pub client: Option<LoggingClient>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionJson {
    pub id: String,
    pub asset_index: AssetIndexRef,
    #[serde(default)]
    pub assets: Option<String>,
    pub downloads: std::collections::HashMap<String, DownloadEntry>,
    #[serde(default)]
    pub libraries: Vec<Library>,
    pub main_class: String,
    /// 1.13+ structured arguments.
    #[serde(default)]
    pub arguments: Option<serde_json::Value>,
    /// Pre-1.13 flat argument string.
    #[serde(default)]
    pub minecraft_arguments: Option<String>,
    #[serde(default)]
    pub logging: Logging,
    #[serde(default)]
    pub version_type: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AssetObject {
    pub hash: String,
    pub size: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AssetIndex {
    pub objects: std::collections::HashMap<String, AssetObject>,
}

// ---------------------------------------------------------------------------
// Library rules (MojangUtils.js)
// ---------------------------------------------------------------------------

/// `validateLibraryRules`. Note the JS returns on the *first* rule that has
/// both an action and an os — it does not evaluate the whole rule set — so
/// this reproduces that early return rather than "correcting" it.
pub fn validate_library_rules(rules: &Option<Vec<Rule>>) -> bool {
    let Some(rules) = rules else {
        return false;
    };
    for rule in rules {
        if let (Some(action), Some(os)) = (&rule.action, &rule.os) {
            if let Some(name) = &os.name {
                if action == "allow" {
                    return name == mojang_os();
                } else if action == "disallow" {
                    return name != mojang_os();
                }
            }
        }
    }
    true
}

pub fn validate_library_natives(
    natives: &Option<std::collections::HashMap<String, String>>,
) -> bool {
    match natives {
        None => true,
        Some(map) => map.contains_key(mojang_os()),
    }
}

/// `isLibraryCompatible`.
pub fn is_library_compatible(
    rules: &Option<Vec<Rule>>,
    natives: &Option<std::collections::HashMap<String, String>>,
) -> bool {
    match rules {
        None => validate_library_natives(natives),
        Some(_) => validate_library_rules(rules),
    }
}

/// The classifier for a native library on this platform, with `${arch}`
/// substituted the way the JS did (`process.arch.replace('x', '')`, so
/// x64 -> 64, and on arm64 the literal arch string).
pub fn native_classifier(
    natives: &std::collections::HashMap<String, String>,
) -> Option<String> {
    let raw = natives.get(mojang_os())?;
    let arch = crate::distribution::node_arch().replace('x', "");
    Some(raw.replace("${arch}", &arch))
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// One file that must exist locally with a known hash.
#[derive(Debug, Clone, Serialize)]
pub struct Asset {
    pub id: String,
    pub hash: String,
    pub size: u64,
    pub url: String,
    pub path: PathBuf,
}

/// SHA1 of a file on disk.
async fn sha1_of(path: &Path) -> Option<String> {
    use sha1::{Digest, Sha1};
    let bytes = tokio::fs::read(path).await.ok()?;
    let mut hasher = Sha1::new();
    hasher.update(&bytes);
    Some(hex::encode(hasher.finalize()))
}

/// `validateLocalFile` — exists and, when a hash is supplied, matches it.
pub async fn validate_local_file(path: &Path, hash: Option<&str>) -> bool {
    if !tokio::fs::try_exists(path).await.unwrap_or(false) {
        return false;
    }
    match hash {
        None => true,
        Some(expected) => sha1_of(path)
            .await
            .map(|actual| actual.eq_ignore_ascii_case(expected))
            .unwrap_or(false),
    }
}

/// Everything that needs downloading before the game can launch.
#[derive(Debug, Default, Serialize)]
pub struct RepairList {
    pub assets: Vec<Asset>,
    pub libraries: Vec<Asset>,
    pub client: Vec<Asset>,
    pub misc: Vec<Asset>,
}

impl RepairList {
    pub fn all(&self) -> impl Iterator<Item = &Asset> {
        self.assets
            .iter()
            .chain(&self.libraries)
            .chain(&self.client)
            .chain(&self.misc)
    }
    pub fn len(&self) -> usize {
        self.all().count()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn total_bytes(&self) -> u64 {
        self.all().map(|a| a.size).sum()
    }
}

/// Resolves and validates the Mojang side of a launch.
pub struct MojangIndexProcessor {
    common_dir: PathBuf,
    version: String,
    client: reqwest::Client,
}

impl MojangIndexProcessor {
    pub fn new(common_dir: PathBuf, version: String) -> Self {
        Self {
            common_dir,
            version,
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("http client"),
        }
    }

    /// Fetch the manifest. A failure is non-fatal — we fall back to whatever
    /// version JSON is already on disk, matching the documented JS behaviour.
    pub async fn load_version_manifest(&self) -> Option<VersionManifest> {
        match self
            .client
            .get(VERSION_MANIFEST_ENDPOINT)
            .send()
            .await
            .and_then(|r| r.error_for_status())
        {
            Ok(resp) => match resp.json::<VersionManifest>().await {
                Ok(m) => Some(m),
                Err(err) => {
                    tracing::error!(%err, "Malformed version manifest");
                    None
                }
            },
            Err(err) => {
                tracing::error!(%err, "Failed to fetch version manifest");
                None
            }
        }
    }

    /// Load the version JSON, downloading it when missing or hash-mismatched.
    pub async fn load_version_json(
        &self,
        manifest: Option<&VersionManifest>,
    ) -> Result<VersionJson> {
        let path = version_json_path(&self.common_dir, &self.version);

        if let Some(entry) = manifest.and_then(|m| m.find(&self.version)) {
            if !validate_local_file(&path, entry.sha1.as_deref()).await {
                tracing::info!(version = %self.version, "Fetching version JSON");
                let body = self
                    .client
                    .get(&entry.url)
                    .send()
                    .await?
                    .error_for_status()?
                    .text()
                    .await?;
                write_atomic(&path, body.as_bytes()).await?;
            }
        }

        let raw = tokio::fs::read_to_string(&path).await.map_err(|_| {
            Error::Other(format!(
                "No version JSON for {} and it could not be downloaded.",
                self.version
            ))
        })?;
        Ok(serde_json::from_str(&raw)?)
    }

    /// Load the asset index for a version, downloading when needed.
    pub async fn load_asset_index(&self, version_json: &VersionJson) -> Result<AssetIndex> {
        let path = asset_index_path(&self.common_dir, &version_json.asset_index.id);
        if !validate_local_file(&path, Some(&version_json.asset_index.sha1)).await {
            tracing::info!(id = %version_json.asset_index.id, "Fetching asset index");
            let body = self
                .client
                .get(&version_json.asset_index.url)
                .send()
                .await?
                .error_for_status()?
                .text()
                .await?;
            write_atomic(&path, body.as_bytes()).await?;
        }
        let raw = tokio::fs::read_to_string(&path).await?;
        Ok(serde_json::from_str(&raw)?)
    }

    /// Assets missing or failing hash validation.
    pub async fn validate_assets(&self, index: &AssetIndex) -> Vec<Asset> {
        let objects_dir = asset_dir(&self.common_dir).join("objects");
        let mut out = Vec::new();
        for (id, obj) in &index.objects {
            let prefix = &obj.hash[..2];
            let path = objects_dir.join(prefix).join(&obj.hash);
            if !validate_local_file(&path, Some(&obj.hash)).await {
                out.push(Asset {
                    id: id.clone(),
                    hash: obj.hash.clone(),
                    size: obj.size,
                    url: format!("{ASSET_RESOURCE_ENDPOINT}/{prefix}/{}", obj.hash),
                    path,
                });
            }
        }
        out
    }

    /// Libraries missing or failing validation, honouring platform rules.
    pub async fn validate_libraries(&self, version_json: &VersionJson) -> Vec<Asset> {
        let lib_dir = library_dir(&self.common_dir);
        let mut out = Vec::new();

        for lib in &version_json.libraries {
            if !is_library_compatible(&lib.rules, &lib.natives) {
                continue;
            }
            let artifact = match &lib.natives {
                None => lib.downloads.artifact.as_ref(),
                Some(natives) => native_classifier(natives)
                    .and_then(|c| lib.downloads.classifiers.get(&c)),
            };
            let Some(artifact) = artifact else { continue };
            let Some(rel) = &artifact.path else { continue };

            // These come from Mojang's version JSON and are relative, so this
            // is defence in depth rather than a live fix — but it is the same
            // join shape the module-download path will need, and a bad entry
            // should cost one library rather than a write outside common/.
            let path = match crate::paths::safe_join(&lib_dir, rel) {
                Ok(p) => p,
                Err(err) => {
                    tracing::warn!(%err, library = %lib.name, "Skipping library with an unsafe path.");
                    continue;
                }
            };
            if !validate_local_file(&path, Some(&artifact.sha1)).await {
                out.push(Asset {
                    id: lib.name.clone(),
                    hash: artifact.sha1.clone(),
                    size: artifact.size,
                    url: artifact.url.clone(),
                    path,
                });
            }
        }
        out
    }

    /// The client jar.
    pub async fn validate_client(&self, version_json: &VersionJson) -> Vec<Asset> {
        let Some(client) = version_json.downloads.get("client") else {
            return Vec::new();
        };
        let path = version_jar_path(&self.common_dir, &version_json.id);
        if validate_local_file(&path, Some(&client.sha1)).await {
            return Vec::new();
        }
        vec![Asset {
            id: format!("{}-client", version_json.id),
            hash: client.sha1.clone(),
            size: client.size,
            url: client.url.clone(),
            path,
        }]
    }

    /// The log4j configuration, when the version declares one.
    pub async fn validate_log_config(&self, version_json: &VersionJson) -> Vec<Asset> {
        let Some(client) = &version_json.logging.client else {
            return Vec::new();
        };
        let path = asset_dir(&self.common_dir)
            .join("log_configs")
            .join(&client.file.id);
        if validate_local_file(&path, Some(&client.file.sha1)).await {
            return Vec::new();
        }
        vec![Asset {
            id: client.file.id.clone(),
            hash: client.file.sha1.clone(),
            size: client.file.size,
            url: client.file.url.clone(),
            path,
        }]
    }

    /// Full validation pass. Returns everything needing download.
    pub async fn validate(&self, version_json: &VersionJson, index: &AssetIndex) -> RepairList {
        RepairList {
            assets: self.validate_assets(index).await,
            libraries: self.validate_libraries(version_json).await,
            client: self.validate_client(version_json).await,
            misc: self.validate_log_config(version_json).await,
        }
    }
}

// ---------------------------------------------------------------------------
// Downloading
// ---------------------------------------------------------------------------

/// Write via a temp file + rename so an interrupted write never leaves a
/// half-written file that would later pass an existence check.
async fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let tmp = path.with_extension("part");
    tokio::fs::write(&tmp, bytes).await?;
    tokio::fs::rename(&tmp, path).await?;
    Ok(())
}

/// Download one asset and verify its hash before committing it to disk.
async fn download_asset(client: &reqwest::Client, asset: &Asset) -> Result<()> {
    use sha1::{Digest, Sha1};

    let bytes = client
        .get(&asset.url)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;

    let mut hasher = Sha1::new();
    hasher.update(&bytes);
    let actual = hex::encode(hasher.finalize());
    if !actual.eq_ignore_ascii_case(&asset.hash) {
        return Err(Error::Other(format!(
            "Hash mismatch for {}: expected {}, got {}",
            asset.id, asset.hash, actual
        )));
    }

    write_atomic(&asset.path, &bytes).await
}

/// Progress emitted to the UI while downloading.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub completed: u64,
    pub total: u64,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub percent: f64,
}

/// Download every asset in `list`, with bounded concurrency and retries.
///
/// `on_progress` is called as files complete. Failures are retried a few
/// times before the whole repair fails — a partial install is worse than a
/// clear error, so this does not silently skip files.
pub async fn download_all<F>(list: &RepairList, concurrency: usize, on_progress: F) -> Result<()>
where
    F: Fn(DownloadProgress) + Send + Sync + 'static,
{
    use futures::stream::StreamExt;

    let assets: Vec<Asset> = list.all().cloned().collect();
    let total = assets.len() as u64;
    let bytes_total = list.total_bytes();
    if total == 0 {
        return Ok(());
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()?;

    let completed = Arc::new(AtomicU64::new(0));
    let bytes_done = Arc::new(AtomicU64::new(0));
    let on_progress = Arc::new(on_progress);

    let results: Vec<Result<()>> = futures::stream::iter(assets.into_iter().map(|asset| {
        let client = client.clone();
        let completed = Arc::clone(&completed);
        let bytes_done = Arc::clone(&bytes_done);
        let on_progress = Arc::clone(&on_progress);
        async move {
            let mut last_err = None;
            for attempt in 0..3u32 {
                match download_asset(&client, &asset).await {
                    Ok(()) => {
                        let c = completed.fetch_add(1, Ordering::Relaxed) + 1;
                        let b = bytes_done.fetch_add(asset.size, Ordering::Relaxed) + asset.size;
                        on_progress(DownloadProgress {
                            completed: c,
                            total,
                            bytes_done: b,
                            bytes_total,
                            percent: (c as f64 / total as f64) * 100.0,
                        });
                        return Ok(());
                    }
                    Err(err) => {
                        tracing::warn!(id = %asset.id, attempt, %err, "Download failed, retrying");
                        last_err = Some(err);
                        tokio::time::sleep(std::time::Duration::from_millis(
                            300 * (attempt as u64 + 1),
                        ))
                        .await;
                    }
                }
            }
            Err(last_err.unwrap_or_else(|| Error::Other(format!("Failed to download {}", asset.id))))
        }
    }))
    .buffer_unordered(concurrency)
    .collect()
    .await;

    for r in results {
        r?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn rule(action: &str, os: Option<&str>) -> Rule {
        Rule {
            action: Some(action.into()),
            os: os.map(|n| RuleOs { name: Some(n.into()) }),
        }
    }

    #[test]
    fn no_rules_and_no_natives_is_compatible() {
        assert!(is_library_compatible(&None, &None));
    }

    #[test]
    fn natives_must_cover_this_platform() {
        let mut n = HashMap::new();
        n.insert(mojang_os().to_string(), "natives-x".to_string());
        assert!(is_library_compatible(&None, &Some(n)));

        let mut other = HashMap::new();
        other.insert("some-other-os".to_string(), "natives-x".to_string());
        assert!(!is_library_compatible(&None, &Some(other)));
    }

    #[test]
    fn allow_rule_matches_only_this_os() {
        assert!(is_library_compatible(&Some(vec![rule("allow", Some(mojang_os()))]), &None));
        assert!(!is_library_compatible(&Some(vec![rule("allow", Some("nope"))]), &None));
    }

    #[test]
    fn disallow_rule_excludes_this_os() {
        assert!(!is_library_compatible(&Some(vec![rule("disallow", Some(mojang_os()))]), &None));
        assert!(is_library_compatible(&Some(vec![rule("disallow", Some("nope"))]), &None));
    }

    #[test]
    fn rules_without_os_fall_through_to_true() {
        // A bare {"action":"allow"} rule has no os, so the loop skips it and
        // the function returns true — matching the JS.
        let rules = Some(vec![Rule { action: Some("allow".into()), os: None }]);
        assert!(is_library_compatible(&rules, &None));
    }

    #[test]
    fn native_classifier_substitutes_arch() {
        let mut n = HashMap::new();
        n.insert(mojang_os().to_string(), "natives-${arch}".to_string());
        let c = native_classifier(&n).unwrap();
        assert!(!c.contains("${arch}"), "arch placeholder must be replaced: {c}");
    }

    #[test]
    fn paths_follow_the_standard_layout() {
        let common = Path::new("/data/common");
        assert_eq!(
            version_json_path(common, "1.20.1"),
            PathBuf::from("/data/common/versions/1.20.1/1.20.1.json")
        );
        assert_eq!(
            version_jar_path(common, "1.20.1"),
            PathBuf::from("/data/common/versions/1.20.1/1.20.1.jar")
        );
        assert_eq!(library_dir(common), PathBuf::from("/data/common/libraries"));
    }

    #[tokio::test]
    async fn validate_local_file_checks_hash() {
        let dir = std::env::temp_dir().join("lunar-dl-test");
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let f = dir.join("hello.txt");
        tokio::fs::write(&f, b"hello").await.unwrap();

        // sha1("hello")
        let expected = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d";
        assert!(validate_local_file(&f, Some(expected)).await);
        assert!(validate_local_file(&f, None).await);
        assert!(!validate_local_file(&f, Some("deadbeef")).await);
        assert!(!validate_local_file(&dir.join("missing.txt"), None).await);

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }
}

#[cfg(test)]
mod integration {
    use super::*;

    /// Exercises the real Mojang pipeline: manifest -> version JSON -> asset
    /// index -> validation, then downloads a small sample to prove hash
    /// verification works against real bytes. Ignored by default (network).
    #[tokio::test]
    #[ignore]
    async fn resolves_and_downloads_from_mojang() {
        let common = std::env::temp_dir().join("lunar-dl-integration/common");
        let _ = tokio::fs::remove_dir_all(&common).await;
        tokio::fs::create_dir_all(&common).await.unwrap();

        let proc = MojangIndexProcessor::new(common.clone(), "1.20.1".into());

        let manifest = proc.load_version_manifest().await.expect("manifest");
        println!("manifest versions: {}", manifest.versions.len());
        assert!(manifest.find("1.20.1").is_some());

        let vj = proc.load_version_json(Some(&manifest)).await.expect("version json");
        println!("version {} mainClass {}", vj.id, vj.main_class);
        assert_eq!(vj.id, "1.20.1");
        assert!(!vj.libraries.is_empty());

        let index = proc.load_asset_index(&vj).await.expect("asset index");
        println!("asset objects: {}", index.objects.len());
        assert!(!index.objects.is_empty());

        let list = proc.validate(&vj, &index).await;
        println!(
            "to download: {} assets, {} libraries, {} client, {} misc ({:.1} MB)",
            list.assets.len(),
            list.libraries.len(),
            list.client.len(),
            list.misc.len(),
            list.total_bytes() as f64 / 1_048_576.0
        );
        assert!(!list.client.is_empty(), "client jar should be missing on a clean dir");
        assert!(!list.libraries.is_empty());

        // Download a small sample rather than the full ~500MB.
        let sample = RepairList {
            assets: list.assets.into_iter().take(5).collect(),
            libraries: list.libraries.into_iter().take(3).collect(),
            client: vec![],
            misc: list.misc,
        };
        let n = sample.len();
        println!("downloading sample of {n} files");
        download_all(&sample, 4, |p| {
            println!("  {}/{} ({:.0}%)", p.completed, p.total, p.percent);
        })
        .await
        .expect("sample download");

        // Everything we downloaded must now validate.
        for a in sample.all() {
            assert!(
                validate_local_file(&a.path, Some(&a.hash)).await,
                "downloaded file failed validation: {}",
                a.id
            );
        }
        println!("all {n} downloaded files validated");

        let _ = tokio::fs::remove_dir_all(std::env::temp_dir().join("lunar-dl-integration")).await;
    }
}
