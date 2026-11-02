//! Client HTTP contre un backend simulé (wiremock), selon `bridge-api-v1`.

mod common;

use gsf_sync::api::{ApiClient, ApiError, DeviceRegistration};
use gsf_sync::snapshot::{Snapshot, SnapshotSource};
use serde_json::json;
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn snapshot() -> Snapshot {
    Snapshot {
        model_version: 1,
        bridge_version: "0.1.0".into(),
        snapshot_hash: "h".repeat(64),
        source: SnapshotSource {
            account_file: "WTF/Account/A/SavedVariables/GuildSelfForever.lua".into(),
            account_mtime: Some(1800000500),
            character_files: vec![],
            game_running: false,
        },
        guilds: json!([{"guild_key_normalized": "fumée#gsf", "guild_name": "Dävin \"]]"}]),
    }
}

async fn client(server: &MockServer) -> ApiClient {
    ApiClient::new(&server.uri(), "0.1.0").unwrap()
}

#[tokio::test]
async fn snapshot_is_gzipped_with_version_and_bearer() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/bridge/snapshots"))
        .and(header("x-bridge-version", "0.1.0"))
        .and(header("authorization", "Bearer tok"))
        .and(header("content-encoding", "gzip"))
        .and(header("content-type", "application/json"))
        .respond_with(
            ResponseTemplate::new(202).set_body_json(json!({"status":"accepted","upload_id":42})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let r = client(&server)
        .await
        .post_snapshot("tok", &snapshot())
        .await
        .unwrap();
    assert_eq!(r.status, "accepted");
    assert!(!r.is_duplicate());
    let req = &server.received_requests().await.unwrap()[0];
    let body: Snapshot = serde_json::from_slice(&common::gunzip(&req.body)).unwrap();
    assert_eq!(body, snapshot());
}

#[tokio::test]
async fn ignored_snapshot_is_reported() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/bridge/snapshots"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"status":"ignored","upload_id":3,"ignored_guilds":["autre#guilde"]}),
        ))
        .mount(&server)
        .await;
    let r = client(&server)
        .await
        .post_snapshot("tok", &snapshot())
        .await
        .unwrap();
    assert!(r.is_ignored());
    assert_eq!(r.ignored(), ["autre#guilde"]);
}

#[tokio::test]
async fn duplicate_snapshot_is_reported() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/bridge/snapshots"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"status":"duplicate"})))
        .mount(&server)
        .await;
    let r = client(&server)
        .await
        .post_snapshot("tok", &snapshot())
        .await
        .unwrap();
    assert!(r.is_duplicate());
}

#[tokio::test]
async fn forbidden_and_unauthorized_require_reauth() {
    for status in [403u16, 401] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(status).set_body_json(
                    json!({"error":{"code":"forbidden","message":"Rôle insuffisant"}}),
                ),
            )
            .mount(&server)
            .await;
        let err = client(&server)
            .await
            .post_snapshot("tok", &snapshot())
            .await
            .unwrap_err();
        assert_eq!(
            err,
            ApiError::AuthRequired {
                status,
                message: "Rôle insuffisant".into()
            }
        );
    }
}

#[tokio::test]
async fn outdated_bridge_gets_min_version() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/bridge/pushed-data"))
        .respond_with(ResponseTemplate::new(426).set_body_json(json!({
            "error":{"code":"bridge_outdated","message":"Mettez à jour","details":{"min_version":"0.2.0"}}
        })))
        .mount(&server)
        .await;
    let err = client(&server)
        .await
        .pushed_data("tok", None)
        .await
        .unwrap_err();
    assert_eq!(
        err,
        ApiError::Outdated {
            min_version: Some("0.2.0".into())
        }
    );
}

