use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::{Duration as ChronoDuration, Utc};
use reqwest::Client;
use ring::digest::{digest, SHA256};
use serde::{Deserialize, Serialize};

use crate::error::{ParaError, Result};
use crate::keyvault::vault::KeyVault;
use crate::store::db::{CalendarAccountRow, CalendarConfigRow, LocalStore, UpcomingCalendarEventRow};

const GOOGLE_PROVIDER: &str = "google";
const MICROSOFT_PROVIDER: &str = "microsoft";
const GOOGLE_SCOPE: &str =
    "openid email profile https://www.googleapis.com/auth/calendar.readonly";
const MICROSOFT_SCOPE: &str = "offline_access openid profile email User.Read Calendars.Read";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CalendarTokenBundle {
    access_token: String,
    refresh_token: Option<String>,
    expires_at: i64,
}

#[derive(Debug, Deserialize)]
struct GoogleTokenResp {
    access_token: String,
    expires_in: i64,
    refresh_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GoogleUserInfoResp {
    email: Option<String>,
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GoogleEventsResp {
    items: Vec<GoogleEventItem>,
}

#[derive(Debug, Deserialize)]
struct GoogleEventItem {
    id: String,
    summary: Option<String>,
    location: Option<String>,
    organizer: Option<GoogleEventOrganizer>,
    attendees: Option<Vec<GoogleEventAttendee>>,
    start: GoogleEventDateTime,
    end: GoogleEventDateTime,
}

#[derive(Debug, Deserialize)]
struct GoogleEventAttendee {
    email: Option<String>,
    #[serde(rename = "displayName")]
    display_name: Option<String>,
    #[serde(rename = "self")]
    is_self: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct GoogleEventOrganizer {
    email: Option<String>,
    display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GoogleEventDateTime {
    #[serde(rename = "dateTime")]
    date_time: Option<String>,
    date: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MicrosoftTokenResp {
    access_token: String,
    expires_in: i64,
    refresh_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MicrosoftMeResp {
    #[serde(rename = "displayName")]
    display_name: Option<String>,
    mail: Option<String>,
    #[serde(rename = "userPrincipalName")]
    user_principal_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MicrosoftEventsResp {
    value: Vec<MicrosoftEventItem>,
}

#[derive(Debug, Deserialize)]
struct MicrosoftEventItem {
    id: String,
    subject: Option<String>,
    location: Option<MicrosoftLocation>,
    organizer: Option<MicrosoftOrganizer>,
    attendees: Option<Vec<MicrosoftAttendee>>,
    start: MicrosoftDateTime,
    end: MicrosoftDateTime,
}

#[derive(Debug, Deserialize)]
struct MicrosoftAttendee {
    #[serde(rename = "emailAddress")]
    email_address: Option<MicrosoftEmailAddress>,
}

#[derive(Debug, Deserialize)]
struct MicrosoftLocation {
    #[serde(rename = "displayName")]
    display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MicrosoftOrganizer {
    #[serde(rename = "emailAddress")]
    email_address: Option<MicrosoftEmailAddress>,
}

#[derive(Debug, Deserialize)]
struct MicrosoftEmailAddress {
    name: Option<String>,
    address: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MicrosoftDateTime {
    #[serde(rename = "dateTime")]
    date_time: String,
}

pub fn list_provider_statuses(store: &LocalStore, keyvault: &KeyVault) -> Result<Vec<CalendarConfigRow>> {
    let accounts = store.list_calendar_accounts()?;
    Ok([GOOGLE_PROVIDER, MICROSOFT_PROVIDER]
        .iter()
        .map(|provider| {
            let account = accounts.iter().find(|row| row.provider == *provider);
            let config = get_provider_config(keyvault, provider);
            CalendarConfigRow {
                provider: provider.to_string(),
                configured: config.is_some(),
                connected: account.is_some() && load_tokens(keyvault, provider).is_some(),
                email: account.and_then(|row| row.email.clone()),
                display_name: account.and_then(|row| row.display_name.clone()),
                client_id: config.as_ref().map(|(id, _, _)| id.clone()),
                client_secret: config.as_ref().and_then(|(_, sec, _)| sec.clone()),
                tenant_id: config.and_then(|(_, _, t)| t),
            }
        })
        .collect())
}

pub fn save_provider_config(
    keyvault: &KeyVault,
    provider: &str,
    client_id: &str,
    client_secret: Option<&str>,
    tenant_id: Option<&str>,
) -> Result<()> {
    let provider = normalize_provider(provider)?;
    if client_id.trim().is_empty() {
      return Err(ParaError::InvalidState("client ID is required".into()));
    }
    keyvault
        .set_secret(&format!("calendar:{}:client_id", provider), client_id.trim())
        .map_err(|e| ParaError::KeyVault(e.to_string()))?;
    if let Some(secret) = client_secret {
        if !secret.trim().is_empty() {
            keyvault
                .set_secret(&format!("calendar:{}:client_secret", provider), secret.trim())
                .map_err(|e| ParaError::KeyVault(e.to_string()))?;
        }
    } else {
        let _ = keyvault.delete_secret(&format!("calendar:{}:client_secret", provider));
    }
    if provider == MICROSOFT_PROVIDER {
        let tenant = tenant_id.unwrap_or("common").trim();
        keyvault
            .set_secret(&format!("calendar:{}:tenant_id", provider), tenant)
            .map_err(|e| ParaError::KeyVault(e.to_string()))?;
    }
    Ok(())
}

pub fn disconnect_provider(store: &LocalStore, keyvault: &KeyVault, provider: &str) -> Result<()> {
    let provider = normalize_provider(provider)?;
    let _ = keyvault.delete_secret(&format!("calendar:{}:tokens", provider));
    let _ = keyvault.delete_secret(&format!("calendar:{}:client_id", provider));
    let _ = keyvault.delete_secret(&format!("calendar:{}:client_secret", provider));
    if provider == MICROSOFT_PROVIDER {
        let _ = keyvault.delete_secret(&format!("calendar:{}:tenant_id", provider));
    }
    store.delete_calendar_account(provider)
}

pub async fn connect_provider(
    store: &LocalStore,
    keyvault: &KeyVault,
    provider: &str,
) -> Result<CalendarAccountRow> {
    let provider = normalize_provider(provider)?;
    match provider {
        GOOGLE_PROVIDER => connect_google(store, keyvault).await,
        MICROSOFT_PROVIDER => connect_microsoft(store, keyvault).await,
        _ => Err(ParaError::InvalidState("unsupported calendar provider".into())),
    }
}

pub async fn list_upcoming_events(
    store: &LocalStore,
    keyvault: &KeyVault,
) -> Result<Vec<UpcomingCalendarEventRow>> {
    let mut events = Vec::new();

    if store.get_calendar_account(GOOGLE_PROVIDER)?.is_some() {
        if let Ok(mut provider_events) = fetch_google_upcoming(store, keyvault).await {
            events.append(&mut provider_events);
        }
    }

    if store.get_calendar_account(MICROSOFT_PROVIDER)?.is_some() {
        if let Ok(mut provider_events) = fetch_microsoft_upcoming(store, keyvault).await {
            events.append(&mut provider_events);
        }
    }

    events.sort_by(|a, b| a.start.cmp(&b.start));
    Ok(events)
}

fn normalize_provider<'a>(provider: &'a str) -> Result<&'a str> {
    match provider {
        GOOGLE_PROVIDER | MICROSOFT_PROVIDER => Ok(provider),
        _ => Err(ParaError::InvalidState(format!(
            "unsupported calendar provider: {}",
            provider
        ))),
    }
}

fn get_provider_config(keyvault: &KeyVault, provider: &str) -> Option<(String, Option<String>, Option<String>)> {
    let client_id = match provider {
        GOOGLE_PROVIDER => std::env::var("AUDIRE_GOOGLE_CALENDAR_CLIENT_ID")
            .ok()
            .or_else(|| keyvault.get_secret("calendar:google:client_id")),
        MICROSOFT_PROVIDER => std::env::var("AUDIRE_MICROSOFT_CALENDAR_CLIENT_ID")
            .ok()
            .or_else(|| keyvault.get_secret("calendar:microsoft:client_id")),
        _ => None,
    }?;

    let client_secret = match provider {
        GOOGLE_PROVIDER => std::env::var("AUDIRE_GOOGLE_CALENDAR_CLIENT_SECRET")
            .ok()
            .or_else(|| keyvault.get_secret("calendar:google:client_secret")),
        MICROSOFT_PROVIDER => std::env::var("AUDIRE_MICROSOFT_CALENDAR_CLIENT_SECRET")
            .ok()
            .or_else(|| keyvault.get_secret("calendar:microsoft:client_secret")),
        _ => None,
    };

    let tenant_id = if provider == MICROSOFT_PROVIDER {
        std::env::var("AUDIRE_MICROSOFT_CALENDAR_TENANT_ID")
            .ok()
            .or_else(|| keyvault.get_secret("calendar:microsoft:tenant_id"))
            .or(Some("common".to_string()))
    } else {
        None
    };

    Some((client_id, client_secret, tenant_id))
}

fn load_tokens(keyvault: &KeyVault, provider: &str) -> Option<CalendarTokenBundle> {
    keyvault
        .get_secret(&format!("calendar:{}:tokens", provider))
        .and_then(|raw| serde_json::from_str::<CalendarTokenBundle>(&raw).ok())
}

fn save_tokens(keyvault: &KeyVault, provider: &str, tokens: &CalendarTokenBundle) -> Result<()> {
    keyvault
        .set_secret(
            &format!("calendar:{}:tokens", provider),
            &serde_json::to_string(tokens).map_err(|e| ParaError::Other(e.to_string()))?,
        )
        .map_err(|e| ParaError::KeyVault(e.to_string()))
}

fn generate_code_verifier() -> String {
    use ring::rand::{SecureRandom, SystemRandom};
    let rng = SystemRandom::new();
    let mut buf = [0u8; 32];
    rng.fill(&mut buf).expect("system random fill failed");
    URL_SAFE_NO_PAD.encode(&buf)
}

fn generate_pkce_challenge(verifier: &str) -> String {
    let digest = digest(&SHA256, verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(digest.as_ref())
}

fn wait_for_oauth_code(listener: TcpListener, expected_state: &str) -> Result<String> {
    let (mut stream, _) = listener
        .accept()
        .map_err(|e| ParaError::Other(format!("oauth callback accept failed: {}", e)))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(20)))
        .map_err(|e| ParaError::Other(format!("oauth callback timeout failed: {}", e)))?;
    let mut buf = [0_u8; 4096];
    let size = stream
        .read(&mut buf)
        .map_err(|e| ParaError::Other(format!("oauth callback read failed: {}", e)))?;
    let request = String::from_utf8_lossy(&buf[..size]);
    let first_line = request
        .lines()
        .next()
        .ok_or_else(|| ParaError::Other("oauth callback missing request line".into()))?;
    let path = first_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| ParaError::Other("oauth callback missing path".into()))?;
    let query = path.split('?').nth(1).unwrap_or("");
    let params = parse_query(query);
    let body = if let Some(error) = params.get("error") {
        format!("Authentication failed: {}", error)
    } else {
        "You can close this window and return to Audire.".to_string()
    };
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nConnection: close\r\n\r\n<html><body style=\"font-family: sans-serif; padding: 24px; background: #1f1f21; color: #f5f2e8;\">{}</body></html>",
        body
    );
    let _ = stream.write_all(response.as_bytes());

    if let Some(error) = params.get("error") {
        return Err(ParaError::Other(format!("oauth failed: {}", error)));
    }

    let state = params
        .get("state")
        .ok_or_else(|| ParaError::Other("oauth callback missing state".into()))?;
    if state != expected_state {
        return Err(ParaError::Other("oauth callback state mismatch".into()));
    }
    params
        .get("code")
        .cloned()
        .ok_or_else(|| ParaError::Other("oauth callback missing code".into()))
}

fn parse_query(query: &str) -> std::collections::HashMap<String, String> {
    query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .filter_map(|pair| {
            let mut it = pair.splitn(2, '=');
            let key = it.next()?;
            let value = it.next().unwrap_or("");
            Some((percent_decode(key), percent_decode(value)))
        })
        .collect()
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let h = &input[i + 1..i + 3];
                if let Ok(v) = u8::from_str_radix(h, 16) {
                    out.push(v);
                    i += 3;
                } else {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).to_string()
}

async fn connect_google(store: &LocalStore, keyvault: &KeyVault) -> Result<CalendarAccountRow> {
    let (client_id, client_secret, _) = get_provider_config(keyvault, GOOGLE_PROVIDER)
        .ok_or_else(|| ParaError::MissingKey("google calendar client ID".into()))?;
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|e| ParaError::Other(format!("google oauth listener bind failed: {}", e)))?;
    let port = listener
        .local_addr()
        .map_err(|e| ParaError::Other(format!("google oauth listener addr failed: {}", e)))?
        .port();
    let redirect_uri = format!("http://127.0.0.1:{}/google/callback", port);
    let verifier = generate_code_verifier();
    let challenge = generate_pkce_challenge(&verifier);
    let state = uuid::Uuid::new_v4().to_string();
    let auth_url = format!(
        "https://accounts.google.com/o/oauth2/v2/auth?client_id={}&redirect_uri={}&response_type=code&scope={}&access_type=offline&prompt=consent&state={}&code_challenge={}&code_challenge_method=S256",
        urlencoding::encode(&client_id),
        urlencoding::encode(&redirect_uri),
        urlencoding::encode(GOOGLE_SCOPE),
        urlencoding::encode(&state),
        urlencoding::encode(&challenge),
    );
    webbrowser::open(&auth_url).map_err(|e| ParaError::Other(format!("google oauth browser open failed: {}", e)))?;
    let expected_state = state.clone();
    let code = tokio::task::spawn_blocking(move || {
        wait_for_oauth_code(listener, &expected_state)
    })
    .await
    .map_err(|e| ParaError::Other(format!("google oauth join failed: {}", e)))??;

    let client = Client::new();
    let mut token_params = vec![
        ("client_id", client_id.as_str()),
        ("code", code.as_str()),
        ("code_verifier", verifier.as_str()),
        ("grant_type", "authorization_code"),
        ("redirect_uri", redirect_uri.as_str()),
    ];
    let secret_ref = client_secret.as_deref().unwrap_or("");
    if !secret_ref.is_empty() {
        token_params.push(("client_secret", secret_ref));
    }
    let resp = client
        .post("https://oauth2.googleapis.com/token")
        .form(&token_params)
        .send()
        .await
        .map_err(|e| ParaError::Other(format!("google token exchange failed: {}", e)))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(ParaError::Other(format!(
            "google token exchange failed ({}): {}",
            status, body
        )));
    }
    let token = resp
        .json::<GoogleTokenResp>()
        .await
        .map_err(|e| ParaError::Other(format!("google token decode failed: {}", e)))?;

    let bundle = CalendarTokenBundle {
        access_token: token.access_token.clone(),
        refresh_token: token.refresh_token.clone(),
        expires_at: (Utc::now() + ChronoDuration::seconds(token.expires_in.saturating_sub(30))).timestamp(),
    };
    save_tokens(keyvault, GOOGLE_PROVIDER, &bundle)?;
    let user = fetch_google_user(&client, &bundle.access_token).await?;
    store.upsert_calendar_account(GOOGLE_PROVIDER, user.email.as_deref(), user.name.as_deref())
}

async fn connect_microsoft(store: &LocalStore, keyvault: &KeyVault) -> Result<CalendarAccountRow> {
    let (client_id, _, tenant_id) = get_provider_config(keyvault, MICROSOFT_PROVIDER)
        .ok_or_else(|| ParaError::MissingKey("microsoft calendar client ID".into()))?;
    let tenant_id = tenant_id.unwrap_or_else(|| "common".to_string());
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|e| ParaError::Other(format!("microsoft oauth listener bind failed: {}", e)))?;
    let port = listener
        .local_addr()
        .map_err(|e| ParaError::Other(format!("microsoft oauth listener addr failed: {}", e)))?
        .port();
    let redirect_uri = format!("http://127.0.0.1:{}/microsoft/callback", port);
    let verifier = generate_code_verifier();
    let challenge = generate_pkce_challenge(&verifier);
    let state = uuid::Uuid::new_v4().to_string();
    let auth_url = format!(
        "https://login.microsoftonline.com/{}/oauth2/v2.0/authorize?client_id={}&response_type=code&redirect_uri={}&response_mode=query&scope={}&state={}&code_challenge={}&code_challenge_method=S256",
        urlencoding::encode(&tenant_id),
        urlencoding::encode(&client_id),
        urlencoding::encode(&redirect_uri),
        urlencoding::encode(MICROSOFT_SCOPE),
        urlencoding::encode(&state),
        urlencoding::encode(&challenge),
    );
    webbrowser::open(&auth_url).map_err(|e| ParaError::Other(format!("microsoft oauth browser open failed: {}", e)))?;
    let expected_state = state.clone();
    let code = tokio::task::spawn_blocking(move || {
        wait_for_oauth_code(listener, &expected_state)
    })
    .await
    .map_err(|e| ParaError::Other(format!("microsoft oauth join failed: {}", e)))??;

    let client = Client::new();
    let token_url = format!(
        "https://login.microsoftonline.com/{}/oauth2/v2.0/token",
        tenant_id
    );
    let resp = client
        .post(token_url)
        .form(&[
            ("client_id", client_id.as_str()),
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("redirect_uri", redirect_uri.as_str()),
            ("code_verifier", verifier.as_str()),
            ("scope", MICROSOFT_SCOPE),
        ])
        .send()
        .await
        .map_err(|e| ParaError::Other(format!("microsoft token exchange failed: {}", e)))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(ParaError::Other(format!(
            "microsoft token exchange failed ({}): {}",
            status, body
        )));
    }
    let token = resp
        .json::<MicrosoftTokenResp>()
        .await
        .map_err(|e| ParaError::Other(format!("microsoft token decode failed: {}", e)))?;

