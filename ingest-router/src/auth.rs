//! The ingest-router authenticates as an internal Relay: it owns an ed25519 keypair and a
//! relay id (from a `credentials.json`). Most forwarded requests are a transparent pass-through —
//! synapse leaves the inbound `X-Sentry-Relay-Id` / `X-Sentry-Relay-Signature` untouched and
//! the upstream verifies the originating relay directly.
//!
//! The exception is the project-configs endpoint: Synapse rewrites the body to fan keys out
//! across cells, which invalidates the inbound signature. In this scenario it re-signs each
//! rewritten body with its own credentials. This module exists to support this use case.
//!
//!  ([`RelaySigner`]) is responsible for re-signing requests with Synapse's credentials.
//! Once synapse re-signs, the upstream accepts the request as Synapse's own trusted traffic.
//! [`RelayVerifier`] checks the inbound signature against a configured set of trusted downstream relays.
//!
//! Signing and verification follow relay-auth's scheme:
//! - `X-Sentry-Relay-Id` contains the Relay ID
//! - `X-Sentry-Relay-Signature` is an ed25519 signature over the request body (plus an embedded timestamp)
//!
//! On verify the timestamp is required: a signature is rejected if its timestamp is
//! missing, stale (older than 5 minutes), or future-dated. This matches relay-auth's
//! scheme after https://github.com/getsentry/relay/pull/6069.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use hyper::header::{HeaderMap, HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

/// Signature freshness window, matching Sentry
/// https://github.com/getsentry/sentry/blob/c9138b328e9aad58f95f087c0f8a8843a06dbbe9/src/sentry/api/authentication.py#L260
const SIGNATURE_MAX_AGE_SECS: i64 = 300;

/// The `relay-auth` signature header, carried (base64url-encoded) inside the signature value.
#[derive(Debug, Serialize, Deserialize)]
struct SignatureHeader {
    /// When the payload was signed.
    #[serde(rename = "t")]
    timestamp: chrono::DateTime<chrono::Utc>,
    /// relay-auth's signature algorithm (`a`), captured only so we can reject anything other
    /// than the default `Regular` (`v0`) scheme — which is the only algorithm synapse verifies.
    /// Synapse never emits it (an absent `a` means `Regular`), so it's skipped on serialize.
    #[serde(rename = "a", default, skip_serializing_if = "Option::is_none")]
    signature_algorithm: Option<String>,
}

/// Header carrying the relay id (a UUID) identifying the signing relay.
pub static RELAY_ID_HEADER: HeaderName = HeaderName::from_static("x-sentry-relay-id");
/// Header carrying the request body signature.
pub static RELAY_SIGNATURE_HEADER: HeaderName = HeaderName::from_static("x-sentry-relay-signature");

#[derive(thiserror::Error, Debug)]
pub enum SigningError {
    #[error("could not read credentials file: {0}")]
    Io(#[from] std::io::Error),
    #[error("could not parse credentials file: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("invalid secret_key encoding")]
    BadKeyEncoding,
    #[error("invalid secret_key length: expected 32 or 64 bytes, got {0}")]
    BadKeyLength(usize),
}

/// Relay credentials, matching the `credentials.json` produced by `relay credentials generate`.
#[derive(Debug, Deserialize)]
struct Credentials {
    secret_key: String,
    id: String,
}

/// Signs outgoing requests with synapse's relay credentials.
#[derive(Clone)]
pub struct RelaySigner {
    signing_key: SigningKey,
    relay_id: HeaderValue,
}

impl RelaySigner {
    /// Loads relay credentials from a `relay credentials generate`-style `credentials.json`.
    pub fn from_file(path: &Path) -> Result<Self, SigningError> {
        let contents = std::fs::read(path)?;
        let credentials: Credentials = serde_json::from_slice(&contents)?;
        Self::from_credentials(credentials)
    }

