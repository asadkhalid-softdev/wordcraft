//! WordCraft-owned OAuth authorization-code + PKCE helper. No external helper repository.
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};
use url::Url;

use super::*;

const CALLBACK: &str = "/auth/callback";
const MAX_HEADERS: usize = 8192;
const HELP: &str = "WordCraft ChatGPT login\n\nFrom the WordCraft repository:\n  cargo run -p wordcraft-ui-egui --bin wordcraft-chatgpt-login\n\nOptions:\n  --no-browser    Print the sign-in URL for manual opening on this computer\n  --new-account   Create a fresh account/workspace registration\n  --help          Show this help without starting sign-in\n\nOutput: <WordCraft profile>/chatgpt-login/wordcraft-account.json\nCopy the file into ChatGPT settings → Protected credential file JSON.\nCredentials are never printed. Keep the output private and out of Git.\n";

struct Attempt {
    state: String,
    nonce: String,
    verifier: zeroize::Zeroizing<String>,
    redirect: String,
    client_id: String,
}

fn random() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| "Could not obtain secure randomness")?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn constant_eq(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |different, (a, b)| different | (a ^ b)) == 0
}

impl Attempt {
    fn new(host: &str, redirect: String, saved: Option<&Credentials>) -> Result<(Self, Url), String> {
        let attempt = Self {
            state: random()?,
            nonce: random()?,
            verifier: zeroize::Zeroizing::new(random()?),
            redirect,
            client_id: saved.map(|s| s.client_id.clone()).unwrap_or_else(|| "dynamic_agent_client".into()),
        };
        let mut url = Url::parse("https://auth.openai.com/api/accounts/authorize").map_err(|_| "Invalid authorization endpoint")?;
        {
            let mut query = url.query_pairs_mut();
            query.extend_pairs([
                ("client_id", attempt.client_id.as_str()),
                ("ext_agent_host_id", host),
                ("response_type", "code"),
                ("redirect_uri", attempt.redirect.as_str()),
                ("scope", "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct"),
                ("resource", API),
                ("state", attempt.state.as_str()),
                ("nonce", attempt.nonce.as_str()),
                ("code_challenge_method", "S256"),
                ("code_challenge", &challenge(&attempt.verifier)),
            ]);
            if saved.is_none() {
                query.append_pair("agent_name_hint", "WordCraft");
            }
        }
        // Returning sign-in uses the account selector. Never put an ID token in a printable URL.
        Ok((attempt, url))
    }

    fn grant(&self, query: &BTreeMap<String, String>) -> Result<(String, String), String> {
        if !query.get("state").is_some_and(|state| constant_eq(state, &self.state)) {
            return Err("Invalid sign-in state".into());
        }
        if query.contains_key("error") {
            return Err("ChatGPT sign-in was denied or failed. Start the helper again.".into());
        }
        let client = query.get("client_id").unwrap_or(&self.client_id);
        if client == "dynamic_agent_client"
            || client.is_empty()
            || client.len() > 512
            || (self.client_id != "dynamic_agent_client" && client != &self.client_id)
        {
            return Err("Sign-in did not return the expected issued client ID".into());
        }
        let code =
            query.get("code").filter(|code| !code.is_empty() && code.len() <= 4096).ok_or("Sign-in did not return a valid authorization code")?;
        Ok((code.clone(), client.clone()))
    }
}

fn challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn parse_callback(raw: &str, host: &str) -> Result<BTreeMap<String, String>, String> {
    if raw.len() > MAX_HEADERS || !raw.ends_with("\r\n\r\n") {
        return Err("Invalid callback request".into());
    }
    let mut lines = raw.split("\r\n");
    let request: Vec<_> = lines.next().unwrap_or("").split_whitespace().collect();
    if request.len() != 3 || request.first().copied() != Some("GET") || request.get(2).copied() != Some("HTTP/1.1") {
        return Err("Invalid callback method".into());
    }
    let mut actual_host = None;
    for line in lines.take_while(|line| !line.is_empty()) {
        let (name, value) = line.split_once(':').ok_or("Invalid callback header")?;
        if name.eq_ignore_ascii_case("host") {
            if actual_host.is_some() {
                return Err("Duplicate callback host".into());
            }
            actual_host = Some(value.trim());
        }
    }
    if actual_host != Some(host) {
        return Err("Invalid callback host".into());
    }
    let target = request.get(1).copied().ok_or("Missing callback path")?;
    if target.split('?').next() != Some(CALLBACK) {
        return Err("Invalid callback path".into());
    }
    let url = Url::parse(&format!("http://{host}{target}")).map_err(|_| "Invalid callback URL")?;
    if url.path() != CALLBACK || url.fragment().is_some() {
        return Err("Invalid callback path".into());
    }
    let mut query = BTreeMap::new();
    for (name, value) in url.query_pairs() {
        if query.insert(name.into_owned(), value.into_owned()).is_some() {
            return Err("Duplicate callback parameter".into());
        }
    }
    Ok(query)
}

