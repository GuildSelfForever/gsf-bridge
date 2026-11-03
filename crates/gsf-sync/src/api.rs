//! Client HTTPS du backend (`/api/v1`, contrat `bridge-api-v1`).
//!
//! - `X-Bridge-Version` sur chaque requête ;
//! - corps des snapshots compressé en gzip (`Content-Encoding: gzip`) ;
//! - erreurs `{"error":{"code","message","details"}}` traduites en [`ApiError`] :
//!   `401`/`403` sur une route protégée → [`ApiError::AuthRequired`] (le token
//!   doit être effacé), `426` → [`ApiError::Outdated`] (mise à jour obligatoire).

use std::io::Write;
use std::time::Duration;

use flate2::write::GzEncoder;
use flate2::Compression;
use reqwest::header::{
    HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_ENCODING, CONTENT_TYPE, IF_NONE_MATCH,
};
use reqwest::{Response, StatusCode};
use serde::{Deserialize, Serialize};
use url::Url;

use gsf_core::model::AuditBaseline;

use crate::pushed::PushedData;
use crate::snapshot::Snapshot;

/// Site de production.
/// Site local tant que le domaine de production n'est pas choisi (réglable dans l'application).
pub const DEFAULT_BASE_URL: &str = "https://gsforever.com";
/// En-tête de version du bridge.
pub const VERSION_HEADER: &str = "X-Bridge-Version";

/// Utilisateur associé au token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BridgeUser {
    pub id: serde_json::Value,
    pub username: String,
    #[serde(default)]
    pub roles: Vec<String>,
}

/// Réponse `201` de `POST /api/v1/bridge/token`.
#[derive(Debug, Clone, Deserialize)]
pub struct TokenGrant {
    pub token: String,
    #[serde(default)]
    pub expires_at: Option<String>,
    pub user: BridgeUser,
}

/// Réponse `202`/`200` de `POST /api/v1/bridge/snapshots`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SnapshotResponse {
    /// `accepted`, `duplicate` ou `ignored` (aucune guilde suivie dans le
    /// snapshot : journalisé par le site, rien n'est traité).
    pub status: String,
    #[serde(default)]
    pub upload_id: Option<serde_json::Value>,
    /// Guildes non suivies par le site (format libre, cf. [`Self::ignored`]).
    #[serde(default)]
    pub ignored_guilds: serde_json::Value,
}

impl SnapshotResponse {
    pub fn is_duplicate(&self) -> bool {
        self.status == "duplicate"
    }

    pub fn is_ignored(&self) -> bool {
        self.status == "ignored"
    }

    /// Clés des guildes ignorées (chaînes, ou objets avec `guild_key`/`key`).
    pub fn ignored(&self) -> Vec<String> {
        self.ignored_guilds
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| match v {
                serde_json::Value::String(s) => Some(s.clone()),
                serde_json::Value::Object(o) => o
                    .get("guild_key")
                    .or_else(|| o.get("key"))
                    .and_then(|k| k.as_str())
                    .map(str::to_owned),
                _ => None,
            })
            .collect()
    }
}

