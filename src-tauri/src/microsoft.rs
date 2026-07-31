//! Port of `helios-core`'s MicrosoftAuth plus the OAuth window flow that
//! lived in `index.js`.
//!
//! The chain is: authorization code -> Microsoft access token -> Xbox Live
//! token -> XSTS token -> Minecraft access token -> Minecraft profile.
//! Each hop is a distinct failure mode, so errors name the step that failed —
//! "your Microsoft login worked but you don't own the game" is a very
//! different message from "the network is down".

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Azure application id. Carried over from `ipcconstants.js`.
///
/// Third parties forking this launcher must register their own; see
/// docs/MicrosoftAuth.md.
pub const AZURE_CLIENT_ID: &str = "1ce6e35a-126f-48fd-97fb-54d143ac6d45";

pub const REDIRECT_URI: &str =
    "https://login.microsoftonline.com/common/oauth2/nativeclient";
pub const TOKEN_ENDPOINT: &str =
    "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";
pub const XBL_AUTH_ENDPOINT: &str = "https://user.auth.xboxlive.com/user/authenticate";
pub const XSTS_AUTH_ENDPOINT: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";
pub const MC_AUTH_ENDPOINT: &str =
    "https://api.minecraftservices.com/authentication/login_with_xbox";
pub const MC_PROFILE_ENDPOINT: &str = "https://api.minecraftservices.com/minecraft/profile";
pub const LOGOUT_ENDPOINT: &str =
    "https://login.microsoftonline.com/common/oauth2/v2.0/logout";

/// The consent page the login window opens.
pub fn authorize_url() -> String {
    format!(
        "https://login.microsoftonline.com/consumers/oauth2/v2.0/authorize\
?prompt=select_account&client_id={AZURE_CLIENT_ID}&response_type=code\
&scope=XboxLive.signin%20offline_access&redirect_uri={REDIRECT_URI}"
    )
}

// ---------------------------------------------------------------------------
// Wire types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct AccessTokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DisplayClaims {
    pub xui: Vec<XuiClaim>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct XuiClaim {
    pub uhs: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct XblResponse {
    pub token: String,
    pub display_claims: DisplayClaims,
}

#[derive(Debug, Clone, Deserialize)]
pub struct McTokenResponse {
    pub access_token: String,
    pub expires_in: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct McProfile {
    pub id: String,
    pub name: String,
}

/// Everything a completed login yields.
#[derive(Debug, Clone)]
pub struct FullAuth {
    pub ms_access_token: String,
    pub ms_refresh_token: String,
    pub ms_expires_in: i64,
    pub mc_access_token: String,
    pub mc_expires_in: i64,
    pub profile: McProfile,
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .expect("http client")
}

// ---------------------------------------------------------------------------
// The chain
// ---------------------------------------------------------------------------

/// Exchange an authorization code (or refresh token) for a Microsoft token.
pub async fn get_access_token(code: &str, refresh: bool) -> Result<AccessTokenResponse> {
    let grant_type = if refresh { "refresh_token" } else { "authorization_code" };
    let key = if refresh { "refresh_token" } else { "code" };

    let form = [
        ("client_id", AZURE_CLIENT_ID),
        ("scope", "XboxLive.signin offline_access"),
        ("redirect_uri", REDIRECT_URI),
        (key, code),
        ("grant_type", grant_type),
    ];

    let resp = client().post(TOKEN_ENDPOINT).form(&form).send().await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(Error::Other(format!(
            "Microsoft rejected the {} request ({status}): {body}",
            if refresh { "token refresh" } else { "sign-in" }
        )));
    }
    Ok(resp.json().await?)
}

/// Authenticate with Xbox Live using a Microsoft access token.
pub async fn get_xbl_token(ms_access_token: &str) -> Result<XblResponse> {
    let body = serde_json::json!({
        "Properties": {
            "AuthMethod": "RPS",
            "SiteName": "user.auth.xboxlive.com",
            "RpsTicket": format!("d={ms_access_token}")
        },
        "RelyingParty": "http://auth.xboxlive.com",
        "TokenType": "JWT"
    });

    let resp = client()
        .post(XBL_AUTH_ENDPOINT)
        .header("Accept", "application/json")
        .json(&body)
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(Error::Other(format!(
            "Xbox Live authentication failed ({}).",
            resp.status()
        )));
    }
    Ok(resp.json().await?)
}