fn read_callback(stream: &mut TcpStream, host: &str) -> Result<BTreeMap<String, String>, String> {
    stream.set_nonblocking(false).map_err(|_| "Could not configure callback connection")?;
    stream.set_read_timeout(Some(Duration::from_secs(2))).map_err(|_| "Could not configure callback connection")?;
    let started = Instant::now();
    let mut raw = Vec::new();
    let mut chunk = [0u8; 512];
    while !raw.ends_with(b"\r\n\r\n") {
        if raw.len() >= MAX_HEADERS || started.elapsed() > Duration::from_secs(5) {
            return Err("Callback request exceeded its limit".into());
        }
        let count = stream.read(&mut chunk).map_err(|_| "Could not read callback")?;
        if count == 0 {
            return Err("Callback connection closed".into());
        }
        if count > MAX_HEADERS.saturating_sub(raw.len()) {
            return Err("Callback request is too large".into());
        }
        raw.extend_from_slice(chunk.get(..count).ok_or("Invalid callback bytes")?);
    }
    let raw = String::from_utf8(raw).map_err(|_| "Invalid callback encoding")?;
    parse_callback(&raw, host)
}

fn reply(stream: &mut TcpStream, accepted: bool) {
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    let (status, body) = if accepted {
        ("200 OK", "WordCraft is verifying your sign-in. You can close this tab and return to WordCraft.")
    } else {
        ("400 Bad Request", "Invalid sign-in callback. Return to the browser sign-in flow.")
    };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nContent-Security-Policy: default-src 'none'\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
}

