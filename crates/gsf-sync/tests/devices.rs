//! Enregistrement sans connexion d'un appareil (contrat « vigies »,
//! `POST /api/v1/bridge/devices`) : pending, approuvé, refusé, secret
//! invalide, et reprise après un 401/403 sur un token vigie.

mod common;

use std::fs;

use common::{engine, wow_tree};
use gsf_sync::auth::{generate_device_secret, DeviceSecretStore};
use gsf_sync::engine::Connection;
use gsf_sync::headline::Tone;
use gsf_sync::state::TokenKind;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn config_device_id(data_dir: &std::path::Path) -> String {
    let config: serde_json::Value =
        serde_json::from_slice(&fs::read(data_dir.join("config.json")).unwrap()).unwrap();
    config["device_id"].as_str().unwrap().to_owned()
}

/// Contrat étape 1 : l'enregistrement anonyme ne reprend que pour un
/// appareil qui a déjà commencé le flux « vigie » avant cette version
/// (secret déjà présent dans le gestionnaire d'identifiants). Les tests de
/// ce fichier simulent un tel appareil.
fn seed_existing_vigie_secret(t: &common::TestEngine) -> String {
    let secret = generate_device_secret();
    t.device_secret_store.save(&secret).unwrap();
    secret
}

#[tokio::test]
async fn fresh_bridge_never_registers_anonymously() {
    let tree = wow_tree();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/bridge/devices"))
        .respond_with(ResponseTemplate::new(202).set_body_json(json!({"status":"pending"})))
        .mount(&server)
        .await;
    // Un nouveau bridge (aucun secret d'appareil encore stocké) : rien n'est
    // envoyé au site tant que l'utilisateur ne s'est pas connecté.
    let t = engine(&tree, &tree.dir.path().join("data"), &server.uri(), None);
    assert_eq!(t.device_secret_store.load().unwrap(), None);

    t.engine.register_if_needed().await;

    assert_eq!(t.engine.status().connection, Connection::Disconnected);
    assert!(t.device_secret_store.load().unwrap().is_none());
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn pending_then_approved_resumes_normal_sync() {
    let tree = wow_tree();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/bridge/devices"))
        .respond_with(ResponseTemplate::new(202).set_body_json(json!({"status":"pending"})))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/bridge/devices"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "approved", "token": "vigie-tok", "expires_at": null
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/bridge/snapshots"))
        .respond_with(
            ResponseTemplate::new(202).set_body_json(json!({"status":"accepted","upload_id":1})),
        )
        .mount(&server)
        .await;
    let t = engine(&tree, &tree.dir.path().join("data"), &server.uri(), None);
    seed_existing_vigie_secret(&t);

    // Premier appel : en attente, aucune action proposée.
    t.engine.register_if_needed().await;
    let st = t.engine.status();
    assert_eq!(st.connection, Connection::RegistrationPending);
    assert_eq!(
        st.headline.text,
        "En attente de validation par un officier."
    );
    assert_eq!(st.headline.tone, Tone::Warn);
    assert!(st.headline.action.is_none());

    // Deuxième appel : approuvé, token enregistré, vigie connectée.
    t.engine.register_if_needed().await;
    assert_eq!(t.engine.token().as_deref(), Some("vigie-tok"));
    match t.engine.status().connection {
        Connection::Connected { kind, username, .. } => {
            assert_eq!(kind, TokenKind::Vigie);
            assert_eq!(username, "Vigie");
        }
        other => panic!("{other:?}"),
    }

    // Plus aucun appel à /devices tant qu'un token est présent.
    let r = t.engine.sync_all("après validation").await;
    assert_eq!(r.sent, 1, "{r:?}");
    t.engine.register_if_needed().await;
    let reqs = server.received_requests().await.unwrap();
    assert_eq!(
        reqs.iter()
            .filter(|r| r.url.path().ends_with("/devices"))
            .count(),
        2
    );

    // Identité d'appareil : device_id uuid persistant, secret 48 car.
    let body: serde_json::Value = serde_json::from_slice(&reqs[0].body).unwrap();
    let device_id = body["device_id"].as_str().unwrap().to_owned();
    assert_eq!(device_id.len(), 36, "uuid v4");
    assert_eq!(device_id, config_device_id(&t.data_dir));
    let secret = body["secret"].as_str().unwrap().to_owned();
    assert_eq!(secret.len(), 48);
    assert!(secret
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'));
    assert_eq!(
        t.device_secret_store.load().unwrap().as_deref(),
        Some(secret.as_str())
    );
}

#[tokio::test]
async fn refused_device_blocks_everything_until_reactivated() {
    let tree = wow_tree();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/bridge/devices"))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({
            "error": {"code": "device_refused", "message": "Coupé"}
        })))
        .mount(&server)
        .await;
    let t = engine(&tree, &tree.dir.path().join("data"), &server.uri(), None);
    seed_existing_vigie_secret(&t);
    t.engine.register_if_needed().await;
    let st = t.engine.status();
    assert_eq!(st.connection, Connection::RegistrationRefused);
    assert_eq!(
        st.headline.text,
        "Ce PC n'a pas été validé par les officiers."
    );
    assert_eq!(st.headline.tone, Tone::Error);
    assert!(st.headline.action.is_none());
    assert!(t.engine.token().is_none());
}