    let bundle = CalendarTokenBundle {
        access_token: token.access_token.clone(),
        refresh_token: token.refresh_token.clone(),
        expires_at: (Utc::now() + ChronoDuration::seconds(token.expires_in.saturating_sub(30))).timestamp(),
    };
    save_tokens(keyvault, MICROSOFT_PROVIDER, &bundle)?;
    let me = fetch_microsoft_profile(&client, &bundle.access_token).await?;
    let email = me.mail.or(me.user_principal_name);
    store.upsert_calendar_account(MICROSOFT_PROVIDER, email.as_deref(), me.display_name.as_deref())
}

async fn fetch_google_user(client: &Client, access_token: &str) -> Result<GoogleUserInfoResp> {
    client
        .get("https://openidconnect.googleapis.com/v1/userinfo")
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| ParaError::Other(format!("google userinfo failed: {}", e)))?
        .error_for_status()
        .map_err(|e| ParaError::Other(format!("google userinfo failed: {}", e)))?
        .json::<GoogleUserInfoResp>()
        .await
        .map_err(|e| ParaError::Other(format!("google userinfo decode failed: {}", e)))
}

async fn fetch_microsoft_profile(client: &Client, access_token: &str) -> Result<MicrosoftMeResp> {
    client
        .get("https://graph.microsoft.com/v1.0/me?$select=displayName,mail,userPrincipalName")
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| ParaError::Other(format!("microsoft profile failed: {}", e)))?
        .error_for_status()
        .map_err(|e| ParaError::Other(format!("microsoft profile failed: {}", e)))?
        .json::<MicrosoftMeResp>()
        .await
        .map_err(|e| ParaError::Other(format!("microsoft profile decode failed: {}", e)))
}