/// Résultat de `POST /api/v1/bridge/devices` (enregistrement sans
/// connexion d'un appareil « vigie »).
/// Réponse de `POST /api/v1/bridge/audit-baselines` (contrat 2026-11-03).
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct AuditBaselinesResponse {
    /// Photos enregistrées (plus récentes que celles du site).
    #[serde(default)]
    pub stored: u64,
    /// Photos plus anciennes ou égales à celles du site, gardées telles quelles.
    #[serde(default)]
    pub kept: u64,
    /// Photos écartées et pourquoi (`not_linked`, `invalid`…).
    #[serde(default)]
    pub ignored: Vec<IgnoredBaseline>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct IgnoredBaseline {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceRegistration {
    /// `202 {"status":"pending"}`.
    Pending,
    /// `403 device_refused` : refusé ou coupé par les officiers.
    Refused,
    /// `403 device_secret_mismatch` : secret local invalide pour ce
    /// `device_id`, à réinitialiser (nouveau `device_id` et secret).
    SecretMismatch,
    /// `200 {"status":"approved","token":…}` : token du compte `vigies`.
    Approved { token: String },
}

#[derive(Deserialize)]
struct DeviceApprovedBody {
    #[serde(default)]
    token: Option<String>,
}

/// Erreur d'appel au backend.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ApiError {
    /// `401`/`403` sur une route bridge : token invalide ou utilisateur
    /// rétrogradé. Le token doit être effacé et la connexion redemandée.
    #[error("connexion requise ({message})")]
    AuthRequired { status: u16, message: String },
    /// `426 bridge_outdated` : mise à jour obligatoire.
    #[error("mise à jour obligatoire (version minimale {})", min_version.as_deref().unwrap_or("?"))]
    Outdated { min_version: Option<String> },
    /// `403` à l'échange de code : compte non officier ou inactif.
    #[error("accès refusé : {0}")]
    Forbidden(String),
    /// `422 invalid_code` à l'échange de code.
    #[error("code de connexion invalide ou expiré")]
    InvalidCode,
    /// Autre réponse d'erreur.
    #[error("erreur {status} {code} : {message}")]
    Http {
        status: u16,
        code: String,
        message: String,
    },
    /// Backend injoignable, délai dépassé…
    #[error("backend injoignable : {0}")]
    Network(String),
    #[error("réponse inattendue : {0}")]
    Decode(String),
    #[error("adresse du site invalide : {0}")]
    InvalidUrl(String),
}

impl ApiError {
    /// Vrai si l'envoi peut réussir plus tard sans action de l'utilisateur
    /// (réseau, 5xx, 429) : on le garde en file d'attente.
    pub fn is_retriable(&self) -> bool {
        match self {
            ApiError::Network(_) => true,
            ApiError::Http { status, .. } => *status >= 500 || *status == 429 || *status == 408,
            _ => false,
        }
    }
}

#[derive(Deserialize)]
struct ErrorBody {
    error: ErrorDetail,
}

#[derive(Deserialize)]
struct ErrorDetail {
    #[serde(default)]
    code: String,
    #[serde(default)]
    message: String,
    #[serde(default)]
    details: serde_json::Value,
}

/// Client du backend, bon marché à cloner.
#[derive(Debug, Clone)]
pub struct ApiClient {
    base: String,
    http: reqwest::Client,
}

/// Valide et normalise l'adresse du site (sans `/` final).
pub fn normalize_base_url(base_url: &str) -> Result<String, ApiError> {
    let trimmed = base_url.trim().trim_end_matches('/');
    let url = Url::parse(trimmed).map_err(|e| ApiError::InvalidUrl(format!("{trimmed} : {e}")))?;
    if !matches!(url.scheme(), "https" | "http") || url.host_str().is_none() {
        return Err(ApiError::InvalidUrl(trimmed.to_owned()));
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(ApiError::InvalidUrl(trimmed.to_owned()));
    }
    Ok(trimmed.to_owned())
}

impl ApiClient {
    pub fn new(base_url: &str, bridge_version: &str) -> Result<Self, ApiError> {
        let base = normalize_base_url(base_url)?;
        let mut headers = HeaderMap::new();
        headers.insert(
            VERSION_HEADER,
            HeaderValue::from_str(bridge_version).map_err(|e| ApiError::Decode(e.to_string()))?,
        );
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        let http = reqwest::Client::builder()
            .default_headers(headers)
            .user_agent(format!("gsf-bridge/{bridge_version}"))
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(60))
            .https_only(base.starts_with("https://"))
            .build()
            .map_err(|e| ApiError::Network(e.to_string()))?;
        Ok(ApiClient { base, http })
    }

    /// Adresse du site, sans `/` final.
    pub fn base_url(&self) -> &str {
        &self.base
    }

    fn api(&self, path: &str) -> String {
        format!("{}/api/v1{path}", self.base)
    }

    /// Page web d'autorisation ouverte dans le navigateur (§1.1, décision 1 :
    /// `device` facultatif, ≤ 60 caractères, affiché sur la page).
    pub fn connect_url(&self, port: u16, state: &str, device: &str) -> String {
        let mut url = Url::parse(&format!("{}/bridge/connect", self.base)).expect("base validée");
        url.query_pairs_mut()
            .append_pair("port", &port.to_string())
            .append_pair("state", state);
        let device: String = device.trim().chars().take(60).collect();
        if !device.is_empty() {
            url.query_pairs_mut().append_pair("device", &device);
        }
        url.into()
    }

    /// Adresse de l'updater (§4) avec les variables de tauri-plugin-updater.
    pub fn update_endpoint(&self) -> String {
        self.api("/bridge/update/{{target}}/{{arch}}/{{current_version}}")
    }

    /// Adresse de l'updater sans authentification (contrat étape 1,
    /// `/bridge/public/update/…`), utilisée quand le bridge n'a pas de
    /// jeton : mêmes réponses, sans `Authorization`.
    pub fn public_update_endpoint(&self) -> String {
        self.api("/bridge/public/update/{{target}}/{{arch}}/{{current_version}}")
    }

    /// `POST /api/v1/bridge/token` (sans authentification).
    pub async fn exchange_code(
        &self,
        code: &str,
        state: &str,
        device_name: &str,
    ) -> Result<TokenGrant, ApiError> {
        let body = serde_json::json!({ "code": code, "state": state, "device_name": device_name });
        let res = self
            .http
            .post(self.api("/bridge/token"))
            .json(&body)
            .send()
            .await
            .map_err(network)?;
        match res.status() {
            s if s.is_success() => decode(res).await,
            StatusCode::FORBIDDEN => {
                let (_, message, _) = error_parts(res).await;
                Err(ApiError::Forbidden(message))
            }
            StatusCode::UNPROCESSABLE_ENTITY => Err(ApiError::InvalidCode),
            _ => Err(error_from(res, false).await),
        }
    }

    /// `POST /api/v1/bridge/snapshots`, corps gzip.
    pub async fn post_snapshot(
        &self,
        token: &str,
        snapshot: &Snapshot,
    ) -> Result<SnapshotResponse, ApiError> {
        let json = serde_json::to_vec(snapshot).map_err(|e| ApiError::Decode(e.to_string()))?;
        let body = gzip(&json).map_err(|e| ApiError::Decode(e.to_string()))?;
        let res = self
            .http
            .post(self.api("/bridge/snapshots"))
            .header(AUTHORIZATION, bearer(token)?)
            .header(CONTENT_TYPE, "application/json")
            .header(CONTENT_ENCODING, "gzip")
            .body(body)
            .send()
            .await
            .map_err(network)?;
        if res.status().is_success() {
            decode(res).await
        } else {
            Err(error_from(res, true).await)
        }
    }

    /// `POST /api/v1/bridge/devices` (sans authentification, contrat
    /// « vigies » : limité à 10 requêtes/min/IP côté site).
    pub async fn register_device(
        &self,
        device_id: &str,
        secret: &str,
        device_name: &str,
        characters: &[String],
        bridge_version: &str,
    ) -> Result<DeviceRegistration, ApiError> {
        let body = serde_json::json!({
            "device_id": device_id,
            "secret": secret,
            "device_name": device_name,
            "characters": characters,
            "bridge_version": bridge_version,
        });
        let res = self
            .http
            .post(self.api("/bridge/devices"))
            .json(&body)
            .send()
            .await
            .map_err(network)?;
        match res.status() {
            StatusCode::ACCEPTED => Ok(DeviceRegistration::Pending),
            StatusCode::FORBIDDEN => {
                let (code, _, _) = error_parts(res).await;
                Ok(if code == "device_secret_mismatch" {
                    DeviceRegistration::SecretMismatch
                } else {
                    DeviceRegistration::Refused
                })
            }
            s if s.is_success() => {
                let body: DeviceApprovedBody = decode(res).await?;
                let token = body
                    .token
                    .ok_or_else(|| ApiError::Decode("token absent de la réponse".into()))?;
                Ok(DeviceRegistration::Approved { token })
            }
            _ => Err(error_from(res, false).await),
        }
    }

    /// `POST /api/v1/bridge/heartbeat` (`bridge:read`, contrat 2026-11-02,
    /// bridge pour tous étape 2) : signal de vie léger, toutes les 5 minutes
    /// et à chaque ouverture/fermeture du jeu. `204` attendu. `guild_key` =
    /// guilde active du PC (normalisée), absente si encore inconnue.
    pub async fn heartbeat(
        &self,
        token: &str,
        game_running: bool,
        guild_key: Option<&str>,
    ) -> Result<(), ApiError> {
        let mut body = serde_json::json!({ "game_running": game_running });
        if let Some(key) = guild_key {
            body["guild_key"] = serde_json::Value::from(key);
        }
        let res = self
            .http
            .post(self.api("/bridge/heartbeat"))
            .header(AUTHORIZATION, bearer(token)?)
            .json(&body)
            .send()
            .await
            .map_err(network)?;
        if res.status().is_success() {
            Ok(())
        } else {
            Err(error_from(res, true).await)
        }
    }

    /// `GET /api/v1/bridge/pushed-data`, pour la guilde active du PC
    /// (`guild_key`, normalisée comme `guild_key_normalized`) ; `None` =
    /// comportement historique, données de la guilde Principale du site.
    pub async fn pushed_data(
        &self,
        token: &str,
        guild_key: Option<&str>,
    ) -> Result<PushedData, ApiError> {
        self.pushed_data_if_changed(token, guild_key, None)
            .await?
            .ok_or_else(|| ApiError::Decode("304 sans version connue".into()))
    }

    /// Comme [`Self::pushed_data`], avec `If-None-Match: "<known>"` quand la
    /// version déjà écrite est connue (contrat 2026-11-02 b) : `Ok(None)` sur
    /// `304` (rien n'a changé, aucun corps).
    pub async fn pushed_data_if_changed(
        &self,
        token: &str,
        guild_key: Option<&str>,
        known: Option<&str>,
    ) -> Result<Option<PushedData>, ApiError> {
        self.pushed_data_request(token, guild_key, known, false)
            .await
    }

    /// Comme [`ApiClient::pushed_data_if_changed`], avec en plus
    /// `audit_baselines=1` quand la case « Synchroniser mon addon entre mes
    /// ordinateurs » est cochée (contrat 2026-11-03).
    pub async fn pushed_data_request(
        &self,
        token: &str,
        guild_key: Option<&str>,
        known: Option<&str>,
        audit_baselines: bool,
    ) -> Result<Option<PushedData>, ApiError> {
        let mut url = Url::parse(&self.api("/bridge/pushed-data"))
            .map_err(|e| ApiError::Decode(e.to_string()))?;
        if let Some(key) = guild_key {
            url.query_pairs_mut().append_pair("guild_key", key);
        }
        if audit_baselines {
            url.query_pairs_mut().append_pair("audit_baselines", "1");
        }
        let mut req = self.http.get(url).header(AUTHORIZATION, bearer(token)?);
        if let Some(version) = known.filter(|v| !v.is_empty() && !v.contains('"')) {
            if let Ok(value) = HeaderValue::from_str(&format!("\"{version}\"")) {
                req = req.header(IF_NONE_MATCH, value);
            }
        }
        let res = req.send().await.map_err(network)?;
        if res.status() == StatusCode::NOT_MODIFIED {
            Ok(None)
        } else if res.status().is_success() {
            decode(res).await.map(Some)
        } else {
            Err(error_from(res, true).await)
        }
    }
}