/// Exchange an Xbox Live token for an XSTS token.
///
/// XSTS is where the well-known account errors surface, so its specific
/// failure codes are translated into messages a player can act on.
pub async fn get_xsts_token(xbl: &XblResponse) -> Result<XblResponse> {
    let body = serde_json::json!({
        "Properties": {
            "SandboxId": "RETAIL",
            "UserTokens": [xbl.token]
        },
        "RelyingParty": "rp://api.minecraftservices.com/",
        "TokenType": "JWT"
    });

    let resp = client()
        .post(XSTS_AUTH_ENDPOINT)
        .header("Accept", "application/json")
        .json(&body)
        .send()
        .await?;

    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        // XErr codes documented at wiki.vg/Microsoft_Authentication_Scheme
        let msg = if text.contains("2148916233") {
            "This Microsoft account has no Xbox profile. Create one at xbox.com and try again."
        } else if text.contains("2148916235") {
            "Xbox Live is not available in this account's region."
        } else if text.contains("2148916238") {
            "This account is a child account and must be added to a family before it can sign in."
        } else if text.contains("2148916227") {
            "This account has been banned from Xbox Live."
        } else {
            "Xbox security token request failed."
        };
        return Err(Error::Other(format!("{msg} ({status})")));
    }
    Ok(resp.json().await?)
}

/// Exchange an XSTS token for a Minecraft access token.
pub async fn get_mc_access_token(xsts: &XblResponse) -> Result<McTokenResponse> {
    let uhs = xsts
        .display_claims
        .xui
        .first()
        .ok_or_else(|| Error::Other("XSTS response contained no user hash.".into()))?
        .uhs
        .clone();

    let body = serde_json::json!({
        "identityToken": format!("XBL3.0 x={uhs};{}", xsts.token)
    });

    let resp = client()
        .post(MC_AUTH_ENDPOINT)
        .header("Accept", "application/json")
        .json(&body)
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(Error::Other(format!(
            "Minecraft services rejected the Xbox token ({}).",
            resp.status()
        )));
    }
    Ok(resp.json().await?)
}

/// Fetch the Minecraft profile.
///
/// A 404 here specifically means the account does not own Minecraft (or is a
/// Game Pass account that has never launched it), which is worth saying
/// plainly rather than reporting as a generic HTTP error.
pub async fn get_mc_profile(mc_access_token: &str) -> Result<McProfile> {
    let resp = client()
        .get(MC_PROFILE_ENDPOINT)
        .bearer_auth(mc_access_token)
        .send()
        .await?;

    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(Error::Other(
            "This account does not own Minecraft: Java Edition. If you have Game Pass, \
             launch the game once through the official launcher first."
                .into(),
        ));
    }
    if !resp.status().is_success() {
        return Err(Error::Other(format!(
            "Failed to fetch the Minecraft profile ({}).",
            resp.status()
        )));
    }
    Ok(resp.json().await?)
}

/// Run the whole chain from an authorization code or a refresh token.
pub async fn full_auth_flow(code: &str, refresh: bool) -> Result<FullAuth> {
    let ms = get_access_token(code, refresh).await?;
    let xbl = get_xbl_token(&ms.access_token).await?;
    let xsts = get_xsts_token(&xbl).await?;
    let mc = get_mc_access_token(&xsts).await?;
    let profile = get_mc_profile(&mc.access_token).await?;

    Ok(FullAuth {
        ms_access_token: ms.access_token,
        ms_refresh_token: ms.refresh_token,
        ms_expires_in: ms.expires_in,
        mc_access_token: mc.access_token,
        mc_expires_in: mc.expires_in,
        profile,
    })
}

