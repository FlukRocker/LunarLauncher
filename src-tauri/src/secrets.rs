//! Encryption for the credentials in `config.json`.
//!
//! # What this protects against, and what it does not
//!
//! The launcher stores a Microsoft access token, a refresh token and an Xbox
//! session. The refresh token is the valuable one: it is long-lived and can be
//! exchanged for new access tokens without the user doing anything, so a copy
//! of `config.json` is a durable account compromise.
//!
//! Sealing those fields with a key held in the operating system's keystore
//! defeats every attack that moves the *file* without the key:
//!
//!  * a backup, or a folder synced to OneDrive/iCloud/Dropbox
//!  * another user account on the same machine reading the file
//!  * a support diagnostic, or a config pasted into a thread
//!  * an infostealer that collects known config paths, which is how most of
//!    them work
//!
//! It does **not** make the token unreadable to a program already running as
//! this user. On Windows the Credential Manager and on Linux the Secret
//! Service will hand the key to any process with the user's identity — the
//! same identity the launcher itself runs under. macOS is somewhat better,
//! since a keychain item is bound to the signing identity that created it, but
//! this application is not yet code-signed and an ad-hoc signature does not
//! give a durable ACL.
//!
//! So: real protection against the file leaving the machine, partial at best
//! against local malware. Claiming more than that would be dishonest, and the
//! honest version is still worth having — file exfiltration is the common
//! case.
//!
//! # A macOS cost worth knowing about
//!
//! A keychain item's access control is bound to the *signing identity* of the
//! binary that created it. This application is not code-signed, so every build
//! is a different identity and macOS asks the user to approve access again —
//! measurably: the test suite takes 109 seconds after a rebuild and 8 seconds
//! on the same binary, and the difference is a dialog.
//!
//! For a user that means one keychain prompt per launcher update, not per
//! launch. Code signing with a stable Developer ID removes it entirely; until
//! then it is the price of not keeping the refresh token in the clear.
//! Windows does not prompt at all, and Linux prompts once per session to
//! unlock the keyring.
//!
//! # Failure behaviour
//!
//! A missing keystore never blocks the launcher and never discards a config.
//! Losing a config to an over-eager safety check is a worse outcome than
//! storing a token in the clear, and this codebase has already shipped one bug
//! that silently wiped a user's accounts.

use base64::Engine as _;
use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    XChaCha20Poly1305, XNonce,
};

/// Marks a sealed value. Anything without it is legacy plaintext, which is
/// what makes migration from an Electron-era config automatic.
const PREFIX: &str = "enc:v1:";

const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 24;

/// Keystore account name holding the data key.
const KEY_ENTRY: &str = "token-encryption-key";

/// Keystore service name.
///
/// Follows the bundle identifier so a branded build gets its own key rather
/// than reading — or overwriting — another brand's.
fn service() -> &'static str {
    match option_env!("LUNAR_APP_IDENTIFIER") {
        Some(id) => id,
        None => "net.hermes-mc.lunarlauncher",
    }
}

/// Resolved once. Re-asking the keystore per field would prompt repeatedly on
/// platforms that prompt at all.
static KEY: std::sync::OnceLock<Option<[u8; KEY_LEN]>> = std::sync::OnceLock::new();

fn random(buf: &mut [u8]) -> Result<(), String> {
    getrandom::getrandom(buf).map_err(|e| format!("no system randomness: {e}"))
}

/// Fetch the data key, creating one on first run.
fn load_or_create_key() -> Result<[u8; KEY_LEN], String> {
    let entry = keyring::Entry::new(service(), KEY_ENTRY)
        .map_err(|e| format!("keystore unavailable: {e}"))?;

    match entry.get_password() {
        Ok(encoded) => {
            let raw = base64::engine::general_purpose::STANDARD
                .decode(encoded.trim())
                .map_err(|e| format!("stored key is not valid base64: {e}"))?;
            let key: [u8; KEY_LEN] = raw
                .try_into()
                .map_err(|_| "stored key is the wrong length".to_string())?;
            Ok(key)
        }
        Err(keyring::Error::NoEntry) => {
            let mut key = [0u8; KEY_LEN];
            random(&mut key)?;
            entry
                .set_password(&base64::engine::general_purpose::STANDARD.encode(key))
                .map_err(|e| format!("could not store key: {e}"))?;
            tracing::info!("Created a new credential-encryption key in the system keystore");
            Ok(key)
        }
        Err(err) => Err(format!("keystore read failed: {err}")),
    }
}

fn key() -> Option<&'static [u8; KEY_LEN]> {
    KEY.get_or_init(|| match load_or_create_key() {
        Ok(k) => Some(k),
        Err(err) => {
            tracing::warn!(
                %err,
                "Credentials will be stored unencrypted. On a desktop this usually means the \
                 keyring is locked; on a headless system there may be no Secret Service at all."
            );
            None
        }
    })
    .as_ref()
}

/// Whether credentials written now would be encrypted.
///
/// Surfaced so the UI and the diagnostic report can say which it is, rather
/// than the user having to infer it from the file.
pub fn available() -> bool {
    key().is_some()
}

