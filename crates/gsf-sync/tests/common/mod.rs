//! Outils partagés des tests d'intégration.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use gsf_sync::auth::{MemoryDeviceSecretStore, MemoryStore};
use gsf_sync::engine::{Engine, EngineOptions};
use gsf_sync::svread::StabilityConfig;
use gsf_sync::wow::{FixedGameDetector, SV_FILE_NAME};

pub fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../gsf-core/tests/fixtures")
        .join(name)
}

/// Arborescence WoW factice : compte + un personnage au nom non ASCII.
pub struct WowTree {
    pub dir: tempfile::TempDir,
    pub root: PathBuf,
    pub flavor: PathBuf,
    pub account_file: PathBuf,
    pub char_file: PathBuf,
}

pub fn wow_tree() -> WowTree {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("World of Warcraft");
    let flavor = root.join("_classic_beta_");
    let acc = flavor.join("WTF/Account/ACCOUNT#1");
    let account_file = acc.join("SavedVariables").join(SV_FILE_NAME);
    let char_file = acc
        .join("Fumée-Ravine/Fumée/SavedVariables")
        .join(SV_FILE_NAME);
    for (src, dst) in [
        ("account.lua", &account_file),
        ("char_Fumée-Ravine.lua", &char_file),
    ] {
        fs::create_dir_all(dst.parent().unwrap()).unwrap();
        fs::copy(fixture(src), dst).unwrap();
    }
    fs::create_dir_all(flavor.join("Interface/AddOns/GuildSelfForever")).unwrap();
    WowTree {
        dir,
        root,
        flavor,
        account_file,
        char_file,
    }
}

pub struct TestEngine {
    pub engine: Engine,
    pub store: Arc<MemoryStore>,
    pub device_secret_store: Arc<MemoryDeviceSecretStore>,
    pub game: Arc<FixedGameDetector>,
    pub data_dir: PathBuf,
}

pub fn fast_options(data_dir: &Path) -> EngineOptions {
    let mut o = EngineOptions::new(data_dir);
    o.stability = StabilityConfig {
        interval: Duration::from_millis(30),
        max_wait: Duration::from_secs(5),
    };
    o.parse_retry_delay = Duration::from_millis(5);
    o.login_timeout = Duration::from_secs(5);
    o
}

/// Moteur pointant vers `base_url`, avec un token en mémoire si `token`.
pub fn engine(tree: &WowTree, data_dir: &Path, base_url: &str, token: Option<&str>) -> TestEngine {
    let config = serde_json::json!({ "base_url": base_url });
    fs::create_dir_all(data_dir).unwrap();
    fs::write(data_dir.join("config.json"), config.to_string()).unwrap();
    let store = Arc::new(match token {
        Some(t) => MemoryStore::with_token(t),
        None => MemoryStore::default(),
    });
    let device_secret_store = Arc::new(MemoryDeviceSecretStore::default());
    let game = Arc::new(FixedGameDetector::new(false));
    let engine = Engine::new(
        fast_options(data_dir),
        store.clone(),
        device_secret_store.clone(),
        game.clone(),
        Some(tree.root.clone()),
        Arc::new(|| {}),
    );
    TestEngine {
        engine,
        store,
        device_secret_store,
        game,
        data_dir: data_dir.to_path_buf(),
    }
}

pub fn gunzip(bytes: &[u8]) -> Vec<u8> {
    use std::io::Read;
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(bytes)
        .read_to_end(&mut out)
        .unwrap();
    out
}
