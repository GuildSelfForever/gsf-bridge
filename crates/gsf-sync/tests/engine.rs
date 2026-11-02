//! Moteur complet : arborescence WoW factice (fixtures réelles), backend
//! simulé, file d'attente persistée, 403/426, données poussées.

mod common;

use std::fs;

use common::{engine, gunzip, wow_tree};
use gsf_sync::canonical::snapshot_hash;
use gsf_sync::engine::Connection;
use gsf_sync::headline::Tone;
use gsf_sync::pushed::WriteOutcome;
use gsf_sync::snapshot::Snapshot;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn accepted() -> ResponseTemplate {
    ResponseTemplate::new(202).set_body_json(json!({"status":"accepted","upload_id":1}))
}

async fn snapshots_received(server: &MockServer) -> Vec<Snapshot> {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path() == "/api/v1/bridge/snapshots")
        .map(|r| serde_json::from_slice(&gunzip(&r.body)).unwrap())
        .collect()
}

fn dead_url() -> String {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    format!("http://127.0.0.1:{}", l.local_addr().unwrap().port())
}

#[tokio::test]
async fn one_logout_gives_exactly_one_send() {
    let tree = wow_tree();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/bridge/snapshots"))
        .respond_with(accepted())
        .mount(&server)
        .await;
    let t = engine(
        &tree,
        &tree.dir.path().join("data"),
        &server.uri(),
        Some("tok"),
    );

    let r = t.engine.sync_all("démarrage").await;
    assert_eq!((r.sent, r.errors, r.queued), (1, 0, 0), "{r:?}");
    let sent = snapshots_received(&server).await;
    assert_eq!(sent.len(), 1);
    let s = &sent[0];
    assert_eq!(s.model_version, 1);
    assert_eq!(s.snapshot_hash, snapshot_hash(&s.guilds));
    assert!(s.guild_count() >= 1);
    assert_eq!(
        s.source.account_file,
        "WTF/Account/ACCOUNT#1/SavedVariables/GuildSelfForever.lua"
    );
    assert_eq!(s.source.character_files.len(), 1);
    assert_eq!(
        s.source.character_files[0].path,
        "WTF/Account/ACCOUNT#1/Fumée-Ravine/Fumée/SavedVariables/GuildSelfForever.lua"
    );
    assert!(s.source.account_mtime.is_some());
    assert!(!s.source.game_running);

    // WoW réécrit les mêmes données (mtime changé) : rien n'est renvoyé.
    let bytes = fs::read(&tree.account_file).unwrap();
    fs::write(&tree.account_file, bytes).unwrap();
    let r = t.engine.sync_all("modification").await;
    assert_eq!((r.sent, r.unchanged), (0, 1), "{r:?}");
    assert_eq!(snapshots_received(&server).await.len(), 1);

    // Le hash envoyé est persisté : un redémarrage ne renvoie rien non plus.
    drop(t);
    let t = engine(
        &tree,
        &tree.dir.path().join("data"),
        &server.uri(),
        Some("tok"),
    );
    let r = t.engine.sync_all("redémarrage").await;
    assert_eq!(r.unchanged, 1);
    assert_eq!(snapshots_received(&server).await.len(), 1);
    let st = t.engine.status();
    assert_eq!(st.last_send.unwrap().status, "accepted");
    assert_eq!(st.wow.flavors.len(), 1);
    assert_eq!(st.wow.flavors[0].character_files, 1);
    assert!(st.wow.flavors[0].addon_installed);
}

