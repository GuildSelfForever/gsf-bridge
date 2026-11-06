//! Moteur du bridge GSF, sans dépendance Tauri (testable sous Linux).
//!
//! - [`wow`] : découverte de l'installation WoW, fichiers SavedVariables,
//!   détection du jeu lancé ;
//! - [`svread`] : attente de stabilité des fichiers et lecture tolérante
//!   (relecture puis repli `.bak`) ;
//! - [`snapshot`] : construction du corps `POST /api/v1/bridge/snapshots`
//!   et de son `snapshot_hash` ;
//! - [`api`] : client HTTPS du backend (contrat `bridge-api-v1`) ;
//! - [`auth`] : connexion navigateur + loopback, stockage du token ;
//! - [`queue`] : file locale des envois en attente, persistée ;
//! - [`lua`] et [`pushed`] : écriture de l'addon `GuildSelfForever_Data` ;
//! - [`watch`] : surveillance des dossiers `WTF/Account` ;
//! - [`engine`] : orchestration utilisée par l'application Tauri ;
//! - [`headline`] : phrase d'état affichée, en mots simples ;
//! - [`tray`], [`alerts`] et [`prefs`] : icône près de l'horloge,
//!   notifications sur changement d'état, préférences de l'application.

pub mod alerts;
pub mod api;
pub mod auth;
pub mod canonical;
pub mod engine;
pub mod fsutil;
pub mod headline;
pub mod lua;
pub mod prefs;
pub mod pushed;
pub mod queue;
pub mod secretfile;
pub mod snapshot;
pub mod state;
pub mod svread;
pub mod tray;
pub mod watch;
pub mod wow;

/// Version du bridge (version de workspace), envoyée dans `X-Bridge-Version`.
pub const BRIDGE_VERSION: &str = env!("CARGO_PKG_VERSION");