    fn from_credentials(credentials: Credentials) -> Result<Self, SigningError> {
        let bytes = URL_SAFE_NO_PAD
            .decode(credentials.secret_key.as_bytes())
            .map_err(|_| SigningError::BadKeyEncoding)?;

        // Relay's SecretKey accepts either a 64-byte keypair or a 32-byte seed so we support both too
        // https://github.com/getsentry/relay/blob/0aac0fc04f8b2e1c834385bb4765380cdf63e138/relay-auth/src/lib.rs#L298-L303
        let signing_key = if let Ok(keypair) = <[u8; 64]>::try_from(bytes.as_slice()) {
            SigningKey::from_keypair_bytes(&keypair).map_err(|_| SigningError::BadKeyEncoding)?
        } else if let Ok(seed) = <[u8; 32]>::try_from(bytes.as_slice()) {
            SigningKey::from_bytes(&seed)
        } else {
            return Err(SigningError::BadKeyLength(bytes.len()));
        };

        let relay_id = HeaderValue::from_str(&credentials.id)
            .map_err(|_| SigningError::BadKeyEncoding)
            .map(|mut v| {
                v.set_sensitive(false);
                v
            })?;

        Ok(Self {
            signing_key,
            relay_id,
        })
    }

    /// Computes the `X-Sentry-Relay-Signature` value for `body`.
    ///
    /// The signature is stamped with the current time, matching relay-auth: each hop re-signs
    /// with its own fresh timestamp rather than carrying the inbound request's age forward.
    fn sign_body(&self, body: &[u8]) -> String {
        let header = SignatureHeader {
            timestamp: chrono::Utc::now(),
            signature_algorithm: None,
        };
        let header_json = serde_json::to_vec(&header).expect("SignatureHeader serializes");

        let mut message = header_json.clone();
        message.push(b'\x00');
        message.extend_from_slice(body);
        let signature = self.signing_key.sign(&message);

        let mut value = URL_SAFE_NO_PAD.encode(signature.to_bytes());
        value.push('.');
        value.push_str(&URL_SAFE_NO_PAD.encode(&header_json));
        value
    }

    /// Replaces any inbound relay-auth headers with synapse's relay id and a fresh
    /// signature over `body`.
    pub fn sign_request(&self, headers: &mut HeaderMap, body: &[u8]) {
        let signature = HeaderValue::from_str(&self.sign_body(body))
            .expect("base64 signature is always a valid header value");

        headers.insert(RELAY_ID_HEADER.clone(), self.relay_id.clone());
        headers.insert(RELAY_SIGNATURE_HEADER.clone(), signature);
    }
}

/// Generates a fresh relay `credentials.json`, matching the format produced by
/// `relay credentials generate` and consumed by [`RelaySigner::from_file`]: a new ed25519
/// keypair (`secret_key` is the 32-byte seed, `public_key` the verifying key, both
/// base64url-nopad) plus a random UUIDv4 relay `id`.
///
/// Returns the pretty-printed JSON to write to disk. The `public_key` must be registered with
/// the upstream (its `static_relays`) before it will accept synapse's signatures.
pub fn generate_credentials_json() -> String {
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).expect("OS entropy is available");
    let signing_key = SigningKey::from_bytes(&seed);

    let credentials = serde_json::json!({
        "secret_key": URL_SAFE_NO_PAD.encode(seed),
        "public_key": URL_SAFE_NO_PAD.encode(signing_key.verifying_key().to_bytes()),
        "id": uuid::Uuid::new_v4().to_string(),
    });
    serde_json::to_string_pretty(&credentials).expect("credentials JSON serializes")
}

#[derive(thiserror::Error, Debug, PartialEq, Eq)]
pub enum VerifyError {
    #[error("invalid trusted relay public key for {0}")]
    InvalidPublicKey(String),
    #[error("missing {} header", RELAY_ID_HEADER.as_str())]
    MissingRelayId,
    #[error("missing {} header", RELAY_SIGNATURE_HEADER.as_str())]
    MissingSignature,
    #[error("relay {0} is not a trusted relay")]
    UntrustedRelay(String),
    #[error("signature verification failed")]
    BadSignature,
    #[error("signature has expired")]
    Expired,
    #[error("unsupported signature algorithm: {0}")]
    UnsupportedAlgorithm(String),
}

/// Configuration for a single trusted downstream relay, matching the upstream's
/// `static_relays` entry shape.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct RelayInfo {
    /// base64url-nopad encoding of the relay's 32-byte ed25519 public key.
    pub public_key: String,
}

