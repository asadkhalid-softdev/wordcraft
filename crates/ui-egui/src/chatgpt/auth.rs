//! Imported OSS OAuth credentials. No token or provider error body reaches UI diagnostics.
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header, jwk::JwkSet};
use reqwest::blocking::{Client, Response};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use zeroize::Zeroize;

pub(super) const API: &str = "https://api.openai.com/v1";
const ISSUER: &str = "https://auth.openai.com";
const TOKEN: &str = "https://auth.openai.com/api/accounts/oauth/token";
pub(super) const MAX_AUTH: usize = 64_000;
const MAX_HTTP: u64 = 2_000_000;
const SCOPES: [&str; 4] = ["openid", "offline_access", "resource.invoke", "chatgpt.tokens.use.direct"];

#[derive(Serialize, Deserialize)]
pub(super) struct Credentials {
    client_id: String,
    subject: String,
    access_token: String,
    refresh_token: String,
    id_token: String,
    #[serde(default)]
    scopes: Vec<String>,
    #[serde(default)]
    expires_at: f64,
    #[serde(default)]
    earliest_refresh_at: f64,
}

impl Drop for Credentials {
    fn drop(&mut self) {
        self.access_token.zeroize();
        self.refresh_token.zeroize();
        self.id_token.zeroize();
    }
}

impl Credentials {
    fn parse(raw: &str) -> Result<Self, String> {
        if raw.len() > MAX_AUTH {
            return Err("Credential JSON is too large".into());
        }
        let value: Self = serde_json::from_str(raw).map_err(|_| "Paste the complete credential JSON from the login helper")?;
        if [&value.client_id, &value.subject, &value.access_token, &value.refresh_token, &value.id_token]
            .iter()
            .any(|s| s.is_empty() || s.len() > 16_000)
            || value.client_id == "dynamic_agent_client"
        {
            return Err("The credential file is incomplete or has no issued client ID".into());
        }
        Ok(value)
    }

    fn verify(&mut self, http: &Client) -> Result<(), String> {
        let header = decode_header(&self.access_token).map_err(|_| "The access token is invalid")?;
        if header.alg != Algorithm::RS256 {
            return Err("The access token uses an unsupported signing algorithm".into());
        }
        let jwks: JwkSet = serde_json::from_value(request_json(http.get(format!("{ISSUER}/.well-known/jwks.json")))?)
            .map_err(|_| "OpenAI returned invalid signing keys")?;
        let jwk = header.kid.as_ref().and_then(|kid| jwks.find(kid)).ok_or("The access token has an unknown signing key")?;
        let key = DecodingKey::from_jwk(jwk).map_err(|_| "The OpenAI signing key is invalid")?;
        self.verify_key(&key)
    }

    fn verify_key(&mut self, key: &DecodingKey) -> Result<(), String> {
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_audience(&[API]);
        validation.set_issuer(&[ISSUER]);
        validation.set_required_spec_claims(&["exp", "iat", "iss", "aud", "sub"]);
        validation.validate_nbf = true;
        validation.leeway = 30;
        let claims = decode::<Value>(&self.access_token, key, &validation)
            .map_err(|_| "Access-token verification failed. Check your clock and run the login helper again.")?
            .claims;
        let scope = claims.get("scope").and_then(Value::as_str).ok_or("The grant has no ChatGPT plan permission")?;
        let issued = claims.get("iat").and_then(Value::as_u64).ok_or("The token has no issue time")?;
        if claims.get("client_id").and_then(Value::as_str) != Some(self.client_id.as_str())
            || claims.get("sub").and_then(Value::as_str) != Some(self.subject.as_str())
            || !SCOPES.iter().all(|required| scope.split_whitespace().any(|s| s == *required))
            || issued as f64 > now()? + 30.0
        {
            return Err("Credentials do not match the authorized account registration or ChatGPT plan grant".into());
        }
        self.expires_at = claims.get("exp").and_then(Value::as_u64).ok_or("The token has no expiry")? as f64;
        self.scopes = scope.split_whitespace().map(str::to_owned).collect();
        // Import metadata never decides whether or when a signed token expires.
        Ok(())
    }

    fn refresh(&mut self, http: &Client) -> Result<(), String> {
        let data = request_json(http.post(TOKEN).form(&[
            ("grant_type", "refresh_token"),
            ("client_id", self.client_id.as_str()),
            ("refresh_token", self.refresh_token.as_str()),
            ("resource", API),
        ]))?;
        self.replace_tokens(&data)
    }

