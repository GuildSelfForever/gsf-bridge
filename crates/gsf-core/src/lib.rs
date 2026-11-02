//! Logique pure du bridge GSF, testable sans Tauri ni bibliothèques système.
//!
//! - [`sv`] : parseur et modèle de valeurs du sous-ensemble Lua des SavedVariables.
//! - [`model`] : modèle normalisé (guildes fusionnées, alias résolus) sérialisable en JSON.
//!
//! Découverte de WoW, envoi, connexion et données poussées : crate `gsf-sync`.

pub mod model;
pub mod sv;

/// Version du crate, identique à celle de l'application (version de workspace).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
