//! « Je joue sur plusieurs ordinateurs » (contrat 2026-11-03) : case
//! décochée par défaut ; cochée, le bridge envoie les photos d'audit des persos
//! du PC (une fois par changement), demande celles de ses autres PC dans les
//! données poussées et les écrit pour l'addon ; décochée, le site les oublie.

mod common;

use std::fs;

use common::{engine, gunzip, wow_tree};
use gsf_core::sv::parse;
use gsf_sync::auth::TokenStore;
use serde_json::{json, Value};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

async fn requests(server: &MockServer, verb: &str, route: &str) -> Vec<Request> {
    server
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.method.as_str() == verb && r.url.path() == route)
        .collect()
}

async fn mount_site(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/api/v1/bridge/snapshots"))
        .respond_with(
            ResponseTemplate::new(202).set_body_json(json!({"status":"accepted","upload_id":1})),
        )
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/bridge/audit-baselines"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "stored": 1, "kept": 0, "ignored": []
        })))
        .mount(server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/api/v1/bridge/audit-baselines"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"deleted": 1})))
        .mount(server)
        .await;
}

/// Photo d'un autre PC, telle que le site la renvoie.
fn other_pc_photo() -> Value {
    json!({
        "name": "fumée ravine", "character": "Fumée Ravine", "saved_at": 1790800000,
        "checksum": "42", "adventure_id": "A1929936150", "guild_key": "classic beta pve#mirabel france",
        "snapshot": {
            "time": 1790800000, "gold": 900, "bag_slots": 20,
            "items": [[159, 2], [6948, 1]], "equipment": [[4, "154:0"], [16, "36:0"]],
            "bank": {"time": 1790759191, "items": [[242709, 1]]}
        }
    })
}

#[tokio::test]
async fn unticked_by_default_nothing_is_sent_nor_asked() {
    let tree = wow_tree();
    let server = MockServer::start().await;
    mount_site(&server).await;
    Mock::given(method("GET"))
        .and(path("/api/v1/bridge/pushed-data"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "schema":1,"guild_key":"realm#home","version":"v1","bans":[]
        })))
        .mount(&server)
        .await;
    let t = engine(
        &tree,
        &tree.dir.path().join("data"),
        &server.uri(),
        Some("tok"),
    );

    assert!(!t.engine.status().audit_sync);
    t.engine.sync_all("démarrage").await;
    t.engine.refresh_pushed_data().await;
    t.engine.tick().await;

    assert!(requests(&server, "POST", "/api/v1/bridge/audit-baselines")
        .await
        .is_empty());
    let gets = requests(&server, "GET", "/api/v1/bridge/pushed-data").await;
    assert_eq!(
        gets.len(),
        1,
        "case décochée : pas de relecture chaque minute"
    );
    assert!(!gets[0]
        .url
        .query()
        .unwrap_or("")
        .contains("audit_baselines"));
}

#[tokio::test]
async fn ticked_sends_this_pc_photos_once_and_writes_the_other_pc_ones() {
    let tree = wow_tree();
    let server = MockServer::start().await;
    mount_site(&server).await;
    Mock::given(method("GET"))
        .and(path("/api/v1/bridge/pushed-data"))
        .and(query_param("audit_baselines", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "schema":1,"guild_key":"realm#home","version":"v-photos","bans":[],
            "audit_baselines":[other_pc_photo(), {"name": "broken"}]
        })))
        .mount(&server)
        .await;
    let t = engine(
        &tree,
        &tree.dir.path().join("data"),
        &server.uri(),
        Some("tok"),
    );

    t.engine.set_audit_sync(true).unwrap();
    t.engine.audit_sync_changed().await;
    assert!(t.engine.status().audit_sync);

    // Photo du perso de ce PC (fixture réelle), au format du contrat.
    let posts = requests(&server, "POST", "/api/v1/bridge/audit-baselines").await;
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0].headers.get("content-encoding").unwrap(), "gzip");
    let body: Value = serde_json::from_slice(&gunzip(&posts[0].body)).unwrap();
    // Le fichier compte porte aussi les copies de secours d'autres persos du compte.
    let photo = body["baselines"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["name"] == "fumée ravine")
        .unwrap();
    assert_eq!(photo["name"], "fumée ravine");
    assert_eq!(photo["checksum"], "1830179085");
    assert_eq!(photo["adventure_id"], "A1929936150");
    assert_eq!(photo["guild_key"], "classic beta pve#mirabel france");
    assert_eq!(photo["saved_at"], 1790761960);
    assert_eq!(photo["snapshot"]["items"][0], json!([159, 2]));
    assert_eq!(photo["snapshot"]["bank"]["items"], json!([[242709, 1]]));

    // Rien de neuf : pas de renvoi.
    t.engine.sync_all("modification").await;
    assert_eq!(
        requests(&server, "POST", "/api/v1/bridge/audit-baselines")
            .await
            .len(),
        1
    );

    // Photos des autres PC écrites pour l'addon (l'entrée invalide écartée seule).
    let lua = fs::read_to_string(
        tree.flavor
            .join("Interface/AddOns/GuildSelfForever_Data/Data.lua"),
    )
    .unwrap();
    let sv = parse(lua.as_bytes()).unwrap();
    let list = sv
        .get("GSF_PushedData")
        .unwrap()
        .get("auditBaselines")
        .unwrap();
    let first = list.get(1i64).unwrap();
    assert!(list.get(2i64).is_none(), "{lua}");
    assert_eq!(first.get("name").unwrap().as_str(), Some("fumée ravine"));
    assert_eq!(
        first.get("adventureID").unwrap().as_str(),
        Some("A1929936150")
    );
    assert_eq!(first.get("savedAt").unwrap().as_i64(), Some(1790800000));
    let snap = first.get("snapshot").unwrap();
    assert_eq!(
        snap.get("items").unwrap().get(159i64).unwrap().as_i64(),
        Some(2)
    );
    assert_eq!(
        snap.get("equipment").unwrap().get(16i64).unwrap().as_str(),
        Some("36:0")
    );
    assert_eq!(
        snap.get("bank")
            .unwrap()
            .get("items")
            .unwrap()
            .get(242709i64)
            .unwrap()
            .as_i64(),
        Some(1)
    );

    // Case cochée, jeu fermé : relecture chaque minute ; jeu ouvert : non.
    let before = requests(&server, "GET", "/api/v1/bridge/pushed-data")
        .await
        .len();
    t.engine.tick().await;
    assert_eq!(
        requests(&server, "GET", "/api/v1/bridge/pushed-data")
            .await
            .len(),
        before + 1
    );
    t.game.set(true);
    t.engine.tick().await;
    assert_eq!(
        requests(&server, "GET", "/api/v1/bridge/pushed-data")
            .await
            .len(),
        before + 1
    );
}

