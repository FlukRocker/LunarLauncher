//! Port of `helios-core`'s distribution layer plus `distromanager.js`.
//!
//! Covers the raw distribution spec (matching `helios-distribution-types`),
//! the `DistributionAPI` fetch/cache/fallback behaviour, and the derived
//! per-server values that `HeliosServer` computed at construction time
//! (parsed address, effective Java options).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::paths;

/// Remote distribution index, carried over from distromanager.js.
///
/// NOTE: this host is stale — it no longer resolves. Override it with
/// `LUNAR_DISTRO_URL` until the new one is set here.
pub const REMOTE_DISTRO_URL: &str =
    "https://hermes-mc.net/downloads/lunarpixel/distribution.json";

/// Environment override for the distribution source.
///
/// Accepts an `http(s)://` URL, a `file://` URL, or a plain filesystem path,
/// so development can run entirely against a local index:
///
/// ```console
/// LUNAR_DISTRO_URL=../docs/sample_distribution.json npm run app:dev
/// ```
pub const DISTRO_URL_ENV: &str = "LUNAR_DISTRO_URL";

/// Where a distribution index should be read from.
#[derive(Debug, Clone)]
pub enum DistroSource {
    Remote(String),
    Local(PathBuf),
}

impl DistroSource {
    /// Resolve the source, preferring the environment override.
    ///
    /// Anything that isn't an `http(s)` URL is treated as a local path, with
    /// `file://` stripped if present.
    pub fn resolve() -> Self {
        match std::env::var(DISTRO_URL_ENV) {
            Ok(value) if !value.trim().is_empty() => Self::parse(value.trim()),
            _ => Self::Remote(REMOTE_DISTRO_URL.to_string()),
        }
    }

    fn parse(value: &str) -> Self {
        if value.starts_with("http://") || value.starts_with("https://") {
            Self::Remote(value.to_string())
        } else if let Some(rest) = value.strip_prefix("file://") {
            Self::Local(PathBuf::from(rest))
        } else {
            Self::Local(PathBuf::from(value))
        }
    }