#[tokio::test]
async fn secret_mismatch_resets_device_identity() {
    let tree = wow_tree();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/bridge/devices"))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({
            "error": {"code": "device_secret_mismatch", "message": "Secret invalide"}
        })))
        .mount(&server)
        .await;
    let t = engine(&tree, &tree.dir.path().join("data"), &server.uri(), None);
    seed_existing_vigie_secret(&t);

    t.engine.register_if_needed().await;
    let first_id = config_device_id(&t.data_dir);
    let first_secret = t.device_secret_store.load().unwrap().unwrap();

    t.engine.register_if_needed().await;
    let second_id = config_device_id(&t.data_dir);
    let second_secret = t.device_secret_store.load().unwrap().unwrap();

    assert_ne!(first_id, second_id, "nouveau device_id");
    assert_ne!(first_secret, second_secret, "nouveau secret");
    assert!(t.engine.token().is_none());
}

#[tokio::test]
async fn revoked_vigie_token_goes_back_to_pending_and_reregisters() {
    let tree = wow_tree();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/bridge/devices"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "approved", "token": "tok1", "expires_at": null
        })))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/bridge/devices"))
        .respond_with(ResponseTemplate::new(202).set_body_json(json!({"status":"pending"})))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/bridge/snapshots"))
        .respond_with(
            ResponseTemplate::new(403)
                .set_body_json(json!({"error":{"code":"bridge_access_revoked","message":"Coupé"}})),
        )
        .mount(&server)
        .await;

    let t = engine(&tree, &tree.dir.path().join("data"), &server.uri(), None);
    seed_existing_vigie_secret(&t);
    t.engine.register_if_needed().await;
    assert_eq!(t.engine.token().as_deref(), Some("tok1"));

    let r = t.engine.sync_all("x").await;
    assert_eq!(r.queued, 1, "{r:?}");
    assert!(t.engine.token().is_none(), "token vigie effacé");
    assert_eq!(
        t.engine.status().connection,
        Connection::RegistrationPending
    );

    // Le prochain cycle d'enregistrement repart de zéro (pending à nouveau).
    t.engine.register_if_needed().await;
    assert_eq!(
        t.engine.status().connection,
        Connection::RegistrationPending
    );
}

/// Requête HTTP brute vers le loopback de connexion (cf. `tests/auth.rs`).
fn browser_get(port: u16, target: &str) {
    use std::io::{Read, Write};
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    write!(
        s,
        "GET {target} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
}

fn query(url: &str, key: &str) -> String {
    url::Url::parse(url)
        .unwrap()
        .query_pairs()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.into_owned())
        .unwrap()
}

#[tokio::test]
async fn officer_login_replaces_a_vigie_token() {
    let tree = wow_tree();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/bridge/devices"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "approved", "token": "vigie-tok", "expires_at": null
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/bridge/token"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "token": "officer-tok", "expires_at": null,
            "user": {"id": 1, "username": "Garou Bel", "roles": ["officer"]}
        })))
        .mount(&server)
        .await;
    let t = engine(&tree, &tree.dir.path().join("data"), &server.uri(), None);
    seed_existing_vigie_secret(&t);

    // Vigie approuvée et connectée.
    t.engine.register_if_needed().await;
    assert_eq!(t.engine.token().as_deref(), Some("vigie-tok"));
    match t.engine.status().connection {
        Connection::Connected { kind, .. } => assert_eq!(kind, TokenKind::Vigie),
        other => panic!("{other:?}"),
    }

    // L'officier se connecte : remplace le token et le genre de connexion.
    let username = t
        .engine
        .login(|url| {
            let port: u16 = query(url, "port").parse().unwrap();
            let state = query(url, "state");
            std::thread::spawn(move || {
                browser_get(port, &format!("/callback?code=x&state={state}"));
            });
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(username, "Garou Bel");
    assert_eq!(t.engine.token().as_deref(), Some("officer-tok"));
    match t.engine.status().connection {
        Connection::Connected { kind, username, .. } => {
            assert_eq!(kind, TokenKind::Officer);
            assert_eq!(username, "Garou Bel");
        }
        other => panic!("{other:?}"),
    }
}
