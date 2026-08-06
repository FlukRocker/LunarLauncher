//! Port of `app/assets/js/configmanager.js`.
//!
//! Behavioural notes carried over from the Electron implementation:
//!
//! * The on-disk JSON shape is **unchanged**, so an existing installation's
//!   config.json loads as-is. Field names must stay camelCase.
//! * `validateKeySet` in the JS walked the default object and filled in any
//!   key the stored config was missing (so launcher upgrades don't break on
//!   newly added settings). Here that falls out of `#[serde(default)]` on
//!   every field — deserialising an old config yields defaults for anything
//!   absent. The config is re-saved after load, same as the JS did.
//! * Unknown keys are preserved deliberately? No — the JS dropped them too
//!   (it rebuilt from the default key set), so plain serde behaviour matches.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use sysinfo::System;

use crate::error::{Error, Result};
use crate::paths;

const BYTES_PER_GB: u64 = 1_073_741_824;

// ---------------------------------------------------------------------------
// Model
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameSettings {
    #[serde(default = "default_res_width")]
    pub res_width: u32,
    #[serde(default = "default_res_height")]
    pub res_height: u32,
    #[serde(default)]
    pub fullscreen: bool,
    #[serde(default = "default_true")]
    pub auto_connect: bool,
    #[serde(default = "default_true")]
    pub launch_detached: bool,
}

fn default_res_width() -> u32 {
    1280
}
fn default_res_height() -> u32 {
    720
}
fn default_true() -> bool {
    true
}

