//! Corps de `POST /api/v1/bridge/snapshots` (contrat `bridge-api-v1` §2).

use std::path::{Path, PathBuf};
use std::time::Duration;

use gsf_core::model::{audit_baselines, normalize, AuditBaseline, MODEL_VERSION};
use gsf_core::sv::SavedVariables;
use serde::{Deserialize, Serialize};

use crate::canonical::snapshot_hash;
use crate::svread::read_saved_variables;
use crate::wow::{relative_display, AccountFiles};

/// Fichier source et sa date de modification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceFile {
    pub path: String,
    pub mtime: Option<i64>,
}

/// Bloc `source` du contrat.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotSource {
    pub account_file: String,
    pub account_mtime: Option<i64>,
    pub character_files: Vec<SourceFile>,
    pub game_running: bool,
}

/// Corps complet envoyé au backend. `guilds` est gardé en JSON pour pouvoir
/// être persisté tel quel dans la file d'attente.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub model_version: u32,
    pub bridge_version: String,
    pub snapshot_hash: String,
    pub source: SnapshotSource,
    pub guilds: serde_json::Value,
}

impl Snapshot {
    /// Nombre de guildes du snapshot.
    pub fn guild_count(&self) -> usize {
        self.guilds.as_array().map_or(0, Vec::len)
    }

    /// Clé normalisée (`guild_key_normalized`) du profil actif
    /// (`lastActiveGuildKey` du compte), `None` si aucun profil n'est actif.
    pub fn active_guild_key(&self) -> Option<String> {
        self.guilds.as_array()?.iter().find_map(|g| {
            (g.get("active").and_then(serde_json::Value::as_bool) == Some(true))
                .then(|| g.get("guild_key_normalized").and_then(|k| k.as_str()))
                .flatten()
                .map(str::to_owned)
        })
    }
}

/// Clés de `GuildData` qu'envoie un bridge de membre simple (contrat
/// 2026-11-02 b) : identité de la guilde et preuves de triche seulement
/// (audits, violations, audits portés, morts), jamais la liste de la guilde,
/// les banques, les demandes, les identités ni les liaisons.
pub const MEMBER_GUILD_KEYS: &[&str] = &[
    "guild_key_normalized",
    "guild_name",
    "realm",
    "source_keys",
    "active",
    "writer_version",
    "writer_saved_at",
    "audit_reports",
    "audit_pending",
    "bridge_violations",
    "bridge_local_violations",
    "bridge_carried_audits",
    "deaths",
    // Contrat 2026-11-10 : preuves supplémentaires d'un membre simple. `guild_bank_log` reste exclu
    // (collecteurs seulement, étape 1 du plan).
    "presence_gaps",
    "crafted_sightings",
    "self_identity",
    // Contrat 2026-11-10 « statistiques du serveur » : ses propres relevés (le site filtre aux persos de
    // l'envoyeur). `stat_checks` (comparaisons faites par un officier) reste réservé aux officiers/collecteurs.
    "server_stats",
];

/// Ne garde de chaque guilde que les clés de [`MEMBER_GUILD_KEYS`] (fonction
/// pure, idempotente). Une valeur qui n'est pas une liste d'objets est
/// renvoyée telle quelle.
pub fn member_guilds(guilds: &serde_json::Value) -> serde_json::Value {
    let Some(list) = guilds.as_array() else {
        return guilds.clone();
    };
    serde_json::Value::Array(
        list.iter()
            .map(|g| match g.as_object() {
                Some(obj) => serde_json::Value::Object(
                    obj.iter()
                        .filter(|(k, _)| MEMBER_GUILD_KEYS.contains(&k.as_str()))
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect(),
                ),
                None => g.clone(),
            })
            .collect(),
    )
}

impl Snapshot {
    /// Version « membre simple » du snapshot : guildes filtrées par
    /// [`member_guilds`], `snapshot_hash` recalculé sur ce contenu.
    pub fn for_member(&self) -> Snapshot {
        let guilds = member_guilds(&self.guilds);
        Snapshot {
            snapshot_hash: snapshot_hash(&guilds),
            guilds,
            ..self.clone()
        }
    }
}

/// Snapshot construit pour un compte, avec sa clé et les remarques de lecture.
#[derive(Debug, Clone)]
pub struct BuiltSnapshot {
    /// Identifiant stable du compte (`<version>/WTF/Account/<ACCOUNT>`), clé
    /// du dernier hash envoyé et de la file d'attente.
    pub key: String,
    pub snapshot: Snapshot,
    /// Repli `.bak`, fichiers de personnages ignorés…
    pub warnings: Vec<String>,
    /// Photos d'audit des persos du compte (« Je joue sur plusieurs
    /// ordinateurs », contrat 2026-11-03) : envoyées à part, seulement si la
    /// case est cochée ; jamais dans `snapshot`.
    pub baselines: Vec<AuditBaseline>,
}