/// Compresse en gzip.
impl ApiClient {
    /// `POST /api/v1/bridge/audit-baselines` (contrat 2026-11-03) : photos
    /// d'audit des persos de ce PC, compressées comme les snapshots.
    pub async fn post_audit_baselines(
        &self,
        token: &str,
        baselines: &[AuditBaseline],
    ) -> Result<AuditBaselinesResponse, ApiError> {
        let json = serde_json::to_vec(&serde_json::json!({ "baselines": baselines }))
            .map_err(|e| ApiError::Decode(e.to_string()))?;
        let body = gzip(&json).map_err(|e| ApiError::Decode(e.to_string()))?;
        let res = self
            .http
            .post(self.api("/bridge/audit-baselines"))
            .header(AUTHORIZATION, bearer(token)?)
            .header(CONTENT_TYPE, "application/json")
            .header(CONTENT_ENCODING, "gzip")
            .body(body)
            .send()
            .await
            .map_err(network)?;
        if res.status().is_success() {
            decode(res).await
        } else {
            Err(error_from(res, true).await)
        }
    }

    /// `DELETE /api/v1/bridge/audit-baselines` : case décochée, le site
    /// oublie les photos du compte.
    pub async fn forget_audit_baselines(&self, token: &str) -> Result<(), ApiError> {
        let res = self
            .http
            .delete(self.api("/bridge/audit-baselines"))
            .header(AUTHORIZATION, bearer(token)?)
            .send()
            .await
            .map_err(network)?;
        if res.status().is_success() {
            Ok(())
        } else {
            Err(error_from(res, true).await)
        }
    }
}

