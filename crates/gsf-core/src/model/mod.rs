//! Modèle normalisé des données GSF, prêt à être sérialisé en JSON pour le
//! backend.
//!
//! Les clés des SavedVariables ne sont pas des identités : [`normalize`]
//! fusionne les profils de guilde dont le royaume ne diffère que par des
//! espaces, rattache les alias « prénom seul » quand ils sont uniques et
//! absorbe les variations de format entre versions de l'addon (tombstones
//! d'audit nombre/table, équipement `before/after` ou `beforeItem/afterItem`,
//! lignes de sauvegarde à 13 ou 15 champs). Tout champ absent ou d'un type
//! inattendu devient `None` ; rien ne panique.

pub mod baseline;
mod names;
mod normalize;
pub mod wire;

use std::collections::BTreeMap;

use serde::Serialize;

pub use baseline::{audit_baselines, AuditBaseline};
pub use names::{
    clean_name, guild_key_from_parts, lower, normalize_guild_key, CharacterRef, Resolver,
    UnresolvedAlias,
};
pub use normalize::normalize;

/// Version du format JSON produit (à incrémenter à chaque rupture).
pub const MODEL_VERSION: u32 = 1;

/// Données d'une guilde, tous profils et fichiers de personnages fusionnés.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GuildData {
    /// Royaume sans espaces en minuscules ASCII, `#`, nom de guilde en minuscules.
    pub guild_key_normalized: String,
    pub guild_name: String,
    pub realm: String,
    /// Clés de profil d'origine fusionnées (compte et sauvegardes perso).
    pub source_keys: Vec<String>,
    /// Profil actif du compte (`lastActiveGuildKey`).
    pub active: bool,
    /// `GuildFoundForeverDB.writerVersion` (addon ≥ 0.17.47).
    pub writer_version: Option<String>,
    /// `GuildFoundForeverDB.writerSavedAt` (addon ≥ 0.17.47).
    pub writer_saved_at: Option<i64>,
    pub members: Vec<MemberPresence>,
    pub adventure_roster: Vec<AdventureMember>,
    /// `bridgeRoster` (addon ≥ 0.17.47), `None` si aucun profil n'en a.
    /// Clé = clé de roster canonique ; entrées de moins de 30 j, ≤ 1000.
    pub bridge_roster: Option<Vec<BridgeRosterEntry>>,
    /// `bridgeRosterAt` : dernier instantané du roster (le plus récent).
    pub bridge_roster_at: Option<i64>,
    pub requests: Vec<Request>,
    /// Lignes de sauvegarde des fichiers de personnages, décodées.
    pub backup_requests: Vec<BackupRequest>,
    pub audit_reports: Vec<AuditReport>,
    /// File d'audits en attente de chaque fichier de personnage.
    pub audit_pending: Vec<PendingAudits>,
    pub audit_tombstones: Vec<AuditTombstone>,
    pub bank_approvals: Vec<BankApproval>,
    pub bank_snapshots: BankSnapshots,
    pub professions: Vec<ProfessionRecord>,
    pub allied_guilds: Vec<AlliedGuild>,
    /// `bridgeLinkOutbox` (addon ≥ 0.17.47) : codes `/gsf link` reçus par
    /// une autorité, clé = code, ≤ 7 j, ≤ 200.
    pub bridge_link_outbox: Option<Vec<LinkOutboxEntry>>,
    /// `bridgeViolations` (addon ≥ 0.17.47) : journal reçu par une autorité,
    /// clé = identifiant, ≤ 30 j, ≤ 500.
    pub bridge_violations: Option<Vec<Violation>>,
    /// `bridgeLocalViolations` (addon ≥ 0.17.47) : journal propre des
    /// personnages du compte, clé = identifiant, ≤ 30 j, ≤ 50.
    pub bridge_local_violations: Option<Vec<Violation>>,
    /// `bridgeCarriedAudits` (addon ≥ 0.18.0, vigies) : audits en attente
    /// portés par un membre non-autorité pour un autre joueur
    /// (store-and-forward, voir `Audit.lua`), pas encore remis à une
    /// autorité. `None` si aucun profil n'en a.
    pub bridge_carried_audits: Option<Vec<CarriedAudit>>,
    /// `requestTombstones[id] = { status, updatedAt }` (addon ≥ 0.20.0) :
    /// décision de fermeture d'une demande conservée après suppression de
    /// `requests`. Fusionné entre profils, le plus récent `updated_at`
    /// gagne par identifiant.
    pub request_tombstones: Vec<RequestTombstone>,
    /// `requestPurgeBefore` (addon ≥ 0.20.0) : repère du bouton « Purger »
    /// d'un officier, le plus récent (max) de tous les profils.
    pub request_purge_before: Option<i64>,
    /// `deathLog[id]` (addon ≥ 0.20.0), dédupliqué par identifiant.
    pub deaths: Vec<Death>,
    /// `memberIdentity[<clé perso>]` (addon ≥ 0.20.x, contrat 2026-10-17) :
    /// empreinte Battle.net/compte d'un membre, reçue en chuchotement par un
    /// officier. Fusionné entre profils comme `memberPresence`, le plus
    /// récent `at` gagne. ≤ 2000 entrées côté addon.
    pub member_identities: Vec<MemberIdentity>,
    /// `bankItemInfo[<itemID>] = { n, q, b, r, l, at }` (addon ≥ 0.22.5, contrat
    /// 2026-10-23) : fiche des objets en banque lue dans le jeu (GetItemInfo),
    /// pour les objets que Wowhead ne connaît pas encore. Fusionné entre
    /// profils par identifiant, le plus récent `at` gagne ; une entrée sans
    /// identifiant ou sans nom est ignorée. ≤ 1000 entrées côté addon.
    pub bank_item_info: Vec<BankItemInfo>,
    /// `collectorSessions["<clé>:<startedAt>"] = { character, name, startedAt, seenAt, endedAt }` (addon ≥ 0.22.8,
    /// contrat 2026-11-02, bridge pour tous étape 2) : sessions de jeu des personnages collecteurs (officiers,
    /// vigies). Fusionné entre profils par (personnage, `started_at`), le plus récent `seen_at` gagne ; une entrée
    /// sans personnage ou sans `startedAt` est ignorée. ≤ 200 entrées, 30 jours côté addon.
    pub collector_sessions: Vec<CollectorSession>,
    /// Décisions de blacklist prises dans l'addon, conservées jusqu'au reçu du site.
    pub blacklist_actions: Vec<BlacklistAction>,
    /// Alias « prénom seul » ambigus, laissés sans rattachement.
    pub unresolved_aliases: Vec<UnresolvedAlias>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MemberPresence {
    pub character: CharacterRef,
    pub name: Option<String>,
    pub ever_seen: Option<bool>,
    pub last_seen: Option<i64>,
    pub last_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AdventureMember {
    pub character: CharacterRef,
    pub id: Option<String>,
    pub name: Option<String>,
    pub realm: Option<String>,
    pub status: Option<String>,
    pub source: Option<String>,
    pub class_file: Option<String>,
    pub enrolled_at: Option<i64>,
    pub updated_at: Option<i64>,
    pub last_guild_seen_at: Option<i64>,
    /// Début du blocage `CHEATER`/`TO_BAN` (ou, pour un signalement d'audit,
    /// début de la période non surveillée `from`) ; `flagged_by`/
    /// `flag_source` = `"audit"` | `"officer"` | `"site-ban"` (addon ≥ 0.20.0).
    pub flagged_at: Option<i64>,
    pub flagged_by: Option<String>,
    pub flag_source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BridgeRosterEntry {
    pub character: CharacterRef,
    pub name: Option<String>,
    pub rank_index: Option<i64>,
    pub rank_name: Option<String>,
    pub class_file: Option<String>,
    pub level: Option<i64>,
    pub seen_at: Option<i64>,
    /// `lastOnlineAt` (addon ≥ 0.17.53) : dernière connexion connue du
    /// serveur, relevée par un officier. La plus récente de tous les relevés.
    pub last_online_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Request {
    pub id: String,
    pub kind: Option<String>,
    pub text: Option<String>,
    pub requester: CharacterRef,
    pub status: Option<String>,
    pub created_at: Option<i64>,
    pub updated_at: Option<i64>,
    pub decided_at: Option<i64>,
    pub decided_by: Option<String>,
    pub taken_by: Option<String>,
    pub taken_at: Option<i64>,
    pub craft_skill_line: Option<i64>,
    pub craft_recipe_id: Option<i64>,
    pub craft_category: Option<String>,
    pub item_id: Option<i64>,
    pub request_number: Option<i64>,
    pub arrival: Option<i64>,
    pub trusted: Option<bool>,
    /// `releasedBy`/`releasedAt` (addon ≥ 0.19.30, `GSF:ReleaseGuildRequest`).
    pub released_by: Option<String>,
    pub released_at: Option<i64>,
    /// Raison d'une fermeture automatique (`status` reste `cancelled`) :
    /// `"expired"` | `"death"` | `"purge"` (addon ≥ 0.20.0).
    pub close_reason: Option<String>,
}

/// `requestTombstones[id]` (addon ≥ 0.20.0) : décision de fermeture d'une
/// demande conservée après suppression de la table `requests`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RequestTombstone {
    pub id: String,
    pub status: Option<String>,
    pub updated_at: Option<i64>,
}

/// Demande décodée d'une ligne `~` d'un fichier de personnage.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BackupRequest {
    #[serde(flatten)]
    pub request: Request,
    /// 13 (≤ 0.17.x sans prise en charge) ou 15 champs.
    pub field_count: usize,
    /// Fichier de personnage d'où vient la version retenue.
    pub source: String,
    pub backup_saved_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ItemDelta {
    pub id: Option<i64>,
    pub delta: Option<i64>,
}

/// Changement d'équipement ; `before`/`after` valent `"itemID:enchantID"`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EquipmentChange {
    pub slot: Option<i64>,
    pub before: Option<String>,
    pub after: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AuditReport {
    pub id: String,
    pub player: CharacterRef,
    pub class_file: Option<String>,
    pub created_at: Option<i64>,
    pub from: Option<i64>,
    pub to: Option<i64>,
    pub gold_before: Option<i64>,
    pub gold_after: Option<i64>,
    pub gold_delta: Option<i64>,
    pub change_count: Option<i64>,
    pub missing_baseline: Option<bool>,
    pub truncated: Option<bool>,
    pub bank_known: Option<bool>,
    pub items: Vec<ItemDelta>,
    pub bank: Vec<ItemDelta>,
    pub equipment: Vec<EquipmentChange>,
    /// File d'attente uniquement : autorités ayant accusé réception.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub acked_by: BTreeMap<String, i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attempts: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_send_at: Option<i64>,
    /// `relayedBy` (addon ≥ 0.17.53) : membre qui a porté l'audit jusqu'à un
    /// officier ; `None` = reçu du joueur audité lui-même. Pour un même
    /// audit, la version directe l'emporte toujours sur une version relayée.
    pub relayed_by: Option<String>,
    /// Rapport « joué sans l'addon » (addon ≥ 0.22.11, contrat 2026-11-03 c) :
    /// temps de jeu attendu d'après l'addon, en secondes. Omis si absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub played_before: Option<i64>,
    /// `/played` du serveur à la connexion. `played_before`/`played_after`
    /// vont ensemble (`played_after > played_before`), sinon tous deux `None`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub played_after: Option<i64>,
    /// Niveau du relevé de référence (1 à 255).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level_before: Option<i64>,
    /// Niveau au moment du contrôle (1 à 255).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level_after: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PendingAudits {
    /// Chemin (indice) du fichier de personnage.
    pub source: String,
    pub character: Option<CharacterRef>,
    pub audits: Vec<AuditReport>,
}

/// Forme d'origine d'une décision de nettoyage d'audit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TombstoneForm {
    /// `auditTombstones[k] = at` (addon ≤ 0.17.40).
    Number,
    /// `auditTombstones[k] = {at, by, adventureID…}` (addon ≥ 0.17.41).
    Table,
    /// Ligne `auditCleanupRows` d'une sauvegarde de personnage.
    BackupRow,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AuditTombstone {
    pub key: String,
    pub player: CharacterRef,
    pub report_id: String,
    pub at: Option<i64>,
    pub by: Option<String>,
    pub adventure_id: Option<String>,
    pub form: TombstoneForm,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BankApproval {
    pub character: CharacterRef,
    pub name: Option<String>,
    pub approved: Option<bool>,
    pub changed_at: Option<i64>,
    pub changed_by: Option<String>,
    /// Présent aussi dans `bankPermanentRegistry`.
    pub permanent: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct BankSnapshots {
    pub personal: Vec<PersonalBank>,
    pub guild_tabs: Vec<GuildBankTab>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BankSlot {
    pub slot: i64,
    pub id: Option<i64>,
    pub count: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PersonalBank {
    pub owner: CharacterRef,
    pub time: Option<i64>,
    pub total_slots: Option<i64>,
    pub slots: Vec<BankSlot>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GuildBankTab {
    pub tab: i64,
    pub name: Option<String>,
    pub time: Option<i64>,
    pub total_slots: Option<i64>,
    pub by: Option<String>,
    pub slots: Vec<BankSlot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Profession {
    pub name: Option<String>,
    pub skill_line: Option<i64>,
    pub rank: Option<i64>,
    pub max_rank: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProfessionRecord {
    pub character: CharacterRef,
    pub name: Option<String>,
    pub origin: Option<String>,
    pub seen: Option<i64>,
    pub list: Vec<Profession>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AlliedGuild {
    pub key: String,
    pub name: Option<String>,
}

/// Code de liaison Discord (`bridgeLinkOutbox[code]`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LinkOutboxEntry {
    /// `GSF-XXXXXX` (ou historique `GFF-XXXXXX`), en majuscules.
    pub code: String,
    /// Personnage émetteur, identifié par `senderKey` (clé de roster
    /// authentifiée par le client WoW), à défaut par `sender`.
    pub sender: CharacterRef,
    /// Nom affiché de l'émetteur (`sender`).
    pub sender_name: Option<String>,
    pub guild_key: Option<String>,
    pub at: Option<i64>,
    /// Le même code a été utilisé par un autre personnage : liaison à refuser.
    pub conflict: bool,
}

/// Violation journalisée (`bridgeViolations[id]` ou `bridgeLocalViolations[id]`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Violation {
    /// `kind-at-hash`, stable entre clients.
    pub id: String,
    /// `auction_house`, `trade_gold`, `trade_item`, `trade_service`,
    /// `mail_auction`, `mail_inbox`, `mail_send`, `recruit_level`.
    pub kind: Option<String>,
    /// Personnage fautif, identifié par `playerKey`, à défaut par `player`.
    pub player: CharacterRef,
    pub player_name: Option<String>,
    /// ≤ 120 octets.
    pub details: Option<String>,
    pub at: Option<i64>,
    /// Journal reçu seulement : qui l'a relayée et quand elle a été stockée.
    pub reporter: Option<String>,
    pub received_at: Option<i64>,
    pub guild_key: Option<String>,
    /// Journal local seulement : relayée à une autorité.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relayed: Option<bool>,
    /// À qui la violation a déjà été remise en jeu (contrat 2026-11-02 b) :
    /// `bridgeViolations[id].handedTo` (liste `{name, at}`, historique reçu
    /// avec un renvoi) ou `bridgeLocalViolations[id].sentTo` (table clé →
    /// epoch). 8 au plus, les plus récentes ; entrée sans nom écartée.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub handed_to: Vec<Handoff>,
}

/// Remise d'une violation à un collecteur ou une autorité (contrat
/// 2026-11-02 b) : `name` = clé du perso qui l'a reçue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Handoff {
    pub name: String,
    pub at: Option<i64>,
}

/// Audit en attente porté par un tiers (`bridgeCarriedAudits[player:id]`) :
/// `payload` est la forme sérialisée opaque de l'addon (`SerializeAudit`),
/// recopiée telle quelle, jamais décodée côté bridge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CarriedAudit {
    /// Nom affiché du joueur audité (pas une clé de roster authentifiée).
    pub player: String,
    /// Identifiant du rapport d'audit porté.
    pub id: String,
    pub payload: String,
    pub carried_at: Option<i64>,
}

/// `memberIdentity[<clé perso>] = { bt, acct, at }` (contrat 2026-10-17) :
/// empreinte d'identité d'un membre. `bt` = sha256 du BattleTag normalisé
/// (32 hex minuscules), `acct` = identifiant de compte WoW (16 hex
/// minuscules), tous deux `None` si absents ou mal formés (l'entrée reste :
/// un champ invalide ne fait jamais tomber les autres).
///
/// `btag` (complément 2026-10-17 b) : BattleTag en clair reçu via `IDB`,
/// `None` si absent ou de forme invalide (rogné, ≤ 64 octets, sans caractère
/// de contrôle, doit contenir `#`). Le bridge ne vérifie pas sa cohérence
/// avec `bt` (fait par l'officier qui écrit les SavedVariables et par le
/// site) ; champ seul écarté si invalide, l'entrée reste.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MemberIdentity {
    pub character: CharacterRef,
    pub bt: Option<String>,
    pub acct: Option<String>,
    pub at: i64,
    pub btag: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BankItemInfo {
    pub id: i64,
    pub name: String,
    /// Qualité (0 gris … 4 épique, 5 légendaire).
    pub quality: Option<i64>,
    /// `bindType` de GetItemInfo : 0 aucun, 1 lié quand ramassé, 2 lié quand
    /// équipé, 3 lié quand utilisé, 4 objet de quête.
    pub bind: Option<i64>,
    pub required_level: Option<i64>,
    pub item_level: Option<i64>,
    pub at: Option<i64>,
}

/// Session de jeu d'un personnage collecteur (contrat 2026-11-02). `ended_at` = `None` si la session est encore
/// ouverte (fichier écrit par un `/reload`) ou si le jeu a planté : le site la termine alors à `seen_at`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CollectorSession {
    pub character: CharacterRef,
    pub name: Option<String>,
    pub started_at: i64,
    pub seen_at: Option<i64>,
    pub ended_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BlacklistAction {
    pub id: String,
    pub operation: String,
    pub target_type: String,
    pub target_key: String,
    pub target_label: String,
    pub reason: Option<String>,
}

/// `deathLog[id]` (addon ≥ 0.20.0) : mort journalisée d'un membre (≤ 500
/// entrées, 90 jours côté addon). `id` = `"<royaume>-<at>"`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Death {
    pub id: String,
    pub name: Option<String>,
    pub realm: Option<String>,
    pub guild_key: Option<String>,
    pub guild_name: Option<String>,
    pub class_file: Option<String>,
    pub race: Option<String>,
    pub level: Option<i64>,
    pub zone: Option<String>,
    /// Nom affiché du tueur, ou type d'environnement (`"Falling"`,
    /// `"Drowning"`, `"Fatigue"`, `"Lava"`, `"Slime"`, `"Fire"`).
    pub killer: Option<String>,
    /// `"npc"` | `"player"` | `"environment"` | `"unknown"`.
    pub killer_kind: Option<String>,
    pub at: Option<i64>,
    /// Personnage qui a enregistré la mort.
    pub reporter: Option<String>,
}