    pub fn describe(&self) -> String {
        match self {
            Self::Remote(url) => url.clone(),
            Self::Local(path) => path.display().to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// Raw spec (helios-distribution-types)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModuleType {
    Library,
    ForgeHosted,
    Forge,
    Fabric,
    LiteLoader,
    ForgeMod,
    FabricMod,
    LiteMod,
    File,
    VersionManifest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JdkDistribution {
    CORRETTO,
    TEMURIN,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub size: u64,
    #[serde(rename = "MD5", default)]
    pub md5: Option<String>,
    pub url: String,
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RequiredSpec {
    #[serde(default)]
    pub value: Option<bool>,
    #[serde(default)]
    pub def: Option<bool>,
}

impl RequiredSpec {
    /// `HeliosModule.resolveRequired` — both fields default to true.
    pub fn value(&self) -> bool {
        self.value.unwrap_or(true)
    }
    pub fn default_on(&self) -> bool {
        self.def.unwrap_or(true)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Module {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub module_type: ModuleType,
    #[serde(default)]
    pub classpath: Option<bool>,
    #[serde(default)]
    pub required: Option<RequiredSpec>,
    pub artifact: Artifact,
    #[serde(rename = "subModules", default)]
    pub sub_modules: Vec<Module>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaVersionProps {
    #[serde(default)]
    pub distribution: Option<JdkDistribution>,
    #[serde(default)]
    pub supported: Option<String>,
    #[serde(default)]
    pub suggested_major: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaPlatformOptions {
    pub platform: String,
    #[serde(default)]
    pub architecture: Option<String>,
    #[serde(flatten)]
    pub props: JavaVersionProps,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaOptions {
    #[serde(flatten)]
    pub props: JavaVersionProps,
    #[serde(default)]
    pub platform_options: Vec<JavaPlatformOptions>,
    #[serde(default)]
    pub ram: Option<RamSpec>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct RamSpec {
    pub recommended: u64,
    pub minimum: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscordServerMeta {
    pub short_id: String,
    pub large_image_text: String,
    pub large_image_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Server {
    pub id: String,
    pub name: String,
    pub description: String,
    pub icon: String,
    pub version: String,
    pub address: String,
    pub minecraft_version: String,
    #[serde(default)]
    pub java_options: Option<JavaOptions>,
    #[serde(default)]
    pub discord: Option<DiscordServerMeta>,
    #[serde(default)]
    pub main_server: bool,
    #[serde(default)]
    pub autoconnect: bool,
    #[serde(default)]
    pub modules: Vec<Module>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscordMeta {
    pub client_id: String,
    pub small_image_text: String,
    pub small_image_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Distribution {
    pub version: String,
    #[serde(default)]
    pub discord: Option<DiscordMeta>,
    #[serde(default)]
    pub rss: Option<String>,
    #[serde(default)]
    pub servers: Vec<Server>,
}

// ---------------------------------------------------------------------------
// Derived values (HeliosServer)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct ServerAddress {
    pub hostname: String,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectiveJavaOptions {
    pub supported: String,
    pub distribution: JdkDistribution,
    pub suggested_major: u32,
}

/// `mcVersionAtLeast` from helios-core's MojangUtils, ported verbatim
/// including its handling of a shorter `actual` (missing parts read as 0).
pub fn mc_version_at_least(desired: &str, actual: &str) -> bool {
    let des: Vec<&str> = desired.split('.').collect();
    let mut act: Vec<&str> = actual.split('.').collect();
    while act.len() < des.len() {
        act.push("0");
    }
    for i in 0..des.len() {
        let pd: i64 = des[i].parse().unwrap_or(0);
        let pa: i64 = act[i].parse().unwrap_or(0);
        if pa > pd {
            return true;
        } else if pa < pd {
            return false;
        }
    }
    true
}

/// Node's `process.platform` value for the host.
pub fn node_platform() -> &'static str {
    if cfg!(target_os = "windows") {
        "win32"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else {
        "linux"
    }
}

/// Node's `process.arch` value for the host.
pub fn node_arch() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "arm64"
    } else if cfg!(target_arch = "x86_64") {
        "x64"
    } else {
        std::env::consts::ARCH
    }
}

/// Mojang's OS name, used for library rule evaluation.
pub fn mojang_os() -> &'static str {
    match node_platform() {
        "darwin" => "osx",
        "win32" => "windows",
        other => other,
    }
}

impl Server {
    /// `HeliosServer.parseAddress`. Defaults to port 25565.
    pub fn parse_address(&self) -> Result<ServerAddress> {
        if let Some((host, port)) = self.address.split_once(':') {
            let port: u16 = port.parse().map_err(|_| {
                Error::Other(format!(
                    "Malformed server address for {}. Port must be an integer!",
                    self.id
                ))
            })?;
            Ok(ServerAddress {
                hostname: host.to_string(),
                port,
            })
        } else {
            Ok(ServerAddress {
                hostname: self.address.clone(),
                port: 25565,
            })
        }
    }

    /// `HeliosServer.defaultJavaVersion` — the supported range and suggested
    /// major keyed off the server's Minecraft version.
    fn default_java_version(&self) -> (&'static str, u32) {
        if mc_version_at_least("1.20.5", &self.minecraft_version) {
            (">=21.x", 21)
        } else if mc_version_at_least("1.17", &self.minecraft_version) {
            (">=17.x", 17)
        } else {
            ("8.x", 8)
        }
    }

    fn default_java_platform(&self) -> JdkDistribution {
        if node_platform() == "darwin" {
            JdkDistribution::CORRETTO
        } else {
            JdkDistribution::TEMURIN
        }
    }

    /// `HeliosServer.parseEffectiveJavaOptions`.
    ///
    /// Precedence, most specific first: an entry matching both platform and
    /// architecture, then one matching platform alone, then the server-level
    /// defaults. The JS achieved this by writing into a sparse array at fixed
    /// indices and folding it back-to-front; this reproduces the same result.
    pub fn effective_java_options(&self) -> EffectiveJavaOptions {
        let opts = self.java_options.as_ref();
        let platform_options = opts.map(|o| o.platform_options.as_slice()).unwrap_or(&[]);

        let mut exact: Option<&JavaVersionProps> = None;
        let mut platform_only: Option<&JavaVersionProps> = None;

        for option in platform_options {
            if option.platform == node_platform() {
                if option.architecture.as_deref() == Some(node_arch()) {
                    exact = Some(&option.props);
                } else {
                    platform_only = Some(&option.props);
                }
            }
        }

        let server_level = opts.map(|o| &o.props);

        // Fold from least to most specific so the most specific wins.
        let mut merged = JavaVersionProps::default();
        for candidate in [server_level, platform_only, exact].into_iter().flatten() {
            merged.distribution = candidate.distribution;
            merged.supported = candidate.supported.clone();
            merged.suggested_major = candidate.suggested_major;
        }

        let (default_range, default_suggestion) = self.default_java_version();
        EffectiveJavaOptions {
            supported: merged.supported.unwrap_or_else(|| default_range.to_string()),
            distribution: merged.distribution.unwrap_or_else(|| self.default_java_platform()),
            suggested_major: merged.suggested_major.unwrap_or(default_suggestion),
        }
    }

    /// RAM hints, forwarded to the config layer for java config defaults.
    pub fn ram_hints(&self) -> Option<crate::config::RamHints> {
        self.java_options
            .as_ref()
            .and_then(|o| o.ram)
            .map(|r| crate::config::RamHints {
                minimum: Some(r.minimum),
                recommended: Some(r.recommended),
            })
    }
}

impl Distribution {
    pub fn server_by_id(&self, id: &str) -> Option<&Server> {
        self.servers.iter().find(|s| s.id == id)
    }

    /// `HeliosDistribution.getMainServer` — the server flagged `mainServer`,
    /// falling back to the first entry.
    pub fn main_server(&self) -> Option<&Server> {
        self.servers
            .iter()
            .find(|s| s.main_server)
            .or_else(|| self.servers.first())
    }
}

// ---------------------------------------------------------------------------
// DistributionAPI
// ---------------------------------------------------------------------------

/// Fetches the distribution index, caching it to disk.
///
/// Matches `DistributionAPI.loadDistribution`: try the remote first, write it
/// through to disk on success, and fall back to the cached copy when the
/// remote is unreachable. Only if both fail does this error — which is the
/// "Fatal Error: Unable to Load Distribution Index" the user sees.
pub struct DistributionApi {
    source: DistroSource,
    cache_path: PathBuf,
}

impl DistributionApi {
    pub fn new() -> Self {
        let source = DistroSource::resolve();
        tracing::info!(source = %source.describe(), "Distribution source");
        Self {
            source,
            cache_path: paths::distribution_path(),
        }
    }

    pub async fn get(&self) -> Result<Distribution> {
        match self.pull_source().await {
            Ok(distro) => Ok(distro),
            Err(err) => {
                tracing::error!(%err, "Pull failed; falling back to cached copy.");
                self.pull_local().ok_or(Error::NoDistribution)
            }
        }
    }

    async fn pull_source(&self) -> Result<Distribution> {
        let body = match &self.source {
            DistroSource::Remote(url) => {
                let client = reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(15))
                    .build()?;
                client
                    .get(url)
                    .send()
                    .await?
                    .error_for_status()?
                    .text()
                    .await?
            }
            DistroSource::Local(path) => tokio::fs::read_to_string(path).await?,
        };

        let distro: Distribution = serde_json::from_str(&body)?;

        // Write through so a later offline start still works. Skip when the
        // source *is* the cache file, to avoid a pointless self-copy.
        let is_cache = matches!(&self.source, DistroSource::Local(p) if p == &self.cache_path);
        if !is_cache {
            if let Some(parent) = self.cache_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Err(err) = std::fs::write(&self.cache_path, &body) {
                tracing::warn!(%err, "Failed to cache distribution index.");
            }
        }
        Ok(distro)
    }

    fn pull_local(&self) -> Option<Distribution> {
        let raw = std::fs::read_to_string(&self.cache_path).ok()?;
        match serde_json::from_str(&raw) {
            Ok(d) => Some(d),
            Err(err) => {
                tracing::error!(%err, "Cached distribution index is malformed.");
                None
            }
        }
    }
}

impl Default for DistributionApi {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mc_version_comparison_matches_js() {
        assert!(mc_version_at_least("1.17", "1.20.1"));
        assert!(mc_version_at_least("1.20", "1.20"));
        assert!(!mc_version_at_least("1.20", "1.19.4"));
        // Shorter actual is zero-padded: 1.17 -> 1.17.0
        assert!(mc_version_at_least("1.17.0", "1.17"));
        assert!(!mc_version_at_least("1.17.1", "1.17"));
        assert!(mc_version_at_least("1.20.5", "1.21"));
        assert!(!mc_version_at_least("1.20.5", "1.20.4"));
    }

    fn server_with(mc: &str, java: Option<JavaOptions>) -> Server {
        Server {
            id: "Test".into(),
            name: "Test".into(),
            description: String::new(),
            icon: String::new(),
            version: "1.0.0".into(),
            address: "mc.example.com".into(),
            minecraft_version: mc.into(),
            java_options: java,
            discord: None,
            main_server: true,
            autoconnect: true,
            modules: vec![],
        }
    }

    #[test]
    fn address_defaults_to_25565() {
        let s = server_with("1.12.2", None);
        let a = s.parse_address().unwrap();
        assert_eq!(a.hostname, "mc.example.com");
        assert_eq!(a.port, 25565);
    }

    #[test]
    fn address_parses_explicit_port() {
        let mut s = server_with("1.12.2", None);
        s.address = "mc.example.com:1337".into();
        assert_eq!(s.parse_address().unwrap().port, 1337);
    }

    #[test]
    fn address_rejects_non_integer_port() {
        let mut s = server_with("1.12.2", None);
        s.address = "mc.example.com:abc".into();
        assert!(s.parse_address().is_err());
    }

    #[test]
    fn java_defaults_track_minecraft_version() {
        assert_eq!(server_with("1.12.2", None).effective_java_options().suggested_major, 8);
        assert_eq!(server_with("1.18", None).effective_java_options().suggested_major, 17);
        assert_eq!(server_with("1.21", None).effective_java_options().suggested_major, 21);
    }

    #[test]
    fn server_level_java_options_override_defaults() {
        let opts = JavaOptions {
            props: JavaVersionProps {
                distribution: None,
                supported: Some(">=11.x".into()),
                suggested_major: Some(11),
            },
            platform_options: vec![],
            ram: None,
        };
        let eff = server_with("1.12.2", Some(opts)).effective_java_options();
        assert_eq!(eff.suggested_major, 11);
        assert_eq!(eff.supported, ">=11.x");
    }

    #[test]
    fn platform_specific_options_win_over_server_level() {
        let opts = JavaOptions {
            props: JavaVersionProps {
                distribution: None,
                supported: Some(">=11.x".into()),
                suggested_major: Some(11),
            },
            platform_options: vec![JavaPlatformOptions {
                platform: node_platform().to_string(),
                architecture: Some(node_arch().to_string()),
                props: JavaVersionProps {
                    distribution: Some(JdkDistribution::CORRETTO),
                    supported: Some(">=17.x".into()),
                    suggested_major: Some(17),
                },
            }],
            ram: None,
        };
        let eff = server_with("1.12.2", Some(opts)).effective_java_options();
        assert_eq!(eff.suggested_major, 17);
        assert_eq!(eff.distribution, JdkDistribution::CORRETTO);
    }

    #[test]
    fn main_server_falls_back_to_first() {
        let mut a = server_with("1.12.2", None);
        a.id = "a".into();
        a.main_server = false;
        let mut b = server_with("1.12.2", None);
        b.id = "b".into();
        b.main_server = false;
        let d = Distribution {
            version: "1.0.0".into(),
            discord: None,
            rss: None,
            servers: vec![a, b],
        };
        assert_eq!(d.main_server().unwrap().id, "a");
    }

    #[test]
    fn parses_the_shipped_sample_distribution() {
        // docs/sample_distribution.json is the upstream reference document;
        // if our model drifts from the spec this fails.
        let raw = include_str!("../../../docs/sample_distribution.json");
        let d: Distribution = serde_json::from_str(raw).expect("sample distro must parse");
        assert!(!d.servers.is_empty());
        let main = d.main_server().unwrap();
        assert!(!main.modules.is_empty());
    }
}
