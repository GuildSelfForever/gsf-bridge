//! Connexion du bridge (contrat §1) : navigateur + écoute loopback, puis
//! échange du code contre un token Sanctum stocké dans le gestionnaire
//! d'identifiants du système ([`KeyringStore`]).

use std::sync::Mutex;
use std::time::{Duration, Instant};

use rand::distr::{Alphanumeric, SampleString};
use tiny_http::{Header, Response, Server};
use url::Url;

use crate::api::{ApiClient, ApiError, BridgeUser};

/// Service du gestionnaire d'identifiants (= identifiant de l'application).
pub const KEYRING_SERVICE: &str = "fr.gs4ever.bridge";
/// Compte sous lequel le token est rangé.
pub const KEYRING_USER: &str = "bridge-token";
/// Compte sous lequel le secret d'appareil (contrat « vigies ») est rangé,
/// même service que le token.
pub const KEYRING_DEVICE_SECRET_USER: &str = "device-secret";

/// Erreur du stockage du token.
#[derive(Debug, thiserror::Error)]
#[error("stockage du token : {0}")]
pub struct StoreError(pub String);

/// Stockage du token, injectable (mémoire dans les tests).
pub trait TokenStore: Send + Sync {
    fn load(&self) -> Result<Option<String>, StoreError>;
    fn save(&self, token: &str) -> Result<(), StoreError>;
    fn clear(&self) -> Result<(), StoreError>;
}

/// Gestionnaire d'identifiants du système (Windows : Credential Manager).
#[derive(Debug, Clone)]
pub struct KeyringStore {
    service: String,
    user: String,
}

impl KeyringStore {
    pub fn new() -> Self {
        KeyringStore {
            service: KEYRING_SERVICE.into(),
            user: KEYRING_USER.into(),
        }
    }

    fn entry(&self) -> Result<keyring::Entry, StoreError> {
        keyring::Entry::new(&self.service, &self.user).map_err(|e| StoreError(e.to_string()))
    }
}

impl Default for KeyringStore {
    fn default() -> Self {
        Self::new()
    }
}

impl TokenStore for KeyringStore {
    fn load(&self) -> Result<Option<String>, StoreError> {
        match self.entry()?.get_password() {
            Ok(t) => Ok(Some(t)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(StoreError(e.to_string())),
        }
    }

    fn save(&self, token: &str) -> Result<(), StoreError> {
        self.entry()?
            .set_password(token)
            .map_err(|e| StoreError(e.to_string()))
    }

    fn clear(&self) -> Result<(), StoreError> {
        match self.entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(StoreError(e.to_string())),
        }
    }
}

/// Stockage en mémoire (tests).
#[derive(Debug, Default)]
pub struct MemoryStore(Mutex<Option<String>>);

impl MemoryStore {
    pub fn with_token(token: &str) -> Self {
        MemoryStore(Mutex::new(Some(token.to_owned())))
    }
}

impl TokenStore for MemoryStore {
    fn load(&self) -> Result<Option<String>, StoreError> {
        Ok(self
            .0
            .lock()
            .map_err(|e| StoreError(e.to_string()))?
            .clone())
    }

    fn save(&self, token: &str) -> Result<(), StoreError> {
        *self.0.lock().map_err(|e| StoreError(e.to_string()))? = Some(token.to_owned());
        Ok(())
    }

    fn clear(&self) -> Result<(), StoreError> {
        *self.0.lock().map_err(|e| StoreError(e.to_string()))? = None;
        Ok(())
    }
}

/// Stockage du secret d'appareil (contrat « vigies », §Bridge), injectable
/// comme [`TokenStore`]. Pas de `clear()` : un appareil refusé ou coupé
/// garde son identité tant qu'il n'a pas reçu `device_secret_mismatch`.
pub trait DeviceSecretStore: Send + Sync {
    fn load(&self) -> Result<Option<String>, StoreError>;
    fn save(&self, secret: &str) -> Result<(), StoreError>;
}

/// Gestionnaire d'identifiants du système, compte [`KEYRING_DEVICE_SECRET_USER`].
#[derive(Debug, Clone, Default)]
pub struct KeyringDeviceSecretStore;

impl KeyringDeviceSecretStore {
    pub fn new() -> Self {
        Self
    }

    fn entry(&self) -> Result<keyring::Entry, StoreError> {
        keyring::Entry::new(KEYRING_SERVICE, KEYRING_DEVICE_SECRET_USER)
            .map_err(|e| StoreError(e.to_string()))
    }
}