#[tokio::test]
async fn backend_down_then_up_sends_queued_snapshot_by_itself() {
    let tree = wow_tree();
    let data = tree.dir.path().join("data");
    // Backend coupé.
    let t = engine(&tree, &data, &dead_url(), Some("tok"));
    let r = t.engine.sync_all("démarrage").await;
    assert_eq!(r.queued, 1, "{r:?}");
    let st = t.engine.status();
    assert_eq!(st.queue.len, 1);
    assert!(st.site_unreachable);
    assert!(!st.busy);
    assert_eq!(st.headline.tone, Tone::Warn);
    assert!(st.headline.text.starts_with("Le site est injoignable"));
    assert!(data.join("queue.json").is_file());
    // Rien de neuf : pas de doublon dans la file.
    let r = t.engine.sync_all("modification").await;
    assert_eq!(r.unchanged, 1);
    drop(t);

    // Backend relancé (et bridge redémarré) : la tâche périodique envoie seule.
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/bridge/snapshots"))
        .respond_with(accepted())
        .mount(&server)
        .await;
    let t = engine(&tree, &data, &server.uri(), Some("tok"));
    assert_eq!(
        t.engine.status().queue.len,
        1,
        "file relue depuis le disque"
    );
    t.engine.tick().await;
    assert_eq!(snapshots_received(&server).await.len(), 1);
    let st = t.engine.status();
    assert_eq!(st.queue.len, 0);
    assert!(!st.site_unreachable);
    assert_eq!(st.headline.tone, Tone::Ok);
    assert_eq!(
        st.headline.text,
        "Tout fonctionne. Dernière mise à jour envoyée à l'instant."
    );
    let r = t.engine.sync_all("après envoi").await;
    assert_eq!(r.unchanged, 1);
    assert_eq!(snapshots_received(&server).await.len(), 1);
}

#[tokio::test]
async fn transient_server_error_is_retried_by_tick() {
    let tree = wow_tree();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/bridge/snapshots"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/bridge/snapshots"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"status":"duplicate"})))
        .mount(&server)
        .await;
    let t = engine(
        &tree,
        &tree.dir.path().join("data"),
        &server.uri(),
        Some("tok"),
    );
    assert_eq!(t.engine.sync_all("x").await.queued, 1);
    let q = t.engine.status().queue;
    assert!(q.last_error.unwrap().contains("503"));
    t.engine.tick().await;
    assert_eq!(snapshots_received(&server).await.len(), 2);
    let st = t.engine.status();
    assert_eq!(st.queue.len, 0);
    assert_eq!(st.last_send.unwrap().status, "duplicate");
}

#[tokio::test]
async fn forbidden_clears_token_and_requires_login() {
    let tree = wow_tree();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/bridge/snapshots"))
        .respond_with(
            ResponseTemplate::new(403)
                .set_body_json(json!({"error":{"code":"forbidden","message":"Rétrogradé"}})),
        )
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/bridge/snapshots"))
        .respond_with(accepted())
        .mount(&server)
        .await;
    let t = engine(
        &tree,
        &tree.dir.path().join("data"),
        &server.uri(),
        Some("tok"),
    );
    assert!(matches!(
        t.engine.status().connection,
        Connection::Connected { .. }
    ));
    let r = t.engine.sync_all("x").await;
    assert_eq!(r.queued, 1);
    assert_eq!(t.engine.token(), None, "token effacé");
    assert_eq!(
        t.engine.status().connection,
        Connection::ReauthRequired {
            reason: "Rétrogradé".into()
        }
    );
    // Sans token, la file attend.
    t.engine.tick().await;
    assert_eq!(snapshots_received(&server).await.len(), 1);
    // Reconnexion (token re-stocké) : la file part.
    use gsf_sync::auth::TokenStore;
    t.store.save("tok2").unwrap();
    t.engine.tick().await;
    assert_eq!(snapshots_received(&server).await.len(), 2);
    assert_eq!(t.engine.status().queue.len, 0);
}

#[tokio::test]
async fn outdated_bridge_stops_sending() {
    let tree = wow_tree();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(426).set_body_json(json!({
            "error":{"code":"bridge_outdated","message":"Trop vieux","details":{"min_version":"0.2.0"}}
        })))
        .mount(&server)
        .await;
    let t = engine(
        &tree,
        &tree.dir.path().join("data"),
        &server.uri(),
        Some("tok"),
    );
    assert_eq!(t.engine.sync_all("x").await.queued, 1);
    let st = t.engine.status();
    assert_eq!(st.update_required.as_deref(), Some("0.2.0"));
    assert_eq!(st.queue.len, 1);
    t.engine.tick().await;
    assert_eq!(t.engine.refresh_pushed_data().await, None);
    assert_eq!(
        server.received_requests().await.unwrap().len(),
        1,
        "plus aucun appel"
    );
    assert!(t.engine.token().is_some(), "le token est conservé");
}

