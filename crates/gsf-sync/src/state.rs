//! Réglages et état persistés dans le dossier de données de l'application.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::api::BridgeUser;

/// Réglages modifiables par l'utilisateur (`config.json`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    /// Adresse du site (dev) ; `None` = production.
    #[serde(default)]
    pub base_url: Option<String>,
    /// Dossier WoW choisi à la main (racine ou dossier de version) ; `None` =
    /// détection automatique (registre).
    #[serde(default)]
    pub wow_dir: Option<PathBuf>,
    /// Identifiant d'appareil (uuid v4, contrat « vigies »), créé une seule
    /// fois au premier lancement et conservé ensuite.
    #[serde(default)]
    pub device_id: Option<String>,
}

/// Genre du token courant : officier (connexion navigateur, confiance
/// totale) ou vigie (appareil validé par un officier, droits restreints
/// côté site). Décide la phrase affichée et la reconnexion après 401/403.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenKind {
    Officer,
    Vigie,
}

/// Résultat du dernier envoi.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendRecord {
    pub at: i64,
    /// `accepted`, `duplicate`, `queued`, `error`.
    pub status: String,
    pub message: String,
}

/// État interne (`state.json`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SyncState {
    /// Dernier `snapshot_hash` accepté par le backend, par compte.
    #[serde(default)]
    pub last_hashes: BTreeMap<String, String>,
    #[serde(default)]
    pub last_send: Option<SendRecord>,
    /// Utilisateur du token courant (affichage uniquement ; le token est
    /// dans le gestionnaire d'identifiants).
    #[serde(default)]
    pub user: Option<BridgeUser>,
    /// Genre du token courant ; `None` tant qu'aucun des deux flux n'a
    /// encore abouti (ou après une déconnexion).
    #[serde(default)]
    pub token_kind: Option<TokenKind>,
    /// `version` des dernières données poussées écrites.
    #[serde(default)]
    pub pushed_version: Option<String>,
    #[serde(default)]
    pub pushed_written_at: Option<i64>,
    /// Clé normalisée (`guild_key_normalized`) de la guilde active du dernier
    /// compte synchronisé (`lastActiveGuildKey`) ; `None` tant qu'aucun
    /// profil actif n'a été vu. Sert de paramètre `guild_key` pour les
    /// données poussées (`refresh_pushed_data`).
    #[serde(default)]
    pub active_guild_key: Option<String>,
    /// `collector` des dernières données poussées reçues (contrat 2026-11-02
    /// b) ; `None` tant qu'aucune n'a été reçue depuis la connexion. Avec
    /// `user.roles`, décide si ce bridge est celui d'un membre simple (il
    /// n'envoie alors que les preuves de triche).
    #[serde(default)]
    pub pushed_collector: Option<bool>,
}

impl SyncState {
    /// Bridge d'un membre simple : jeton obtenu par la connexion (pas une
    /// vigie), sans rôle `officer` ni `admin`, et dernières données poussées
    /// sans `collector = true` (y compris tant qu'aucune n'a été reçue).
    pub fn is_plain_member(&self) -> bool {
        let Some(user) = &self.user else { return false };
        self.token_kind != Some(TokenKind::Vigie)
            && !user.roles.iter().any(|r| r == "officer" || r == "admin")
            && self.pushed_collector != Some(true)
    }
}

/// Emplacements des fichiers de l'application.
#[derive(Debug, Clone)]
pub struct AppPaths {
    pub data_dir: PathBuf,
}

impl AppPaths {
    pub fn new(data_dir: &Path) -> Self {
        AppPaths {
            data_dir: data_dir.to_path_buf(),
        }
    }

    pub fn config(&self) -> PathBuf {
        self.data_dir.join("config.json")
    }

    pub fn state(&self) -> PathBuf {
        self.data_dir.join("state.json")
    }

    pub fn queue(&self) -> PathBuf {
        self.data_dir.join("queue.json")
    }
}