fn wait(listener: TcpListener, host: &str, attempt: &Attempt, cancelled: &AtomicBool) -> Result<(String, String), String> {
    listener.set_nonblocking(true).map_err(|_| "Could not configure the sign-in listener")?;
    let deadline = Instant::now();
    while deadline.elapsed() < Duration::from_secs(300) {
        check_cancelled(cancelled)?;
        match listener.accept() {
            Ok((mut stream, peer)) => {
                if !peer.ip().is_loopback() {
                    continue;
                }
                let query = read_callback(&mut stream, host);
                let valid = query.as_ref().is_ok_and(|q| q.get("state").is_some_and(|s| constant_eq(s, &attempt.state)));
                reply(&mut stream, valid);
                if valid {
                    return attempt.grant(&query?);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(25)),
            Err(_) => return Err("Could not accept the sign-in callback".into()),
        }
    }
    Err("Sign-in timed out after five minutes. Start the helper again.".into())
}

fn id_claims(http: &Client, token: &str, client: &str, nonce: &str) -> Result<Value, String> {
    let header = decode_header(token).map_err(|_| "OpenAI returned an invalid ID token")?;
    if header.alg != Algorithm::RS256 {
        return Err("Unsupported ID-token signing algorithm".into());
    }
    let keys: JwkSet = serde_json::from_value(request_json(http.get(format!("{ISSUER}/.well-known/jwks.json")))?)
        .map_err(|_| "OpenAI returned invalid signing keys")?;
    let key = keys.find(header.kid.as_deref().ok_or("ID token has no signing key")?).ok_or("Unknown ID-token signing key")?;
    let key = DecodingKey::from_jwk(key).map_err(|_| "Invalid ID-token signing key")?;
    verify_id(token, client, nonce, &key)
}

fn verify_id(token: &str, client: &str, nonce: &str, key: &DecodingKey) -> Result<Value, String> {
    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_audience(&[client]);
    validation.set_issuer(&[ISSUER]);
    validation.set_required_spec_claims(&["exp", "iat", "iss", "aud", "sub"]);
    validation.validate_nbf = true;
    validation.leeway = 30;
    let claims = decode::<Value>(token, key, &validation).map_err(|_| "ID-token verification failed. Check your clock and sign in again.")?.claims;
    let current_time = now()?;
    if !claims.get("nonce").and_then(Value::as_str).is_some_and(|n| constant_eq(n, nonce))
        || !claims.get("iat").and_then(Value::as_u64).is_some_and(|iat| iat as f64 <= current_time + 30.0)
    {
        return Err("ID-token nonce or issue time does not match this sign-in".into());
    }
    Ok(claims)
}

fn exchange(http: &Client, attempt: &Attempt, code: &str, client_id: &str, saved: Option<&Credentials>) -> Result<Credentials, String> {
    let data = request_json(http.post(TOKEN).form(&[
        ("grant_type", "authorization_code"),
        ("client_id", client_id),
        ("code", code),
        ("code_verifier", attempt.verifier.as_str()),
        ("redirect_uri", attempt.redirect.as_str()),
        ("resource", API),
    ]))?;
    let id_token = required(&data, "id_token")?;
    let claims = id_claims(http, &id_token, client_id, &attempt.nonce)?;
    let subject = required(&claims, "sub")?;
    if saved.is_some_and(|s| s.subject != subject) {
        return Err("Sign-in returned a different account. Start a new account registration to switch accounts.".into());
    }
    let scope = data.get("scope").and_then(Value::as_str).unwrap_or("");
    if !SCOPES.iter().all(|required| scope.split_whitespace().any(|scope| scope == *required)) {
        return Err("ChatGPT plan access was not granted. Review your account and workspace permissions.".into());
    }
    let mut credentials = Credentials {
        client_id: client_id.into(),
        subject,
        access_token: String::new(),
        refresh_token: String::new(),
        id_token,
        scopes: Vec::new(),
        expires_at: 0.0,
        earliest_refresh_at: 0.0,
    };
    credentials.replace_tokens(&data)?;
    credentials.verify(http)?;
    Ok(credentials)
}

fn check_cancelled(cancelled: &AtomicBool) -> Result<(), String> {
    if cancelled.load(Ordering::Relaxed) { Err("Sign-in cancelled".into()) } else { Ok(()) }
}

fn login_directory() -> Result<PathBuf, String> {
    let directory = wordcraft_control_key::settings_dir().ok_or("Could not locate WordCraft settings")?.join("chatgpt-login");
    fs::create_dir_all(&directory).map_err(|_| "Could not create the WordCraft login folder")?;
    Ok(directory)
}

fn host_id() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| "Could not obtain secure randomness")?;
    if let Some(value) = bytes.get_mut(6) {
        *value = (*value & 0x0f) | 0x40;
    }
    if let Some(value) = bytes.get_mut(8) {
        *value = (*value & 0x3f) | 0x80;
    }
    let mut id = String::from("urn:uuid:");
    for (i, byte) in bytes.iter().enumerate() {
        if [4, 6, 8, 10].contains(&i) {
            id.push('-');
        }
        id.push_str(&format!("{byte:02x}"));
    }
    Ok(id)
}

fn small_json(path: &Path) -> Result<Value, String> {
    let mut text = String::new();
    File::open(path).and_then(|file| file.take(1025).read_to_string(&mut text)).map_err(|_| "Could not read WordCraft sign-in metadata")?;
    if text.len() > 1024 {
        return Err("WordCraft sign-in metadata is too large".into());
    }
    serde_json::from_str(&text).map_err(|_| "Invalid WordCraft sign-in metadata".into())
}

fn saved_registration(directory: &Path, new_account: bool) -> Result<Option<Credentials>, String> {
    if new_account {
        return Ok(None);
    }
    let marker = directory.join("registration.json");
    if !marker.exists() {
        return Ok(None);
    }
    let marker = small_json(&marker)?;
    // Reuse only registrations made by WordCraft itself; an imported grant remains separate.
    let (managed, _lock) = locked()?;
    for path in [managed, directory.join("wordcraft-account.json")] {
        if let Ok(Some(saved)) = load(&path)
            && marker.get("client_id").and_then(Value::as_str) == Some(saved.client_id.as_str())
            && marker.get("subject").and_then(Value::as_str) == Some(saved.subject.as_str())
        {
            return Ok(Some(saved));
        }
    }
    // Signing out removes tokens, while this private mapping remains for reauthorization.
    registration_metadata(&marker).map(Some)
}