#[tokio::test]
async fn server_errors_and_unreachable_backend_are_retriable() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(503).set_body_string("<html>maintenance</html>"))
        .mount(&server)
        .await;
    let err = client(&server)
        .await
        .post_snapshot("tok", &snapshot())
        .await
        .unwrap_err();
    assert!(matches!(err, ApiError::Http { status: 503, .. }), "{err:?}");
    assert!(err.is_retriable());

    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let dead = ApiClient::new(&format!("http://127.0.0.1:{port}"), "0.1.0").unwrap();
    let err = dead.post_snapshot("tok", &snapshot()).await.unwrap_err();
    assert!(matches!(err, ApiError::Network(_)), "{err:?}");
    assert!(err.is_retriable());

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(422)
                .set_body_json(json!({"error":{"code":"validation","message":"guilds requis"}})),
        )
        .mount(&server)
        .await;
    let err = client(&server)
        .await
        .post_snapshot("tok", &snapshot())
        .await
        .unwrap_err();
    assert_eq!(
        err,
        ApiError::Http {
            status: 422,
            code: "validation".into(),
            message: "guilds requis".into()
        }
    );
    assert!(!err.is_retriable());
}

#[tokio::test]
async fn token_exchange() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/bridge/token"))
        .and(header("x-bridge-version", "0.1.0"))
        .and(body_json(
            json!({"code":"c0de","state":"S","device_name":"PC de X"}),
        ))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "token":"1|plain","expires_at":"2027-01-01T00:00:00Z",
            "user":{"id":7,"username":"Dävin","roles":["officer"]}
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/bridge/token"))
        .and(body_json(
            json!({"code":"old","state":"S","device_name":"PC de X"}),
        ))
        .respond_with(
            ResponseTemplate::new(422)
                .set_body_json(json!({"error":{"code":"invalid_code","message":"Code invalide"}})),
        )
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/bridge/token"))
        .and(body_json(
            json!({"code":"member","state":"S","device_name":"PC de X"}),
        ))
        .respond_with(
            ResponseTemplate::new(403).set_body_json(
                json!({"error":{"code":"forbidden","message":"Réservé aux officiers"}}),
            ),
        )
        .mount(&server)
        .await;
    let c = client(&server).await;
    let g = c.exchange_code("c0de", "S", "PC de X").await.unwrap();
    assert_eq!(g.token, "1|plain");
    assert_eq!(g.user.username, "Dävin");
    assert_eq!(g.user.roles, ["officer"]);
    assert_eq!(
        c.exchange_code("old", "S", "PC de X").await.unwrap_err(),
        ApiError::InvalidCode
    );
    assert_eq!(
        c.exchange_code("member", "S", "PC de X").await.unwrap_err(),
        ApiError::Forbidden("Réservé aux officiers".into())
    );
}

#[tokio::test]
async fn device_registration_statuses() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/bridge/devices"))
        .and(body_json(json!({
            "device_id": "d1", "secret": "s1", "device_name": "PC",
            "characters": ["Ann-Realm"], "bridge_version": "0.5.0"
        })))
        .respond_with(ResponseTemplate::new(202).set_body_json(json!({"status":"pending"})))
        .mount(&server)
        .await;
    let c = client(&server).await;
    let characters = vec!["Ann-Realm".to_owned()];
    assert_eq!(
        c.register_device("d1", "s1", "PC", &characters, "0.5.0")
            .await
            .unwrap(),
        DeviceRegistration::Pending
    );

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/bridge/devices"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "approved", "token": "svc|token", "expires_at": null
        })))
        .mount(&server)
        .await;
    let c = client(&server).await;
    assert_eq!(
        c.register_device("d1", "s1", "PC", &[], "0.5.0")
            .await
            .unwrap(),
        DeviceRegistration::Approved {
            token: "svc|token".into()
        }
    );

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/bridge/devices"))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({
            "error": {"code": "device_refused", "message": "Coupé"}
        })))
        .mount(&server)
        .await;
    let c = client(&server).await;
    assert_eq!(
        c.register_device("d1", "s1", "PC", &[], "0.5.0")
            .await
            .unwrap(),
        DeviceRegistration::Refused
    );

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/bridge/devices"))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({
            "error": {"code": "device_secret_mismatch", "message": "Secret invalide"}
        })))
        .mount(&server)
        .await;
    let c = client(&server).await;
    assert_eq!(
        c.register_device("d1", "s1", "PC", &[], "0.5.0")
            .await
            .unwrap(),
        DeviceRegistration::SecretMismatch
    );
}