#[tokio::test]
async fn not_connected_queues_without_calling_backend() {
    let tree = wow_tree();
    let server = MockServer::start().await;
    let t = engine(&tree, &tree.dir.path().join("data"), &server.uri(), None);
    assert_eq!(t.engine.status().connection, Connection::Disconnected);
    assert_eq!(t.engine.sync_all("x").await.queued, 1);
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn permanent_rejection_is_not_queued() {
    let tree = wow_tree();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(422).set_body_json(
            json!({"error":{"code":"validation","message":"model_version inconnue"}}),
        ))
        .mount(&server)
        .await;
    let t = engine(
        &tree,
        &tree.dir.path().join("data"),
        &server.uri(),
        Some("tok"),
    );
    let r = t.engine.sync_all("x").await;
    assert_eq!((r.errors, r.queued), (1, 0));
    let st = t.engine.status();
    assert_eq!(st.queue.len, 0);
    assert_eq!(st.last_send.unwrap().status, "error");
}

#[tokio::test]
async fn truncated_account_file_falls_back_to_bak() {
    let tree = wow_tree();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(accepted())
        .mount(&server)
        .await;
    fs::copy(
        common::fixture("account.lua.bak"),
        tree.account_file.with_extension("lua.bak"),
    )
    .unwrap();
    let full = fs::read(&tree.account_file).unwrap();
    fs::write(&tree.account_file, &full[..full.len() / 2]).unwrap();
    let t = engine(
        &tree,
        &tree.dir.path().join("data"),
        &server.uri(),
        Some("tok"),
    );
    let r = t.engine.sync_all("x").await;
    assert_eq!(r.sent, 1, "{r:?}");
    let log = t.engine.status().log;
    assert!(log.iter().any(|e| e.message.contains(".bak")), "{log:?}");
}

#[tokio::test]
async fn pushed_data_written_only_once_game_is_closed() {
    let tree = wow_tree();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/bridge/pushed-data"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "schema":1,"guild_key":"realm#home","generated_at":1800000000,"version":"v42",
            "bans":[{"name":"Cheater","reason":"Or \"externe\" ]]","expiresAt":1900000000}]
        })))
        .mount(&server)
        .await;
    let t = engine(
        &tree,
        &tree.dir.path().join("data"),
        &server.uri(),
        Some("tok"),
    );
    let data_dir = tree.flavor.join("Interface/AddOns/GuildSelfForever_Data");

    t.game.set(true);
    assert_eq!(
        t.engine.refresh_pushed_data().await,
        Some(WriteOutcome::GameRunning)
    );
    assert!(!data_dir.exists());
    assert!(t.engine.status().pushed.waiting_game_closed);
    t.engine.tick().await;
    assert!(!data_dir.exists(), "toujours rien tant que le jeu tourne");

    t.game.set(false);
    t.engine.tick().await;
    let lua = fs::read(data_dir.join("Data.lua")).unwrap();
    let sv = gsf_core::sv::parse(&lua).unwrap();
    let d = sv.get("GSF_PushedData").unwrap();
    assert_eq!(
        d.get("guildKey").and_then(|v| v.as_str()),
        Some("realm#home")
    );
    let ban = d.get("bans").unwrap().get(1).unwrap();
    assert_eq!(
        ban.get("reason").and_then(|v| v.as_str()),
        Some("Or \"externe\" ]]")
    );
    assert_eq!(ban.get("name").and_then(|v| v.as_str()), Some("Cheater"));
    assert_eq!(
        ban.get("expiresAt").and_then(|v| v.as_i64()),
        Some(1900000000)
    );
    assert_eq!(
        d.get("generatedAt").and_then(|v| v.as_i64()),
        Some(1800000000)
    );
    assert!(data_dir.join("GuildSelfForever_Data.toc").is_file());
    let st = t.engine.status();
    assert_eq!(st.pushed.version.as_deref(), Some("v42"));
    assert!(!st.pushed.waiting_game_closed);
    assert_eq!(
        t.engine.refresh_pushed_data().await,
        Some(WriteOutcome::Unchanged)
    );
}

#[tokio::test]
async fn pushed_data_collector_flag_is_written_into_the_addon() {
    let tree = wow_tree();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/bridge/pushed-data"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "schema":1,"guild_key":"realm#home","generated_at":1800000000,
            "version":"v-collector","collector":true,"bans":[]
        })))
        .mount(&server)
        .await;
    let t = engine(
        &tree,
        &tree.dir.path().join("data"),
        &server.uri(),
        Some("tok"),
    );
    let data_dir = tree.flavor.join("Interface/AddOns/GuildSelfForever_Data");
    assert_eq!(
        t.engine.refresh_pushed_data().await,
        Some(WriteOutcome::Written(vec![data_dir.clone()]))
    );
    let lua = fs::read_to_string(data_dir.join("Data.lua")).unwrap();
    assert!(lua.contains("collector = true"), "{lua}");
}

