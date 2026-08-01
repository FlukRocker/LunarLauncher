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
///
/// `redirect_uri` must match the one the code was obtained with; Microsoft
/// validates it on redemption.
pub async fn get_access_token_with_redirect(
    code: &str,
    refresh: bool,
    redirect_uri: &str,
) -> Result<AccessTokenResponse> {
    let grant_type = if refresh { "refresh_token" } else { "authorization_code" };
    let key = if refresh { "refresh_token" } else { "code" };

    let form = [
        ("client_id", AZURE_CLIENT_ID),
        ("scope", "XboxLive.signin offline_access"),
        ("redirect_uri", redirect_uri),
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

/// Convenience wrapper using the embedded-webview redirect URI.
pub async fn get_access_token(code: &str, refresh: bool) -> Result<AccessTokenResponse> {
    get_access_token_with_redirect(code, refresh, REDIRECT_URI).await
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

/// The consent page URL for an arbitrary redirect URI.
pub fn authorize_url_for(redirect_uri: &str) -> String {
    let encoded = urlencoding::encode(redirect_uri);
    format!(
        "https://login.microsoftonline.com/consumers/oauth2/v2.0/authorize\
?prompt=select_account&client_id={AZURE_CLIENT_ID}&response_type=code\
&scope=XboxLive.signin%20offline_access&redirect_uri={encoded}"
    )
}

/// Run the whole chain, redeeming the code against a specific redirect URI.
pub async fn full_auth_flow_with_redirect(
    code: &str,
    refresh: bool,
    redirect_uri: &str,
) -> Result<FullAuth> {
    let ms = get_access_token_with_redirect(code, refresh, redirect_uri).await?;
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

// ---------------------------------------------------------------------------
// Loopback (system browser) flow
// ---------------------------------------------------------------------------

/// Page shown in the browser once the code has been captured.
const DONE_PAGE: &str = "<!doctype html><meta charset=utf-8>\
<title>Lunar Launcher</title>\
<body style=\"background:#171614;color:#fff;font-family:-apple-system,Segoe UI,sans-serif;\
display:flex;align-items:center;justify-content:center;height:100vh;margin:0\">\
<div style=\"text-align:center\"><h2>Signed in</h2>\
<p style=\"opacity:.7\">You can close this tab and return to Lunar Launcher.</p></div>";

const FAIL_PAGE: &str = "<!doctype html><meta charset=utf-8>\
<title>Lunar Launcher</title>\
<body style=\"background:#171614;color:#fff;font-family:-apple-system,Segoe UI,sans-serif;\
display:flex;align-items:center;justify-content:center;height:100vh;margin:0\">\
<div style=\"text-align:center\"><h2>Sign-in failed</h2>\
<p style=\"opacity:.7\">Return to Lunar Launcher for details.</p></div>";

fn http_response(body: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n\
Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

/// Listen on an ephemeral loopback port for the OAuth redirect.
///
/// This is the flow RFC 8252 prescribes for native apps: the consent page
/// opens in the user's own browser — where the address bar is visible and
/// existing sessions and password managers work — and the authorization code
/// comes back to a short-lived local listener rather than being scraped out
/// of an embedded webview.
///
/// Returns the bound redirect URI and a future that resolves with the code.
pub async fn start_loopback() -> Result<(String, tokio::net::TcpListener)> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    Ok((format!("http://127.0.0.1:{port}"), listener))
}

/// Accept exactly one request and pull the `code` (or `error`) from it.
pub async fn await_loopback_code(listener: tokio::net::TcpListener) -> Result<String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let (mut sock, _) = listener.accept().await?;
    let mut buf = vec![0u8; 8192];
    let n = sock.read(&mut buf).await?;
    let req = String::from_utf8_lossy(&buf[..n]);

    // First line: "GET /?code=... HTTP/1.1"
    let target = req
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .unwrap_or("/");
    let full = format!("http://127.0.0.1{target}");

    let outcome = match extract_code_from_redirect(&full) {
        Some(code) => Ok(code),
        None => Err(extract_error_from_redirect(&full)
            .unwrap_or_else(|| "No authorization code was returned.".into())),
    };

    let page = if outcome.is_ok() { DONE_PAGE } else { FAIL_PAGE };
    let _ = sock.write_all(http_response(page).as_bytes()).await;
    let _ = sock.flush().await;

    outcome.map_err(Error::Other)
}

#[cfg(test)]
mod loopback_tests {
    use super::*;

    #[tokio::test]
    async fn loopback_binds_an_ephemeral_port() {
        let (uri, listener) = start_loopback().await.unwrap();
        assert!(uri.starts_with("http://127.0.0.1:"));
        let port: u16 = uri.rsplit(':').next().unwrap().parse().unwrap();
        assert!(port > 0);
        drop(listener);
    }

    #[tokio::test]
    async fn loopback_captures_the_code_from_a_real_request() {
        let (uri, listener) = start_loopback().await.unwrap();
        let port: u16 = uri.rsplit(':').next().unwrap().parse().unwrap();

        let task = tokio::spawn(await_loopback_code(listener));

        // Pretend to be the browser following the redirect.
        use tokio::io::AsyncWriteExt;
        let mut c = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        c.write_all(b"GET /?code=M.C123_ABC&state=x HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .await
            .unwrap();

        assert_eq!(task.await.unwrap().unwrap(), "M.C123_ABC");
    }

    #[tokio::test]
    async fn loopback_surfaces_a_denied_consent() {
        let (uri, listener) = start_loopback().await.unwrap();
        let port: u16 = uri.rsplit(':').next().unwrap().parse().unwrap();
        let task = tokio::spawn(await_loopback_code(listener));

        use tokio::io::AsyncWriteExt;
        let mut c = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        c.write_all(b"GET /?error=access_denied&error_description=User%20cancelled HTTP/1.1\r\n\r\n")
            .await
            .unwrap();

        let err = task.await.unwrap().unwrap_err().to_string();
        assert!(err.contains("access_denied"), "got: {err}");
    }

    #[test]
    fn authorize_url_encodes_the_loopback_redirect() {
        let u = authorize_url_for("http://127.0.0.1:51234");
        assert!(u.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A51234"));
    }
}