impl Default for GameSettings {
    fn default() -> Self {
        Self {
            res_width: default_res_width(),
            res_height: default_res_height(),
            fullscreen: false,
            auto_connect: true,
            launch_detached: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LauncherSettings {
    #[serde(default)]
    pub allow_prerelease: bool,
    #[serde(default = "paths::default_data_directory")]
    pub data_directory: PathBuf,
}

impl Default for LauncherSettings {
    fn default() -> Self {
        Self {
            allow_prerelease: false,
            data_directory: paths::default_data_directory(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub game: GameSettings,
    #[serde(default)]
    pub launcher: LauncherSettings,
    /// Absent from an Electron-written config, so it defaults to disabled.
    #[serde(default)]
    pub telemetry: crate::telemetry::TelemetryConfig,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewsCache {
    #[serde(default)]
    pub date: Option<String>,
    #[serde(default)]
    pub content: Option<serde_json::Value>,
    #[serde(default)]
    pub dismissed: bool,
}

/// Microsoft-specific token bundle, nested under `microsoft` in the JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MicrosoftTokens {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: String,
}

/// An entry in `authenticationDatabase`, discriminated by the `type` field.
///
/// `mojang` is retained only so existing configs continue to deserialise;
/// authserver.mojang.com is shut down and no new mojang accounts can be added.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Account {
    Microsoft {
        #[serde(rename = "accessToken")]
        access_token: String,
        username: String,
        uuid: String,
        #[serde(rename = "displayName")]
        display_name: String,
        #[serde(rename = "expiresAt")]
        expires_at: String,
        microsoft: MicrosoftTokens,
    },
    /// Fork-specific offline account. UUID is the MD5 of the username.
    Lunar {
        username: String,
        #[serde(rename = "displayName")]
        display_name: String,
        uuid: String,
        #[serde(rename = "expiresAt")]
        expires_at: String,
    },
    Mojang {
        #[serde(rename = "accessToken")]
        access_token: String,
        username: String,
        uuid: String,
        #[serde(rename = "displayName")]
        display_name: String,
    },
}

/// Apply `f` to every credential this account carries, with the context
/// string that binds each ciphertext to its field.
///
/// Written as one walker used by both directions so a field cannot be
/// encrypted on save but forgotten on load — which would present as a token
/// that works until the first restart.
fn for_each_secret(account: &mut Account, mut f: impl FnMut(&mut String, &str)) {
    match account {
        Account::Microsoft { access_token, microsoft, .. } => {
            f(access_token, "accessToken");
            f(&mut microsoft.access_token, "microsoft.accessToken");
            f(&mut microsoft.refresh_token, "microsoft.refreshToken");
        }
        Account::Mojang { access_token, .. } => f(access_token, "accessToken"),
        // Offline: the uuid is derived from the username and there is nothing
        // to protect.
        Account::Lunar { .. } => {}
    }
}

impl Account {
    /// Encrypt credentials for storage. Non-secret fields — username, uuid,
    /// display name — stay readable so the account list still renders if the
    /// keystore is later unavailable.
    pub fn seal_secrets(&mut self) {
        for_each_secret(self, |value, context| {
            *value = crate::secrets::seal(value, context);
        });
    }

    /// Decrypt credentials after loading.
    ///
    /// A value that cannot be opened is cleared rather than kept, so the
    /// account re-authenticates instead of sending a ciphertext to Microsoft
    /// as if it were a token. The account itself is never dropped: a locked
    /// keychain is transient, and discarding accounts over it would repeat the
    /// config-wiping failure this codebase has already had once.
    pub fn unseal_secrets(&mut self) {
        for_each_secret(self, |value, context| match crate::secrets::open(value, context) {
            Ok(plain) => *value = plain,
            Err(err) => {
                tracing::warn!(%err, context, "Could not decrypt a stored credential; re-login required");
                value.clear();
            }
        });
    }

    pub fn uuid(&self) -> &str {
        match self {
            Account::Microsoft { uuid, .. }
            | Account::Lunar { uuid, .. }
            | Account::Mojang { uuid, .. } => uuid,
        }
    }

    pub fn display_name(&self) -> &str {
        match self {
            Account::Microsoft { display_name, .. }
            | Account::Lunar { display_name, .. }
            | Account::Mojang { display_name, .. } => display_name,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaConfig {
    // NOTE: the Electron config writes `minRAM`/`maxRAM` with a capitalised
    // "RAM" (see defaultJavaConfig8/17 in configmanager.js). camelCase would
    // render these as `minRam`/`maxRam`, which fails to deserialise an
    // existing user's config — and because a parse failure makes the loader
    // fall back to defaults, that would silently wipe their accounts and
    // settings. These renames are load-bearing; do not remove them.
    #[serde(rename = "minRAM")]
    pub min_ram: String,
    #[serde(rename = "maxRAM")]
    pub max_ram: String,
    #[serde(default)]
    pub executable: Option<PathBuf>,
    #[serde(default)]
    pub jvm_options: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModConfiguration {
    pub id: String,
    #[serde(default)]
    pub mods: serde_json::Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    #[serde(default)]
    pub settings: Settings,
    #[serde(default)]
    pub news_cache: NewsCache,
    #[serde(default)]
    pub client_token: Option<String>,
    /// Resolved at startup against the distribution index.
    #[serde(default)]
    pub selected_server: Option<String>,
    #[serde(default)]
    pub selected_account: Option<String>,
    #[serde(default)]
    pub authentication_database: BTreeMap<String, Account>,
    #[serde(default)]
    pub mod_configurations: Vec<ModConfiguration>,
    #[serde(default)]
    pub java_config: BTreeMap<String, JavaConfig>,
}

// ---------------------------------------------------------------------------
// RAM helpers (ported verbatim from configmanager.js)
// ---------------------------------------------------------------------------

fn total_memory() -> u64 {
    let mut sys = System::new();
    sys.refresh_memory();
    sys.total_memory()
}

/// Distribution-declared RAM hints, in megabytes.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub struct RamHints {
    pub minimum: Option<u64>,
    pub recommended: Option<u64>,
}

/// Absolute minimum RAM in gigabytes.
pub fn absolute_min_ram(ram: Option<RamHints>) -> f64 {
    match ram.and_then(|r| r.minimum) {
        Some(min) => min as f64 / 1024.0,
        // Legacy behaviour: 3G if the machine has >= 6G, else 2G.
        None => {
            if total_memory() >= 6 * BYTES_PER_GB {
                3.0
            } else {
                2.0
            }
        }
    }
}

/// Absolute maximum RAM in gigabytes.
///
/// Ported from the JS, which reserves a quarter of the first 16G and an
/// eighth of anything beyond that.
pub fn absolute_max_ram() -> f64 {
    let mem = total_memory() as f64;
    let g_t16 = mem - (16.0 * BYTES_PER_GB as f64);
    let reserved = if g_t16 > 0.0 {
        (g_t16 / 8.0).trunc() + (16.0 * BYTES_PER_GB as f64) / 4.0
    } else {
        mem / 4.0
    };
    ((mem - reserved) / BYTES_PER_GB as f64).floor()
}

fn resolve_selected_ram(ram: Option<RamHints>) -> String {
    match ram.and_then(|r| r.recommended) {
        Some(rec) => format!("{rec}M"),
        None => {
            let mem = total_memory();
            if mem >= 8 * BYTES_PER_GB {
                "4G".into()
            } else if mem >= 6 * BYTES_PER_GB {
                "3G".into()
            } else {
                "2G".into()
            }
        }
    }
}

fn default_java_config(suggested_major: u32, ram: Option<RamHints>) -> JavaConfig {
    let selected = resolve_selected_ram(ram);
    let jvm_options = if suggested_major > 8 {
        vec![
            "-XX:+UnlockExperimentalVMOptions".into(),
            "-XX:+UseG1GC".into(),
            "-XX:G1NewSizePercent=20".into(),
            "-XX:G1ReservePercent=20".into(),
            "-XX:MaxGCPauseMillis=50".into(),
            "-XX:G1HeapRegionSize=32M".into(),
        ]
    } else {
        vec![
            "-XX:+UseConcMarkSweepGC".into(),
            "-XX:+CMSIncrementalMode".into(),
            "-XX:-UseAdaptiveSizePolicy".into(),
            "-Xmn128M".into(),
        ]
    };

    JavaConfig {
        min_ram: selected.clone(),
        max_ram: selected,
        executable: None,
        jvm_options,
    }
}

// ---------------------------------------------------------------------------
// Manager
// ---------------------------------------------------------------------------

/// In-memory config plus its file backing. Held in Tauri's managed state.
///
/// The JS kept a module-level `config` and required callers to invoke
/// `ConfigManager.save()` explicitly; that contract is preserved rather than
/// auto-saving on mutation, because several call sites deliberately batch
/// several edits before one save.
pub struct ConfigManager {
    inner: Mutex<Option<Config>>,
    /// True when neither the current nor the legacy config file existed at
    /// startup, i.e. this is a fresh install (drives the welcome screen).
    first_launch: bool,
}

impl ConfigManager {
    pub fn new() -> Self {
        let first_launch =
            !paths::config_path().exists() && !paths::legacy_config_path().exists();
        Self {
            inner: Mutex::new(None),
            first_launch,
        }
    }

    pub fn is_first_launch(&self) -> bool {
        self.first_launch
    }

    /// Load config from disk, migrating the legacy location and regenerating
    /// on corruption. Mirrors `ConfigManager.load()`.
    pub fn load(&self) -> Result<()> {
        let path = paths::config_path();
        let legacy = paths::legacy_config_path();

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let config = if path.exists() {
            Self::read_or_default(&path)
        } else if legacy.exists() {
            tracing::info!("Migrating configuration from legacy location.");
            std::fs::rename(&legacy, &path)?;
            Self::read_or_default(&path)
        } else {
            tracing::info!("No configuration found, generating defaults.");
            Config::default()
        };

        *self.inner.lock().unwrap() = Some(config);
        // The JS re-saved after load so newly added default keys are persisted.
        self.save()?;
        tracing::info!("Successfully Loaded");
        Ok(())
    }

    fn read_or_default(path: &PathBuf) -> Config {
        match std::fs::read_to_string(path)
            .map_err(Error::from)
            .and_then(|raw| serde_json::from_str::<Config>(&raw).map_err(Error::from))
        {
            Ok(mut cfg) => {
                for account in cfg.authentication_database.values_mut() {
                    account.unseal_secrets();
                }
                cfg
            }
            Err(err) => {
                tracing::error!(%err, "Configuration file is malformed or corrupt; regenerating.");
                Config::default()
            }
        }
    }

    pub fn save(&self) -> Result<()> {
        let guard = self.inner.lock().unwrap();
        let config = guard.as_ref().ok_or(Error::ConfigNotLoaded)?;
        let path = paths::config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // 4-space indent, matching the Electron output (JSON.stringify(_, _, 4))
        // so a user's config.json doesn't churn when switching between builds.
        // serde_json's to_string_pretty defaults to 2 spaces, hence the
        // explicit formatter.
        // Sealed on a clone. Encrypting in place would leave the live config
        // holding ciphertext, and the next launch would send it to Microsoft
        // as a token.
        let mut on_disk = config.clone();
        for account in on_disk.authentication_database.values_mut() {
            account.seal_secrets();
        }

        let mut buf = Vec::new();
        let formatter = serde_json::ser::PrettyFormatter::with_indent(b"    ");
        let mut ser = serde_json::Serializer::with_formatter(&mut buf, formatter);
        on_disk.serialize(&mut ser)?;
        std::fs::write(&path, buf)?;
        Ok(())
    }

    /// Read-only access to the loaded config.
    pub fn with<T>(&self, f: impl FnOnce(&Config) -> T) -> Result<T> {
        let guard = self.inner.lock().unwrap();
        let config = guard.as_ref().ok_or(Error::ConfigNotLoaded)?;
        Ok(f(config))
    }

    /// Mutable access. Does not save; call `save()` when the batch is done.
    pub fn with_mut<T>(&self, f: impl FnOnce(&mut Config) -> T) -> Result<T> {
        let mut guard = self.inner.lock().unwrap();
        let config = guard.as_mut().ok_or(Error::ConfigNotLoaded)?;
        Ok(f(config))
    }

    /// Ensure a java config exists for `server_id`, creating defaults if not.
    /// Mirrors `ConfigManager.ensureJavaConfig`.
    pub fn ensure_java_config(
        &self,
        server_id: &str,
        suggested_major: u32,
        ram: Option<RamHints>,
    ) -> Result<()> {
        self.with_mut(|config| {
            config
                .java_config
                .entry(server_id.to_string())
                .or_insert_with(|| default_java_config(suggested_major, ram));
        })
    }

    /// Add or replace an offline "lunar" account and select it.
    /// Mirrors `ConfigManager.addLunarAuthAccount`.
    pub fn add_lunar_account(&self, username: &str) -> Result<Account> {
        let uuid = md5_hex(username);
        let expires_at = (chrono::Utc::now() + chrono::Duration::days(365)).to_rfc3339();
        let account = Account::Lunar {
            username: username.to_string(),
            display_name: username.to_string(),
            uuid: uuid.clone(),
            expires_at,
        };
        self.with_mut(|config| {
            config.selected_account = Some(uuid.clone());
            config
                .authentication_database
                .insert(uuid.clone(), account.clone());
            if config.client_token.is_none() {
                config.client_token = Some(uuid.clone());
            }
        })?;
        self.save()?;
        Ok(account)
    }

    /// Remove an account, re-selecting another if the removed one was active.
    /// Mirrors `ConfigManager.removeAuthAccount`.
    pub fn remove_account(&self, uuid: &str) -> Result<bool> {
        let removed = self.with_mut(|config| {
            if config.authentication_database.remove(uuid).is_none() {
                return false;
            }
            if config.selected_account.as_deref() == Some(uuid) {
                config.selected_account = config
                    .authentication_database
                    .keys()
                    .next()
                    .cloned();
            }
            true
        })?;
        if removed {
            self.save()?;
        }
        Ok(removed)
    }
}

impl Default for ConfigManager {
    fn default() -> Self {
        Self::new()
    }
}

/// MD5 hex digest — used for offline account UUIDs, matching the fork's
/// `md5Encode` in authmanager.js.
pub fn md5_hex(input: &str) -> String {
    use md5::{Digest, Md5};
    let mut hasher = Md5::new();
    hasher.update(input.as_bytes());
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn md5_matches_node_crypto() {
        // node: crypto.createHash('md5').update('FlukRocker').digest('hex')
        assert_eq!(md5_hex("FlukRocker").len(), 32);
        assert_eq!(
            md5_hex("abc"),
            "900150983cd24fb0d6963f7d28e17f72"
        );
    }

    #[test]
    fn default_config_roundtrips_as_camel_case() {
        let cfg = Config::default();
        let json = serde_json::to_string(&cfg).unwrap();
        assert!(json.contains("\"selectedServer\""));
        assert!(json.contains("\"authenticationDatabase\""));
        assert!(json.contains("\"javaConfig\""));
        assert!(json.contains("\"launchDetached\""));
    }

    #[test]
    fn partial_config_fills_in_defaults() {
        // Simulates an older config.json missing newly added keys, which the
        // JS handled via validateKeySet.
        let raw = r#"{ "selectedServer": "Some_Server" }"#;
        let cfg: Config = serde_json::from_str(raw).unwrap();
        assert_eq!(cfg.selected_server.as_deref(), Some("Some_Server"));
        assert_eq!(cfg.settings.game.res_width, 1280);
        assert!(cfg.settings.game.auto_connect);
    }

    #[test]
    fn lunar_account_deserialises_from_electron_shape() {
        let raw = r#"{
            "type": "lunar",
            "username": "Steve",
            "displayName": "Steve",
            "uuid": "abc123",
            "expiresAt": "2027-01-01T00:00:00.000Z"
        }"#;
        let acct: Account = serde_json::from_str(raw).unwrap();
        assert_eq!(acct.display_name(), "Steve");
        assert_eq!(acct.uuid(), "abc123");
    }

    /// Guards the whole Electron -> Tauri config compatibility contract.
    ///
    /// This is a real config.json as written by the Electron launcher. If any
    /// field name drifts, deserialisation fails, `read_or_default` falls back
    /// to defaults, and the user silently loses their accounts and settings —
    /// so this asserts on the round trip, not just on parsing.
    #[test]
    fn electron_written_config_survives_a_round_trip() {
        let raw = r#"{
            "settings": {
                "game": {
                    "resWidth": 1600,
                    "resHeight": 900,
                    "fullscreen": false,
                    "autoConnect": true,
                    "launchDetached": true
                },
                "launcher": {
                    "allowPrerelease": false,
                    "dataDirectory": "/home/steve/.lunarlauncher"
                }
            },
            "newsCache": { "date": null, "content": null, "dismissed": false },
            "clientToken": "9a1b2c3d",
            "selectedServer": "Lunar_1.20.1",
            "selectedAccount": "abc123",
            "authenticationDatabase": {
                "abc123": {
                    "type": "lunar",
                    "username": "Steve",
                    "displayName": "Steve",
                    "uuid": "abc123",
                    "expiresAt": "2027-01-01T00:00:00.000Z"
                }
            },
            "modConfigurations": [],
            "javaConfig": {
                "Lunar_1.20.1": {
                    "minRAM": "3G",
                    "maxRAM": "3G",
                    "executable": "/usr/lib/jvm/jdk-17/bin/java",
                    "jvmOptions": ["-XX:+UseG1GC"]
                }
            }
        }"#;

        let cfg: Config = serde_json::from_str(raw).expect("Electron config must parse");

        // Nothing was silently defaulted away.
        assert_eq!(cfg.selected_server.as_deref(), Some("Lunar_1.20.1"));
        assert_eq!(cfg.selected_account.as_deref(), Some("abc123"));
        assert_eq!(cfg.client_token.as_deref(), Some("9a1b2c3d"));
        assert_eq!(cfg.settings.game.res_width, 1600);
        assert_eq!(cfg.authentication_database.len(), 1);

        let java = cfg.java_config.get("Lunar_1.20.1").expect("javaConfig entry");
        assert_eq!(java.min_ram, "3G");
        assert_eq!(java.max_ram, "3G");
        assert_eq!(java.jvm_options, vec!["-XX:+UseG1GC"]);

        // And we write the names back exactly as Electron expects them.
        let out = serde_json::to_string(&cfg).unwrap();
        assert!(out.contains("\"minRAM\""), "must write minRAM, not minRam");
        assert!(out.contains("\"maxRAM\""), "must write maxRAM, not maxRam");
        assert!(out.contains("\"jvmOptions\""));
        assert!(out.contains("\"expiresAt\""));

        // Re-reading our own output must be lossless.
        let again: Config = serde_json::from_str(&out).unwrap();
        assert_eq!(again.java_config["Lunar_1.20.1"].min_ram, "3G");
        assert_eq!(again.selected_account.as_deref(), Some("abc123"));
    }

    #[test]
    fn java_defaults_switch_on_major_version() {
        let j17 = default_java_config(17, None);
        assert!(j17.jvm_options.iter().any(|o| o == "-XX:+UseG1GC"));
        let j8 = default_java_config(8, None);
        assert!(j8.jvm_options.iter().any(|o| o == "-Xmn128M"));
    }
}

#[cfg(test)]
mod credential_encryption_tests {
    use super::*;

    fn microsoft_account() -> Account {
        Account::Microsoft {
            access_token: "mc-access-token".into(),
            username: "steve@example.test".into(),
            uuid: "abc123".into(),
            display_name: "Steve".into(),
            expires_at: "2027-01-01T00:00:00.000Z".into(),
            microsoft: MicrosoftTokens {
                access_token: "ms-access-token".into(),
                refresh_token: "ms-refresh-token".into(),
                expires_at: "2027-01-01T00:00:00.000Z".into(),
            },
        }
    }

    /// The claim the whole feature rests on: a config.json that leaves the
    /// machine carries no usable credential. The refresh token is the one that
    /// matters — it is long-lived and exchangeable without user interaction.
    #[test]
    fn a_saved_account_leaves_no_plaintext_credential_in_the_json() {
        if !crate::secrets::available() {
            eprintln!("no keystore on this host; skipping");
            return;
        }
        let mut account = microsoft_account();
        account.seal_secrets();
        let json = serde_json::to_string(&account).unwrap();

        for secret in ["mc-access-token", "ms-access-token", "ms-refresh-token"] {
            assert!(!json.contains(secret), "{secret} survived into {json}");
        }
        // Non-secret fields stay readable, so the account list still renders
        // even if the keystore later goes away.
        assert!(json.contains("Steve"));
        assert!(json.contains("abc123"));
    }

    #[test]
    fn sealing_then_unsealing_returns_the_original_credentials() {
        if !crate::secrets::available() {
            return;
        }
        let mut account = microsoft_account();
        account.seal_secrets();
        account.unseal_secrets();

        let Account::Microsoft { access_token, microsoft, .. } = &account else {
            panic!("wrong variant");
        };
        assert_eq!(access_token, "mc-access-token");
        assert_eq!(microsoft.access_token, "ms-access-token");
        assert_eq!(microsoft.refresh_token, "ms-refresh-token");
    }

    /// Migration from an Electron-era config, where tokens are bare. They must
    /// keep working; the next save is what encrypts them.
    #[test]
    fn a_legacy_plaintext_account_loads_unchanged() {
        let mut account = microsoft_account();
        account.unseal_secrets();
        let Account::Microsoft { microsoft, .. } = &account else {
            panic!("wrong variant");
        };
        assert_eq!(microsoft.refresh_token, "ms-refresh-token");
    }

    /// A config copied from another machine, or a keystore that was reset.
    /// The token is cleared so the account re-authenticates — but the account
    /// itself survives. Dropping it would repeat the config-wiping bug this
    /// codebase already shipped once.
    #[test]
    fn an_undecryptable_credential_is_cleared_but_the_account_survives() {
        if !crate::secrets::available() {
            return;
        }
        let mut account = microsoft_account();
        account.seal_secrets();

        // Corrupt the sealed refresh token the way a foreign key would.
        if let Account::Microsoft { microsoft, .. } = &mut account {
            microsoft.refresh_token = "enc:v1:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".into();
        }
        account.unseal_secrets();

        let Account::Microsoft { microsoft, display_name, uuid, .. } = &account else {
            panic!("wrong variant");
        };
        assert!(microsoft.refresh_token.is_empty(), "unusable token must not be kept");
        assert_eq!(display_name, "Steve", "the account must survive");
        assert_eq!(uuid, "abc123");
    }

    #[test]
    fn an_offline_account_has_nothing_to_seal() {
        let mut account = Account::Lunar {
            username: "Steve".into(),
            display_name: "Steve".into(),
            uuid: "abc123".into(),
            expires_at: "2027-01-01T00:00:00.000Z".into(),
        };
        let before = serde_json::to_string(&account).unwrap();
        account.seal_secrets();
        assert_eq!(serde_json::to_string(&account).unwrap(), before);
    }

    /// Saving repeatedly must not re-seal an already-sealed value, or the
    /// stored token grows on every write.
    #[test]
    fn repeated_saves_do_not_nest_the_encryption() {
        if !crate::secrets::available() {
            return;
        }
        let mut account = microsoft_account();
        account.seal_secrets();
        let once = serde_json::to_string(&account).unwrap().len();
        account.seal_secrets();
        account.seal_secrets();
        assert_eq!(serde_json::to_string(&account).unwrap().len(), once);
    }
}
