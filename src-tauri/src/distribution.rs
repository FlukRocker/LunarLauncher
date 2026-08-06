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

/// Remote distribution index.
///
/// **This is a LAN test address, not a shipping default.** It points at a
/// controller on the local network; a build handed to anyone else will fail to
/// load a distribution at all. Set it to the real public host before release.
///
/// It replaced `https://hermes-mc.net/downloads/lunarpixel/distribution.json`,
/// which was carried over from distromanager.js and no longer resolves — so the
/// launcher failed at startup with a DNS error unless `LUNAR_DISTRO_URL` was
/// set, which a GUI app started from Explorer never sees.
pub const REMOTE_DISTRO_URL: &str = "http://192.168.1.115:8080/d/acme/distribution.json";

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
    /// Any `type` this build does not recognise.
    ///
    /// Without this, one unknown string anywhere in the document fails
    /// deserialisation of the *entire* index — every server and every mod —
    /// because the document is parsed in a single `from_str`. An existing
    /// install would then serve its stale cache silently and a fresh install
    /// would show the fatal "Unable to Load Distribution Index" screen, with no
    /// way to recover while the updater is inactive.
    ///
    /// Degrading to "ignored module" instead means a future spec addition costs
    /// one unusable module rather than the whole launcher. Unknown is matched by
    /// nothing, so it is never treated as a mod, a loader, or a download target.
    ///
    /// Note this is deliberately not `Unknown(String)`: the enum is `Copy`, and
    /// carrying the original tag would force every `matches!(m.module_type, ..)`
    /// site to borrow instead. The raw text is still available in the cached
    /// body, which is stored verbatim rather than re-serialised.
    #[serde(other)]
    Unknown,
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