/// Extract the `code` parameter from the OAuth redirect URI.
///
/// Upstream fixed a bug here (#388): the original split on `&` and called
/// `decodeURI` per value, which mangled codes containing encoded characters.
/// Using a real URL parser avoids the whole class of problem.
pub fn extract_code_from_redirect(uri: &str) -> Option<String> {
    let url = reqwest::Url::parse(uri).ok()?;
    url.query_pairs()
        .find(|(k, _)| k == "code")
        .map(|(_, v)| v.into_owned())
}

/// Some redirects carry an error instead of a code (user cancelled, consent
/// denied). Surfacing the description beats a silent hang.
pub fn extract_error_from_redirect(uri: &str) -> Option<String> {
    let url = reqwest::Url::parse(uri).ok()?;
    let pairs: std::collections::HashMap<_, _> = url.query_pairs().collect();
    let err = pairs.get("error")?;
    let desc = pairs
        .get("error_description")
        .map(|d| d.to_string())
        .unwrap_or_default();
    Some(if desc.is_empty() {
        err.to_string()
    } else {
        format!("{err}: {desc}")
    })
}

/// Expiry timestamp `expires_in` seconds from now, in the RFC3339 form the
/// Electron config stored.
pub fn expiry_from_now(expires_in: i64) -> String {
    (chrono::Utc::now() + chrono::Duration::seconds(expires_in)).to_rfc3339()
}

/// Has this timestamp passed?
pub fn is_expired(timestamp: &str) -> bool {
    match chrono::DateTime::parse_from_rfc3339(timestamp) {
        Ok(t) => chrono::Utc::now() >= t,
        // An unparseable expiry is treated as expired so we refresh rather
        // than launch with a token the server will reject.
        Err(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_code_from_redirect() {
        let uri = format!("{REDIRECT_URI}?code=M.C123_BAY.2.U.abc-def&state=x");
        assert_eq!(
            extract_code_from_redirect(&uri).as_deref(),
            Some("M.C123_BAY.2.U.abc-def")
        );
    }

    #[test]
    fn handles_percent_encoded_codes() {
        // The bug upstream fixed in #388: naive splitting mangled these.
        let uri = format!("{REDIRECT_URI}?code=abc%2Bdef%3Dghi");
        assert_eq!(extract_code_from_redirect(&uri).as_deref(), Some("abc+def=ghi"));
    }

    #[test]
    fn no_code_when_absent() {
        assert!(extract_code_from_redirect(&format!("{REDIRECT_URI}?state=x")).is_none());
        assert!(extract_code_from_redirect("not a url").is_none());
    }

    #[test]
    fn extracts_error_description() {
        let uri = format!("{REDIRECT_URI}?error=access_denied&error_description=User%20cancelled");
        assert_eq!(
            extract_error_from_redirect(&uri).as_deref(),
            Some("access_denied: User cancelled")
        );
        assert!(extract_error_from_redirect(&format!("{REDIRECT_URI}?code=abc")).is_none());
    }

    #[test]
    fn expiry_round_trips_and_compares() {
        let future = expiry_from_now(3600);
        assert!(!is_expired(&future));
        let past = expiry_from_now(-10);
        assert!(is_expired(&past));
        // Garbage is treated as expired rather than valid.
        assert!(is_expired("not-a-timestamp"));
    }

    #[test]
    fn authorize_url_carries_required_params() {
        let u = authorize_url();
        assert!(u.contains(AZURE_CLIENT_ID));
        assert!(u.contains("response_type=code"));
        assert!(u.contains("XboxLive.signin"));
        assert!(u.contains("offline_access"));
    }
}