/// Verifies inbound requests against a configured set of trusted downstream relays.
///
/// Synapse re-signs forwarded requests with its own (upstream-trusted) credentials, so it
/// must authenticate the caller first. Only relays whose public key is configured here are
/// allowed; anyone else is rejected before their request is re-signed.
///
/// The trusted set is fixed and small, so keys are configured statically rather than resolved
/// at runtime via Sentry's `publickeys` endpoint (the mechanism relay-to-relay verification uses
/// for a dynamic relay set).
#[derive(Clone, Default)]
pub struct RelayVerifier {
    /// Trusted downstream relays, keyed by relay id (a UUID).
    trusted_relays: Arc<HashMap<String, VerifyingKey>>,
}

impl RelayVerifier {
    /// Builds a verifier from a `relay_id -> RelayInfo` map (the upstream's `static_relays`
    /// equivalent).
    pub fn from_relays(relays: HashMap<String, RelayInfo>) -> Result<Self, VerifyError> {
        let trusted_relays = relays
            .into_iter()
            .map(|(id, info)| Ok((id.clone(), parse_public_key(&info.public_key, &id)?)))
            .collect::<Result<HashMap<_, _>, VerifyError>>()?;
        Ok(Self {
            trusted_relays: Arc::new(trusted_relays),
        })
    }

    /// Verifies the `X-Sentry-Relay-Id` / `X-Sentry-Relay-Signature` headers against `body`.
    ///
    /// Mirrors `relay-auth`'s `unpack`: the signature is checked against the relay's public
    /// key and the embedded timestamp must lie within the freshness window (neither older
    /// than `SIGNATURE_MAX_AGE_SECS` nor in the future).
    pub fn verify_request(&self, headers: &HeaderMap, body: &[u8]) -> Result<(), VerifyError> {
        let relay_id = headers
            .get(&RELAY_ID_HEADER)
            .and_then(|v| v.to_str().ok())
            .ok_or(VerifyError::MissingRelayId)?;
        let signature = headers
            .get(&RELAY_SIGNATURE_HEADER)
            .and_then(|v| v.to_str().ok())
            .ok_or(VerifyError::MissingSignature)?;

        let key = self
            .trusted_relays
            .get(relay_id)
            .ok_or_else(|| VerifyError::UntrustedRelay(relay_id.to_string()))?;

        // `relay-auth` signature value is `base64url(sig).base64url(header_json)`.
        let (sig_b64, header_b64) = signature.split_once('.').ok_or(VerifyError::BadSignature)?;
        let sig_bytes = URL_SAFE_NO_PAD
            .decode(sig_b64)
            .map_err(|_| VerifyError::BadSignature)?;
        let signature = Signature::from_slice(&sig_bytes).map_err(|_| VerifyError::BadSignature)?;
        let header_json = URL_SAFE_NO_PAD
            .decode(header_b64)
            .map_err(|_| VerifyError::BadSignature)?;

        // Parse the header before verifying so an unsupported algorithm produces a clear error.
        // A header without a timestamp fails to parse and is rejected as a bad signature.
        let header: SignatureHeader =
            serde_json::from_slice(&header_json).map_err(|_| VerifyError::BadSignature)?;

        // Synapse only produces and verifies the default `Regular` (`v0`) algorithm. Reject any
        // other algorithm up front: `key.verify` below only checks `Regular` signatures, so a
        // prehashed (`v1`) or future signature would otherwise fail as an opaque mismatch.
        if let Some(algo) = header.signature_algorithm.as_deref()
            && algo != "v0"
        {
            return Err(VerifyError::UnsupportedAlgorithm(algo.to_string()));
        }

        let mut message = header_json.clone();
        message.push(b'\x00');
        message.extend_from_slice(body);
        key.verify(&message, &signature)
            .map_err(|_| VerifyError::BadSignature)?;

        // Reject stale and future-dated signatures (replay protection), matching relay-auth's
        // `is_valid_time`: the timestamp must lie within [now - max_age, now].
        let age = chrono::Utc::now() - header.timestamp;
        if age < chrono::Duration::zero() || age > chrono::Duration::seconds(SIGNATURE_MAX_AGE_SECS)
        {
            return Err(VerifyError::Expired);
        }

        Ok(())
    }
}