pub fn gzip(bytes: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut enc = GzEncoder::new(Vec::new(), Compression::default());
    enc.write_all(bytes)?;
    enc.finish()
}

fn bearer(token: &str) -> Result<HeaderValue, ApiError> {
    let mut v =
        HeaderValue::from_str(&format!("Bearer {token}")).map_err(|_| ApiError::AuthRequired {
            status: 0,
            message: "token illisible".into(),
        })?;
    v.set_sensitive(true);
    Ok(v)
}

fn network(e: reqwest::Error) -> ApiError {
    ApiError::Network(e.to_string())
}

async fn decode<T: serde::de::DeserializeOwned>(res: Response) -> Result<T, ApiError> {
    let bytes = res.bytes().await.map_err(network)?;
    serde_json::from_slice(&bytes).map_err(|e| ApiError::Decode(e.to_string()))
}

async fn error_parts(res: Response) -> (String, String, serde_json::Value) {
    let status = res.status();
    let bytes = res.bytes().await.unwrap_or_default();
    match serde_json::from_slice::<ErrorBody>(&bytes) {
        Ok(b) => {
            let message = if b.error.message.is_empty() {
                status.canonical_reason().unwrap_or("").to_owned()
            } else {
                b.error.message
            };
            (b.error.code, message, b.error.details)
        }
        Err(_) => (
            String::new(),
            status.canonical_reason().unwrap_or("erreur").to_owned(),
            serde_json::Value::Null,
        ),
    }
}

