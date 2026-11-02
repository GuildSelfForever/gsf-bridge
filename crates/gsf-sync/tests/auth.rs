//! Flux de connexion loopback de bout en bout, avec un faux navigateur et un
//! faux endpoint `POST /api/v1/bridge/token`.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gsf_sync::api::ApiClient;
use gsf_sync::auth::{login, AuthError, MemoryStore, TokenStore};
use serde_json::json;
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Requête HTTP brute vers le loopback, renvoie (statut, corps).
fn browser_get(port: u16, target: &str) -> (u16, String) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    write!(
        s,
        "GET {target} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    let status = out.split_whitespace().nth(1).unwrap().parse().unwrap();
    (status, out)
}

fn query(url: &str, key: &str) -> String {
    Url::parse(url)
        .unwrap()
        .query_pairs()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.into_owned())
        .unwrap()
}

async fn token_server() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/bridge/token"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "token":"42|secret","expires_at":null,
            "user":{"id":1,"username":"Garou Bel","roles":["officer"]}
        })))
        .mount(&server)
        .await;
    server
}

#[tokio::test]
async fn full_login_flow_stores_token() {
    let server = token_server().await;
    let api = ApiClient::new(&server.uri(), "0.1.0").unwrap();
    let store = MemoryStore::default();
    let opened = Arc::new(Mutex::new(String::new()));
    let pages = Arc::new(Mutex::new(Vec::new()));
    let (o, p) = (opened.clone(), pages.clone());
    let user = login(
        &api,
        &store,
        "PC de test",
        Duration::from_secs(10),
        move |url| {
            *o.lock().unwrap() = url.to_owned();
            let port: u16 = query(url, "port").parse().unwrap();
            let state = query(url, "state");
            std::thread::spawn(move || {
                let mut p = p.lock().unwrap();
                // Bruit et tentative CSRF : ignorés, l'attente continue.
                p.push(browser_get(port, "/favicon.ico"));
                p.push(browser_get(port, "/callback?code=evil&state=WRONG"));
                p.push(browser_get(
                    port,
                    &format!("/callback?code=abc123&state={state}"),
                ));
            });
            Ok(())
        },
    )
    .await
    .unwrap();

    assert_eq!(user.username, "Garou Bel");
    assert_eq!(store.load().unwrap().as_deref(), Some("42|secret"));

    let url = opened.lock().unwrap().clone();
    assert!(url.starts_with(&format!("{}/bridge/connect?port=", server.uri())));
    let port: u16 = query(&url, "port").parse().unwrap();
    assert!(port >= 1024);
    let state = query(&url, "state");
    assert_eq!(state.len(), 32);

    // Laisse le faux navigateur lire la dernière réponse.
    for _ in 0..50 {
        if pages.lock().unwrap().len() == 3 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let pages = pages.lock().unwrap().clone();
    assert_eq!(pages[0].0, 404);
    assert_eq!(pages[1].0, 400);
    assert_eq!(pages[2].0, 200);
    assert!(pages[2].1.contains("Vous pouvez fermer cet onglet"));

    let reqs = server.received_requests().await.unwrap();
    assert_eq!(reqs.len(), 1, "le code CSRF n'est jamais échangé");
    let body: serde_json::Value = serde_json::from_slice(&reqs[0].body).unwrap();
    assert_eq!(
        body,
        json!({"code":"abc123","state":state,"device_name":"PC de test"})
    );
}

#[tokio::test]
async fn denied_in_browser() {
    let server = token_server().await;
    let api = ApiClient::new(&server.uri(), "0.1.0").unwrap();
    let store = MemoryStore::default();
    let err = login(&api, &store, "PC", Duration::from_secs(10), |url| {
        let port: u16 = query(url, "port").parse().unwrap();
        let state = query(url, "state");
        std::thread::spawn(move || {
            browser_get(
                port,
                &format!("/callback?error=access_denied&state={state}"),
            )
        });
        Ok(())
    })
    .await
    .unwrap_err();
    assert!(
        matches!(err, AuthError::Denied(ref r) if r == "access_denied"),
        "{err:?}"
    );
    assert_eq!(store.load().unwrap(), None);
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn timeout_and_browser_failure() {
    let api = ApiClient::new("http://127.0.0.1:9", "0.1.0").unwrap();
    let store = MemoryStore::default();
    let err = login(&api, &store, "PC", Duration::from_millis(200), |_| Ok(()))
        .await
        .unwrap_err();
    assert!(matches!(err, AuthError::Timeout), "{err:?}");
    let err = login(&api, &store, "PC", Duration::from_secs(1), |_| {
        Err("pas de navigateur".into())
    })
    .await
    .unwrap_err();
    assert!(matches!(err, AuthError::OpenBrowser(_)), "{err:?}");
}

#[tokio::test]
async fn member_account_gets_no_token() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/bridge/token"))
        .respond_with(
            ResponseTemplate::new(403).set_body_json(
                json!({"error":{"code":"forbidden","message":"Réservé aux officiers"}}),
            ),
        )
        .mount(&server)
        .await;
    let api = ApiClient::new(&server.uri(), "0.1.0").unwrap();
    let store = MemoryStore::default();
    let err = login(&api, &store, "PC", Duration::from_secs(10), |url| {
        let port: u16 = query(url, "port").parse().unwrap();
        let state = query(url, "state");
        std::thread::spawn(move || browser_get(port, &format!("/callback?code=x&state={state}")));
        Ok(())
    })
    .await
    .unwrap_err();
    assert!(
        matches!(err, AuthError::Api(gsf_sync::api::ApiError::Forbidden(_))),
        "{err:?}"
    );
    assert_eq!(store.load().unwrap(), None);
}
