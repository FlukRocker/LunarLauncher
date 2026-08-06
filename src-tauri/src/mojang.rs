//! Yggdrasil authentication — the "Mojang" login method.
//!
//! Mojang's own `authserver.mojang.com` was permanently shut down, so the
//! default endpoint no longer answers. The protocol lives on, though: private
//! servers commonly run Yggdrasil-compatible auth (authlib-injector, ely.by
//! and others), so the endpoint is configurable rather than hardcoded, which
//! is what makes this method useful at all today.

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Mojang's original endpoint. Retained as the default for compatibility;
/// it no longer resolves, so a custom one is normally required.
pub const DEFAULT_AUTH_SERVER: &str = "https://authserver.mojang.com";

/// Environment override, mirroring how the distribution URL is handled.
pub const AUTH_SERVER_ENV: &str = "LUNAR_AUTH_SERVER";

/// Runtime environment, then the build-time value, then the default. Same
/// chain as the distribution URL; see `DistroSource::resolve`.
pub fn auth_server() -> String {
    std::env::var(AUTH_SERVER_ENV)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .or_else(|| option_env!("LUNAR_AUTH_SERVER").map(str::to_string))
        .filter(|v| !v.trim().is_empty())
        .map(|v| v.trim().trim_end_matches('/').to_string())
        .unwrap_or_else(|| DEFAULT_AUTH_SERVER.to_string())
}

#[derive(Debug, Clone, Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthResponse {
    pub access_token: String,
    pub client_token: String,
    #[serde(default)]
    pub selected_profile: Option<Profile>,
}

#[derive(Debug, Serialize)]
struct Agent {
    name: &'static str,
    version: u8,
}

/// `POST /authenticate`. `username` is an email for Mojang accounts, but
/// custom servers often accept a plain username.
pub async fn authenticate(
    username: &str,
    password: &str,
    client_token: &str,
) -> Result<AuthResponse> {
    let body = serde_json::json!({
        "agent": Agent { name: "Minecraft", version: 1 },
        "username": username,
        "password": password,
        "clientToken": client_token,
        "requestUser": true
    });

    let url = format!("{}/authenticate", auth_server());
    let resp = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()?
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| {
            // A dead host is by far the most likely failure now, so say so
            // rather than surfacing a bare transport error.
            Error::Other(format!(
                "Could not reach the authentication server at {}. \
                 Mojang's own server is shut down — set {AUTH_SERVER_ENV} to your \
                 server's Yggdrasil endpoint. ({e})",
                auth_server()
            ))
        })?;

    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        let msg = if status == reqwest::StatusCode::FORBIDDEN {
            "Invalid username or password.".to_string()
        } else if text.contains("UserMigratedException") {
            "This account was migrated to a Microsoft account. Use Microsoft sign-in."
                .to_string()
        } else {
            format!("Authentication failed ({status}).")
        };
        return Err(Error::Other(msg));
    }

    let auth: AuthResponse = resp.json().await?;
    if auth.selected_profile.is_none() {
        return Err(Error::Other(
            "This account has no Minecraft profile attached.".into(),
        ));
    }
    Ok(auth)
}

/// `POST /validate` — is the access token still usable?
pub async fn validate(access_token: &str, client_token: &str) -> bool {
    let body = serde_json::json!({
        "accessToken": access_token,
        "clientToken": client_token
    });
    match reqwest::Client::new()
        .post(format!("{}/validate", auth_server()))
        .json(&body)
        .send()
        .await
    {
        Ok(r) => r.status().is_success(),
        Err(_) => false,
    }
}

/// `POST /invalidate` — best effort; failure must not block sign-out.
pub async fn invalidate(access_token: &str, client_token: &str) {
    let body = serde_json::json!({
        "accessToken": access_token,
        "clientToken": client_token
    });
    let _ = reqwest::Client::new()
        .post(format!("{}/invalidate", auth_server()))
        .json(&body)
        .send()
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_defaults_to_mojang_and_honours_the_override() {
        std::env::remove_var(AUTH_SERVER_ENV);
        assert_eq!(auth_server(), DEFAULT_AUTH_SERVER);

        std::env::set_var(AUTH_SERVER_ENV, "https://auth.example.com/");
        assert_eq!(auth_server(), "https://auth.example.com");

        // Blank is treated as unset rather than as an empty endpoint.
        std::env::set_var(AUTH_SERVER_ENV, "   ");
        assert_eq!(auth_server(), DEFAULT_AUTH_SERVER);
        std::env::remove_var(AUTH_SERVER_ENV);
    }

    #[test]
    fn auth_response_parses_the_yggdrasil_shape() {
        let raw = r#"{
            "accessToken": "abc",
            "clientToken": "def",
            "selectedProfile": { "id": "uuid123", "name": "Steve" },
            "availableProfiles": []
        }"#;
        let a: AuthResponse = serde_json::from_str(raw).unwrap();
        assert_eq!(a.access_token, "abc");
        assert_eq!(a.selected_profile.unwrap().name, "Steve");
    }

    #[test]
    fn response_without_a_profile_is_detectable() {
        let a: AuthResponse =
            serde_json::from_str(r#"{"accessToken":"a","clientToken":"b"}"#).unwrap();
        assert!(a.selected_profile.is_none());
    }
}