async fn get_valid_google_access_token(store: &LocalStore, keyvault: &KeyVault) -> Result<String> {
    let (client_id, client_secret, _) = get_provider_config(keyvault, GOOGLE_PROVIDER)
        .ok_or_else(|| ParaError::MissingKey("google calendar client ID".into()))?;
    let mut bundle = load_tokens(keyvault, GOOGLE_PROVIDER)
        .ok_or_else(|| ParaError::InvalidState("google calendar is not connected".into()))?;
    if bundle.expires_at > Utc::now().timestamp() + 30 {
        return Ok(bundle.access_token);
    }
    let refresh_token = bundle
        .refresh_token
        .clone()
        .ok_or_else(|| ParaError::InvalidState("google calendar refresh token missing".into()))?;
    let client = Client::new();
    let mut refresh_params = vec![
        ("client_id", client_id.as_str()),
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token.as_str()),
    ];
    let secret_ref = client_secret.as_deref().unwrap_or("");
    if !secret_ref.is_empty() {
        refresh_params.push(("client_secret", secret_ref));
    }
    let refreshed = client
        .post("https://oauth2.googleapis.com/token")
        .form(&refresh_params)
        .send()
        .await
        .map_err(|e| ParaError::Other(format!("google token refresh failed: {}", e)))?
        .error_for_status()
        .map_err(|e| ParaError::Other(format!("google token refresh failed: {}", e)))?
        .json::<GoogleTokenResp>()
        .await
        .map_err(|e| ParaError::Other(format!("google token refresh decode failed: {}", e)))?;
    bundle.access_token = refreshed.access_token;
    bundle.expires_at = (Utc::now() + ChronoDuration::seconds(refreshed.expires_in.saturating_sub(30))).timestamp();
    if refreshed.refresh_token.is_some() {
        bundle.refresh_token = refreshed.refresh_token;
    }
    save_tokens(keyvault, GOOGLE_PROVIDER, &bundle)?;
    if store.get_calendar_account(GOOGLE_PROVIDER)?.is_none() {
        let user = fetch_google_user(&client, &bundle.access_token).await?;
        let _ = store.upsert_calendar_account(GOOGLE_PROVIDER, user.email.as_deref(), user.name.as_deref());
    }
    Ok(bundle.access_token)
}

