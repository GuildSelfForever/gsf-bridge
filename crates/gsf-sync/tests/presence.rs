//! Signal de vie (contrat 2026-11-02, bridge pour tous étape 2) : `POST
//! /api/v1/bridge/heartbeat` avec `game_running` et la guilde active, pour
//! tout jeton (officier, vigie, membre) ; rien sans jeton ; un signal tout de
//! suite quand le jeu s'ouvre ou se ferme ; 403 => jeton effacé.

mod common;

use common::{engine, wow_tree};
use gsf_sync::engine::Connection;
use serde_json::json;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn heartbeats(server: &MockServer) -> Vec<serde_json::Value> {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path() == "/api/v1/bridge/heartbeat")
        .map(|r| serde_json::from_slice(&r.body).unwrap())
        .collect()
}

async fn mount_heartbeat(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/api/v1/bridge/heartbeat"))
        .and(header("authorization", "Bearer tok"))
        .respond_with(ResponseTemplate::new(204))
        .mount(server)
        .await;
}

#[tokio::test]
async fn heartbeat_sends_game_state_with_the_token() {
    let tree = wow_tree();
    let server = MockServer::start().await;
    mount_heartbeat(&server).await;
    let t = engine(
        &tree,
        &tree.dir.path().join("data"),
        &server.uri(),
        Some("tok"),
    );
    t.game.set(true);

    t.engine.heartbeat().await;

    let sent = heartbeats(&server).await;
    assert_eq!(sent, [json!({"game_running": true})]);
    let req = &server.received_requests().await.unwrap()[0];
    assert!(req.headers.contains_key("x-bridge-version"));
}

#[tokio::test]
async fn heartbeat_carries_the_active_guild_once_known() {
    let tree = wow_tree();
    let server = MockServer::start().await;
    mount_heartbeat(&server).await;
    Mock::given(method("POST"))
        .and(path("/api/v1/bridge/snapshots"))
        .respond_with(
            ResponseTemplate::new(202).set_body_json(json!({"status":"accepted","upload_id":1})),
        )
        .mount(&server)
        .await;
    let t = engine(
        &tree,
        &tree.dir.path().join("data"),
        &server.uri(),
        Some("tok"),
    );

    t.engine.sync_all("test").await;
    t.engine.heartbeat().await;

    let sent = heartbeats(&server).await;
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0]["game_running"], false);
    let key = sent[0]["guild_key"].as_str().unwrap();
    assert!(key.contains('#'), "{key}");
}

#[tokio::test]
async fn no_heartbeat_without_a_token() {
    let tree = wow_tree();
    let server = MockServer::start().await;
    mount_heartbeat(&server).await;
    let t = engine(&tree, &tree.dir.path().join("data"), &server.uri(), None);

    t.engine.heartbeat().await;
    t.engine.tick().await;

    assert!(heartbeats(&server).await.is_empty());
}

#[tokio::test]
async fn opening_or_closing_the_game_sends_a_heartbeat_at_the_next_tick() {
    let tree = wow_tree();
    let server = MockServer::start().await;
    mount_heartbeat(&server).await;
    let t = engine(
        &tree,
        &tree.dir.path().join("data"),
        &server.uri(),
        Some("tok"),
    );

    t.engine.heartbeat().await;
    // Jeu inchangé : la minuterie d'une minute n'envoie rien de plus.
    t.engine.tick().await;
    assert_eq!(heartbeats(&server).await.len(), 1);

    t.game.set(true);
    t.engine.tick().await;
    t.game.set(false);
    t.engine.tick().await;

    let states: Vec<bool> = heartbeats(&server)
        .await
        .iter()
        .map(|b| b["game_running"].as_bool().unwrap())
        .collect();
    assert_eq!(states, [false, true, false]);
}

#[tokio::test]
async fn revoked_access_clears_the_token() {
    let tree = wow_tree();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/bridge/heartbeat"))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({
            "error": {"code": "bridge_access_revoked", "message": "Ton compte ne peut plus utiliser le bridge (ban ou perso délié)."}
        })))
        .mount(&server)
        .await;
    let t = engine(
        &tree,
        &tree.dir.path().join("data"),
        &server.uri(),
        Some("tok"),
    );

    t.engine.heartbeat().await;

    assert_eq!(t.engine.token(), None);
    assert!(matches!(
        t.engine.status().connection,
        Connection::ReauthRequired { .. }
    ));
}

#[tokio::test]
async fn outdated_bridge_stops_heartbeats() {
    let tree = wow_tree();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/bridge/heartbeat"))
        .respond_with(ResponseTemplate::new(426).set_body_json(json!({
            "error": {"code": "bridge_outdated", "message": "Mise à jour", "details": {"min_version": "9.0.0"}}
        })))
        .mount(&server)
        .await;
    let t = engine(
        &tree,
        &tree.dir.path().join("data"),
        &server.uri(),
        Some("tok"),
    );

    t.engine.heartbeat().await;
    assert_eq!(t.engine.update_required().as_deref(), Some("9.0.0"));
    t.engine.heartbeat().await;

    assert_eq!(heartbeats(&server).await.len(), 1);
}