#[tokio::test]
async fn unticking_asks_the_site_to_forget_and_stops_asking() {
    let tree = wow_tree();
    let server = MockServer::start().await;
    mount_site(&server).await;
    Mock::given(method("GET"))
        .and(path("/api/v1/bridge/pushed-data"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "schema":1,"guild_key":"realm#home","version":"v1","bans":[]
        })))
        .mount(&server)
        .await;
    let data = tree.dir.path().join("data");
    let t = engine(&tree, &data, &server.uri(), Some("tok"));
    t.engine.set_audit_sync(true).unwrap();
    t.engine.audit_sync_changed().await;

    t.engine.set_audit_sync(false).unwrap();
    t.engine.audit_sync_changed().await;
    assert_eq!(
        requests(&server, "DELETE", "/api/v1/bridge/audit-baselines")
            .await
            .len(),
        1
    );
    let gets = requests(&server, "GET", "/api/v1/bridge/pushed-data").await;
    assert!(!gets
        .last()
        .unwrap()
        .url
        .query()
        .unwrap_or("")
        .contains("audit_baselines"));
    let state: Value =
        serde_json::from_str(&fs::read_to_string(data.join("state.json")).unwrap()).unwrap();
    assert_eq!(state["audit_forget_pending"], false);
    let config: Value =
        serde_json::from_str(&fs::read_to_string(data.join("config.json")).unwrap()).unwrap();
    assert_eq!(config["audit_sync"], false);

    // Recocher renvoie tout (le site a oublié).
    t.engine.set_audit_sync(true).unwrap();
    t.engine.audit_sync_changed().await;
    assert_eq!(
        requests(&server, "POST", "/api/v1/bridge/audit-baselines")
            .await
            .len(),
        2
    );
}

#[tokio::test]
async fn photos_wait_for_the_site_and_the_forget_request_is_retried() {
    let tree = wow_tree();
    let data = tree.dir.path().join("data");
    let dead = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        format!("http://127.0.0.1:{}", l.local_addr().unwrap().port())
    };
    let t = engine(&tree, &data, &dead, Some("tok"));
    t.engine.set_audit_sync(true).unwrap();
    t.engine.sync_all("démarrage").await;
    t.engine.set_audit_sync(false).unwrap();
    t.engine.audit_sync_changed().await;
    let state: Value =
        serde_json::from_str(&fs::read_to_string(data.join("state.json")).unwrap()).unwrap();
    assert_eq!(
        state["audit_forget_pending"], true,
        "redemandé tant que le site ne répond pas"
    );
    drop(t);

    // Site revenu (bridge relancé) : la tâche d'une minute fait oublier.
    let server = MockServer::start().await;
    mount_site(&server).await;
    let t = engine(&tree, &data, &server.uri(), Some("tok"));
    // `engine()` réécrit config.json : la case reste décochée, l'oubli reste dû.
    t.engine.tick().await;
    assert_eq!(
        requests(&server, "DELETE", "/api/v1/bridge/audit-baselines")
            .await
            .len(),
        1
    );
    assert!(requests(&server, "POST", "/api/v1/bridge/audit-baselines")
        .await
        .is_empty());

    // Photos en attente quand le site ne répond pas, envoyées à la minute suivante.
    let t2 = engine(&tree, &tree.dir.path().join("data2"), &dead, Some("tok"));
    t2.engine.set_audit_sync(true).unwrap();
    t2.engine.sync_all("démarrage").await;
    t2.engine.set_base_url(Some(server.uri())).unwrap();
    t2.store.save("tok").unwrap();
    t2.engine.tick().await;
    assert_eq!(
        requests(&server, "POST", "/api/v1/bridge/audit-baselines")
            .await
            .len(),
        1
    );
    drop(t);
}