impl DeviceSecretStore for KeyringDeviceSecretStore {
    fn load(&self) -> Result<Option<String>, StoreError> {
        match self.entry()?.get_password() {
            Ok(s) => Ok(Some(s)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(StoreError(e.to_string())),
        }
    }

    fn save(&self, secret: &str) -> Result<(), StoreError> {
        self.entry()?
            .set_password(secret)
            .map_err(|e| StoreError(e.to_string()))
    }
}

/// Stockage en mémoire (tests).
#[derive(Debug, Default)]
pub struct MemoryDeviceSecretStore(Mutex<Option<String>>);

impl DeviceSecretStore for MemoryDeviceSecretStore {
    fn load(&self) -> Result<Option<String>, StoreError> {
        Ok(self
            .0
            .lock()
            .map_err(|e| StoreError(e.to_string()))?
            .clone())
    }

    fn save(&self, secret: &str) -> Result<(), StoreError> {
        *self.0.lock().map_err(|e| StoreError(e.to_string()))? = Some(secret.to_owned());
        Ok(())
    }
}

/// Secret aléatoire de 48 caractères `[A-Za-z0-9_-]` (contrat `bridge/devices`).
pub fn generate_device_secret() -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_-";
    let mut rng = rand::rng();
    (0..48)
        .map(|_| CHARS[rand::Rng::random_range(&mut rng, 0..CHARS.len())] as char)
        .collect()
}

/// Erreur de connexion.
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("écoute locale impossible : {0}")]
    Listen(String),
    #[error("ouverture du navigateur impossible : {0}")]
    OpenBrowser(String),
    #[error("délai de connexion dépassé")]
    Timeout,
    #[error("connexion refusée dans le navigateur : {0}")]
    Denied(String),
    #[error(transparent)]
    Api(#[from] ApiError),
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// `state` aléatoire de 32 caractères alphanumériques.
pub fn generate_state() -> String {
    Alphanumeric.sample_string(&mut rand::rng(), 32)
}

/// Nom de l'appareil envoyé au site (`device_name`).
pub fn device_name() -> String {
    let host = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_default();
    let host = host.trim();
    if host.is_empty() {
        "GSF Bridge".into()
    } else {
        format!("GSF Bridge ({host})")
    }
}

const PAGE_OK: &str = "<!doctype html><html lang=\"fr\"><head><meta charset=\"utf-8\"><title>GSF Bridge</title></head>\
<body style=\"font-family:system-ui,sans-serif;text-align:center;margin-top:4rem\">\
<h1>GSF Bridge connecté</h1><p>Vous pouvez fermer cet onglet et revenir à l'application.</p></body></html>";
const PAGE_DENIED: &str = "<!doctype html><html lang=\"fr\"><head><meta charset=\"utf-8\"><title>GSF Bridge</title></head>\
<body style=\"font-family:system-ui,sans-serif;text-align:center;margin-top:4rem\">\
<h1>Connexion annulée</h1><p>Vous pouvez fermer cet onglet.</p></body></html>";
const PAGE_INVALID: &str = "<!doctype html><html lang=\"fr\"><head><meta charset=\"utf-8\"><title>GSF Bridge</title></head>\
<body style=\"font-family:system-ui,sans-serif;text-align:center;margin-top:4rem\">\
<h1>Requête invalide</h1><p>Relancez la connexion depuis l'application.</p></body></html>";

/// Écoute `http://127.0.0.1:<port aléatoire>/callback`.
pub struct LoopbackListener {
    server: Server,
    port: u16,
}

impl LoopbackListener {
    /// Ouvre l'écoute sur un port libre de `127.0.0.1` (jamais `0.0.0.0`).
    pub fn bind() -> Result<Self, AuthError> {
        let server = Server::http("127.0.0.1:0").map_err(|e| AuthError::Listen(e.to_string()))?;
        let port = server
            .server_addr()
            .to_ip()
            .map(|a| a.port())
            .ok_or_else(|| AuthError::Listen("adresse inconnue".into()))?;
        Ok(LoopbackListener { server, port })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// Attend `GET /callback?code=…&state=…` avec le bon `state` et renvoie
    /// le code. Les requêtes au `state` incorrect reçoivent une erreur 400 et
    /// l'attente continue (protection CSRF). Bloquant.
    pub fn wait_for_code(
        &self,
        expected_state: &str,
        timeout: Duration,
    ) -> Result<String, AuthError> {
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(AuthError::Timeout);
            }
            let request = match self.server.recv_timeout(remaining) {
                Ok(Some(r)) => r,
                Ok(None) => return Err(AuthError::Timeout),
                Err(e) => return Err(AuthError::Listen(e.to_string())),
            };
            let outcome = classify(request.method(), request.url(), expected_state);
            let (status, page) = match &outcome {
                Callback::Code(_) => (200, PAGE_OK),
                Callback::Denied(_) => (200, PAGE_DENIED),
                Callback::Invalid => (400, PAGE_INVALID),
                Callback::NotFound => (404, PAGE_INVALID),
            };
            let response = Response::from_string(page)
                .with_status_code(status)
                .with_header(
                    Header::from_bytes("Content-Type", "text/html; charset=utf-8")
                        .expect("en-tête valide"),
                )
                .with_header(
                    Header::from_bytes("Cache-Control", "no-store").expect("en-tête valide"),
                );
            let _ = request.respond(response);
            match outcome {
                Callback::Code(code) => return Ok(code),
                Callback::Denied(reason) => return Err(AuthError::Denied(reason)),
                Callback::Invalid | Callback::NotFound => continue,
            }
        }
    }
}