/// Parses a base64url-nopad ed25519 public key, as found in `static_relays` config.
fn parse_public_key(key: &str, relay_id: &str) -> Result<VerifyingKey, VerifyError> {
    let err = || VerifyError::InvalidPublicKey(relay_id.to_string());
    let bytes = URL_SAFE_NO_PAD.decode(key).map_err(|_| err())?;
    let array: [u8; 32] = bytes.as_slice().try_into().map_err(|_| err())?;
    VerifyingKey::from_bytes(&array).map_err(|_| err())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Verifier, VerifyingKey};

    // A credentials.json as produced by `relay credentials generate`: the `secret_key` is the
    // 32-byte seed, which is what relay's `SecretKey` serializes by default. (The 64-byte keypair
    // form is also accepted on load; see `accepts_64_byte_keypair_form`.)
    fn test_credentials() -> (Credentials, VerifyingKey) {
        let signing_key = SigningKey::from_bytes(&[7u8; 32]);
        let verifying_key = signing_key.verifying_key();
        let credentials = Credentials {
            secret_key: URL_SAFE_NO_PAD.encode(signing_key.to_bytes()),
            id: "00000000-0000-0000-0000-000000000000".to_string(),
        };
        (credentials, verifying_key)
    }

    /// Reconstructs the signed message from a signature value and verifies it, mirroring
    /// what Sentry's `unpack` does on the receiving end.
    fn verify(verifying_key: &VerifyingKey, body: &[u8], value: &str) -> bool {
        let (sig_b64, header_b64) = value.split_once('.').expect("signature has header part");
        let sig_bytes = URL_SAFE_NO_PAD.decode(sig_b64).unwrap();
        let header_json = URL_SAFE_NO_PAD.decode(header_b64).unwrap();

        let mut message = header_json;
        message.push(b'\x00');
        message.extend_from_slice(body);

        let signature = ed25519_dalek::Signature::from_slice(&sig_bytes).unwrap();
        verifying_key.verify(&message, &signature).is_ok()
    }

    #[test]
    fn signs_and_verifies() {
        let (credentials, verifying_key) = test_credentials();
        let signer = RelaySigner::from_credentials(credentials).unwrap();

        let body = br#"{"publicKeys":["abc"]}"#;
        let value = signer.sign_body(body);

        assert!(verify(&verifying_key, body, &value));
        // A different body must not verify against the same signature.
        assert!(!verify(&verifying_key, b"tampered", &value));
    }

    #[test]
    fn sign_request_replaces_inbound_headers() {
        let (credentials, verifying_key) = test_credentials();
        let signer = RelaySigner::from_credentials(credentials).unwrap();

        let mut headers = HeaderMap::new();
        headers.insert(
            RELAY_ID_HEADER.clone(),
            HeaderValue::from_static("inbound-relay"),
        );
        headers.insert(
            RELAY_SIGNATURE_HEADER.clone(),
            HeaderValue::from_static("stale-signature"),
        );

        let body = br#"{"publicKeys":["key1"]}"#;
        signer.sign_request(&mut headers, body);

        assert_eq!(
            headers.get(&RELAY_ID_HEADER).unwrap(),
            "00000000-0000-0000-0000-000000000000"
        );
        let value = headers
            .get(&RELAY_SIGNATURE_HEADER)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(verify(&verifying_key, body, value));
    }

    #[test]
    fn accepts_64_byte_keypair_form() {
        // relay also accepts the expanded 64-byte keypair encoding (`SecretKey`'s alternate
        // `{:#}` form), so `from_credentials` must load it too.
        let signing_key = SigningKey::from_bytes(&[3u8; 32]);
        let credentials = Credentials {
            secret_key: URL_SAFE_NO_PAD.encode(signing_key.to_keypair_bytes()),
            id: "11111111-1111-1111-1111-111111111111".to_string(),
        };

        let signer = RelaySigner::from_credentials(credentials).unwrap();
        let body = b"body";
        assert!(verify(
            &signing_key.verifying_key(),
            body,
            &signer.sign_body(body)
        ));
    }

    #[test]
    fn rejects_bad_key_length() {
        let credentials = Credentials {
            secret_key: URL_SAFE_NO_PAD.encode([0u8; 16]),
            id: "id".to_string(),
        };
        assert!(matches!(
            RelaySigner::from_credentials(credentials),
            Err(SigningError::BadKeyLength(16))
        ));
    }

    fn write_tmp_file(s: &str) -> tempfile::NamedTempFile {
        use std::io::Write as _;
        let mut tmp = tempfile::NamedTempFile::new().expect("create temp file");
        write!(tmp, "{s}").expect("write temp file");
        tmp
    }

    #[test]
    fn from_file_missing_file_is_io_error() {
        let result = RelaySigner::from_file(Path::new("/no/such/credentials.json"));
        assert!(matches!(result, Err(SigningError::Io(_))));
    }

    #[test]
    fn from_file_malformed_json_is_parse_error() {
        let file = write_tmp_file("this is not json");
        let result = RelaySigner::from_file(file.path());
        assert!(matches!(result, Err(SigningError::Parse(_))));
    }

    #[test]
    fn generated_credentials_round_trip() {
        // Generated credentials must load via `from_file` and produce signatures that verify
        // against the `public_key` embedded in the same file — locking the generator's output
        // format to what synapse itself consumes.
        let json = generate_credentials_json();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        // `id` is a valid UUID, and the verifier trusts the generated relay's public key.
        let id = parsed["id"].as_str().unwrap();
        assert!(uuid::Uuid::parse_str(id).is_ok());
        let public_key = parsed["public_key"].as_str().unwrap().to_string();

        let file = write_tmp_file(&json);
        let signer = RelaySigner::from_file(file.path()).unwrap();
        let verifier =
            RelayVerifier::from_relays(HashMap::from([(id.to_string(), RelayInfo { public_key })]))
                .unwrap();

        let body = br#"{"publicKeys":["key1"]}"#;
        let headers = signed_headers(&signer, body);
        assert_eq!(verifier.verify_request(&headers, body), Ok(()));
    }

    const DOWNSTREAM_ID: &str = "00000000-0000-0000-0000-000000000000";

    /// Builds a signer plus a verifier that trusts that signer's relay id + public key.
    fn signer_and_verifier() -> (RelaySigner, RelayVerifier) {
        let (credentials, verifying_key) = test_credentials();
        let signer = RelaySigner::from_credentials(credentials).unwrap();
        let verifier = RelayVerifier::from_relays(HashMap::from([(
            DOWNSTREAM_ID.to_string(),
            RelayInfo {
                public_key: URL_SAFE_NO_PAD.encode(verifying_key.to_bytes()),
            },
        )]))
        .unwrap();
        (signer, verifier)
    }

    fn signed_headers(signer: &RelaySigner, body: &[u8]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        signer.sign_request(&mut headers, body);
        headers
    }

    #[test]
    fn verifies_a_signed_request() {
        let (signer, verifier) = signer_and_verifier();
        let body = br#"{"publicKeys":["key1"]}"#;
        let headers = signed_headers(&signer, body);
        assert_eq!(verifier.verify_request(&headers, body), Ok(()));
    }

    #[test]
    fn rejects_tampered_body() {
        let (signer, verifier) = signer_and_verifier();
        let headers = signed_headers(&signer, br#"{"publicKeys":["key1"]}"#);
        assert_eq!(
            verifier.verify_request(&headers, b"tampered"),
            Err(VerifyError::BadSignature)
        );
    }

    #[test]
    fn rejects_untrusted_relay() {
        let (signer, _) = signer_and_verifier();
        let verifier = RelayVerifier::default(); // trusts nobody
        let body = b"body";
        let headers = signed_headers(&signer, body);
        assert_eq!(
            verifier.verify_request(&headers, body),
            Err(VerifyError::UntrustedRelay(DOWNSTREAM_ID.to_string()))
        );
    }

    #[test]
    fn rejects_missing_headers() {
        let (_, verifier) = signer_and_verifier();
        assert_eq!(
            verifier.verify_request(&HeaderMap::new(), b"body"),
            Err(VerifyError::MissingRelayId)
        );

        let mut only_id = HeaderMap::new();
        only_id.insert(
            RELAY_ID_HEADER.clone(),
            HeaderValue::from_static(DOWNSTREAM_ID),
        );
        assert_eq!(
            verifier.verify_request(&only_id, b"body"),
            Err(VerifyError::MissingSignature)
        );
    }

    /// Produces a genuinely-signed request over caller-supplied header JSON, bypassing
    /// `sign_body` (which always stamps a fresh timestamp and never sets an algorithm).
    /// The signature is real and verifies fine; only the header content is chosen by the
    /// caller, so tests can drive the timestamp/algorithm guards (which run *after*
    /// signature verification) in isolation.
    fn sign_raw_header(signer: &RelaySigner, header_json: &[u8], body: &[u8]) -> HeaderMap {
        let mut message = header_json.to_vec();
        message.push(b'\x00');
        message.extend_from_slice(body);
        let sig = signer.signing_key.sign(&message);
        let value = format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(sig.to_bytes()),
            URL_SAFE_NO_PAD.encode(header_json)
        );

        let mut headers = HeaderMap::new();
        headers.insert(
            RELAY_ID_HEADER.clone(),
            HeaderValue::from_static(DOWNSTREAM_ID),
        );
        headers.insert(
            RELAY_SIGNATURE_HEADER.clone(),
            HeaderValue::from_str(&value).unwrap(),
        );
        headers
    }

    /// Serializes a `SignatureHeader` with the given timestamp (no algorithm).
    fn header_with_timestamp(timestamp: chrono::DateTime<chrono::Utc>) -> Vec<u8> {
        serde_json::to_vec(&SignatureHeader {
            timestamp,
            signature_algorithm: None,
        })
        .unwrap()
    }

    #[test]
    fn rejects_expired_signature() {
        let (signer, verifier) = signer_and_verifier();
        let body = b"body";
        let stale = chrono::Utc::now() - chrono::Duration::seconds(SIGNATURE_MAX_AGE_SECS + 60);
        let headers = sign_raw_header(&signer, &header_with_timestamp(stale), body);

        assert_eq!(
            verifier.verify_request(&headers, body),
            Err(VerifyError::Expired)
        );
    }

    #[test]
    fn rejects_future_signature() {
        let (signer, verifier) = signer_and_verifier();
        let body = b"body";
        let future = chrono::Utc::now() + chrono::Duration::seconds(60);
        let headers = sign_raw_header(&signer, &header_with_timestamp(future), body);

        assert_eq!(
            verifier.verify_request(&headers, body),
            Err(VerifyError::Expired)
        );
    }

    #[test]
    fn rejects_missing_timestamp() {
        let (signer, verifier) = signer_and_verifier();
        let body = b"body";
        // A validly-signed header with no `t` field must not bypass the freshness check.
        let headers = sign_raw_header(&signer, b"{}", body);

        assert_eq!(
            verifier.verify_request(&headers, body),
            Err(VerifyError::BadSignature)
        );
    }

    #[test]
    fn rejects_unsupported_algorithm() {
        let (signer, verifier) = signer_and_verifier();
        let body = b"body";
        // A validly-signed header requesting the prehashed (`v1`) algorithm, which synapse does
        // not implement, must be rejected with a clear error rather than an opaque mismatch.
        let header_json = serde_json::to_vec(&serde_json::json!({
            "t": chrono::Utc::now(),
            "a": "v1",
        }))
        .unwrap();
        let headers = sign_raw_header(&signer, &header_json, body);

        assert_eq!(
            verifier.verify_request(&headers, body),
            Err(VerifyError::UnsupportedAlgorithm("v1".to_string()))
        );
    }

    #[test]
    fn from_relays_rejects_bad_key() {
        let result = RelayVerifier::from_relays(HashMap::from([(
            "relay-x".to_string(),
            RelayInfo {
                public_key: "not-valid-base64-key!!".to_string(),
            },
        )]));
        assert_eq!(
            result.err(),
            Some(VerifyError::InvalidPublicKey("relay-x".to_string()))
        );
    }
}