/// Outbound links for the landing page's social row.
///
/// A local extension to the Helios spec, not part of upstream. Every field is
/// optional and the whole object is optional, so an index that omits it — which
/// is every index that exists today — parses exactly as before, and a launcher
/// that predates this ignores the object entirely (nothing here sets
/// `deny_unknown_fields`).
///
/// This exists because `DiscordMeta` is Rich Presence config — a client id and
/// image keys — not a link, so before this there was nowhere in the document to
/// put a Discord invite or a website. The five icons on the landing page were
/// rendered with no `href` at all as a result.
///
/// An absent field means "hide that icon", never "render a dead one".
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SocialLinks {
    #[serde(default)]
    pub website: Option<String>,
    #[serde(default)]
    pub discord: Option<String>,
    #[serde(default)]
    pub x: Option<String>,
    #[serde(default)]
    pub instagram: Option<String>,
    #[serde(default)]
    pub youtube: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Distribution {
    pub version: String,
    #[serde(default)]
    pub discord: Option<DiscordMeta>,
    #[serde(default)]
    pub rss: Option<String>,
    /// See `SocialLinks` — a local addition, absent from every upstream index.
    #[serde(default)]
    pub links: Option<SocialLinks>,
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

    /// Human-readable source, for diagnostics.
    pub fn source_description(&self) -> String {
        self.source.describe()
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
            links: None,
            servers: vec![a, b],
        };
        assert_eq!(d.main_server().unwrap().id, "a");
    }

    #[test]
    fn parses_the_shipped_sample_distribution() {
        // docs/sample_distribution.json is the upstream reference document;
        // if our model drifts from the spec this fails.
        let raw = include_str!("../../docs/sample_distribution.json");
        let d: Distribution = serde_json::from_str(raw).expect("sample distro must parse");
        assert!(!d.servers.is_empty());
        let main = d.main_server().unwrap();
        assert!(!main.modules.is_empty());
    }

    #[test]
    fn known_module_types_still_round_trip() {
        // The fallback must not swallow the tags we actually support.
        for (tag, expected) in [
            ("Library", ModuleType::Library),
            ("ForgeHosted", ModuleType::ForgeHosted),
            ("Forge", ModuleType::Forge),
            ("Fabric", ModuleType::Fabric),
            ("LiteLoader", ModuleType::LiteLoader),
            ("ForgeMod", ModuleType::ForgeMod),
            ("FabricMod", ModuleType::FabricMod),
            ("LiteMod", ModuleType::LiteMod),
            ("File", ModuleType::File),
            ("VersionManifest", ModuleType::VersionManifest),
        ] {
            let got: ModuleType =
                serde_json::from_str(&format!("\"{tag}\"")).expect("known tag must parse");
            assert_eq!(got, expected, "{tag} deserialised to the wrong variant");
        }
    }

    #[test]
    fn unknown_module_type_degrades_instead_of_failing() {
        let got: ModuleType = serde_json::from_str("\"NeoForgeMod\"")
            .expect("an unrecognised type must not fail deserialisation");
        assert_eq!(got, ModuleType::Unknown);
    }

    #[test]
    fn one_unknown_module_does_not_destroy_the_whole_index() {
        // The regression this guards: the document is parsed in a single
        // from_str, so before the Unknown fallback existed a single unrecognised
        // type anywhere took out every server and every mod in the index.
        let raw = r#"{
            "version": "1.0.0",
            "servers": [{
                "id": "Test",
                "name": "Test",
                "description": "",
                "icon": "",
                "version": "1.0.0",
                "address": "mc.example.com:25565",
                "minecraftVersion": "1.21.1",
                "mainServer": true,
                "autoconnect": true,
                "modules": [
                    {
                        "id": "com.example:known:1.0",
                        "name": "Known",
                        "type": "ForgeMod",
                        "artifact": { "size": 1, "url": "https://example.com/a.jar" }
                    },
                    {
                        "id": "com.example:future:1.0",
                        "name": "From a newer spec",
                        "type": "SomeTypeThisBuildHasNeverHeardOf",
                        "artifact": { "size": 1, "url": "https://example.com/b.jar" }
                    }
                ]
            }]
        }"#;

        let d: Distribution =
            serde_json::from_str(raw).expect("index must survive an unknown module type");

        let server = d.main_server().expect("server must still be present");
        assert_eq!(server.modules.len(), 2, "both modules should still be held");
        assert_eq!(server.modules[0].module_type, ModuleType::ForgeMod);
        assert_eq!(server.modules[1].module_type, ModuleType::Unknown);
    }
}

#[cfg(test)]
mod social_links_tests {
    use super::*;

    #[test]
    fn an_index_without_links_still_parses() {
        // Every index in the field today omits this. It must stay optional.
        let raw = r#"{ "version": "1.0.0", "servers": [] }"#;
        let d: Distribution = serde_json::from_str(raw).expect("links must be optional");
        assert!(d.links.is_none());
    }

    #[test]
    fn a_partial_links_object_leaves_the_rest_absent() {
        // The controller can only supply two of the five today, so a partial
        // object is the normal case, not an edge case.
        let raw = r#"{
            "version": "1.0.0",
            "links": {
                "website": "https://example.com",
                "discord": "https://discord.gg/example"
            },
            "servers": []
        }"#;
        let d: Distribution = serde_json::from_str(raw).unwrap();
        let links = d.links.expect("links present");
        assert_eq!(links.website.as_deref(), Some("https://example.com"));
        assert_eq!(links.discord.as_deref(), Some("https://discord.gg/example"));
        assert!(links.x.is_none(), "unsupplied links stay absent, not empty");
        assert!(links.instagram.is_none());
        assert!(links.youtube.is_none());
    }

    #[test]
    fn links_are_camel_case_on_the_wire() {
        let d = Distribution {
            version: "1.0.0".into(),
            discord: None,
            rss: None,
            links: Some(SocialLinks {
                website: Some("https://example.com".into()),
                ..Default::default()
            }),
            servers: Vec::new(),
        };
        let v = serde_json::to_value(&d).unwrap();
        assert_eq!(v["links"]["website"], "https://example.com");
    }
}
