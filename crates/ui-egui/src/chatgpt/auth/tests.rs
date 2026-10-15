use super::*;
use jsonwebtoken::{EncodingKey, Header, encode};
use rsa::{
    RsaPrivateKey, RsaPublicKey,
    pkcs1::{EncodeRsaPrivateKey, EncodeRsaPublicKey},
    rand_core::OsRng,
};

fn example() -> Credentials {
    Credentials {
        client_id: "test-registration".into(),
        subject: "test-subject".into(),
        access_token: "placeholder".into(),
        refresh_token: "placeholder".into(),
        id_token: "placeholder".into(),
        scopes: Vec::new(),
        expires_at: 0.0,
        earliest_refresh_at: 0.0,
    }
}

#[test]
fn imports_require_signed_identity_scope_audience_issuer_and_expiry() {
    // Keys exist only for this test run; no private credential fixture is committed.
    let private = RsaPrivateKey::new(&mut OsRng, 2048).unwrap();
    let public = RsaPublicKey::from(&private).to_pkcs1_der().unwrap();
    let private_pem = private.to_pkcs1_der().unwrap();
    let signing = EncodingKey::from_rsa_der(private_pem.as_bytes());
    let verifying = DecodingKey::from_rsa_der(public.as_bytes());
    let now = now().unwrap() as u64;
    let good = json!({"iss": ISSUER, "aud": API, "sub": "test-subject", "client_id": "test-registration", "iat": now,
        "exp": now + 3600, "scope": SCOPES.join(" ")});
    let mut credentials = example();
    credentials.access_token = encode(&Header::new(Algorithm::RS256), &good, &signing).unwrap();
    credentials.expires_at = f64::MAX;
    credentials.verify_key(&verifying).unwrap();
    assert_eq!(credentials.expires_at, (now + 3600) as f64);
    assert_eq!(credentials.scopes.len(), 4);
    for (field, value) in [
        ("iss", json!("https://evil.test")),
        ("aud", json!("wrong-audience")),
        ("sub", json!("another-user")),
        ("client_id", json!("another-registration")),
        ("exp", json!(now - 300)),
        ("iat", json!(now + 300)),
        ("scope", json!("openid offline_access")),
        ("nbf", json!(now + 300)),
    ] {
        let mut claims = good.clone();
        claims[field] = value;
        credentials.access_token = encode(&Header::new(Algorithm::RS256), &claims, &signing).unwrap();
        assert!(credentials.verify_key(&verifying).is_err(), "accepted {field}");
    }
    credentials.access_token = encode(&Header::new(Algorithm::HS256), &good, &EncodingKey::from_secret(b"test-only")).unwrap();
    assert!(credentials.verify_key(&verifying).is_err());
    let wrong = RsaPrivateKey::new(&mut OsRng, 2048).unwrap().to_pkcs1_der().unwrap();
    credentials.access_token = encode(&Header::new(Algorithm::RS256), &good, &EncodingKey::from_rsa_der(wrong.as_bytes())).unwrap();
    assert!(credentials.verify_key(&verifying).is_err());
}

#[test]
fn private_credentials_survive_replacement_and_are_size_bounded() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("account.json");
    assert!(load(&path).unwrap().is_none());
    let mut credentials = example();
    save(&path, &credentials).unwrap();
    credentials.refresh_token = "rotated-placeholder".into();
    save(&path, &credentials).unwrap();
    assert_eq!(load(&path).unwrap().unwrap().refresh_token, "rotated-placeholder");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    }
    assert!(Credentials::parse(&" ".repeat(MAX_AUTH + 1)).is_err());
    assert!(Credentials::parse("{}").is_err());
    credentials.client_id = "dynamic_agent_client".into();
    assert!(Credentials::parse(&serde_json::to_string(&credentials).unwrap()).is_err());
    fs::write(path, "x".repeat(MAX_AUTH + 1)).unwrap();
    assert!(load(&dir.path().join("account.json")).is_err());
}

#[test]
fn rotating_refresh_requires_a_complete_replacement_and_retains_id_token_when_omitted() {
    let mut credentials = example();
    let replacement =
        json!({"access_token": "new-access-placeholder", "refresh_token": "new-refresh-placeholder", "expires_in": 3600, "token_type": "Bearer"});
    for field in ["access_token", "refresh_token", "expires_in", "token_type"] {
        let mut incomplete = replacement.clone();
        incomplete.as_object_mut().unwrap().remove(field);
        assert!(credentials.replace_tokens(&incomplete).is_err());
        assert_eq!(credentials.refresh_token, "placeholder");
    }
    credentials.replace_tokens(&replacement).unwrap();
    assert_eq!(credentials.access_token, "new-access-placeholder");
    assert_eq!(credentials.refresh_token, "new-refresh-placeholder");
    assert_eq!(credentials.id_token, "placeholder");
    assert!(credentials.expires_at > now().unwrap() + 3500.0);
}