async fn get_valid_microsoft_access_token(store: &LocalStore, keyvault: &KeyVault) -> Result<String> {
    let (client_id, _, tenant_id) = get_provider_config(keyvault, MICROSOFT_PROVIDER)
        .ok_or_else(|| ParaError::MissingKey("microsoft calendar client ID".into()))?;
    let tenant_id = tenant_id.unwrap_or_else(|| "common".to_string());
    let mut bundle = load_tokens(keyvault, MICROSOFT_PROVIDER)
        .ok_or_else(|| ParaError::InvalidState("microsoft calendar is not connected".into()))?;
    if bundle.expires_at > Utc::now().timestamp() + 30 {
        return Ok(bundle.access_token);
    }
    let refresh_token = bundle
        .refresh_token
        .clone()
        .ok_or_else(|| ParaError::InvalidState("microsoft calendar refresh token missing".into()))?;
    let client = Client::new();
    let token_url = format!(
        "https://login.microsoftonline.com/{}/oauth2/v2.0/token",
        tenant_id
    );
    let refreshed = client
        .post(token_url)
        .form(&[
            ("client_id", client_id.as_str()),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token.as_str()),
            ("scope", MICROSOFT_SCOPE),
        ])
        .send()
        .await
        .map_err(|e| ParaError::Other(format!("microsoft token refresh failed: {}", e)))?
        .error_for_status()
        .map_err(|e| ParaError::Other(format!("microsoft token refresh failed: {}", e)))?
        .json::<MicrosoftTokenResp>()
        .await
        .map_err(|e| ParaError::Other(format!("microsoft token refresh decode failed: {}", e)))?;
    bundle.access_token = refreshed.access_token;
    bundle.expires_at = (Utc::now() + ChronoDuration::seconds(refreshed.expires_in.saturating_sub(30))).timestamp();
    if refreshed.refresh_token.is_some() {
        bundle.refresh_token = refreshed.refresh_token;
    }
    save_tokens(keyvault, MICROSOFT_PROVIDER, &bundle)?;
    if store.get_calendar_account(MICROSOFT_PROVIDER)?.is_none() {
        let me = fetch_microsoft_profile(&client, &bundle.access_token).await?;
        let email = me.mail.or(me.user_principal_name);
        let _ = store.upsert_calendar_account(MICROSOFT_PROVIDER, email.as_deref(), me.display_name.as_deref());
    }
    Ok(bundle.access_token)
}