    fn replace_tokens(&mut self, data: &Value) -> Result<(), String> {
        let access = required(data, "access_token")?;
        let refresh = required(data, "refresh_token")?;
        let expires = data
            .get("expires_in")
            .and_then(Value::as_f64)
            .filter(|n| n.is_finite() && *n > 0.0 && *n <= 86400.0)
            .ok_or("OpenAI returned an invalid token lifetime")?;
        if !data.get("token_type").and_then(Value::as_str).is_some_and(|t| t.eq_ignore_ascii_case("bearer")) {
            return Err("OpenAI returned an unsupported token type".into());
        }
        self.access_token.zeroize();
        self.refresh_token.zeroize();
        self.access_token = access;
        self.refresh_token = refresh;
        if let Some(id) = data.get("id_token").and_then(Value::as_str).filter(|s| !s.is_empty() && s.len() <= 16_000) {
            self.id_token.zeroize();
            self.id_token = id.into();
        }
        self.expires_at = now()? + expires;
        self.earliest_refresh_at = data.get("earliest_refresh_at").and_then(Value::as_f64).filter(|n| n.is_finite()).unwrap_or(0.0);
        Ok(())
    }
}

fn required(value: &Value, key: &str) -> Result<String, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty() && s.len() <= 16_000)
        .map(str::to_owned)
        .ok_or_else(|| "OpenAI returned an incomplete token set".into())
}

pub(super) fn now() -> Result<f64, String> {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).map_err(|_| "Check the system clock".into())
}

pub(super) fn http() -> Result<Client, String> {
    Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(180))
        .user_agent("WordCraft/0.7 ChatGPT")
        .build()
        .map_err(|_| "Could not initialize the HTTPS connection".into())
}

pub(super) fn body(response: Response, cap: u64) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    response.take(cap.saturating_add(1)).read_to_end(&mut bytes).map_err(|_| "Could not read the OpenAI response")?;
    if bytes.len() as u64 > cap {
        return Err("OpenAI response exceeded the size limit".into());
    }
    Ok(bytes)
}

pub(super) fn request_json(request: reqwest::blocking::RequestBuilder) -> Result<Value, String> {
    let response = request.send().map_err(|_| "Could not reach OpenAI. Check the connection and try again.")?;
    if !response.status().is_success() {
        return Err(status_error(response.status().as_u16()));
    }
    serde_json::from_slice(&body(response, MAX_HTTP)?).map_err(|_| "OpenAI returned invalid JSON".into())
}

pub(super) fn status_error(status: u16) -> String {
    match status {
        400 | 401 => "ChatGPT authorization or request failed. Run the login helper again if needed.",
        403 => "OpenAI denied access. Check ChatGPT account, workspace and plan permissions.",
        429 => "ChatGPT usage or rate limit reached. Review ChatGPT Settings → Usage.",
        _ => "OpenAI could not complete the request. Try again later.",
    }
    .into()
}

fn directory() -> Result<PathBuf, String> {
    let dir = wordcraft_control_key::settings_dir().ok_or("Could not locate WordCraft settings")?.join("chatgpt-direct");
    fs::create_dir_all(&dir).map_err(|_| "Could not create ChatGPT settings")?;
    Ok(dir)
}

// The file lock serializes rotating refreshes across processes as well as reconnecting clients.
fn locked() -> Result<(PathBuf, File), String> {
    let dir = directory()?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(dir.join("account.lock"))
        .map_err(|_| "Could not open the ChatGPT account lock")?;
    fs2::FileExt::try_lock_exclusive(&file).map_err(|_| "Another WordCraft connection is updating this account. Try again shortly.")?;
    Ok((dir.join("account.json"), file))
}

fn load(path: &Path) -> Result<Option<Credentials>, String> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("Could not read saved ChatGPT credentials".into()),
    };
    let mut raw = String::new();
    file.take(MAX_AUTH as u64 + 1).read_to_string(&mut raw).map_err(|_| "Could not read saved ChatGPT credentials")?;
    let result = Credentials::parse(&raw);
    raw.zeroize();
    result.map(Some)
}

fn save(path: &Path, credentials: &impl Serialize) -> Result<(), String> {
    let parent = path.parent().ok_or("Invalid credential path")?;
    let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(|_| "Could not create private credential storage")?;
    protect(temp.path())?;
    serde_json::to_writer(temp.as_file_mut(), credentials).map_err(|_| "Could not save ChatGPT credentials")?;
    temp.as_file_mut().flush().and_then(|()| temp.as_file().sync_all()).map_err(|_| "Could not sync ChatGPT credentials")?;
    temp.persist(path).map_err(|_| "Could not replace saved ChatGPT credentials")?;
    Ok(())
}

#[cfg(unix)]
pub(super) fn protect(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|_| "Could not protect ChatGPT credentials".into())
}

#[cfg(windows)]
pub(super) fn protect(path: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    let output = Command::new("whoami.exe")
        .args(["/user", "/fo", "csv", "/nh"])
        .creation_flags(0x0800_0000)
        .output()
        .map_err(|_| "Could not identify the Windows account")?;
    let text = String::from_utf8(output.stdout).map_err(|_| "Could not identify the Windows account")?;
    let sid = text.trim().rsplit(',').next().unwrap_or("").trim_matches('"');
    if !output.status.success() || !sid.starts_with("S-1-") || !sid.chars().all(|c| c.is_ascii_digit() || c == 'S' || c == '-') {
        return Err("Could not identify the Windows account".into());
    }
    let status = Command::new("icacls.exe")
        .arg(path)
        .args(["/inheritance:r", "/grant:r", &format!("*{sid}:(F)")])
        .creation_flags(0x0800_0000)
        .output()
        .map_err(|_| "Could not protect Windows credentials")?;
    if !status.status.success() {
        return Err("Could not protect credentials. Use a private local NTFS profile folder.".into());
    }
    Ok(())
}

