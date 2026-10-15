use super::*;
use jsonwebtoken::{EncodingKey, Header, encode};
use rsa::{
    RsaPrivateKey, RsaPublicKey,
    pkcs1::{EncodeRsaPrivateKey, EncodeRsaPublicKey},
    rand_core::OsRng,
};

fn callback(host: &str, query: &str) -> String {
    format!("GET /auth/callback?{query} HTTP/1.1\r\nHost: {host}\r\n\r\n")
}

#[test]
fn wordcraft_registration_uses_pkce_nonce_and_a_stable_callback() {
    assert_eq!(challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    let (a, url) = Attempt::new("urn:uuid:test", "http://127.0.0.1:1234/auth/callback".into(), None).unwrap();
    let params: BTreeMap<_, _> = url.query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect();
    assert_eq!(params["agent_name_hint"], "WordCraft");
    assert_eq!(params["client_id"], "dynamic_agent_client");
    assert_eq!(params["code_challenge"], challenge(&a.verifier));
    assert_eq!(params["nonce"], a.nonce);
    assert_eq!(params["redirect_uri"], a.redirect);
    assert!(!params.contains_key("id_token_hint"));
    let (b, _) = Attempt::new("urn:uuid:test", a.redirect.clone(), None).unwrap();
    assert_ne!(a.state, b.state);
    assert_ne!(a.nonce, b.nonce);
    assert_ne!(a.verifier, b.verifier);
    let id = host_id().unwrap();
    assert!(id.starts_with("urn:uuid:"));
    assert_eq!(id.len(), 45);
    let mut saved = super::super::tests::example();
    saved.client_id = "issued-wordcraft-client".into();
    let (returning, url) = Attempt::new(&id, a.redirect, Some(&saved)).unwrap();
    assert!(!url.query_pairs().any(|(key, _)| key == "agent_name_hint" || key == "id_token_hint"));
    let query = BTreeMap::from([("state".into(), returning.state.clone()), ("code".into(), "test-code".into())]);
    assert_eq!(returning.grant(&query).unwrap().1, "issued-wordcraft-client");
    let mut changed = query;
    changed.insert("client_id".into(), "another-client".into());
    assert!(returning.grant(&changed).is_err());
}

#[test]
fn callbacks_reject_wrong_host_state_duplicate_parameters_denial_and_missing_client() {
    let (attempt, _) = Attempt::new("test-host", "http://127.0.0.1:1234/auth/callback".into(), None).unwrap();
    let query = format!("state={}&code=test-code&client_id=issued-client", attempt.state);
    let parsed = parse_callback(&callback("127.0.0.1:1234", &query), "127.0.0.1:1234").unwrap();
    assert_eq!(attempt.grant(&parsed).unwrap().1, "issued-client");
    assert!(parse_callback(&callback("evil.test", &query), "127.0.0.1:1234").is_err());
    assert!(parse_callback(&callback("127.0.0.1:1234", &(query.clone() + "&state=duplicate")), "127.0.0.1:1234").is_err());
    assert!(parse_callback(&"x".repeat(MAX_HEADERS + 1), "127.0.0.1:1234").is_err());
    let mut bad = parsed.clone();
    bad.insert("state".into(), "wrong".into());
    assert!(attempt.grant(&bad).is_err());
    let mut bad = parsed.clone();
    bad.remove("client_id");
    assert!(attempt.grant(&bad).is_err());
    let mut bad = parsed;
    bad.insert("error".into(), "access_denied".into());
    let error = attempt.grant(&bad).unwrap_err();
    assert!(error.contains("denied"));
    assert!(!error.contains("test-code"));
}

#[test]
fn loopback_listener_ignores_unrelated_requests_and_honors_cancellation() {
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
    let host = listener.local_addr().unwrap().to_string();
    let (attempt, _) = Attempt::new("test-host", format!("http://{host}/auth/callback"), None).unwrap();
    let valid = callback(&host, &format!("state={}&code=test-code&client_id=issued-client", attempt.state));
    let address = host.clone();
    let browser = std::thread::spawn(move || {
        let mut bad = TcpStream::connect(&address).unwrap();
        bad.write_all(callback("evil.test", "state=wrong").as_bytes()).unwrap();
        let mut response = String::new();
        bad.read_to_string(&mut response).unwrap();
        assert!(response.contains("400 Bad Request"));
        let mut good = TcpStream::connect(address).unwrap();
        good.write_all(valid.as_bytes()).unwrap();
        let mut response = String::new();
        good.read_to_string(&mut response).unwrap();
        assert!(response.contains("200 OK"));
        assert!(!response.contains("test-code"));
        assert!(response.contains("no-store"));
    });
    assert_eq!(wait(listener, &host, &attempt, &AtomicBool::new(false)).unwrap().1, "issued-client");
    browser.join().unwrap();
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
    assert!(wait(listener, &host, &attempt, &AtomicBool::new(true)).is_err());
}

#[test]
fn identity_token_signature_audience_nonce_and_clock_are_checked() {
    let private = RsaPrivateKey::new(&mut OsRng, 2048).unwrap();
    let public = RsaPublicKey::from(&private).to_pkcs1_der().unwrap();
    let key = private.to_pkcs1_der().unwrap();
    let signing = EncodingKey::from_rsa_der(key.as_bytes());
    let verifying = DecodingKey::from_rsa_der(public.as_bytes());
    let time = now().unwrap() as u64;
    let claims = json!({"iss":ISSUER,"aud":"issued-client","sub":"test-subject","exp":time+3600,"iat":time,"nonce":"test-nonce"});
    let token = encode(&Header::new(Algorithm::RS256), &claims, &signing).unwrap();
    assert!(verify_id(&token, "issued-client", "test-nonce", &verifying).is_ok());
    assert!(verify_id(&token, "wrong-client", "test-nonce", &verifying).is_err());
    assert!(verify_id(&token, "issued-client", "wrong-nonce", &verifying).is_err());
    for (field, value) in [("exp", json!(time - 300)), ("iat", json!(time + 300)), ("iss", json!("https://evil.test"))] {
        let mut bad = claims.clone();
        bad[field] = value;
        let token = encode(&Header::new(Algorithm::RS256), &bad, &signing).unwrap();
        assert!(verify_id(&token, "issued-client", "test-nonce", &verifying).is_err());
    }
}

#[test]
fn returning_registration_can_survive_sign_out_without_retaining_tokens() {
    let registration = registration_metadata(&json!({"client_id":"issued-wordcraft-client","subject":"test-subject"})).unwrap();
    assert!(registration.access_token.is_empty());
    assert!(registration.refresh_token.is_empty());
    assert!(registration.id_token.is_empty());
    let (attempt, url) = Attempt::new("urn:uuid:test", "http://127.0.0.1:1234/auth/callback".into(), Some(&registration)).unwrap();
    assert_eq!(attempt.client_id, "issued-wordcraft-client");
    assert!(!url.query_pairs().any(|(key, _)| key == "agent_name_hint" || key == "id_token_hint"));
    assert!(registration_metadata(&json!({"client_id":"dynamic_agent_client","subject":"test-subject"})).is_err());
    assert!(registration_metadata(&json!({"client_id":"issued-wordcraft-client"})).is_err());
}