async fn fetch_google_upcoming(
    store: &LocalStore,
    keyvault: &KeyVault,
) -> Result<Vec<UpcomingCalendarEventRow>> {
    let access_token = get_valid_google_access_token(store, keyvault).await?;
    let account = store.get_calendar_account(GOOGLE_PROVIDER)?;
    let time_min = Utc::now().to_rfc3339();
    let client = Client::new();
    let resp = client
        .get("https://www.googleapis.com/calendar/v3/calendars/primary/events")
        .bearer_auth(access_token)
        .query(&[
            ("singleEvents", "true"),
            ("orderBy", "startTime"),
            ("timeMin", time_min.as_str()),
            ("maxResults", "12"),
        ])
        .send()
        .await
        .map_err(|e| ParaError::Other(format!("google calendar fetch failed: {}", e)))?
        .error_for_status()
        .map_err(|e| ParaError::Other(format!("google calendar fetch failed: {}", e)))?
        .json::<GoogleEventsResp>()
        .await
        .map_err(|e| ParaError::Other(format!("google calendar decode failed: {}", e)))?;

    let acct_email = account.as_ref().and_then(|row| row.email.clone());
    Ok(resp
        .items
        .into_iter()
        .map(|item| {
            let attendees = item
                .attendees
                .unwrap_or_default()
                .into_iter()
                .filter(|a| a.is_self != Some(true))
                .filter_map(|a| {
                    let email = a.email?;
                    // Skip the account's own email
                    if acct_email.as_deref() == Some(email.as_str()) {
                        return None;
                    }
                    Some(crate::store::db::EventAttendee {
                        name: a.display_name,
                        email,
                    })
                })
                .collect();
            UpcomingCalendarEventRow {
                provider: GOOGLE_PROVIDER.to_string(),
                account_email: acct_email.clone(),
                external_id: item.id,
                title: item.summary.unwrap_or_else(|| "Untitled event".to_string()),
                start: item
                    .start
                    .date_time
                    .or(item.start.date)
                    .unwrap_or_default(),
                end: item.end.date_time.or(item.end.date).unwrap_or_default(),
                organizer: item
                    .organizer
                    .and_then(|o| o.display_name.or(o.email)),
                location: item.location,
                attendees,
            }
        })
        .collect())
}