#[cfg(not(any(unix, windows)))]
pub(super) fn protect(_path: &Path) -> Result<(), String> {
    Err("Protected credential storage is unavailable on this platform".into())
}

pub(super) fn models(http: &Client, token: &str) -> Result<Vec<Value>, String> {
    let data = request_json(http.get(format!("{API}/models")).bearer_auth(token))?;
    let models: Vec<_> = data
        .get("models")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(512)
        .filter(|m| m.get("visibility").and_then(Value::as_str) == Some("list"))
        .filter_map(|m| {
            let slug = m.get("slug")?.as_str().filter(|s| !s.is_empty() && s.len() <= 256)?;
            let label = m.get("display_name").and_then(Value::as_str).unwrap_or(slug);
            Some(json!({"slug": slug, "display_name": label.chars().take(256).collect::<String>()}))
        })
        .collect();
    if models.is_empty() {
        return Err("This account returned no eligible ChatGPT models".into());
    }
    Ok(models)
}

pub(super) fn import(http: &Client, raw: &str) -> Result<Value, String> {
    install(http, Credentials::parse(raw)?, &std::sync::atomic::AtomicBool::new(false), false)
}

fn install(http: &Client, mut credentials: Credentials, cancelled: &std::sync::atomic::AtomicBool, fresh: bool) -> Result<Value, String> {
    let (path, _lock) = locked()?;
    credentials.verify(http)?;
    credentials.earliest_refresh_at = 0.0;
    if !fresh
        && load(&path)?.is_some_and(|saved| {
            saved.client_id == credentials.client_id
                && saved.subject == credentials.subject
                && saved.expires_at >= credentials.expires_at
                && saved.access_token != credentials.access_token
        })
    {
        return Err("This registration has newer saved credentials. Create a fresh helper output file instead of importing an old token set.".into());
    }
    let models = models(http, &credentials.access_token)?;
    if cancelled.load(std::sync::atomic::Ordering::Relaxed) {
        return Err("Sign-in cancelled".into());
    }
    save(&path, &credentials)?;
    // The imported email/host/expiry are untrusted. Show an issued registration suffix instead.
    let label = format!("ChatGPT account · {}", credentials.client_id.chars().rev().take(8).collect::<String>().chars().rev().collect::<String>());
    Ok(json!({"account": {"type": "chatgpt", "email": label}, "models": models}))
}

pub(super) fn access(http: &Client) -> Result<Option<zeroize::Zeroizing<String>>, String> {
    let (path, _lock) = locked()?;
    let Some(mut credentials) = load(&path)? else { return Ok(None) };
    let current_time = now()?;
    if (!credentials.expires_at.is_finite() || credentials.expires_at < current_time + 120.0)
        && (!credentials.earliest_refresh_at.is_finite() || credentials.earliest_refresh_at <= current_time)
    {
        // Persist rotated tokens even if verification/network fails afterwards; never reuse an old refresh token.
        credentials.refresh(http)?;
        save(&path, &credentials)?;
    }
    credentials.verify(http)?;
    Ok(Some(zeroize::Zeroizing::new(credentials.access_token.clone())))
}

pub(super) fn account(http: &Client) -> Result<Value, String> {
    let Some(token) = access(http)? else { return Ok(json!({"account": null, "models": []})) };
    let models = models(http, &token)?;
    Ok(json!({"account": {"type": "chatgpt", "email": "ChatGPT account"}, "models": models}))
}

pub(super) fn logout(http: &Client) -> Result<Value, String> {
    let (path, _lock) = locked()?;
    let mut confirmed = true;
    if let Some(credentials) = load(&path)? {
        confirmed = (|| -> Result<(), String> {
            let config = request_json(http.get(format!("{ISSUER}/.well-known/openid-configuration")))?;
            let endpoint = config
                .get("revocation_endpoint")
                .and_then(Value::as_str)
                .filter(|s| s.starts_with("https://auth.openai.com/"))
                .ok_or("Invalid revocation endpoint")?;
            let response = http
                .post(endpoint)
                .form(&[
                    ("token", credentials.refresh_token.as_str()),
                    ("token_type_hint", "refresh_token"),
                    ("client_id", credentials.client_id.as_str()),
                ])
                .send()
                .map_err(|_| "Could not revoke the session")?;
            if response.status().as_u16() != 200 {
                return Err("Session revocation was not confirmed".into());
            }
            Ok(())
        })()
        .is_ok();
    }
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err("Could not remove saved credentials".into()),
    }
    Ok(json!({"revoked": confirmed}))
}

#[cfg(test)]
mod tests;

// Native helper shares verification and protected storage with the document assistant.
pub(super) mod login;
