//! Google Official (Gemini) OAuth login commands for the Auth Center.
//!
//! Gemini CLI authenticates via Google's OAuth **loopback** flow (it opens a
//! browser and listens on a localhost callback), not the device-code flow used
//! by GitHub Copilot / Codex / xAI. CC Switch therefore does not implement the
//! OAuth handshake itself; it:
//!
//! 1. Detects login state by reading `~/.gemini/oauth_creds.json`, which the
//!    Gemini CLI writes after a successful browser login.
//! 2. Launches `gemini auth login` in a terminal so the CLI drives the
//!    loopback + browser + token exchange, reusing the shared terminal
//!    launcher.
//! 3. Logs out by deleting `~/.gemini/oauth_creds.json`.

use std::time::{SystemTime, UNIX_EPOCH};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};

use crate::config::delete_file;
use crate::gemini_config::get_gemini_dir;

/// Login state of Google Official (Gemini) as surfaced to the Auth Center.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiOAuthStatus {
    pub authenticated: bool,
    pub email: Option<String>,
    pub message: Option<String>,
}

/// Credential file written by the Gemini CLI after a browser login.
///
/// Mirrors `GeminiOAuthCredsFile` in `services/subscription.rs`, extended with
/// `id_token` so we can surface the logged-in email address.
#[derive(Deserialize)]
struct GeminiOAuthCredsFile {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expiry_date: Option<i64>, // milliseconds since epoch
    id_token: Option<String>,
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

/// Decode the `email` claim from a Google OAuth `id_token` (JWT) payload.
///
/// Only the middle (claims) segment is read; the signature is never verified
/// here because the token came from a file the CLI already validated. Returns
/// `None` on any malformed input rather than failing the status query.
fn extract_email_from_id_token(id_token: &str) -> Option<String> {
    let payload = id_token.split('.').nth(1)?;
    let claims: serde_json::Value =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload).ok()?).ok()?;
    claims
        .get("email")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn read_gemini_oauth_status() -> GeminiOAuthStatus {
    let cred_path = get_gemini_dir().join("oauth_creds.json");

    let content = match std::fs::read_to_string(&cred_path) {
        Ok(c) => c,
        Err(_) => {
            return GeminiOAuthStatus {
                authenticated: false,
                email: None,
                message: Some("未登录".to_string()),
            }
        }
    };

    let creds: GeminiOAuthCredsFile = match serde_json::from_str(&content) {
        Ok(c) => c,
        Err(_) => {
            return GeminiOAuthStatus {
                authenticated: false,
                email: None,
                message: Some("凭据文件解析失败".to_string()),
            }
        }
    };

    let email = creds.id_token.as_deref().and_then(extract_email_from_id_token);

    let has_access_token = creds
        .access_token
        .as_deref()
        .is_some_and(|t| !t.trim().is_empty());
    let has_refresh_token = creds
        .refresh_token
        .as_deref()
        .is_some_and(|t| !t.trim().is_empty());

    // A valid (unexpired) access token is enough. Otherwise fall back to a
    // refresh token, which can still be exchanged for a fresh token.
    let access_token_valid = has_access_token
        && creds
            .expiry_date
            .is_none_or(|expiry| expiry > now_millis());

    let authenticated = access_token_valid || (!has_access_token && has_refresh_token);

    let message = if authenticated {
        None
    } else if has_access_token && has_refresh_token {
        Some("access_token 已过期，可尝试重新登录".to_string())
    } else {
        Some("未登录".to_string())
    };

    GeminiOAuthStatus {
        authenticated,
        email,
        message,
    }
}

/// Query whether Google Official (Gemini) is logged in via the Gemini CLI.
#[tauri::command(rename_all = "camelCase")]
pub async fn gemini_auth_status() -> Result<GeminiOAuthStatus, String> {
    Ok(read_gemini_oauth_status())
}

/// Kick off Gemini CLI's browser OAuth login.
///
/// Ensures `~/.gemini/settings.json` selects `oauth-personal` so the CLI uses
/// OAuth rather than an API key, then launches `gemini auth login` in a
/// terminal. The CLI owns the loopback server + browser + token exchange and
/// writes `~/.gemini/oauth_creds.json` on success.
#[tauri::command(rename_all = "camelCase")]
pub async fn gemini_auth_login() -> Result<(), String> {
    crate::gemini_config::write_google_oauth_settings().map_err(|e| e.to_string())?;

    crate::commands::misc::launch_terminal_running("gemini auth login", "gemini_oauth_login")
        .map_err(|e| e.to_string())
}

/// Log out of Google Official (Gemini) by removing the CLI's credential file.
#[tauri::command(rename_all = "camelCase")]
pub async fn gemini_auth_logout() -> Result<(), String> {
    let cred_path = get_gemini_dir().join("oauth_creds.json");
    if cred_path.exists() {
        delete_file(&cred_path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_email_from_valid_id_token() {
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none"}"#);
        let payload = URL_SAFE_NO_PAD.encode(br#"{"email":"dev@example.com"}"#);
        let id_token = format!("{header}.{payload}.sig");
        assert_eq!(
            extract_email_from_id_token(&id_token).as_deref(),
            Some("dev@example.com")
        );
    }

    #[test]
    fn extract_email_returns_none_on_malformed_token() {
        assert_eq!(extract_email_from_id_token("not-a-jwt"), None);
        assert_eq!(extract_email_from_id_token("a.b.c.d"), None);
        assert_eq!(extract_email_from_id_token(""), None);
    }

    #[test]
    fn extract_email_returns_none_when_claim_absent() {
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none"}"#);
        let payload = URL_SAFE_NO_PAD.encode(br#"{"sub":"123"}"#);
        let id_token = format!("{header}.{payload}.sig");
        assert_eq!(extract_email_from_id_token(&id_token), None);
    }
}