async fn fetch_microsoft_upcoming(
    store: &LocalStore,
    keyvault: &KeyVault,
) -> Result<Vec<UpcomingCalendarEventRow>> {
    let access_token = get_valid_microsoft_access_token(store, keyvault).await?;
    let account = store.get_calendar_account(MICROSOFT_PROVIDER)?;
    let start = Utc::now().to_rfc3339();
    let end = (Utc::now() + ChronoDuration::days(14)).to_rfc3339();
    let client = Client::new();
    let resp = client
        .get("https://graph.microsoft.com/v1.0/me/calendarview")
        .bearer_auth(access_token)
        .query(&[
            ("startDateTime", start.as_str()),
            ("endDateTime", end.as_str()),
            ("$orderby", "start/dateTime"),
            ("$top", "12"),
        ])
        .send()
        .await
        .map_err(|e| ParaError::Other(format!("microsoft calendar fetch failed: {}", e)))?
        .error_for_status()
        .map_err(|e| ParaError::Other(format!("microsoft calendar fetch failed: {}", e)))?
        .json::<MicrosoftEventsResp>()
        .await
        .map_err(|e| ParaError::Other(format!("microsoft calendar decode failed: {}", e)))?;

    let acct_email = account.as_ref().and_then(|row| row.email.clone());
    Ok(resp
        .value
        .into_iter()
        .map(|item| {
            let attendees = item
                .attendees
                .unwrap_or_default()
                .into_iter()
                .filter_map(|a| {
                    let ea = a.email_address?;
                    let email = ea.address?;
                    // Skip the account's own email
                    if acct_email.as_deref() == Some(email.as_str()) {
                        return None;
                    }
                    Some(crate::store::db::EventAttendee {
                        name: ea.name,
                        email,
                    })
                })
                .collect();
            UpcomingCalendarEventRow {
                provider: MICROSOFT_PROVIDER.to_string(),
                account_email: acct_email.clone(),
                external_id: item.id,
                title: item.subject.unwrap_or_else(|| "Untitled event".to_string()),
                start: item.start.date_time,
                end: item.end.date_time,
                organizer: item
                    .organizer
                    .and_then(|o| o.email_address.and_then(|e| e.name.or(e.address))),
                location: item.location.and_then(|loc| loc.display_name),
                attendees,
            }
        })
        .collect())
}