/// Encrypt a credential.
///
/// `context` is authenticated but not secret. It binds a ciphertext to the
/// field it came from, so a sealed access token cannot be moved into the
/// refresh token slot and still decrypt.
///
/// Returns the input unchanged when no keystore is available — see the module
/// note on failure behaviour.
pub fn seal(plaintext: &str, context: &str) -> String {
    if plaintext.is_empty() || plaintext.starts_with(PREFIX) {
        return plaintext.to_string();
    }
    let Some(key) = key() else {
        return plaintext.to_string();
    };

    let mut nonce = [0u8; NONCE_LEN];
    if let Err(err) = random(&mut nonce) {
        tracing::error!(%err, "storing credential unencrypted");
        return plaintext.to_string();
    }

    let cipher = XChaCha20Poly1305::new(key.into());
    let sealed = cipher.encrypt(
        XNonce::from_slice(&nonce),
        Payload { msg: plaintext.as_bytes(), aad: context.as_bytes() },
    );

    match sealed {
        Ok(ct) => {
            let mut blob = nonce.to_vec();
            blob.extend_from_slice(&ct);
            format!("{PREFIX}{}", base64::engine::general_purpose::STANDARD.encode(blob))
        }
        Err(err) => {
            tracing::error!(%err, "encryption failed; storing credential unencrypted");
            plaintext.to_string()
        }
    }
}

/// Decrypt a credential.
///
/// A value with no prefix is returned as-is: that is an Electron-era config,
/// or one written while the keystore was unavailable, and it is still valid.
///
/// `Err` means the value *is* sealed but could not be opened — a config copied
/// from another machine, or a keystore that was reset. The caller clears the
/// token so the account re-authenticates; it must not discard the account, or
/// a transient keychain problem would look like the user being logged out of
/// everything permanently.
pub fn open(value: &str, context: &str) -> Result<String, String> {
    let Some(encoded) = value.strip_prefix(PREFIX) else {
        return Ok(value.to_string());
    };
    let key = key().ok_or("no keystore available to decrypt with")?;

    let blob = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|e| format!("not valid base64: {e}"))?;
    if blob.len() <= NONCE_LEN {
        return Err("ciphertext is truncated".into());
    }
    let (nonce, ct) = blob.split_at(NONCE_LEN);

    let cipher = XChaCha20Poly1305::new(key.into());
    let plain = cipher
        .decrypt(
            XNonce::from_slice(nonce),
            Payload { msg: ct, aad: context.as_bytes() },
        )
        .map_err(|_| "wrong key, or the value was tampered with".to_string())?;

    String::from_utf8(plain).map_err(|e| format!("decrypted value is not text: {e}"))
}

/// Whether a stored value is sealed. Used by tests and the diagnostic report.
pub fn is_sealed(value: &str) -> bool {
    value.starts_with(PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Turns the skip branches below into a signal rather than a silence.
    ///
    /// Every other test in this module returns early when no keystore is
    /// present, which means a platform that quietly lost keystore support
    /// would show a green suite that asserted nothing. Linux is exempt because
    /// a headless container genuinely has no Secret Service, and the
    /// documented fallback there is plaintext with a warning.
    #[test]
    fn the_keystore_is_usable_on_the_platforms_that_have_one() {
        if cfg!(target_os = "linux") {
            return;
        }
        assert!(
            available(),
            "no keystore: credentials would silently be stored in the clear"
        );
    }

    /// The round trip, exercised only where a keystore actually exists. CI
    /// containers have no Secret Service, and a test that silently passed by
    /// falling through to plaintext would assert nothing.
    #[test]
    fn a_sealed_credential_round_trips() {
        if !available() {
            eprintln!("no keystore on this host; skipping");
            return;
        }
        let sealed = seal("refresh-token-abc", "microsoft.refreshToken");
        assert!(is_sealed(&sealed), "should be marked sealed");
        assert!(!sealed.contains("refresh-token-abc"), "plaintext must not survive");
        assert_eq!(open(&sealed, "microsoft.refreshToken").unwrap(), "refresh-token-abc");
    }

    /// Ciphertext must not be movable between fields. Without the AAD binding,
    /// a sealed access token dropped into the refresh token slot would decrypt
    /// cleanly and be sent to Microsoft as a refresh token.
    #[test]
    fn a_ciphertext_does_not_open_under_a_different_context() {
        if !available() {
            return;
        }
        let sealed = seal("secret", "accessToken");
        assert!(open(&sealed, "microsoft.refreshToken").is_err());
    }

    #[test]
    fn two_seals_of_the_same_value_differ() {
        if !available() {
            return;
        }
        // A fixed nonce would make identical tokens visibly identical on disk.
        assert_ne!(seal("same", "ctx"), seal("same", "ctx"));
    }

    /// Migration: an Electron-era config has bare tokens, and they must keep
    /// working rather than being treated as corrupt.
    #[test]
    fn legacy_plaintext_opens_unchanged() {
        assert_eq!(open("bare-token", "accessToken").unwrap(), "bare-token");
        assert!(!is_sealed("bare-token"));
    }

    #[test]
    fn an_empty_value_is_left_alone() {
        assert_eq!(seal("", "accessToken"), "");
        assert_eq!(open("", "accessToken").unwrap(), "");
    }

    /// Sealing twice must not nest, or the value grows on every save.
    #[test]
    fn sealing_an_already_sealed_value_is_a_no_op() {
        if !available() {
            return;
        }
        let once = seal("token", "ctx");
        assert_eq!(seal(&once, "ctx"), once);
    }

    #[test]
    fn a_tampered_ciphertext_is_rejected_not_returned() {
        if !available() {
            return;
        }
        let sealed = seal("token", "ctx");
        let mut bytes = sealed.clone().into_bytes();
        let last = bytes.len() - 1;
        bytes[last] = if bytes[last] == b'A' { b'B' } else { b'A' };
        let tampered = String::from_utf8(bytes).unwrap();
        assert!(open(&tampered, "ctx").is_err());
    }

    #[test]
    fn a_truncated_ciphertext_is_an_error_not_a_panic() {
        let short = format!("{PREFIX}{}", base64::engine::general_purpose::STANDARD.encode([0u8; 8]));
        assert!(open(&short, "ctx").is_err());
    }

    #[test]
    fn garbage_after_the_prefix_is_an_error_not_a_panic() {
        assert!(open("enc:v1:not base64!!", "ctx").is_err());
    }
}