/// Traduit une réponse d'erreur ; `protected` = route authentifiée par token.
async fn error_from(res: Response, protected: bool) -> ApiError {
    let status = res.status();
    let (code, message, details) = error_parts(res).await;
    match status {
        StatusCode::UPGRADE_REQUIRED => ApiError::Outdated {
            min_version: details
                .get("min_version")
                .and_then(|v| v.as_str())
                .map(str::to_owned),
        },
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN if protected => ApiError::AuthRequired {
            status: status.as_u16(),
            message,
        },
        _ => ApiError::Http {
            status: status.as_u16(),
            code,
            message,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_url_is_validated_and_trimmed() {
        assert_eq!(
            normalize_base_url("https://gsforever.com/").unwrap(),
            "https://gsforever.com"
        );
        assert_eq!(
            normalize_base_url(" http://localhost:8000 ").unwrap(),
            "http://localhost:8000"
        );
        assert!(normalize_base_url("ftp://x").is_err());
        assert!(normalize_base_url("gsforever.com").is_err());
        assert!(normalize_base_url("https://x/?a=1").is_err());
    }

    #[test]
    fn urls_follow_the_contract() {
        let c = ApiClient::new("https://gsforever.com/", "0.1.0").unwrap();
        assert_eq!(
            c.connect_url(51234, "abc", ""),
            "https://gsforever.com/bridge/connect?port=51234&state=abc"
        );
        assert_eq!(
            c.connect_url(51234, "abc", "PC de Dävin"),
            "https://gsforever.com/bridge/connect?port=51234&state=abc&device=PC+de+D%C3%A4vin"
        );
        assert_eq!(
            c.update_endpoint(),
            "https://gsforever.com/api/v1/bridge/update/{{target}}/{{arch}}/{{current_version}}"
        );
        assert_eq!(
            c.public_update_endpoint(),
            "https://gsforever.com/api/v1/bridge/public/update/{{target}}/{{arch}}/{{current_version}}"
        );
    }

    #[test]
    fn retriable_errors() {
        assert!(ApiError::Network("x".into()).is_retriable());
        let http = |status| ApiError::Http {
            status,
            code: String::new(),
            message: String::new(),
        };
        assert!(http(503).is_retriable());
        assert!(http(429).is_retriable());
        assert!(!http(422).is_retriable());
        assert!(!ApiError::Outdated { min_version: None }.is_retriable());
        assert!(!ApiError::AuthRequired {
            status: 403,
            message: String::new()
        }
        .is_retriable());
    }

    #[test]
    fn ignored_guilds_accept_strings_or_objects() {
        let r: SnapshotResponse = serde_json::from_str(
            r#"{"status":"accepted","upload_id":7,"ignored_guilds":["a#b",{"guild_key":"c#d"}]}"#,
        )
        .unwrap();
        assert_eq!(r.ignored(), ["a#b", "c#d"]);
        let r: SnapshotResponse = serde_json::from_str(r#"{"status":"duplicate"}"#).unwrap();
        assert!(r.is_duplicate() && r.ignored().is_empty());
    }
}