fn registration_metadata(marker: &Value) -> Result<Credentials, String> {
    let client_id = required(marker, "client_id")?;
    if client_id == "dynamic_agent_client" || client_id.len() > 512 {
        return Err("Invalid saved WordCraft registration".into());
    }
    Ok(Credentials {
        client_id,
        subject: required(marker, "subject")?,
        access_token: String::new(),
        refresh_token: String::new(),
        id_token: String::new(),
        scopes: Vec::new(),
        expires_at: 0.0,
        earliest_refresh_at: 0.0,
    })
}

fn authorize(http: &Client, new_account: bool, cancelled: &AtomicBool, open: impl FnOnce(&Url) -> Result<(), String>) -> Result<Credentials, String> {
    check_cancelled(cancelled)?;
    let directory = login_directory()?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .read(true)
        .open(directory.join("login.lock"))
        .map_err(|_| "Could not open the WordCraft login lock")?;
    fs2::FileExt::try_lock_exclusive(&lock).map_err(|_| "Another WordCraft sign-in is already running")?;
    let host_path = directory.join("host.json");
    let host = if host_path.exists() {
        small_json(&host_path)?.as_str().ok_or("Invalid saved WordCraft host ID")?.to_owned()
    } else {
        let host = host_id()?;
        save(&host_path, &host)?;
        host
    };
    if host.is_empty() || host.len() > 128 {
        return Err("Invalid saved WordCraft host ID".into());
    }
    let saved = saved_registration(&directory, new_account)?;
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).map_err(|_| "Could not start the loopback sign-in listener")?;
    let host_address = listener.local_addr().map_err(|_| "Could not read the callback address")?.to_string();
    let (attempt, url) = Attempt::new(&host, format!("http://{host_address}{CALLBACK}"), saved.as_ref())?;
    open(&url)?;
    let (code, client_id) = wait(listener, &host_address, &attempt, cancelled)?;
    check_cancelled(cancelled)?;
    let credentials = exchange(http, &attempt, &code, &client_id, saved.as_ref())?;
    check_cancelled(cancelled)?;
    save(&directory.join("registration.json"), &json!({"client_id": credentials.client_id, "subject": credentials.subject}))?;
    Ok(credentials)
}

pub(in crate::chatgpt) fn connect(
    http: &Client,
    new_account: bool,
    cancelled: &AtomicBool,
    open: impl FnOnce(&Url) -> Result<(), String>,
) -> Result<Value, String> {
    let credentials = authorize(http, new_account, cancelled, open)?;
    install(http, credentials, cancelled, true)
}

pub(in crate::chatgpt) fn run(args: &[String]) -> Result<(), String> {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print!("{HELP}");
        return Ok(());
    }
    if args.iter().any(|a| !matches!(a.as_str(), "--no-browser" | "--new-account")) {
        return Err("Unknown option. Run with --help for usage.".into());
    }
    println!("Continue with ChatGPT: complete WordCraft sign-in and consent in your browser.");
    let cancelled = AtomicBool::new(false);
    let credentials = authorize(&http()?, args.iter().any(|a| a == "--new-account"), &cancelled, |url| {
        if args.iter().any(|a| a == "--no-browser") || webbrowser::open(url.as_str()).is_err() {
            println!("Open this URL on this computer:\n{url}");
        }
        Ok(())
    })?;
    let output = login_directory()?.join("wordcraft-account.json");
    save(&output, &credentials)?;
    println!("Sign-in verified. Protected JSON saved to: {}", output.display());
    println!("Paste its complete contents into WordCraft → ChatGPT settings → Protected credential file JSON, then click Import and connect.");
    println!("Close the credential editor and clear your clipboard afterwards. Do not share or commit the JSON.");
    Ok(())
}

#[cfg(test)]
mod tests;