#[tokio::test]
async fn ignored_snapshot_and_unconfigured_guild_are_reported() {
    let tree = wow_tree();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/bridge/snapshots"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"status":"ignored","upload_id":5,"ignored_guilds":["autre#guilde"]}),
        ))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/bridge/pushed-data"))
        .respond_with(ResponseTemplate::new(409).set_body_json(json!({
            "error":{"code":"guild_not_configured","message":"Aucune guilde suivie"}
        })))
        .mount(&server)
        .await;
    let t = engine(
        &tree,
        &tree.dir.path().join("data"),
        &server.uri(),
        Some("tok"),
    );
    t.engine.sync_all("x").await;
    let st = t.engine.status();
    assert_eq!(st.last_send.unwrap().status, "ignored");
    assert!(st.log.iter().any(|e| e.message.contains("autre#guilde")));
    // Pas de renvoi en boucle du même contenu.
    assert_eq!(t.engine.sync_all("x").await.unchanged, 1);
    assert_eq!(t.engine.refresh_pushed_data().await, None);
    assert!(!tree
        .flavor
        .join("Interface/AddOns/GuildSelfForever_Data")
        .exists());
    assert!(t
        .engine
        .status()
        .log
        .iter()
        .any(|e| e.message.contains("aucune guilde suivie")));
    assert!(t.engine.token().is_some());
}

#[tokio::test]
async fn pushed_data_requests_the_active_guild_and_writes_allied_guilds() {
    let tree = wow_tree();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/bridge/snapshots"))
        .respond_with(accepted())
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/bridge/pushed-data"))
        .and(wiremock::matchers::query_param(
            "guild_key",
            "classicbetapve#mirabel france",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "schema":1,"guild_key":"classicbetapve#mirabel france","generated_at":1800000000,
            "version":"v-fed","bans":[],
            "allied_guilds":[
                {"key":"classicbetapve#mirabel france bank","name":"Mirabel Banque"},
                {"key":"classicbetapve#petite mirabel","name":"Petite Mirabel"}
            ]
        })))
        .mount(&server)
        .await;
    let t = engine(
        &tree,
        &tree.dir.path().join("data"),
        &server.uri(),
        Some("tok"),
    );
    // La guilde active vient du dernier `sync_all` (profil `lastActiveGuildKey`
    // de la fixture réelle), pas d'un réglage manuel.
    t.engine.sync_all("démarrage").await;

    let data_dir = tree.flavor.join("Interface/AddOns/GuildSelfForever_Data");
    assert_eq!(
        t.engine.refresh_pushed_data().await,
        Some(WriteOutcome::Written(vec![data_dir.clone()]))
    );
    let lua = fs::read_to_string(data_dir.join("Data.lua")).unwrap();
    let sv = gsf_core::sv::parse(lua.as_bytes()).unwrap();
    let d = sv.get("GSF_PushedData").unwrap();
    let allies: Vec<_> = d.get("alliedGuilds").unwrap().array().collect();
    assert_eq!(allies.len(), 2);
    assert_eq!(
        allies[0].get("key").and_then(|v| v.as_str()),
        Some("classicbetapve#mirabel france bank")
    );
    assert_eq!(
        allies[0].get("name").and_then(|v| v.as_str()),
        Some("Mirabel Banque")
    );
}

#[tokio::test]
async fn pushed_data_untracked_guild_writes_nothing_and_logs_clearly() {
    let tree = wow_tree();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/bridge/snapshots"))
        .respond_with(accepted())
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/bridge/pushed-data"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({
            "error":{"code":"guild_not_tracked","message":"Guilde non suivie"}
        })))
        .mount(&server)
        .await;
    let t = engine(
        &tree,
        &tree.dir.path().join("data"),
        &server.uri(),
        Some("tok"),
    );
    t.engine.sync_all("démarrage").await;
    assert_eq!(t.engine.refresh_pushed_data().await, None);
    assert!(!tree
        .flavor
        .join("Interface/AddOns/GuildSelfForever_Data")
        .exists());
    let st = t.engine.status();
    assert!(st
        .log
        .iter()
        .any(|e| e.message.contains("mirabel france") && e.message.contains("n'est pas suivie")));
    assert!(t.engine.token().is_some());
}