#[derive(Debug, thiserror::Error)]
pub enum SnapshotError {
    #[error("aucun fichier compte GuildSelfForever.lua pour {0}")]
    NoAccountFile(String),
    #[error(transparent)]
    Read(#[from] crate::svread::ReadError),
    #[error("sérialisation : {0}")]
    Json(#[from] serde_json::Error),
}

/// Clé stable d'un compte : chemin relatif à la racine WoW.
pub fn account_key(install_root: &Path, account: &AccountFiles) -> String {
    relative_display(install_root, &account.account_dir)
}

/// Lit les fichiers d'un compte (déjà stables), normalise et calcule le hash.
///
/// - fichier compte illisible (même `.bak`) → erreur ;
/// - fichier de personnage illisible → ignoré avec une remarque.
pub fn build_snapshot(
    install_root: &Path,
    flavor_dir: &Path,
    account: &AccountFiles,
    game_running: bool,
    retry_delay: Duration,
    bridge_version: &str,
) -> Result<BuiltSnapshot, SnapshotError> {
    let key = account_key(install_root, account);
    let account_path: &PathBuf = account
        .account_file
        .as_ref()
        .ok_or_else(|| SnapshotError::NoAccountFile(key.clone()))?;
    let mut warnings = Vec::new();

    let acc = read_saved_variables(account_path, retry_delay)?;
    let account_rel = relative_display(flavor_dir, account_path);
    if acc.from_backup {
        warnings.push(format!("{account_rel} : lu depuis la sauvegarde .bak"));
    }

    let mut characters: Vec<(String, SavedVariables)> = Vec::new();
    let mut character_files = Vec::new();
    for c in &account.characters {
        let rel = relative_display(flavor_dir, &c.path);
        match read_saved_variables(&c.path, retry_delay) {
            Ok(r) => {
                if r.from_backup {
                    warnings.push(format!("{rel} : lu depuis la sauvegarde .bak"));
                }
                character_files.push(SourceFile {
                    path: rel.clone(),
                    mtime: r.mtime,
                });
                characters.push((rel, r.sv));
            }
            Err(e) => warnings.push(format!("{rel} ignoré : {e}")),
        }
    }

    let guilds = serde_json::to_value(normalize(&acc.sv, &characters))?;
    let baselines = audit_baselines(&acc.sv, &characters);
    let snapshot = Snapshot {
        model_version: MODEL_VERSION,
        bridge_version: bridge_version.to_owned(),
        snapshot_hash: snapshot_hash(&guilds),
        source: SnapshotSource {
            account_file: account_rel,
            account_mtime: acc.mtime,
            character_files,
            game_running,
        },
        guilds,
    };
    Ok(BuiltSnapshot {
        key,
        snapshot,
        warnings,
        baselines,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot_with_guilds(guilds: serde_json::Value) -> Snapshot {
        Snapshot {
            model_version: MODEL_VERSION,
            bridge_version: "0.1.0".into(),
            snapshot_hash: "x".into(),
            source: SnapshotSource {
                account_file: "ACCOUNT.lua".into(),
                account_mtime: None,
                character_files: Vec::new(),
                game_running: false,
            },
            guilds,
        }
    }

    #[test]
    fn member_snapshot_keeps_only_proofs_and_rehashes() {
        let s = snapshot_with_guilds(serde_json::json!([{
            "guild_key_normalized": "realm#home", "guild_name": "Home", "realm": "Realm",
            "source_keys": ["k"], "active": true, "writer_version": "0.22.9", "writer_saved_at": 1,
            "audit_reports": [], "audit_pending": [{"source": "x"}], "bridge_violations": null,
            "bridge_local_violations": [{"id": "v"}], "bridge_carried_audits": null, "deaths": [],
            "members": [{"x": 1}], "bridge_roster": [], "member_identities": [], "requests": [],
            "bank_snapshots": {}, "bridge_link_outbox": [], "collector_sessions": [],
            "presence_gaps": [], "crafted_sightings": [], "self_identity": [],
            "server_stats": [], "stat_checks": [{"id": "officer-only"}],
            "guild_bank_log": [{"id": "collector-only"}]
        }]));
        let m = s.for_member();
        let g = m.guilds[0].as_object().unwrap();
        assert!(
            !g.contains_key("guild_bank_log"),
            "collecteurs seulement, jamais envoyé par un membre simple"
        );
        assert!(
            g.contains_key("server_stats"),
            "ses propres relevés, le site filtre aux persos de l'envoyeur"
        );
        assert!(
            !g.contains_key("stat_checks"),
            "comparaison faite par un officier, jamais envoyée par un membre simple"
        );
        let mut keys: Vec<&str> = g.keys().map(String::as_str).collect();
        keys.sort_unstable();
        let mut expected = MEMBER_GUILD_KEYS.to_vec();
        expected.sort_unstable();
        assert_eq!(keys, expected);
        assert_eq!(m.snapshot_hash, snapshot_hash(&m.guilds));
        assert_ne!(m.snapshot_hash, snapshot_hash(&s.guilds));
        assert_eq!(m.source, s.source);
        // Idempotent.
        assert_eq!(m.for_member(), m);
        assert_eq!(
            member_guilds(&serde_json::json!(null)),
            serde_json::json!(null)
        );
    }

    #[test]
    fn active_guild_key_finds_the_active_profile() {
        let s = snapshot_with_guilds(serde_json::json!([
            {"guild_key_normalized": "realm#secondaire", "active": false},
            {"guild_key_normalized": "realm#principale", "active": true},
        ]));
        assert_eq!(s.active_guild_key().as_deref(), Some("realm#principale"));
    }

    #[test]
    fn active_guild_key_is_none_without_an_active_profile() {
        let s = snapshot_with_guilds(serde_json::json!([
            {"guild_key_normalized": "realm#secondaire", "active": false},
        ]));
        assert_eq!(s.active_guild_key(), None);
        assert_eq!(
            snapshot_with_guilds(serde_json::json!([])).active_guild_key(),
            None
        );
    }
}