enum Callback {
    Code(String),
    Denied(String),
    Invalid,
    NotFound,
}

fn classify(method: &tiny_http::Method, raw_url: &str, expected_state: &str) -> Callback {
    let Ok(url) = Url::parse(&format!("http://127.0.0.1{raw_url}")) else {
        return Callback::Invalid;
    };
    if *method != tiny_http::Method::Get || url.path() != "/callback" {
        return Callback::NotFound;
    }
    let (mut code, mut state, mut error) = (None, None, None);
    for (k, v) in url.query_pairs() {
        match k.as_ref() {
            "code" => code = Some(v.into_owned()),
            "state" => state = Some(v.into_owned()),
            "error" => error = Some(v.into_owned()),
            _ => {}
        }
    }
    if state.as_deref() != Some(expected_state) {
        return Callback::Invalid;
    }
    if let Some(e) = error {
        return Callback::Denied(e);
    }
    match code {
        Some(c) if !c.is_empty() && c.len() <= 512 => Callback::Code(c),
        _ => Callback::Invalid,
    }
}

/// Flux complet : écoute loopback, ouverture du navigateur (fonction
/// injectée), attente du code, échange contre un token, stockage.
pub async fn login<F>(
    api: &ApiClient,
    store: &dyn TokenStore,
    device_name: &str,
    timeout: Duration,
    open_url: F,
) -> Result<BridgeUser, AuthError>
where
    F: FnOnce(&str) -> Result<(), String>,
{
    let listener = LoopbackListener::bind()?;
    let state = generate_state();
    open_url(&api.connect_url(listener.port(), &state, device_name))
        .map_err(AuthError::OpenBrowser)?;
    let expected = state.clone();
    let code = tokio::task::spawn_blocking(move || listener.wait_for_code(&expected, timeout))
        .await
        .map_err(|e| AuthError::Listen(e.to_string()))??;
    let grant = api.exchange_code(&code, &state, device_name).await?;
    store.save(&grant.token)?;
    Ok(grant.user)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_is_32_alphanumeric_and_random() {
        let a = generate_state();
        assert_eq!(a.len(), 32);
        assert!(a.chars().all(|c| c.is_ascii_alphanumeric()));
        assert_ne!(a, generate_state());
    }

    #[test]
    fn device_secret_is_48_chars_from_the_allowed_set_and_random() {
        let a = generate_device_secret();
        assert_eq!(a.len(), 48);
        assert!(a
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'));
        assert_ne!(a, generate_device_secret());
    }

    #[test]
    fn memory_device_secret_store_roundtrip() {
        let s = MemoryDeviceSecretStore::default();
        assert_eq!(s.load().unwrap(), None);
        s.save("secret").unwrap();
        assert_eq!(s.load().unwrap().as_deref(), Some("secret"));
    }

    #[test]
    fn memory_store_roundtrip() {
        let s = MemoryStore::default();
        assert_eq!(s.load().unwrap(), None);
        s.save("t").unwrap();
        assert_eq!(s.load().unwrap().as_deref(), Some("t"));
        s.clear().unwrap();
        assert_eq!(s.load().unwrap(), None);
    }

    #[test]
    fn callback_classification() {
        let get = tiny_http::Method::Get;
        assert!(
            matches!(classify(&get, "/callback?code=abc&state=S", "S"), Callback::Code(c) if c == "abc")
        );
        assert!(matches!(
            classify(&get, "/callback?code=abc&state=X", "S"),
            Callback::Invalid
        ));
        assert!(matches!(
            classify(&get, "/callback?code=abc", "S"),
            Callback::Invalid
        ));
        assert!(matches!(
            classify(&get, "/callback?state=S", "S"),
            Callback::Invalid
        ));
        assert!(matches!(
            classify(&get, "/callback?error=denied&state=S", "S"),
            Callback::Denied(_)
        ));
        assert!(matches!(
            classify(&get, "/favicon.ico", "S"),
            Callback::NotFound
        ));
        assert!(matches!(
            classify(&tiny_http::Method::Post, "/callback?code=a&state=S", "S"),
            Callback::NotFound
        ));
    }
}