fn write_member_state(data: &std::path::Path, roles: &[&str]) {
    fs::create_dir_all(data).unwrap();
    fs::write(
        data.join("state.json"),
        json!({"user": {"id": 7, "username": "membre", "roles": roles}, "token_kind": "officer"})
            .to_string(),
    )
    .unwrap();
}

#[tokio::test]
async fn plain_member_sends_only_proofs_until_the_site_says_collector() {
    let tree = wow_tree();
    let data = tree.dir.path().join("data");
    write_member_state(&data, &["member"]);
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/bridge/snapshots"))
        .respond_with(accepted())
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/bridge/pushed-data"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "schema":1,"guild_key":"realm#home","version":"c1","collector":true,"bans":[]
        })))
        .mount(&server)
        .await;
    let t = engine(&tree, &data, &server.uri(), Some("tok"));

    let r = t.engine.sync_all("démarrage").await;
    assert_eq!(r.sent, 1, "{r:?}");
    let sent = snapshots_received(&server).await;
    let g = sent[0].guilds[0].as_object().unwrap();
    assert!(
        g.keys()
            .all(|k| gsf_sync::snapshot::MEMBER_GUILD_KEYS.contains(&k.as_str())),
        "{:?}",
        g.keys()
    );
    assert!(g.contains_key("guild_key_normalized"));
    assert!(!g.contains_key("members"));
    assert_eq!(sent[0].snapshot_hash, snapshot_hash(&sent[0].guilds));

    // Le site le désigne collecteur de confiance : le snapshot complet part.
    t.engine.refresh_pushed_data().await;
    let r = t.engine.sync_all("collecteur").await;
    assert_eq!(r.sent, 1, "{r:?}");
    let sent = snapshots_received(&server).await;
    assert!(sent[1].guilds[0]
        .as_object()
        .unwrap()
        .contains_key("members"));
}

#[tokio::test]
async fn officer_snapshot_is_never_filtered() {
    let tree = wow_tree();
    let data = tree.dir.path().join("data");
    write_member_state(&data, &["officer"]);
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/bridge/snapshots"))
        .respond_with(accepted())
        .mount(&server)
        .await;
    let t = engine(&tree, &data, &server.uri(), Some("tok"));
    t.engine.sync_all("démarrage").await;
    let sent = snapshots_received(&server).await;
    assert!(sent[0].guilds[0]
        .as_object()
        .unwrap()
        .contains_key("members"));
}

#[tokio::test]
async fn pushed_data_asks_only_if_changed_once_the_file_is_written() {
    let tree = wow_tree();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/bridge/pushed-data"))
        .and(wiremock::matchers::header("if-none-match", "\"v9\""))
        .respond_with(ResponseTemplate::new(304))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/bridge/pushed-data"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "schema":1,"guild_key":"realm#home","version":"v9","bans":[]
        })))
        .mount(&server)
        .await;
    let t = engine(
        &tree,
        &tree.dir.path().join("data"),
        &server.uri(),
        Some("tok"),
    );
    let data_dir = tree.flavor.join("Interface/AddOns/GuildSelfForever_Data");
    let if_none_match = |reqs: &[wiremock::Request]| {
        reqs.iter()
            .filter(|r| r.url.path() == "/api/v1/bridge/pushed-data")
            .map(|r| r.headers.get("if-none-match").is_some())
            .collect::<Vec<_>>()
    };

    // Premier appel : rien d'écrit, requête complète.
    assert!(matches!(
        t.engine.refresh_pushed_data().await,
        Some(WriteOutcome::Written(_))
    ));
    // Deuxième : version connue et fichier présent → 304, rien de réécrit.
    assert_eq!(
        t.engine.refresh_pushed_data().await,
        Some(WriteOutcome::Unchanged)
    );
    assert!(!t.engine.status().site_unreachable);
    // Fichier supprimé à la main : requête complète, fichier réécrit.
    fs::remove_file(data_dir.join("Data.lua")).unwrap();
    assert!(matches!(
        t.engine.refresh_pushed_data().await,
        Some(WriteOutcome::Written(_))
    ));
    assert!(data_dir.join("Data.lua").is_file());
    let reqs = server.received_requests().await.unwrap();
    assert_eq!(if_none_match(&reqs), vec![false, true, false]);
}