#[tokio::test]
async fn pushed_data_is_decoded() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/bridge/pushed-data"))
        .and(header("authorization", "Bearer tok"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "schema":1,"guild_key":"realm#home","generated_at":1800000000,"version":"v1",
            "bans":[{"name":"Cheater","reason":"Or externe"},{"name":"Autre","expiresAt":1900000000}],
            "allied_guilds":[{"key":"realm#secondaire","name":"Secondaire"}]
        })))
        .mount(&server)
        .await;
    let d = client(&server)
        .await
        .pushed_data("tok", None)
        .await
        .unwrap();
    assert_eq!(d.guild_key, "realm#home");
    assert_eq!(d.bans.len(), 2);
    assert_eq!(d.bans[0].name, "Cheater");
    assert_eq!(d.bans[0].reason.as_deref(), Some("Or externe"));
    assert_eq!(d.bans[0].expires_at, None);
    assert_eq!(d.bans[1].expires_at, Some(1900000000));
    assert_eq!(d.allied_guilds.len(), 1);
    assert_eq!(d.allied_guilds[0].key, "realm#secondaire");
    assert_eq!(d.allied_guilds[0].name, "Secondaire");
}

#[tokio::test]
async fn pushed_data_defaults_allied_guilds_to_empty_for_an_older_contract() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/bridge/pushed-data"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "schema":1,"guild_key":"realm#home","bans":[]
        })))
        .mount(&server)
        .await;
    let d = client(&server)
        .await
        .pushed_data("tok", None)
        .await
        .unwrap();
    assert!(d.allied_guilds.is_empty());
}

#[tokio::test]
async fn pushed_data_sends_the_active_guild_key_as_a_query_parameter() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/bridge/pushed-data"))
        .and(wiremock::matchers::query_param(
            "guild_key",
            "classicbetapve#beta self forever",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "schema":1,"guild_key":"classicbetapve#beta self forever","bans":[]
        })))
        .mount(&server)
        .await;
    let d = client(&server)
        .await
        .pushed_data("tok", Some("classicbetapve#beta self forever"))
        .await
        .unwrap();
    assert_eq!(d.guild_key, "classicbetapve#beta self forever");
}

#[tokio::test]
async fn pushed_data_without_a_guild_key_omits_the_query_parameter() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/bridge/pushed-data"))
        .and(wiremock::matchers::query_param_is_missing("guild_key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "schema":1,"guild_key":"realm#home","bans":[]
        })))
        .mount(&server)
        .await;
    client(&server)
        .await
        .pushed_data("tok", None)
        .await
        .unwrap();
}

#[tokio::test]
async fn pushed_data_untracked_guild_is_a_distinct_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/bridge/pushed-data"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({
            "error":{"code":"guild_not_tracked","message":"Guilde non suivie"}
        })))
        .mount(&server)
        .await;
    let err = client(&server)
        .await
        .pushed_data("tok", Some("realm#inconnue"))
        .await
        .unwrap_err();
    assert_eq!(
        err,
        ApiError::Http {
            status: 404,
            code: "guild_not_tracked".into(),
            message: "Guilde non suivie".into(),
        }
    );
}

#[tokio::test]
async fn pushed_data_sends_if_none_match_and_handles_not_modified() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/bridge/pushed-data"))
        .and(header("if-none-match", "\"v7\""))
        .respond_with(ResponseTemplate::new(304).insert_header("etag", "\"v7\""))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/bridge/pushed-data"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "schema":1,"guild_key":"realm#home","version":"v8","bans":[],
            "collector_proofs":[{"name":"vigie","expires_at":5,"sig":"ab"}],
            "received_violations":["v-1"]
        })))
        .mount(&server)
        .await;
    let api = client(&server).await;
    assert_eq!(
        api.pushed_data_if_changed("tok", None, Some("v7"))
            .await
            .unwrap(),
        None
    );
    let d = api
        .pushed_data_if_changed("tok", None, None)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(d.version.as_deref(), Some("v8"));
    assert_eq!(d.collector_proofs.len(), 1);
    assert_eq!(d.received_violations, vec!["v-1".to_owned()]);
}
