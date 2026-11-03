//! Données poussées vers le jeu (contrat §3) : addon séparé
//! `Interface/AddOns/GuildSelfForever_Data/` (`.toc` + `Data.lua` qui définit
//! `GSF_PushedData`), écrit **uniquement jeu fermé**, atomiquement.
//!
//! Le format suit `GSF:LoadPushedData` (`BridgeExport.lua` de l'addon,
//! README « Exports pour le bridge officier ») :
//!
//! ```lua
//! GSF_PushedData = {
//!     schema = 1,
//!     guildKey = "royaume#guilde",
//!     generatedAt = 1700000000,
//!     collector = true, -- seulement pour un bridge collecteur (officier ou vigie)
//!     bans = { { name = "Nom", reason = "texte", expiresAt = 1800000000 }, ... },
//!     alliedGuilds = { { key = "royaume#autre-guilde", name = "Nom en jeu" }, ... },
//!     federation = {
//!         channel = "gsf0a1b2c3d4e",
//!         password = "0123456789abcdef",
//!         guilds = { { key = "royaume#guilde", name = "Nom en jeu", type = "principale", mode = "classicbetapve" }, ... },
//!     },
//!     requests = { expiryDays = 7 }, -- réglage du site (contrat 2026-10-15), absent si non réglé
//!     cleared = { { name = "Nom", at = 1700000000 }, ... }, -- « Ce n'est rien » (contrat 2026-10-15)
//!     bannedIdentities = { { bt = "32 hex", acct = "16 hex", reason = "texte", expiresAt = 1800000000 }, ... }, -- ban de la personne (contrat 2026-10-17)
//!     collectorProofs = { { name = "perso", expiresAt = 1800000000, sig = "512 hex" }, ... }, -- collecteur prouvé (contrat 2026-11-02 b)
//!     receivedViolations = { "trade_gold-1700000000-123", ... }, -- violations reçues par le site (contrat 2026-11-02 b)
//!     auditBaselines = { { name = "perso", character = "Perso", savedAt = 1700000000, checksum = "123",
//!                          adventureID = "A1", guildKey = "royaume#guilde",
//!                          snapshot = { time = …, gold = …, bagSlots = …, items = { [id] = n }, equipment = { [slot] = "id:enchant" },
//!                                       bank = { time = …, items = { [id] = n } },
//!                                       played = …, playedObserved = …, level = … } }, ... }, -- /played : contrat 2026-11-03 c
//!                      -- photos d'audit de ses autres PC (contrat 2026-11-03), seulement si la case est cochée
//!     tombola = { -- tombola du streamer (contrat 2026-10-16), absente sans tombola
//!         id = 12,
//!         announce = { text = "Tombola en cours…", endsAt = 1800000000 }, -- absente hors inscriptions
//!         toSend = { { lotId = 34, kind = "item", label = "Cape", itemId = 2059, count = 1,
//!                      location = "Guilde · onglet 2", winner = "Nom", drawnAt = 1800000000 }, ... },
//!     },
//!     tombolaPublic = { -- onglet Tombola ouvert à tous (contrat 2026-11-03 b), absent sans rien à dire
//!         id = 12, status = "open", streamer = "Celetik", twitch = "celetik", drawAt = 1800000000,
//!         lots = { { lotId = 34, kind = "item", label = "Cape", itemId = 2059, count = 1, minLevel = 25,
//!                    drawn = true, entered = true, won = true }, ... }, -- booléens écrits seulement s'ils sont vrais
//!         won = { { lotId = 34, kind = "item", label = "Cape", itemId = 2059, count = 1, character = "Perso",
//!                   drawnAt = 1800000100, sent = true }, ... }, -- gains du compte de ce bridge seulement
//!     },
//!     returns = { -- retours d'inactifs (contrat 2026-11-04 b), collecteurs seulement, absent sur un ancien site
//!         allowed = { { name = "Nom", bt = "32 hex", acct = "16 hex", reason = "inactivity", since = 1800000000 }, ... },
//!         validated = { { name = "Nom", at = 1800100000 }, ... },
//!     },
//! }
//! ```
//!
//! L'addon rejette **tout** le fichier au moindre champ invalide : on filtre
//! donc ici chaque ban selon les mêmes règles (nom ≤ 64 octets sans
//! caractère de contrôle ni `|`, raison ≤ 200 octets, 500 bans maximum), et
//! chaque guilde alliée de la même façon (clé et nom bornés, sans caractère
//! de contrôle, `MAX_ALLIED_GUILDS` au maximum). `federation` (canal de
//! discussion inter-guildes, optionnel, absent sur un ancien site) est
//! entièrement écarté si `channel` ou `password` contient autre chose que
//! `[a-z0-9]` ; chaque guilde de la liste suit les mêmes règles que les
//! guildes alliées (clé, nom), plus `type`/`mode` bornés à 32 octets sans
//! `|`, `MAX_FEDERATION_GUILDS` au maximum. `requests` (contrat 2026-10-15,
//! réglage d'expiration des demandes) est écarté si `expiry_days` est hors
//! de `[1, 60]` ; `cleared` (joueurs blanchis) suit les mêmes règles de nom
//! que les bans, `MAX_CLEARED` au maximum. `tombola` (contrat 2026-10-16) :
//! l'annonce est écartée si son texte dépasse [`MAX_TOMBOLA_ANNOUNCE_BYTES`]
//! (message du jour de l'addon) ou contient `|`/un caractère de contrôle ;
//! chaque lot à envoyer suit les règles de nom des bans pour le gagnant,
//! libellé et emplacement bornés, `MAX_TOMBOLA_PRIZES` au maximum ; la clé
//! est omise s'il ne reste rien. `bannedIdentities` (contrat 2026-10-17, ban
//! « personne ») : liste toujours présente, même vide ; chaque entrée garde
//! `bt`/`acct` seulement s'ils sont exactement 32/16 caractères hexadécimaux
//! minuscules, est écartée si aucun des deux n'est valide, `reason` suit les
//! mêmes règles que pour `bans`, `expiresAt` doit être positif ou nul,
//! `MAX_BANNED_IDENTITIES` au maximum. `collectorProofs` (contrat 2026-11-02 b,
//! collecteur prouvé) : liste toujours présente, même vide ; une preuve est
//! gardée si `name` fait 1 à 64 octets sans caractère de contrôle, `|` ni `~`,
//! `expires_at` > 0 et `sig` exactement 512 caractères hexadécimaux
//! minuscules, [`MAX_COLLECTOR_PROOFS`] au maximum. `receivedViolations`
//! (accusé de réception du site) : liste toujours présente, même vide, de
//! chaînes de 1 à 128 octets faites de `[A-Za-z0-9_.:-]`,
//! [`MAX_RECEIVED_VIOLATIONS`] au maximum ; une entrée invalide est écartée
//! seule. `tombolaPublic` (contrat 2026-11-03 b, onglet Tombola ouvert à
//! tous) : chaque lot est écarté seul s'il est invalide (mêmes règles que les
//! lots à envoyer, détail ≤ [`MAX_TOMBOLA_DETAILS_BYTES`], perso gagnant non
//! vide ≤ 64 octets sans `|` ni caractère de contrôle, sinon omis),
//! [`MAX_TOMBOLA_PUBLIC_LOTS`] lots et
//! [`MAX_TOMBOLA_PUBLIC_WON`] gains au plus ; `twitch` gardé seulement s'il fait
//! 1 à 25 caractères `[a-z0-9_]`, `status` seulement `open`/`drawing` ; la clé
//! est omise s'il ne reste ni tombola en cours ni lot. `returns` (contrat
//! 2026-11-04 b, retours d'inactifs) : écrite seulement si le site l'envoie ;
//! chaque entrée est validée seule (nom : règles des bans ; `bt`/`acct` gardés
//! seulement s'ils font exactement 32/16 caractères hexadécimaux minuscules,
//! entrée `allowed` écartée s'il ne reste aucun des deux ; `reason` gardée
//! seulement si `inactivity`/`absence` ; `since` négatif omis ; entrée
//! `validated` écartée si `at` est négatif), [`MAX_RETURNS`] au plus par liste.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use gsf_core::model::baseline::MAX_BASELINES;
use gsf_core::model::AuditBaseline;
use serde::{Deserialize, Serialize};

use crate::fsutil::atomic_write;
use crate::lua::{assignment, LuaKey, LuaValue};
use crate::wow::GameDetector;

/// Dossier de l'addon de données.
pub const DATA_ADDON_DIR: &str = "GuildSelfForever_Data";
/// Interface du client (identique à l'addon principal).
pub const TOC_INTERFACE: &str = "16001";
pub const SCHEMA: u32 = 1;
pub const MAX_BANS: usize = 500;
pub const MAX_NAME_BYTES: usize = 64;
pub const MAX_REASON_BYTES: usize = 200;
pub const MAX_GUILD_KEY_BYTES: usize = 160;
/// Guildes de la fédération suivies par le site : une poignée en pratique,
/// large marge de sécurité contre un fichier anormalement gros.
pub const MAX_ALLIED_GUILDS: usize = 200;
/// Guildes listées dans `federation.guilds` : même ordre de grandeur que
/// [`MAX_ALLIED_GUILDS`], liste distincte (comprend la guilde demandeuse).
pub const MAX_FEDERATION_GUILDS: usize = 200;
/// `type`/`mode` d'une guilde de fédération.
pub const MAX_FEDERATION_FIELD_BYTES: usize = 32;
/// Bornes de `requests.expiry_days` (contrat 2026-10-15, défaut 7 côté site).
pub const MIN_EXPIRY_DAYS: i64 = 1;
pub const MAX_EXPIRY_DAYS: i64 = 60;
/// Joueurs blanchis (`cleared`) : défensif, large marge (le site ne garde
/// que les 30 derniers jours).
pub const MAX_CLEARED: usize = 500;
/// Annonce de la tombola : longueur maximale du message du jour de l'addon
/// (`MOTD_MAX`, mesurée en octets par `#text` en Lua).
pub const MAX_TOMBOLA_ANNOUNCE_BYTES: usize = 140;
/// Lots gagnés à envoyer : le site en sert 50 au plus.
pub const MAX_TOMBOLA_PRIZES: usize = 50;
pub const MAX_TOMBOLA_LABEL_BYTES: usize = 160;
pub const MAX_TOMBOLA_LOCATION_BYTES: usize = 200;
/// Onglet Tombola ouvert à tous (contrat 2026-11-03 b) : lots de la tombola
/// en cours, gains du compte, détail d'un lot libre en octets.
pub const MAX_TOMBOLA_PUBLIC_LOTS: usize = 50;
pub const MAX_TOMBOLA_PUBLIC_WON: usize = 20;
pub const MAX_TOMBOLA_DETAILS_BYTES: usize = 200;
/// Bans par identité Battle.net/compte (contrat 2026-10-17) : défensif,
/// large marge au-delà de ce que le site sert en pratique.
pub const MAX_BANNED_IDENTITIES: usize = 500;
/// Preuves de collecteur (contrat 2026-11-02 b) : une par perso, 20 au plus.
pub const MAX_COLLECTOR_PROOFS: usize = 20;
/// Nom d'une preuve de collecteur, en octets.
pub const MAX_PROOF_NAME_BYTES: usize = 64;
/// Signature RSA 2048 bits en hexadécimal.
pub const PROOF_SIG_HEX_LEN: usize = 512;
/// Identifiants de violations reçues par le site (contrat 2026-11-02 b).
pub const MAX_RECEIVED_VIOLATIONS: usize = 2000;
/// Identifiant de violation, en octets.
pub const MAX_VIOLATION_ID_BYTES: usize = 128;
/// Retours d'inactifs (contrat 2026-11-04 b) : au plus par liste
/// (`allowed`, `validated`), comme le site.
pub const MAX_RETURNS: usize = 500;

/// Ban tel que servi par le backend (contrat, décision 12) :
/// `{"name","reason"?,"expiresAt"?}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ban {
    pub name: String,
    #[serde(default)]
    pub reason: Option<String>,
    /// Fin du ban (epoch), absent = définitif.
    #[serde(default, rename = "expiresAt")]
    pub expires_at: Option<i64>,
}

/// Guilde alliée (autre guilde suivie par le site, contrat §3) : noms tels
/// qu'affichés en jeu, servent de liste blanche d'échanges côté addon.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlliedGuild {
    pub key: String,
    pub name: String,
}

/// Guilde listée dans le canal de fédération (`federation.guilds`) : toutes
/// les guildes de la fédération suivies par le site, la guilde demandeuse
/// comprise.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FederationGuild {
    pub key: String,
    pub name: String,
    /// `principale` / `secondaire` / `alt` / `banque`.
    #[serde(rename = "type")]
    pub kind: String,
    pub mode: String,
}

/// Canal de discussion inter-guildes (fédération) : nom et mot de passe du
/// canal WoW personnalisé, plus la liste des guildes qui peuvent y
/// participer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Federation {
    pub channel: String,
    pub password: String,
    #[serde(default)]
    pub guilds: Vec<FederationGuild>,
}

/// Réglage du site pour l'expiration automatique des demandes (contrat
/// 2026-10-15, `requestExpiryDays` côté addon) : `expiry_days` entre
/// [`MIN_EXPIRY_DAYS`] et [`MAX_EXPIRY_DAYS`], défaut 7 appliqué côté site
/// si absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestSettings {
    #[serde(default)]
    pub expiry_days: Option<i64>,
}

/// Ban par identité Battle.net/compte (contrat 2026-10-17, ban « personne ») :
/// en plus du ban de personnage (`bans`), ban en cascade de tous les
/// personnages connus d'une personne via son empreinte BattleTag et/ou
/// compte WoW. Au moins un des deux champs doit être valide, sinon l'entrée
/// est écartée (jamais toute la réponse, voir `to_addon_value`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BannedIdentity {
    #[serde(default)]
    pub bt: Option<String>,
    #[serde(default)]
    pub acct: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    /// Fin du ban (epoch), absent = définitif.
    #[serde(default, rename = "expiresAt")]
    pub expires_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameBlacklistEntry {
    #[serde(rename = "targetType")]
    pub target_type: String,
    #[serde(rename = "targetKey")]
    pub target_key: String,
    #[serde(rename = "targetLabel")]
    pub target_label: String,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlacklistReceipt {
    pub id: String,
    pub status: String,
    #[serde(default)]
    pub message: Option<String>,
}

/// Preuve signée par le site qu'un perso est collecteur (contrat 2026-11-02
/// b) : l'addon la montre à ses camarades, qui vérifient la signature avec la
/// clé publique du site.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollectorProof {
    pub name: String,
    pub expires_at: i64,
    pub sig: String,
}

/// Joueur dont un signalement a été classé « Ce n'est rien » par le site
/// (contrat 2026-10-15) : un officier qui le lit repasse le statut
/// `CHEATER` à `ENROLLED` si `flaggedAt <= at`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cleared {
    pub name: String,
    pub at: i64,
}

/// Perso qui peut revenir par « gsf » sans la règle du niveau 1 (contrat
/// 2026-11-04 b) : exclu pour inactivité ou parti pendant une absence
/// déclarée, ni banni ni signalé. `bt`/`acct` : empreinte d'identité connue
/// du site (au moins une des deux pour être gardé).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReturnAllowed {
    pub name: String,
    #[serde(default)]
    pub bt: Option<String>,
    #[serde(default)]
    pub acct: Option<String>,
    /// `inactivity` ou `absence` (autre valeur omise).
    #[serde(default)]
    pub reason: Option<String>,
    /// Heure du départ (epoch).
    #[serde(default)]
    pub since: Option<i64>,
}

/// Retour validé par un officier (contrat 2026-11-04 b) : l'addon repasse
/// « Retour à vérifier » à inscrit si `flaggedAt <= at`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReturnValidated {
    pub name: String,
    pub at: i64,
}

/// Retours d'inactifs (contrat 2026-11-04 b), servis aux collecteurs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Returns {
    #[serde(default)]
    pub allowed: Vec<ReturnAllowed>,
    #[serde(default)]
    pub validated: Vec<ReturnValidated>,
}

/// `returns` tolérant : un objet est gardé avec ses seules entrées bien
/// typées (les autres écartées une par une) ; toute autre forme (nombre,
/// chaîne, liste…) donne `None`, comme un ancien site. `null` donne `None`.
fn lenient_returns<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Returns>, D::Error> {
    let raw = Option::<serde_json::Value>::deserialize(d)?;
    let Some(serde_json::Value::Object(map)) = raw else {
        return Ok(None);
    };
    fn entries<T: serde::de::DeserializeOwned>(v: Option<&serde_json::Value>) -> Vec<T> {
        v.and_then(serde_json::Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(|e| serde_json::from_value(e.clone()).ok())
                    .collect()
            })
            .unwrap_or_default()
    }
    Ok(Some(Returns {
        allowed: entries(map.get("allowed")),
        validated: entries(map.get("validated")),
    }))
}

/// Tombola du streamer (contrat 2026-10-16).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tombola {
    #[serde(default)]
    pub id: Option<i64>,
    /// Annonce à diffuser en guilde pendant les inscriptions.
    #[serde(default)]
    pub announce: Option<TombolaAnnounce>,
    /// Lots gagnés pas encore envoyés en jeu.
    #[serde(default)]
    pub to_send: Vec<TombolaPrize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TombolaAnnounce {
    pub text: String,
    pub ends_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TombolaPrize {
    pub lot_id: i64,
    #[serde(default)]
    pub kind: Option<String>,
    pub label: String,
    #[serde(default)]
    pub item_id: Option<i64>,
    #[serde(default = "one")]
    pub count: i64,
    #[serde(default)]
    pub location: Option<String>,
    pub winner: String,
    #[serde(default)]
    pub drawn_at: Option<i64>,
}

fn one() -> i64 {
    1
}

/// `null` → chaîne vide (le lot est ensuite écarté par la validation).
fn null_as_empty<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    Ok(Option::<String>::deserialize(d)?.unwrap_or_default())
}

/// Tombola vue par chaque joueur (contrat 2026-11-03 b) : la tombola en
/// cours et, pour le compte de ce bridge seulement, ses inscriptions et ses
/// gains ; jamais le nom d'un autre joueur. Servie à tout bridge, membre
/// compris.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TombolaPublic {
    /// Tombola en cours (inscriptions ouvertes ou tirage), absente sinon.
    #[serde(default)]
    pub id: Option<i64>,
    /// `open` ou `drawing`.
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub streamer: Option<String>,
    /// Identifiant de la chaîne Twitch du streamer.
    #[serde(default)]
    pub twitch: Option<String>,
    #[serde(default)]
    pub draw_at: Option<i64>,
    #[serde(default)]
    pub lots: Vec<TombolaPublicLot>,
    /// Lots gagnés par le compte de ce bridge.
    #[serde(default)]
    pub won: Vec<TombolaPublicLot>,
}

/// Lot de `tombolaPublic` : lot en cours (`min_level`, `drawn`, `entered`,
/// `won`) ou lot gagné (`character`, `drawn_at`, `sent`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TombolaPublicLot {
    pub lot_id: i64,
    #[serde(default)]
    pub kind: Option<String>,
    /// Libellé absent ou `null` : lot écarté seul, jamais toute la réponse.
    #[serde(default, deserialize_with = "null_as_empty")]
    pub label: String,
    #[serde(default)]
    pub item_id: Option<i64>,
    #[serde(default = "one")]
    pub count: i64,
    #[serde(default)]
    pub details: Option<String>,
    #[serde(default)]
    pub min_level: Option<i64>,
    #[serde(default)]
    pub drawn: bool,
    #[serde(default)]
    pub entered: bool,
    #[serde(default)]
    pub won: bool,
    #[serde(default)]
    pub character: Option<String>,
    #[serde(default)]
    pub drawn_at: Option<i64>,
    #[serde(default)]
    pub sent: bool,
}

/// Réponse de `GET /api/v1/bridge/pushed-data`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushedData {
    pub schema: u32,
    /// Clé attendue par l'addon (`GetActiveGuildStorageKey()`), telle quelle.
    pub guild_key: String,
    #[serde(default)]
    pub generated_at: Option<i64>,
    /// Hash du contenu (sans `generated_at`) : sert à savoir s'il faut réécrire.
    #[serde(default)]
    pub version: Option<String>,
    /// Vrai pour tout bridge autorisé qui sert de collecteur (officier ou
    /// vigie, contrat « vigies ») ; fait partie du calcul de `version` côté
    /// site, donc une bascule réécrit toujours le fichier.
    #[serde(default)]
    pub collector: bool,
    /// Liste ordonnée, recopiée dans cet ordre.
    #[serde(default)]
    pub bans: Vec<Ban>,
    /// Autres guildes suivies par le site (fédération) ; défaut vide pour un
    /// ancien contrat ou un site non fédéré. Fait partie du calcul de
    /// `version` côté site.
    #[serde(default)]
    pub allied_guilds: Vec<AlliedGuild>,
    /// Canal de discussion inter-guildes (fédération) ; absent sur un ancien
    /// site. Fait partie du calcul de `version` côté site.
    #[serde(default)]
    pub federation: Option<Federation>,
    /// Réglage d'expiration des demandes (contrat 2026-10-15) ; absent sur
    /// un ancien site. Fait partie du calcul de `version` côté site.
    #[serde(default)]
    pub requests: Option<RequestSettings>,
    /// Joueurs blanchis (contrat 2026-10-15) ; défaut vide pour un ancien
    /// contrat. Fait partie du calcul de `version` côté site.
    #[serde(default)]
    pub cleared: Vec<Cleared>,
    /// Tombola du streamer (contrat 2026-10-16) ; absente sans tombola ou
    /// sur un ancien site. Fait partie du calcul de `version` côté site.
    #[serde(default)]
    pub tombola: Option<Tombola>,
    /// Bans par identité Battle.net/compte (contrat 2026-10-17) ; défaut
    /// vide pour un ancien contrat. Fait partie du calcul de `version` côté
    /// site.
    #[serde(default)]
    pub banned_identities: Vec<BannedIdentity>,
    #[serde(default)]
    pub game_blacklist: Vec<GameBlacklistEntry>,
    #[serde(default)]
    pub blacklist_receipts: Vec<BlacklistReceipt>,
    /// Preuves de collecteur des persos de ce bridge (contrat 2026-11-02 b) ;
    /// défaut vide (membre simple, ancien site, clé non réglée).
    #[serde(default)]
    pub collector_proofs: Vec<CollectorProof>,
    /// Violations déjà reçues par le site (accusé de réception, contrat
    /// 2026-11-02 b) ; défaut vide.
    #[serde(default)]
    pub received_violations: Vec<String>,
    /// Photos d'audit des persos du compte (contrat 2026-11-03, « Je joue sur
    /// plusieurs ordinateurs ») : présentes seulement si le bridge les a
    /// demandées (case cochée). Gardées brutes : chaque entrée est validée
    /// seule à l'écriture, une entrée invalide n'invalide pas le reste.
    #[serde(default)]
    pub audit_baselines: Option<Vec<serde_json::Value>>,
    /// Onglet Tombola ouvert à tous (contrat 2026-11-03 b) ; absent sans
    /// tombola en cours ni gain, ou sur un ancien site.
    #[serde(default)]
    pub tombola_public: Option<TombolaPublic>,
    /// Retours d'inactifs (contrat 2026-11-04 b) ; absent pour un membre
    /// simple ou sur un ancien site, alors non écrit. Lu entrée par entrée
    /// ([`lenient_returns`]) : une entrée mal typée est écartée seule, jamais
    /// toute la réponse (bans, identités bannies… restent écrits).
    #[serde(default, deserialize_with = "lenient_returns")]
    pub returns: Option<Returns>,
}

#[derive(Debug, thiserror::Error)]
pub enum PushedError {
    #[error("schéma de données poussées non pris en charge : {0}")]
    Schema(u32),
    #[error("clé de guilde invalide")]
    GuildKey,
    #[error("écriture de {path} : {source}")]
    Io { path: String, source: io::Error },
}

/// Résultat d'une demande d'écriture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteOutcome {
    /// Fichiers écrits dans ces dossiers d'addon.
    Written(Vec<PathBuf>),
    /// Contenu déjà identique sur le disque.
    Unchanged,
    /// Jeu lancé : rien n'a été écrit, à refaire plus tard.
    GameRunning,
    /// Aucune version de WoW avec l'addon GuildSelfForever installé.
    NoTarget,
}

/// `true` si `s` n'est composée que de minuscules ASCII et de chiffres (non
/// vide) : charte du canal/mot de passe de fédération.
fn is_lower_alnum(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
}

/// Tronque à `max` octets sans couper un caractère.
fn truncate_bytes(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// `true` si `s` est exactement `len` caractères hexadécimaux minuscules
/// (empreinte d'identité `bt`/`acct`, contrat 2026-10-17).
fn is_hex_id(s: &str, len: usize) -> bool {
    s.len() == len
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Texte sûr pour l'addon : non vide, borné en octets, sans `|` ni caractère de contrôle.
fn clean_text(raw: &str, max: usize) -> Option<String> {
    let text = raw.trim();
    (!text.is_empty() && text.len() <= max && !text.chars().any(|c| c.is_control() || c == '|'))
        .then(|| text.to_owned())
}

/// Table Lua de la tombola, `None` s'il ne reste rien de valide.
fn tombola_value(tombola: &Tombola, warnings: &mut Vec<String>) -> Option<LuaValue> {
    let mut fields = Vec::new();
    if let Some(id) = tombola.id.filter(|id| *id > 0) {
        fields.push((LuaKey::Name("id".into()), LuaValue::Int(id)));
    }
    let mut useful = false;
    if let Some(announce) = &tombola.announce {
        match clean_text(&announce.text, MAX_TOMBOLA_ANNOUNCE_BYTES) {
            Some(text) if announce.ends_at >= 0 => {
                useful = true;
                fields.push((
                    LuaKey::Name("announce".into()),
                    LuaValue::Table(vec![
                        (LuaKey::Name("text".into()), LuaValue::Str(text)),
                        (
                            LuaKey::Name("endsAt".into()),
                            LuaValue::Int(announce.ends_at),
                        ),
                    ]),
                ));
            }
            _ => warnings.push(format!(
                "annonce de tombola ignorée (texte ou date invalide) : {:?}",
                announce.text
            )),
        }
    }
    let mut prizes = Vec::new();
    for (idx, prize) in tombola.to_send.iter().enumerate() {
        if prizes.len() == MAX_TOMBOLA_PRIZES {
            warnings.push(format!(
                "{} lot(s) de tombola au-delà de {MAX_TOMBOLA_PRIZES} ignoré(s)",
                tombola.to_send.len() - idx
            ));
            break;
        }
        let winner = clean_text(&prize.winner, MAX_NAME_BYTES);
        let label = clean_text(&prize.label, MAX_TOMBOLA_LABEL_BYTES);
        let kind = prize.kind.as_deref().unwrap_or("item");
        let (Some(winner), Some(label)) = (winner, label) else {
            warnings.push(format!(
                "lot de tombola ignoré (gagnant ou libellé invalide) : {}",
                prize.lot_id
            ));
            continue;
        };
        if prize.lot_id < 1
            || !(1..=1000).contains(&prize.count)
            || !matches!(kind, "item" | "free")
        {
            warnings.push(format!(
                "lot de tombola ignoré (valeurs invalides) : {}",
                prize.lot_id
            ));
            continue;
        }
        let mut entry = vec![
            (LuaKey::Name("lotId".into()), LuaValue::Int(prize.lot_id)),
            (LuaKey::Name("kind".into()), LuaValue::Str(kind.to_owned())),
            (LuaKey::Name("label".into()), LuaValue::Str(label)),
        ];
        if let Some(item_id) = prize.item_id.filter(|id| *id > 0) {
            entry.push((LuaKey::Name("itemId".into()), LuaValue::Int(item_id)));
        }
        entry.push((LuaKey::Name("count".into()), LuaValue::Int(prize.count)));
        if let Some(location) = prize
            .location
            .as_deref()
            .and_then(|l| clean_text(l, MAX_TOMBOLA_LOCATION_BYTES))
        {
            entry.push((LuaKey::Name("location".into()), LuaValue::Str(location)));
        }
        entry.push((LuaKey::Name("winner".into()), LuaValue::Str(winner)));
        if let Some(at) = prize.drawn_at.filter(|a| *a >= 0) {
            entry.push((LuaKey::Name("drawnAt".into()), LuaValue::Int(at)));
        }
        prizes.push((
            LuaKey::Index(i64::try_from(prizes.len() + 1).unwrap_or(i64::MAX)),
            LuaValue::Table(entry),
        ));
    }
    useful |= !prizes.is_empty();
    fields.push((LuaKey::Name("toSend".into()), LuaValue::Table(prizes)));
    useful.then_some(LuaValue::Table(fields))
}

/// Table Lua d'un lot de `tombolaPublic`, `None` s'il est invalide.
fn tombola_public_lot(lot: &TombolaPublicLot) -> Option<LuaValue> {
    let kind = lot.kind.as_deref().unwrap_or("item");
    let label = clean_text(&lot.label, MAX_TOMBOLA_LABEL_BYTES)?;
    if lot.lot_id < 1 || !(1..=1000).contains(&lot.count) || !matches!(kind, "item" | "free") {
        return None;
    }
    let mut entry = vec![
        (LuaKey::Name("lotId".into()), LuaValue::Int(lot.lot_id)),
        (LuaKey::Name("kind".into()), LuaValue::Str(kind.to_owned())),
        (LuaKey::Name("label".into()), LuaValue::Str(label)),
    ];
    if let Some(item_id) = lot.item_id.filter(|id| *id > 0) {
        entry.push((LuaKey::Name("itemId".into()), LuaValue::Int(item_id)));
    }
    entry.push((LuaKey::Name("count".into()), LuaValue::Int(lot.count)));
    if let Some(details) = lot
        .details
        .as_deref()
        .and_then(|d| clean_text(d, MAX_TOMBOLA_DETAILS_BYTES))
    {
        entry.push((LuaKey::Name("details".into()), LuaValue::Str(details)));
    }
    if let Some(level) = lot.min_level.filter(|l| (1..=100).contains(l)) {
        entry.push((LuaKey::Name("minLevel".into()), LuaValue::Int(level)));
    }
    if let Some(name) = lot
        .character
        .as_deref()
        .and_then(|c| clean_text(c, MAX_NAME_BYTES))
    {
        entry.push((LuaKey::Name("character".into()), LuaValue::Str(name)));
    }
    if let Some(at) = lot.drawn_at.filter(|a| *a >= 0) {
        entry.push((LuaKey::Name("drawnAt".into()), LuaValue::Int(at)));
    }
    for (name, flag) in [
        ("drawn", lot.drawn),
        ("entered", lot.entered),
        ("won", lot.won),
        ("sent", lot.sent),
    ] {
        if flag {
            entry.push((LuaKey::Name(name.into()), LuaValue::Bool(true)));
        }
    }
    Some(LuaValue::Table(entry))
}

/// Liste Lua de lots de `tombolaPublic`, bornée, lots invalides écartés seuls.
fn tombola_public_lots(
    lots: &[TombolaPublicLot],
    max: usize,
    what: &str,
    warnings: &mut Vec<String>,
) -> Vec<(LuaKey, LuaValue)> {
    let mut out = Vec::new();
    for (idx, lot) in lots.iter().enumerate() {
        if out.len() == max {
            warnings.push(format!(
                "{} {what} de tombola au-delà de {max} ignoré(s)",
                lots.len() - idx
            ));
            break;
        }
        match tombola_public_lot(lot) {
            Some(value) => out.push((
                LuaKey::Index(i64::try_from(out.len() + 1).unwrap_or(i64::MAX)),
                value,
            )),
            None => warnings.push(format!(
                "{what} de tombola ignoré (valeurs invalides) : {}",
                lot.lot_id
            )),
        }
    }
    out
}

/// Table Lua `tombolaPublic`, `None` s'il ne reste ni tombola en cours ni lot.
fn tombola_public_value(t: &TombolaPublic, warnings: &mut Vec<String>) -> Option<LuaValue> {
    let mut fields = Vec::new();
    let id = t.id.filter(|id| *id > 0);
    if let Some(id) = id {
        fields.push((LuaKey::Name("id".into()), LuaValue::Int(id)));
    }
    if let Some(status) = t
        .status
        .as_deref()
        .filter(|s| matches!(*s, "open" | "drawing"))
    {
        fields.push((
            LuaKey::Name("status".into()),
            LuaValue::Str(status.to_owned()),
        ));
    }
    if let Some(streamer) = t
        .streamer
        .as_deref()
        .and_then(|s| clean_text(s, MAX_NAME_BYTES))
    {
        fields.push((LuaKey::Name("streamer".into()), LuaValue::Str(streamer)));
    }
    if let Some(twitch) = t.twitch.as_deref().filter(|s| {
        (1..=25).contains(&s.len())
            && s.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    }) {
        fields.push((
            LuaKey::Name("twitch".into()),
            LuaValue::Str(twitch.to_owned()),
        ));
    }
    if let Some(at) = t.draw_at.filter(|a| *a >= 0) {
        fields.push((LuaKey::Name("drawAt".into()), LuaValue::Int(at)));
    }
    let lots = tombola_public_lots(&t.lots, MAX_TOMBOLA_PUBLIC_LOTS, "lot", warnings);
    let won = tombola_public_lots(&t.won, MAX_TOMBOLA_PUBLIC_WON, "gain", warnings);
    let useful = id.is_some() || !lots.is_empty() || !won.is_empty();
    fields.push((LuaKey::Name("lots".into()), LuaValue::Table(lots)));
    fields.push((LuaKey::Name("won".into()), LuaValue::Table(won)));
    useful.then_some(LuaValue::Table(fields))
}

/// `true` si la preuve de collecteur suit les règles du contrat.
fn valid_proof(p: &CollectorProof) -> bool {
    !p.name.is_empty()
        && p.name.len() <= MAX_PROOF_NAME_BYTES
        && !p
            .name
            .chars()
            .any(|c| c.is_control() || c == '|' || c == '~')
        && p.expires_at > 0
        && is_hex_id(&p.sig, PROOF_SIG_HEX_LEN)
}

/// `true` si l'identifiant de violation suit les règles du contrat.
fn valid_violation_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_VIOLATION_ID_BYTES
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b':' | b'-'))
}

/// Tables Lua `collectorProofs` et `receivedViolations`.
fn proofs_and_received(data: &PushedData, warnings: &mut Vec<String>) -> (LuaValue, LuaValue) {
    let mut proofs = Vec::new();
    for (idx, p) in data.collector_proofs.iter().enumerate() {
        if proofs.len() == MAX_COLLECTOR_PROOFS {
            warnings.push(format!(
                "{} preuve(s) de collecteur au-delà de {MAX_COLLECTOR_PROOFS} ignorée(s)",
                data.collector_proofs.len() - idx
            ));
            break;
        }
        if !valid_proof(p) {
            warnings.push(format!(
                "preuve de collecteur ignorée (invalide) : {:?}",
                p.name
            ));
            continue;
        }
        proofs.push((
            LuaKey::Index(i64::try_from(proofs.len() + 1).unwrap_or(i64::MAX)),
            LuaValue::Table(vec![
                (LuaKey::Name("name".into()), LuaValue::Str(p.name.clone())),
                (
                    LuaKey::Name("expiresAt".into()),
                    LuaValue::Int(p.expires_at),
                ),
                (LuaKey::Name("sig".into()), LuaValue::Str(p.sig.clone())),
            ]),
        ));
    }
    let mut received = Vec::new();
    let mut dropped = 0usize;
    for (idx, id) in data.received_violations.iter().enumerate() {
        if received.len() == MAX_RECEIVED_VIOLATIONS {
            warnings.push(format!(
                "{} violation(s) reçue(s) au-delà de {MAX_RECEIVED_VIOLATIONS} ignorée(s)",
                data.received_violations.len() - idx
            ));
            break;
        }
        if !valid_violation_id(id) {
            dropped += 1;
            continue;
        }
        received.push((
            LuaKey::Index(i64::try_from(received.len() + 1).unwrap_or(i64::MAX)),
            LuaValue::Str(id.clone()),
        ));
    }
    if dropped > 0 {
        warnings.push(format!(
            "{dropped} identifiant(s) de violation reçue(s) ignoré(s) (invalides)"
        ));
    }
    (LuaValue::Table(proofs), LuaValue::Table(received))
}

/// Table Lua `{ [clé] = nombre }` d'une liste `[[clé, nombre]]`.
fn int_map(pairs: &[(i64, i64)]) -> LuaValue {
    LuaValue::Table(
        pairs
            .iter()
            .map(|&(k, v)| (LuaKey::Index(k), LuaValue::Int(v)))
            .collect(),
    )
}

/// Photo d'audit au format de l'addon (`makeBaselineRecord` d'`Audit.lua`) :
/// `{ name, character?, savedAt, checksum, adventureID, guildKey, snapshot }`.
fn baseline_value(b: &AuditBaseline) -> LuaValue {
    let s = &b.snapshot;
    let mut snap = vec![
        (LuaKey::Name("time".into()), LuaValue::Int(s.time)),
        (LuaKey::Name("gold".into()), LuaValue::Int(s.gold)),
        (LuaKey::Name("bagSlots".into()), LuaValue::Int(s.bag_slots)),
    ];
    if s.legacy {
        snap.push((LuaKey::Name("legacy".into()), LuaValue::Bool(true)));
    }
    snap.push((LuaKey::Name("items".into()), int_map(&s.items)));
    snap.push((
        LuaKey::Name("equipment".into()),
        LuaValue::Table(
            s.equipment
                .iter()
                .map(|(slot, v)| (LuaKey::Index(*slot), LuaValue::Str(v.clone())))
                .collect(),
        ),
    ));
    if let Some(bank) = &s.bank {
        let mut fields = vec![(LuaKey::Name("time".into()), LuaValue::Int(bank.time))];
        if bank.legacy {
            fields.push((LuaKey::Name("legacy".into()), LuaValue::Bool(true)));
        }
        fields.push((LuaKey::Name("items".into()), int_map(&bank.items)));
        snap.push((LuaKey::Name("bank".into()), LuaValue::Table(fields)));
    }
    // `/played` (contrat 2026-11-03 c), hors `checksum` : déjà passés par
    // `sanitize_played`, donc ensemble ou pas du tout.
    if let (Some(played), Some(observed)) = (s.played, s.played_observed) {
        snap.push((LuaKey::Name("played".into()), LuaValue::Int(played)));
        snap.push((
            LuaKey::Name("playedObserved".into()),
            LuaValue::Int(observed),
        ));
    }
    if let Some(level) = s.level {
        snap.push((LuaKey::Name("level".into()), LuaValue::Int(level)));
    }
    let mut fields = vec![(LuaKey::Name("name".into()), LuaValue::Str(b.name.clone()))];
    if let Some(c) = &b.character {
        fields.push((LuaKey::Name("character".into()), LuaValue::Str(c.clone())));
    }
    fields.extend([
        (LuaKey::Name("savedAt".into()), LuaValue::Int(b.saved_at)),
        (
            LuaKey::Name("checksum".into()),
            LuaValue::Str(b.checksum.clone()),
        ),
        (
            LuaKey::Name("adventureID".into()),
            LuaValue::Str(b.adventure_id.clone()),
        ),
        (
            LuaKey::Name("guildKey".into()),
            LuaValue::Str(b.guild_key.clone()),
        ),
        (LuaKey::Name("snapshot".into()), LuaValue::Table(snap)),
    ]);
    LuaValue::Table(fields)
}

/// Table Lua `auditBaselines` : chaque photo validée seule
/// ([`AuditBaseline::is_valid`]), [`MAX_BASELINES`] au plus.
fn audit_baselines_value(raw: &[serde_json::Value], warnings: &mut Vec<String>) -> LuaValue {
    let mut out = Vec::new();
    let mut dropped = 0usize;
    for (idx, v) in raw.iter().enumerate() {
        if out.len() == MAX_BASELINES {
            warnings.push(format!(
                "{} photo(s) d'inventaire au-delà de {MAX_BASELINES} ignorée(s)",
                raw.len() - idx
            ));
            break;
        }
        match serde_json::from_value::<AuditBaseline>(v.clone()) {
            Ok(mut b) if b.is_valid() => {
                b.snapshot.sanitize_played();
                out.push((
                    LuaKey::Index(i64::try_from(out.len() + 1).unwrap_or(i64::MAX)),
                    baseline_value(&b),
                ));
            }
            _ => dropped += 1,
        }
    }
    if dropped > 0 {
        warnings.push(format!(
            "{dropped} photo(s) d'inventaire reçue(s) ignorée(s) (invalides)"
        ));
    }
    LuaValue::Table(out)
}

/// Contenu Lua de `GSF_PushedData` et remarques (bans écartés).
pub fn to_addon_value(data: &PushedData) -> Result<(LuaValue, Vec<String>), PushedError> {
    if data.schema != SCHEMA {
        return Err(PushedError::Schema(data.schema));
    }
    let key = &data.guild_key;
    if key.is_empty() || key.len() > MAX_GUILD_KEY_BYTES || key.chars().any(char::is_control) {
        return Err(PushedError::GuildKey);
    }
    let mut warnings = Vec::new();
    let mut bans = Vec::new();
    for (idx, ban) in data.bans.iter().enumerate() {
        let raw_name = &ban.name;
        let name = raw_name.trim();
        if name.is_empty()
            || name.len() > MAX_NAME_BYTES
            || name.chars().any(|c| c.is_control() || c == '|')
        {
            warnings.push(format!("ban ignoré (nom invalide) : {raw_name:?}"));
            continue;
        }
        if bans.len() == MAX_BANS {
            warnings.push(format!(
                "{} bans au-delà de {MAX_BANS} ignorés",
                data.bans.len() - idx
            ));
            break;
        }
        let mut fields = vec![(LuaKey::Name("name".into()), LuaValue::Str(name.to_owned()))];
        if let Some(reason) = &ban.reason {
            let cleaned: String = reason
                .chars()
                .map(|c| if c.is_control() { ' ' } else { c })
                .collect();
            fields.push((
                LuaKey::Name("reason".into()),
                LuaValue::Str(truncate_bytes(&cleaned, MAX_REASON_BYTES).to_owned()),
            ));
        }
        if let Some(until) = ban.expires_at.filter(|u| *u >= 0) {
            fields.push((LuaKey::Name("expiresAt".into()), LuaValue::Int(until)));
        }
        bans.push((
            LuaKey::Index(i64::try_from(bans.len() + 1).unwrap_or(i64::MAX)),
            LuaValue::Table(fields),
        ));
    }
    let mut allied_guilds = Vec::new();
    for (idx, ally) in data.allied_guilds.iter().enumerate() {
        if allied_guilds.len() == MAX_ALLIED_GUILDS {
            warnings.push(format!(
                "{} guilde(s) alliée(s) au-delà de {MAX_ALLIED_GUILDS} ignorée(s)",
                data.allied_guilds.len() - idx
            ));
            break;
        }
        let raw_key = &ally.key;
        let ally_key = raw_key.trim();
        if ally_key.is_empty()
            || ally_key.len() > MAX_GUILD_KEY_BYTES
            || ally_key.chars().any(char::is_control)
        {
            warnings.push(format!(
                "guilde alliée ignorée (clé invalide) : {raw_key:?}"
            ));
            continue;
        }
        let raw_name = &ally.name;
        let ally_name = raw_name.trim();
        if ally_name.is_empty()
            || ally_name.len() > MAX_NAME_BYTES
            || ally_name.chars().any(|c| c.is_control() || c == '|')
        {
            warnings.push(format!(
                "guilde alliée ignorée (nom invalide) : {raw_name:?}"
            ));
            continue;
        }
        allied_guilds.push((
            LuaKey::Index(i64::try_from(allied_guilds.len() + 1).unwrap_or(i64::MAX)),
            LuaValue::Table(vec![
                (
                    LuaKey::Name("key".into()),
                    LuaValue::Str(ally_key.to_owned()),
                ),
                (
                    LuaKey::Name("name".into()),
                    LuaValue::Str(ally_name.to_owned()),
                ),
            ]),
        ));
    }
    let mut federation = None;
    if let Some(fed) = &data.federation {
        if !is_lower_alnum(&fed.channel) || !is_lower_alnum(&fed.password) {
            warnings.push("fédération ignorée (canal ou mot de passe invalide)".to_owned());
        } else {
            let mut guilds = Vec::new();
            for (idx, g) in fed.guilds.iter().enumerate() {
                if guilds.len() == MAX_FEDERATION_GUILDS {
                    warnings.push(format!(
                        "{} guilde(s) de fédération au-delà de {MAX_FEDERATION_GUILDS} ignorée(s)",
                        fed.guilds.len() - idx
                    ));
                    break;
                }
                let key = g.key.trim();
                if key.is_empty()
                    || key.len() > MAX_GUILD_KEY_BYTES
                    || key.chars().any(char::is_control)
                {
                    warnings.push(format!(
                        "guilde de fédération ignorée (clé invalide) : {:?}",
                        g.key
                    ));
                    continue;
                }
                let name = g.name.trim();
                if name.is_empty()
                    || name.len() > MAX_NAME_BYTES
                    || name.chars().any(|c| c.is_control() || c == '|')
                {
                    warnings.push(format!(
                        "guilde de fédération ignorée (nom invalide) : {:?}",
                        g.name
                    ));
                    continue;
                }
                let kind = g.kind.trim();
                if kind.is_empty()
                    || kind.len() > MAX_FEDERATION_FIELD_BYTES
                    || kind.chars().any(|c| c.is_control() || c == '|')
                {
                    warnings.push(format!(
                        "guilde de fédération ignorée (type invalide) : {:?}",
                        g.kind
                    ));
                    continue;
                }
                let mode = g.mode.trim();
                if mode.is_empty()
                    || mode.len() > MAX_FEDERATION_FIELD_BYTES
                    || mode.chars().any(|c| c.is_control() || c == '|')
                {
                    warnings.push(format!(
                        "guilde de fédération ignorée (mode invalide) : {:?}",
                        g.mode
                    ));
                    continue;
                }
                guilds.push((
                    LuaKey::Index(i64::try_from(guilds.len() + 1).unwrap_or(i64::MAX)),
                    LuaValue::Table(vec![
                        (LuaKey::Name("key".into()), LuaValue::Str(key.to_owned())),
                        (LuaKey::Name("name".into()), LuaValue::Str(name.to_owned())),
                        (LuaKey::Name("type".into()), LuaValue::Str(kind.to_owned())),
                        (LuaKey::Name("mode".into()), LuaValue::Str(mode.to_owned())),
                    ]),
                ));
            }
            federation = Some(LuaValue::Table(vec![
                (
                    LuaKey::Name("channel".into()),
                    LuaValue::Str(fed.channel.clone()),
                ),
                (
                    LuaKey::Name("password".into()),
                    LuaValue::Str(fed.password.clone()),
                ),
                (LuaKey::Name("guilds".into()), LuaValue::Table(guilds)),
            ]));
        }
    }
    let mut requests_settings = None;
    if let Some(settings) = &data.requests {
        match settings.expiry_days {
            Some(days) if (MIN_EXPIRY_DAYS..=MAX_EXPIRY_DAYS).contains(&days) => {
                requests_settings = Some(LuaValue::Table(vec![(
                    LuaKey::Name("expiryDays".into()),
                    LuaValue::Int(days),
                )]));
            }
            other => {
                warnings.push(format!(
                    "réglage des demandes ignoré (expiry_days invalide) : {other:?}"
                ));
            }
        }
    }
    let mut cleared = Vec::new();
    for (idx, c) in data.cleared.iter().enumerate() {
        if cleared.len() == MAX_CLEARED {
            warnings.push(format!(
                "{} joueur(s) blanchi(s) au-delà de {MAX_CLEARED} ignoré(s)",
                data.cleared.len() - idx
            ));
            break;
        }
        let raw_name = &c.name;
        let name = raw_name.trim();
        if name.is_empty()
            || name.len() > MAX_NAME_BYTES
            || name.chars().any(|ch| ch.is_control() || ch == '|')
        {
            warnings.push(format!(
                "joueur blanchi ignoré (nom invalide) : {raw_name:?}"
            ));
            continue;
        }
        if c.at < 0 {
            warnings.push(format!("joueur blanchi ignoré (date invalide) : {}", c.at));
            continue;
        }
        cleared.push((
            LuaKey::Index(i64::try_from(cleared.len() + 1).unwrap_or(i64::MAX)),
            LuaValue::Table(vec![
                (LuaKey::Name("name".into()), LuaValue::Str(name.to_owned())),
                (LuaKey::Name("at".into()), LuaValue::Int(c.at)),
            ]),
        ));
    }
    let mut banned_identities = Vec::new();
    for (idx, bi) in data.banned_identities.iter().enumerate() {
        if banned_identities.len() == MAX_BANNED_IDENTITIES {
            warnings.push(format!(
                "{} identité(s) bannie(s) au-delà de {MAX_BANNED_IDENTITIES} ignorée(s)",
                data.banned_identities.len() - idx
            ));
            break;
        }
        let bt = bi.bt.as_deref().filter(|s| is_hex_id(s, 32));
        let acct = bi.acct.as_deref().filter(|s| is_hex_id(s, 16));
        if bt.is_none() && acct.is_none() {
            warnings.push(format!(
                "identité bannie ignorée (bt et acct invalides) : {:?} / {:?}",
                bi.bt, bi.acct
            ));
            continue;
        }
        let mut fields = Vec::new();
        if let Some(bt) = bt {
            fields.push((LuaKey::Name("bt".into()), LuaValue::Str(bt.to_owned())));
        }
        if let Some(acct) = acct {
            fields.push((LuaKey::Name("acct".into()), LuaValue::Str(acct.to_owned())));
        }
        if let Some(reason) = &bi.reason {
            let cleaned: String = reason
                .chars()
                .map(|c| if c.is_control() { ' ' } else { c })
                .collect();
            fields.push((
                LuaKey::Name("reason".into()),
                LuaValue::Str(truncate_bytes(&cleaned, MAX_REASON_BYTES).to_owned()),
            ));
        }
        if let Some(until) = bi.expires_at.filter(|u| *u >= 0) {
            fields.push((LuaKey::Name("expiresAt".into()), LuaValue::Int(until)));
        }
        banned_identities.push((
            LuaKey::Index(i64::try_from(banned_identities.len() + 1).unwrap_or(i64::MAX)),
            LuaValue::Table(fields),
        ));
    }

    let mut game_blacklist = Vec::new();
    for entry in data.game_blacklist.iter().take(500) {
        if !matches!(entry.target_type.as_str(), "character" | "battle_tag")
            || clean_text(&entry.target_key, 64).is_none()
        {
            continue;
        }
        let Some(label) = clean_text(&entry.target_label, 80) else {
            continue;
        };
        let mut fields = vec![
            (
                LuaKey::Name("targetType".into()),
                LuaValue::Str(entry.target_type.clone()),
            ),
            (
                LuaKey::Name("targetKey".into()),
                LuaValue::Str(entry.target_key.clone()),
            ),
            (LuaKey::Name("targetLabel".into()), LuaValue::Str(label)),
        ];
        if let Some(reason) = entry
            .reason
            .as_deref()
            .and_then(|v| clean_text(v, MAX_REASON_BYTES))
        {
            fields.push((LuaKey::Name("reason".into()), LuaValue::Str(reason)));
        }
        game_blacklist.push((
            LuaKey::Index(i64::try_from(game_blacklist.len() + 1).unwrap_or(i64::MAX)),
            LuaValue::Table(fields),
        ));
    }
    let mut blacklist_receipts = Vec::new();
    for receipt in data.blacklist_receipts.iter().take(1000) {
        let Some(id) = clean_text(&receipt.id, 64) else {
            continue;
        };
        if !matches!(receipt.status.as_str(), "applied" | "rejected") {
            continue;
        }
        let mut fields = vec![
            (LuaKey::Name("id".into()), LuaValue::Str(id)),
            (
                LuaKey::Name("status".into()),
                LuaValue::Str(receipt.status.clone()),
            ),
        ];
        if let Some(message) = receipt.message.as_deref().and_then(|v| clean_text(v, 200)) {
            fields.push((LuaKey::Name("message".into()), LuaValue::Str(message)));
        }
        blacklist_receipts.push((
            LuaKey::Index(i64::try_from(blacklist_receipts.len() + 1).unwrap_or(i64::MAX)),
            LuaValue::Table(fields),
        ));
    }

    let mut root = vec![
        (
            LuaKey::Name("schema".into()),
            LuaValue::Int(i64::from(SCHEMA)),
        ),
        (LuaKey::Name("guildKey".into()), LuaValue::Str(key.clone())),
    ];
    if let Some(at) = data.generated_at.filter(|a| *a >= 0) {
        root.push((LuaKey::Name("generatedAt".into()), LuaValue::Int(at)));
    }
    if data.collector {
        root.push((LuaKey::Name("collector".into()), LuaValue::Bool(true)));
    }
    root.push((LuaKey::Name("bans".into()), LuaValue::Table(bans)));
    root.push((
        LuaKey::Name("alliedGuilds".into()),
        LuaValue::Table(allied_guilds),
    ));
    if let Some(federation) = federation {
        root.push((LuaKey::Name("federation".into()), federation));
    }
    if let Some(requests) = requests_settings {
        root.push((LuaKey::Name("requests".into()), requests));
    }
    root.push((LuaKey::Name("cleared".into()), LuaValue::Table(cleared)));
    root.push((
        LuaKey::Name("bannedIdentities".into()),
        LuaValue::Table(banned_identities),
    ));
    root.push((
        LuaKey::Name("gameBlacklist".into()),
        LuaValue::Table(game_blacklist),
    ));
    root.push((
        LuaKey::Name("blacklistReceipts".into()),
        LuaValue::Table(blacklist_receipts),
    ));
    let (proofs, received) = proofs_and_received(data, &mut warnings);
    root.push((LuaKey::Name("collectorProofs".into()), proofs));
    root.push((LuaKey::Name("receivedViolations".into()), received));
    if let Some(tombola) = data
        .tombola
        .as_ref()
        .and_then(|t| tombola_value(t, &mut warnings))
    {
        root.push((LuaKey::Name("tombola".into()), tombola));
    }
    if let Some(raw) = &data.audit_baselines {
        root.push((
            LuaKey::Name("auditBaselines".into()),
            audit_baselines_value(raw, &mut warnings),
        ));
    }
    if let Some(public) = data
        .tombola_public
        .as_ref()
        .and_then(|t| tombola_public_value(t, &mut warnings))
    {
        root.push((LuaKey::Name("tombolaPublic".into()), public));
    }
    if let Some(returns) = &data.returns {
        root.push((
            LuaKey::Name("returns".into()),
            returns_value(returns, &mut warnings),
        ));
    }
    Ok((LuaValue::Table(root), warnings))
}

/// Nom d'un perso venant du site : règles des bans (non vide, ≤ 64 octets,
/// sans caractère de contrôle ni `|`), rogné.
fn clean_name(raw: &str) -> Option<&str> {
    let name = raw.trim();
    (!name.is_empty()
        && name.len() <= MAX_NAME_BYTES
        && !name.chars().any(|ch| ch.is_control() || ch == '|'))
    .then_some(name)
}

fn index_key(len: usize) -> LuaKey {
    LuaKey::Index(i64::try_from(len + 1).unwrap_or(i64::MAX))
}

/// Table Lua `returns` (contrat 2026-11-04 b) : chaque entrée validée seule.
fn returns_value(r: &Returns, warnings: &mut Vec<String>) -> LuaValue {
    let mut allowed = Vec::new();
    for (idx, e) in r.allowed.iter().enumerate() {
        if allowed.len() == MAX_RETURNS {
            warnings.push(format!(
                "{} retour(s) autorisé(s) au-delà de {MAX_RETURNS} ignoré(s)",
                r.allowed.len() - idx
            ));
            break;
        }
        let Some(name) = clean_name(&e.name) else {
            warnings.push(format!(
                "retour autorisé ignoré (nom invalide) : {:?}",
                e.name
            ));
            continue;
        };
        let bt = e.bt.as_deref().filter(|v| is_hex_id(v, 32));
        let acct = e.acct.as_deref().filter(|v| is_hex_id(v, 16));
        if bt.is_none() && acct.is_none() {
            warnings.push(format!(
                "retour autorisé ignoré (identité invalide) : {name:?}"
            ));
            continue;
        }
        let mut entry = vec![(LuaKey::Name("name".into()), LuaValue::Str(name.to_owned()))];
        if let Some(bt) = bt {
            entry.push((LuaKey::Name("bt".into()), LuaValue::Str(bt.to_owned())));
        }
        if let Some(acct) = acct {
            entry.push((LuaKey::Name("acct".into()), LuaValue::Str(acct.to_owned())));
        }
        if let Some(reason) = e
            .reason
            .as_deref()
            .filter(|v| *v == "inactivity" || *v == "absence")
        {
            entry.push((
                LuaKey::Name("reason".into()),
                LuaValue::Str(reason.to_owned()),
            ));
        }
        if let Some(since) = e.since.filter(|v| *v >= 0) {
            entry.push((LuaKey::Name("since".into()), LuaValue::Int(since)));
        }
        allowed.push((index_key(allowed.len()), LuaValue::Table(entry)));
    }
    let mut validated = Vec::new();
    for (idx, e) in r.validated.iter().enumerate() {
        if validated.len() == MAX_RETURNS {
            warnings.push(format!(
                "{} retour(s) validé(s) au-delà de {MAX_RETURNS} ignoré(s)",
                r.validated.len() - idx
            ));
            break;
        }
        let Some(name) = clean_name(&e.name) else {
            warnings.push(format!(
                "retour validé ignoré (nom invalide) : {:?}",
                e.name
            ));
            continue;
        };
        if e.at < 0 {
            warnings.push(format!("retour validé ignoré (date invalide) : {}", e.at));
            continue;
        }
        validated.push((
            index_key(validated.len()),
            LuaValue::Table(vec![
                (LuaKey::Name("name".into()), LuaValue::Str(name.to_owned())),
                (LuaKey::Name("at".into()), LuaValue::Int(e.at)),
            ]),
        ));
    }
    LuaValue::Table(vec![
        (LuaKey::Name("allowed".into()), LuaValue::Table(allowed)),
        (LuaKey::Name("validated".into()), LuaValue::Table(validated)),
    ])
}

fn sanitize_version(version: Option<&str>) -> String {
    version
        .unwrap_or("-")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        .take(80)
        .collect()
}

/// `true` si chaque dossier contient déjà le `.toc` et un `Data.lua` de la
/// même `version` (le site change `generated_at` à chaque appel : seule la
/// version dit s'il faut réécrire).
fn is_current(addons_dirs: &[PathBuf], version: Option<&str>) -> bool {
    let version = sanitize_version(version);
    if version.is_empty() || version == "-" {
        return false;
    }
    let marker = format!("\n-- version : {version}\n");
    addons_dirs.iter().all(|a| {
        let dir = a.join(DATA_ADDON_DIR);
        dir.join(format!("{DATA_ADDON_DIR}.toc")).is_file()
            && fs::read(dir.join("Data.lua"))
                .ok()
                .and_then(|b| String::from_utf8(b).ok())
                .is_some_and(|s| s.contains(&marker))
    })
}

/// `true` si chaque dossier `Interface/AddOns` donné (non vide) contient
/// déjà `GuildSelfForever_Data/Data.lua` : condition pour demander au site
/// « seulement si changé » (`If-None-Match`, contrat 2026-11-02 b).
pub fn data_files_present(addons_dirs: &[PathBuf]) -> bool {
    !addons_dirs.is_empty()
        && addons_dirs
            .iter()
            .all(|a| a.join(DATA_ADDON_DIR).join("Data.lua").is_file())
}

/// Contenu de `Data.lua`.
pub fn render_data_lua(
    data: &PushedData,
    bridge_version: &str,
) -> Result<(String, Vec<String>), PushedError> {
    let (value, warnings) = to_addon_value(data)?;
    let version = sanitize_version(data.version.as_deref());
    let src = format!(
        "-- Généré par GSF Bridge {bridge_version} : ne pas modifier, réécrit jeu fermé.\n\
         -- version : {version}\n{}",
        assignment("GSF_PushedData", &value)
    );
    Ok((src, warnings))
}

/// Contenu du `.toc`.
pub fn render_toc(bridge_version: &str) -> String {
    format!(
        "## Interface: {TOC_INTERFACE}\n\
         ## Title: GuildSelfForever_Data\n\
         ## Notes: Données poussées par le bridge officier GSF (générées automatiquement).\n\
         ## Author: GSF Bridge\n\
         ## Version: {bridge_version}\n\
         Data.lua\n"
    )
}

/// Dossiers `Interface/AddOns` des versions où GuildSelfForever est installé.
pub fn addon_targets(flavor_dirs: &[PathBuf]) -> Vec<PathBuf> {
    flavor_dirs
        .iter()
        .filter(|f| crate::wow::has_main_addon(f))
        .map(|f| crate::wow::addons_dir(f))
        .collect()
}

fn write_if_changed(path: &Path, content: &str) -> Result<bool, PushedError> {
    if fs::read(path).ok().as_deref() == Some(content.as_bytes()) {
        return Ok(false);
    }
    atomic_write(path, content.as_bytes()).map_err(|source| PushedError::Io {
        path: path.display().to_string(),
        source,
    })?;
    Ok(true)
}

/// Écrit l'addon de données dans chaque dossier `Interface/AddOns` donné,
/// **seulement si le jeu est fermé** (sinon [`WriteOutcome::GameRunning`],
/// rien n'est touché).
pub fn write_pushed_data(
    addons_dirs: &[PathBuf],
    data: &PushedData,
    detector: &dyn GameDetector,
    bridge_version: &str,
) -> Result<(WriteOutcome, Vec<String>), PushedError> {
    let (lua, warnings) = render_data_lua(data, bridge_version)?;
    if addons_dirs.is_empty() {
        return Ok((WriteOutcome::NoTarget, warnings));
    }
    if is_current(addons_dirs, data.version.as_deref()) {
        return Ok((WriteOutcome::Unchanged, warnings));
    }
    if detector.is_game_running() {
        return Ok((WriteOutcome::GameRunning, warnings));
    }
    let toc = render_toc(bridge_version);
    let mut written = Vec::new();
    for addons in addons_dirs {
        let dir = addons.join(DATA_ADDON_DIR);
        let toc_changed = write_if_changed(&dir.join(format!("{DATA_ADDON_DIR}.toc")), &toc)?;
        let data_changed = write_if_changed(&dir.join("Data.lua"), &lua)?;
        if toc_changed || data_changed {
            written.push(dir);
        }
    }
    let outcome = if written.is_empty() {
        WriteOutcome::Unchanged
    } else {
        WriteOutcome::Written(written)
    };
    Ok((outcome, warnings))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wow::FixedGameDetector;
    use gsf_core::sv::{parse, Value};

    fn sample() -> PushedData {
        serde_json::from_str(
            r#"{"schema":1,"guild_key":"realm#home","generated_at":1800000000,"version":"abc123",
                "bans":[
                  {"name":"cheater","reason":"Or externe \"x\" \\ ]] \n fin","expiresAt":1900000000},
                  {"name":"bad|name","reason":"x"},
                  {"name":"Dävin-Fumée"},
                  {"name":"   ","reason":"x"}
                ]}"#,
        )
        .unwrap()
    }

    #[test]
    fn collector_proofs_and_received_violations_are_written_and_filtered() {
        let sig = "0a".repeat(256);
        let mut d = sample();
        d.collector_proofs = vec![
            CollectorProof {
                name: "vigie".into(),
                expires_at: 1800000000,
                sig: sig.clone(),
            },
            CollectorProof {
                name: "écharpe bleue".into(),
                expires_at: 1800000000,
                sig: sig.clone(),
            },
            CollectorProof {
                name: "a~b".into(),
                expires_at: 1,
                sig: sig.clone(),
            },
            CollectorProof {
                name: "x".into(),
                expires_at: 0,
                sig: sig.clone(),
            },
            CollectorProof {
                name: "x".into(),
                expires_at: 1,
                sig: sig.to_uppercase(),
            },
            CollectorProof {
                name: "x".into(),
                expires_at: 1,
                sig: "0a".repeat(255),
            },
        ];
        d.received_violations = vec![
            "trade_gold-1700000000-123".into(),
            "bad id".into(),
            String::new(),
            "x".repeat(129),
            "a.b:c-d_e".into(),
        ];
        let (v, warnings) = to_addon_value(&d).unwrap();
        let src = assignment("GSF_PushedData", &v);
        let sv = parse(src.as_bytes()).unwrap();
        let root = sv.get("GSF_PushedData").unwrap();
        let proofs: Vec<&Value> = root.get("collectorProofs").unwrap().array().collect();
        assert_eq!(proofs.len(), 2);
        assert_eq!(
            proofs[1].get("name").and_then(Value::as_str),
            Some("écharpe bleue")
        );
        assert_eq!(proofs[0].get("expiresAt"), Some(&Value::Int(1800000000)));
        assert_eq!(
            proofs[0].get("sig").and_then(Value::as_str),
            Some(sig.as_str())
        );
        let received: Vec<&str> = root
            .get("receivedViolations")
            .unwrap()
            .array()
            .filter_map(Value::as_str)
            .collect();
        assert_eq!(received, vec!["trade_gold-1700000000-123", "a.b:c-d_e"]);
        assert!(
            warnings.iter().any(|w| w.contains("3 identifiant(s)")),
            "{warnings:?}"
        );
    }

    #[test]
    fn audit_baselines_only_when_asked_and_each_entry_checked_alone() {
        let mut d = sample();
        let (lua, _) = render_data_lua(&d, "0.7.1").unwrap();
        assert!(
            !lua.contains("auditBaselines"),
            "case décochée : clé absente"
        );

        d.audit_baselines = Some(Vec::new());
        let (lua, _) = render_data_lua(&d, "0.7.1").unwrap();
        assert!(lua.contains("auditBaselines = {}"), "{lua}");

        let good = serde_json::json!({
            "name": "tester", "saved_at": 1800, "checksum": "77", "adventure_id": "A1",
            "guild_key": "realm#home",
            "snapshot": {"time": 1800, "gold": 5, "items": [[12, 3]], "equipment": [[1, "12:0"]], "legacy": true}
        });
        let mut bad = good.clone();
        bad["snapshot"]["gold"] = serde_json::json!(-1);
        d.audit_baselines = Some(vec![bad, good, serde_json::json!("x")]);
        let (lua, warnings) = render_data_lua(&d, "0.7.1").unwrap();
        let sv = parse(lua.as_bytes()).unwrap();
        let list = sv
            .get("GSF_PushedData")
            .unwrap()
            .get("auditBaselines")
            .unwrap();
        let only = list.get(1i64).unwrap();
        assert!(list.get(2i64).is_none());
        assert_eq!(only.get("checksum").and_then(Value::as_str), Some("77"));
        assert_eq!(only.get("character"), None);
        let snap = only.get("snapshot").unwrap();
        assert_eq!(snap.get("legacy").and_then(Value::as_bool), Some(true));
        assert_eq!(snap.get("bagSlots").and_then(Value::as_i64), Some(0));
        assert_eq!(
            snap.get("items")
                .unwrap()
                .get(12i64)
                .and_then(Value::as_i64),
            Some(3)
        );
        assert!(snap.get("bank").is_none());
        assert!(
            warnings.iter().any(|w| w.contains("2 photo(s)")),
            "{warnings:?}"
        );
    }

    #[test]
    fn audit_baselines_carry_played_outside_the_checksum() {
        // Contrat 2026-11-03 c : `played`, `played_observed`, `level` réécrits en
        // `played`, `playedObserved`, `level` ; invalides écartés seuls.
        let mut d = sample();
        let base = serde_json::json!({
            "name": "tester", "saved_at": 1800, "checksum": "77", "adventure_id": "A1",
            "guild_key": "realm#home",
            "snapshot": {"time": 1800, "gold": 5, "items": [], "equipment": [],
                         "played": 51210, "played_observed": 120, "level": 20}
        });
        let mut lone = base.clone();
        lone["name"] = serde_json::json!("lone");
        lone["snapshot"]
            .as_object_mut()
            .unwrap()
            .remove("played_observed");
        lone["snapshot"]["level"] = serde_json::json!(0);
        let mut old = base.clone();
        old["name"] = serde_json::json!("old");
        for k in ["played", "played_observed", "level"] {
            old["snapshot"].as_object_mut().unwrap().remove(k);
        }
        let mut typed = base.clone();
        typed["name"] = serde_json::json!("typed");
        typed["snapshot"]["played"] = serde_json::json!("5");
        typed["snapshot"]["played_observed"] = serde_json::json!(5.5);
        typed["snapshot"]["level"] = serde_json::json!(1e30);
        d.audit_baselines = Some(vec![base, lone, old, typed]);
        let (lua, warnings) = render_data_lua(&d, "0.7.3").unwrap();
        assert!(
            !warnings.iter().any(|w| w.contains("photo")),
            "{warnings:?}"
        );
        let sv = parse(lua.as_bytes()).unwrap();
        let list = sv
            .get("GSF_PushedData")
            .unwrap()
            .get("auditBaselines")
            .unwrap();
        let snap = |i: i64| list.get(i).unwrap().get("snapshot").unwrap();
        let int = |i: i64, k: &str| snap(i).get(k).and_then(Value::as_i64);
        assert_eq!(int(1, "played"), Some(51_210));
        assert_eq!(int(1, "playedObserved"), Some(120));
        assert_eq!(int(1, "level"), Some(20));
        for i in [2, 3, 4] {
            for k in ["played", "playedObserved", "level"] {
                assert_eq!(snap(i).get(k), None, "photo {i} : {k}");
            }
            assert_eq!(int(i, "gold"), Some(5), "photo {i} gardée");
        }
    }

    #[test]
    fn collector_proofs_and_received_violations_always_present_and_capped() {
        let (v, _) = to_addon_value(&sample()).unwrap();
        let sv = parse(assignment("GSF_PushedData", &v).as_bytes()).unwrap();
        let root = sv.get("GSF_PushedData").unwrap();
        assert_eq!(root.get("collectorProofs").unwrap().array().count(), 0);
        assert_eq!(root.get("receivedViolations").unwrap().array().count(), 0);

        let mut d = sample();
        let sig = "f".repeat(512);
        d.collector_proofs = (0..25)
            .map(|i| CollectorProof {
                name: format!("p{i}"),
                expires_at: 1,
                sig: sig.clone(),
            })
            .collect();
        d.received_violations = (0..2100).map(|i| format!("v-{i}")).collect();
        let (v, _) = to_addon_value(&d).unwrap();
        let sv = parse(assignment("GSF_PushedData", &v).as_bytes()).unwrap();
        let root = sv.get("GSF_PushedData").unwrap();
        assert_eq!(
            root.get("collectorProofs").unwrap().array().count(),
            MAX_COLLECTOR_PROOFS
        );
        assert_eq!(
            root.get("receivedViolations").unwrap().array().count(),
            MAX_RECEIVED_VIOLATIONS
        );
    }

    #[test]
    fn new_keys_follow_the_contract_json_names() {
        let d: PushedData = serde_json::from_str(
            r#"{"schema":1,"guild_key":"realm#home","collector_proofs":[{"name":"vigie","expires_at":5,"sig":"ab"}],
                "received_violations":["v-1"]}"#,
        )
        .unwrap();
        assert_eq!(d.collector_proofs[0].expires_at, 5);
        assert_eq!(d.received_violations, vec!["v-1".to_owned()]);
    }

    #[test]
    fn data_files_present_requires_every_target() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        assert!(!data_files_present(&[]));
        fs::create_dir_all(a.join(DATA_ADDON_DIR)).unwrap();
        fs::write(a.join(DATA_ADDON_DIR).join("Data.lua"), "x").unwrap();
        assert!(data_files_present(std::slice::from_ref(&a)));
        assert!(!data_files_present(&[a.clone(), b]));
    }

    #[test]
    fn data_lua_round_trips_with_addon_structure() {
        let (src, warnings) = render_data_lua(&sample(), "0.1.0").unwrap();
        assert_eq!(warnings.len(), 2, "{warnings:?}");
        let sv = parse(src.as_bytes()).unwrap();
        let d = sv.get("GSF_PushedData").unwrap();
        assert_eq!(d.get("schema"), Some(&Value::Int(1)));
        assert_eq!(
            d.get("guildKey").and_then(Value::as_str),
            Some("realm#home")
        );
        assert_eq!(d.get("generatedAt"), Some(&Value::Int(1800000000)));
        let bans: Vec<&Value> = d.get("bans").unwrap().array().collect();
        assert_eq!(bans.len(), 2);
        // Tableau séquentiel 1..n uniquement (l'addon refuse les clés chaînes et les trous).
        assert_eq!(d.get("bans").unwrap().as_table().unwrap().len(), 2);
        assert_eq!(bans[0].get("name").and_then(Value::as_str), Some("cheater"));
        assert_eq!(
            bans[0].get("reason").and_then(Value::as_str),
            Some("Or externe \"x\" \\ ]]   fin")
        );
        assert_eq!(bans[0].get("expiresAt"), Some(&Value::Int(1900000000)));
        assert_eq!(
            bans[1].get("name").and_then(Value::as_str),
            Some("Dävin-Fumée")
        );
        assert_eq!(bans[1].get("reason"), None);
        assert_eq!(bans[1].get("expiresAt"), None);
    }

    #[test]
    fn blacklist_entries_and_receipts_reach_the_addon() {
        let mut data = sample();
        data.game_blacklist.push(GameBlacklistEntry {
            target_type: "battle_tag".into(),
            target_key: "fa2ce2459aec6c5c0d72ca5991cd4cfd".into(),
            target_label: "Davin#54321".into(),
            reason: Some("Test".into()),
        });
        data.blacklist_receipts.push(BlacklistReceipt {
            id: "1800000000-12345678".into(),
            status: "applied".into(),
            message: Some("Ban enregistré.".into()),
        });
        let (src, _) = render_data_lua(&data, "0.1.0").unwrap();
        let sv = parse(src.as_bytes()).unwrap();
        let root = sv.get("GSF_PushedData").unwrap();
        let entry = root.get("gameBlacklist").unwrap().array().next().unwrap();
        assert_eq!(
            entry.get("targetLabel").and_then(Value::as_str),
            Some("Davin#54321")
        );
        let receipt = root
            .get("blacklistReceipts")
            .unwrap()
            .array()
            .next()
            .unwrap();
        assert_eq!(
            receipt.get("status").and_then(Value::as_str),
            Some("applied")
        );
    }

    #[test]
    fn limits_follow_the_addon_validation() {
        let mut d = sample();
        d.bans.clear();
        d.bans.push(Ban {
            name: "x".repeat(65),
            reason: None,
            expires_at: None,
        });
        for i in 0..520 {
            d.bans.push(Ban {
                name: format!("n{i:03}"),
                reason: Some("é".repeat(150)),
                expires_at: Some(-1),
            });
        }
        let (v, warnings) = to_addon_value(&d).unwrap();
        let src = assignment("GSF_PushedData", &v);
        let sv = parse(src.as_bytes()).unwrap();
        let bans: Vec<&Value> = sv
            .get("GSF_PushedData")
            .unwrap()
            .get("bans")
            .unwrap()
            .array()
            .collect();
        assert_eq!(bans.len(), MAX_BANS);
        let reason = bans[0].get("reason").and_then(Value::as_str).unwrap();
        assert_eq!(reason.len(), 200);
        assert_eq!(bans[0].get("expiresAt"), None);
        assert_eq!(warnings.len(), 2);
    }

    #[test]
    fn collector_flag_is_present_only_when_true() {
        let mut d = sample();
        d.collector = true;
        let (v, _) = to_addon_value(&d).unwrap();
        let src = assignment("GSF_PushedData", &v);
        let sv = parse(src.as_bytes()).unwrap();
        assert_eq!(
            sv.get("GSF_PushedData").unwrap().get("collector"),
            Some(&Value::Bool(true))
        );

        d.collector = false;
        let (v, _) = to_addon_value(&d).unwrap();
        let src = assignment("GSF_PushedData", &v);
        let sv = parse(src.as_bytes()).unwrap();
        assert_eq!(sv.get("GSF_PushedData").unwrap().get("collector"), None);
    }

    #[test]
    fn collector_change_forces_a_rewrite_once_the_version_changes() {
        let dir = tempfile::tempdir().unwrap();
        let flavor = dir.path().join("_classic_beta_");
        fs::create_dir_all(flavor.join("Interface/AddOns/GuildSelfForever")).unwrap();
        let targets = addon_targets(std::slice::from_ref(&flavor));
        let data_dir = flavor.join("Interface/AddOns").join(DATA_ADDON_DIR);
        let game = FixedGameDetector::new(false);

        let mut d = sample();
        d.version = Some("v1".into());
        d.collector = false;
        write_pushed_data(&targets, &d, &game, "0.1.0").unwrap();
        let lua = fs::read_to_string(data_dir.join("Data.lua")).unwrap();
        assert!(!lua.contains("collector ="));

        // Le site change `version` quand `collector` change (fait partie du calcul).
        d.version = Some("v2".into());
        d.collector = true;
        let (out, _) = write_pushed_data(&targets, &d, &game, "0.1.0").unwrap();
        assert_eq!(out, WriteOutcome::Written(vec![data_dir.clone()]));
        let lua = fs::read_to_string(data_dir.join("Data.lua")).unwrap();
        assert!(lua.contains("collector = true"));
    }

    #[test]
    fn allied_guilds_are_written_and_invalid_entries_filtered() {
        let mut d = sample();
        d.allied_guilds = vec![
            AlliedGuild {
                key: "realm#alt".into(),
                name: "Guilde Alt".into(),
            },
            AlliedGuild {
                key: String::new(),
                name: "Clé vide".into(),
            },
            AlliedGuild {
                key: "realm#bad".into(),
                name: "x".repeat(65),
            },
            AlliedGuild {
                key: "realm#bank".into(),
                name: "Banque".into(),
            },
        ];
        let (v, warnings) = to_addon_value(&d).unwrap();
        assert_eq!(warnings.len(), 4, "{warnings:?}"); // 2 bans + 2 guildes alliées invalides
        let src = assignment("GSF_PushedData", &v);
        let sv = parse(src.as_bytes()).unwrap();
        let allies: Vec<&Value> = sv
            .get("GSF_PushedData")
            .unwrap()
            .get("alliedGuilds")
            .unwrap()
            .array()
            .collect();
        assert_eq!(allies.len(), 2);
        assert_eq!(
            allies[0].get("key").and_then(Value::as_str),
            Some("realm#alt")
        );
        assert_eq!(
            allies[0].get("name").and_then(Value::as_str),
            Some("Guilde Alt")
        );
        assert_eq!(
            allies[1].get("key").and_then(Value::as_str),
            Some("realm#bank")
        );
    }

    #[test]
    fn allied_guilds_empty_by_default_and_capped() {
        let (v, warnings) = to_addon_value(&sample()).unwrap();
        let src = assignment("GSF_PushedData", &v);
        let sv = parse(src.as_bytes()).unwrap();
        assert_eq!(
            sv.get("GSF_PushedData")
                .unwrap()
                .get("alliedGuilds")
                .unwrap()
                .as_table()
                .unwrap()
                .len(),
            0
        );
        assert_eq!(warnings.len(), 2);

        let mut d = sample();
        for i in 0..(MAX_ALLIED_GUILDS + 5) {
            d.allied_guilds.push(AlliedGuild {
                key: format!("realm#guild{i}"),
                name: format!("Guilde {i}"),
            });
        }
        let (v, warnings) = to_addon_value(&d).unwrap();
        let src = assignment("GSF_PushedData", &v);
        let sv = parse(src.as_bytes()).unwrap();
        assert_eq!(
            sv.get("GSF_PushedData")
                .unwrap()
                .get("alliedGuilds")
                .unwrap()
                .as_table()
                .unwrap()
                .len(),
            MAX_ALLIED_GUILDS
        );
        assert!(warnings.iter().any(|w| w.contains("guilde(s) alliée(s)")));
    }

    #[test]
    fn federation_absent_by_default() {
        let (v, warnings) = to_addon_value(&sample()).unwrap();
        let src = assignment("GSF_PushedData", &v);
        let sv = parse(src.as_bytes()).unwrap();
        assert_eq!(sv.get("GSF_PushedData").unwrap().get("federation"), None);
        assert_eq!(warnings.len(), 2, "{warnings:?}");
    }

    #[test]
    fn federation_is_written_and_invalid_entries_filtered() {
        let mut d = sample();
        d.federation = Some(Federation {
            channel: "gsf0a1b2c3d4e".into(),
            password: "0123456789abcdef".into(),
            guilds: vec![
                FederationGuild {
                    key: "classicbetapve#beta self forever".into(),
                    name: "Beta Self Forever".into(),
                    kind: "principale".into(),
                    mode: "classicbetapve".into(),
                },
                FederationGuild {
                    key: String::new(),
                    name: "Clé vide".into(),
                    kind: "secondaire".into(),
                    mode: "classicbetapve".into(),
                },
                FederationGuild {
                    key: "realm#bad-name".into(),
                    name: "x".repeat(65),
                    kind: "alt".into(),
                    mode: "classicbetapve".into(),
                },
                FederationGuild {
                    key: "realm#bad-type".into(),
                    name: "Nom".into(),
                    kind: "x".repeat(33),
                    mode: "classicbetapve".into(),
                },
                FederationGuild {
                    key: "realm#bad-mode".into(),
                    name: "Nom".into(),
                    kind: "banque".into(),
                    mode: "a|b".into(),
                },
                FederationGuild {
                    key: "realm#bank".into(),
                    name: "Banque".into(),
                    kind: "banque".into(),
                    mode: "classicbetapve".into(),
                },
            ],
        });
        let (v, warnings) = to_addon_value(&d).unwrap();
        assert_eq!(warnings.len(), 6, "{warnings:?}"); // 2 bans + 4 guildes de fédération invalides
        let src = assignment("GSF_PushedData", &v);
        let sv = parse(src.as_bytes()).unwrap();
        let fed = sv.get("GSF_PushedData").unwrap().get("federation").unwrap();
        assert_eq!(
            fed.get("channel").and_then(Value::as_str),
            Some("gsf0a1b2c3d4e")
        );
        assert_eq!(
            fed.get("password").and_then(Value::as_str),
            Some("0123456789abcdef")
        );
        let guilds: Vec<&Value> = fed.get("guilds").unwrap().array().collect();
        assert_eq!(guilds.len(), 2);
        assert_eq!(
            guilds[0].get("key").and_then(Value::as_str),
            Some("classicbetapve#beta self forever")
        );
        assert_eq!(
            guilds[0].get("name").and_then(Value::as_str),
            Some("Beta Self Forever")
        );
        assert_eq!(
            guilds[0].get("type").and_then(Value::as_str),
            Some("principale")
        );
        assert_eq!(
            guilds[0].get("mode").and_then(Value::as_str),
            Some("classicbetapve")
        );
        assert_eq!(
            guilds[1].get("key").and_then(Value::as_str),
            Some("realm#bank")
        );
    }

    #[test]
    fn federation_dropped_whole_on_invalid_channel_or_password() {
        let mut d = sample();
        d.federation = Some(Federation {
            channel: "GSF0A1B2C3D4E".into(), // majuscules refusées
            password: "0123456789abcdef".into(),
            guilds: vec![FederationGuild {
                key: "realm#guild".into(),
                name: "Nom".into(),
                kind: "principale".into(),
                mode: "classicbetapve".into(),
            }],
        });
        let (v, warnings) = to_addon_value(&d).unwrap();
        let src = assignment("GSF_PushedData", &v);
        let sv = parse(src.as_bytes()).unwrap();
        assert_eq!(sv.get("GSF_PushedData").unwrap().get("federation"), None);
        assert!(warnings.iter().any(|w| w.contains("fédération ignorée")));

        let mut d = sample();
        d.federation = Some(Federation {
            channel: "gsf0a1b2c3d4e".into(),
            password: "mot de passe".into(), // espaces refusés
            guilds: vec![],
        });
        let (v, _) = to_addon_value(&d).unwrap();
        let src = assignment("GSF_PushedData", &v);
        let sv = parse(src.as_bytes()).unwrap();
        assert_eq!(sv.get("GSF_PushedData").unwrap().get("federation"), None);

        let mut d = sample();
        d.federation = Some(Federation {
            channel: String::new(), // vide refusé
            password: "0123456789abcdef".into(),
            guilds: vec![],
        });
        let (v, _) = to_addon_value(&d).unwrap();
        let src = assignment("GSF_PushedData", &v);
        let sv = parse(src.as_bytes()).unwrap();
        assert_eq!(sv.get("GSF_PushedData").unwrap().get("federation"), None);
    }

    #[test]
    fn federation_guilds_are_capped() {
        let mut d = sample();
        let mut guilds = Vec::new();
        for i in 0..(MAX_FEDERATION_GUILDS + 5) {
            guilds.push(FederationGuild {
                key: format!("realm#guild{i}"),
                name: format!("Guilde {i}"),
                kind: "secondaire".into(),
                mode: "classicbetapve".into(),
            });
        }
        d.federation = Some(Federation {
            channel: "gsf0a1b2c3d4e".into(),
            password: "0123456789abcdef".into(),
            guilds,
        });
        let (v, warnings) = to_addon_value(&d).unwrap();
        let src = assignment("GSF_PushedData", &v);
        let sv = parse(src.as_bytes()).unwrap();
        let count = sv
            .get("GSF_PushedData")
            .unwrap()
            .get("federation")
            .unwrap()
            .get("guilds")
            .unwrap()
            .as_table()
            .unwrap()
            .len();
        assert_eq!(count, MAX_FEDERATION_GUILDS);
        assert!(warnings
            .iter()
            .any(|w| w.contains("guilde(s) de fédération")));
    }

    #[test]
    fn federation_strings_are_escaped_through_the_serializer() {
        let mut d = sample();
        d.federation = Some(Federation {
            channel: "gsf0a1b2c3d4e".into(),
            password: "0123456789abcdef".into(),
            guilds: vec![FederationGuild {
                key: "classicbetapve#beta self forever".into(),
                name: "Dävin \"Fumée\" Guild".into(),
                kind: "principale".into(),
                mode: "classicbetapve".into(),
            }],
        });
        let (v, _) = to_addon_value(&d).unwrap();
        let src = assignment("GSF_PushedData", &v);
        let sv = parse(src.as_bytes()).unwrap();
        let guild = sv
            .get("GSF_PushedData")
            .unwrap()
            .get("federation")
            .unwrap()
            .get("guilds")
            .unwrap()
            .array()
            .next()
            .unwrap();
        assert_eq!(
            guild.get("name").and_then(Value::as_str),
            Some("Dävin \"Fumée\" Guild")
        );
    }

    #[test]
    fn requests_settings_absent_by_default_and_written_when_valid() {
        let (v, warnings) = to_addon_value(&sample()).unwrap();
        let src = assignment("GSF_PushedData", &v);
        let sv = parse(src.as_bytes()).unwrap();
        assert_eq!(sv.get("GSF_PushedData").unwrap().get("requests"), None);
        assert_eq!(warnings.len(), 2, "{warnings:?}");

        let mut d = sample();
        d.requests = Some(RequestSettings {
            expiry_days: Some(7),
        });
        let (v, warnings) = to_addon_value(&d).unwrap();
        let src = assignment("GSF_PushedData", &v);
        let sv = parse(src.as_bytes()).unwrap();
        assert_eq!(
            sv.get("GSF_PushedData")
                .unwrap()
                .get("requests")
                .unwrap()
                .get("expiryDays"),
            Some(&Value::Int(7))
        );
        assert_eq!(warnings.len(), 2, "{warnings:?}");
    }

    #[test]
    fn requests_settings_dropped_when_expiry_days_out_of_range_or_absent() {
        for invalid in [None, Some(0), Some(61), Some(-1)] {
            let mut d = sample();
            d.requests = Some(RequestSettings {
                expiry_days: invalid,
            });
            let (v, warnings) = to_addon_value(&d).unwrap();
            let src = assignment("GSF_PushedData", &v);
            let sv = parse(src.as_bytes()).unwrap();
            assert_eq!(
                sv.get("GSF_PushedData").unwrap().get("requests"),
                None,
                "{invalid:?}"
            );
            assert!(
                warnings
                    .iter()
                    .any(|w| w.contains("réglage des demandes ignoré")),
                "{invalid:?} : {warnings:?}"
            );
        }
    }

    #[test]
    fn cleared_players_are_written_and_invalid_entries_filtered() {
        let mut d = sample();
        d.cleared = vec![
            Cleared {
                name: "Ann Bee".into(),
                at: 1_700_000_000,
            },
            Cleared {
                name: "bad|name".into(),
                at: 1_700_000_000,
            },
            Cleared {
                name: "x".repeat(65),
                at: 1_700_000_000,
            },
            Cleared {
                name: "Cid".into(),
                at: -1,
            },
        ];
        let (v, warnings) = to_addon_value(&d).unwrap();
        assert_eq!(warnings.len(), 5, "{warnings:?}"); // 2 bans + 3 joueurs blanchis invalides
        let src = assignment("GSF_PushedData", &v);
        let sv = parse(src.as_bytes()).unwrap();
        let cleared: Vec<&Value> = sv
            .get("GSF_PushedData")
            .unwrap()
            .get("cleared")
            .unwrap()
            .array()
            .collect();
        assert_eq!(cleared.len(), 1);
        assert_eq!(
            cleared[0].get("name").and_then(Value::as_str),
            Some("Ann Bee")
        );
        assert_eq!(cleared[0].get("at"), Some(&Value::Int(1_700_000_000)));
    }

    #[test]
    fn tombola_is_written_with_announce_and_prizes_and_invalid_entries_filtered() {
        let mut d = sample();
        d.tombola = Some(
            serde_json::from_str(
                r#"{"id":12,"announce":{"text":"Tombola de Brindille en cours : inscris-toi sur le Discord.","ends_at":1800000000},
                    "to_send":[
                      {"lot_id":34,"kind":"item","label":"Cape de factionnaire","item_id":2059,"count":1,"guild_key":"realm#home",
                       "location":"Mirabel · Banque de guilde — onglet 2","winner":"Brindefer","drawn_at":1800000100},
                      {"lot_id":35,"kind":"free","label":"500 pièces d'or","item_id":null,"count":1,"location":"Lot du streamer","winner":"Brumelle"},
                      {"lot_id":36,"label":"Bad","winner":"bad|name"},
                      {"lot_id":37,"label":"Bad kind","winner":"Ok","kind":"boost"}
                    ]}"#,
            )
            .unwrap(),
        );
        let (v, warnings) = to_addon_value(&d).unwrap();
        assert_eq!(warnings.len(), 4, "{warnings:?}"); // 2 bans + 2 lots invalides
        let src = assignment("GSF_PushedData", &v);
        let sv = parse(src.as_bytes()).unwrap();
        let t = sv.get("GSF_PushedData").unwrap().get("tombola").unwrap();
        assert_eq!(t.get("id"), Some(&Value::Int(12)));
        let announce = t.get("announce").unwrap();
        assert_eq!(announce.get("endsAt"), Some(&Value::Int(1_800_000_000)));
        assert!(announce
            .get("text")
            .and_then(Value::as_str)
            .unwrap()
            .starts_with("Tombola de Brindille"));
        let prizes: Vec<&Value> = t.get("toSend").unwrap().array().collect();
        assert_eq!(prizes.len(), 2);
        assert_eq!(
            prizes[0].get("winner").and_then(Value::as_str),
            Some("Brindefer")
        );
        assert_eq!(prizes[0].get("itemId"), Some(&Value::Int(2059)));
        assert_eq!(prizes[0].get("drawnAt"), Some(&Value::Int(1_800_000_100)));
        assert_eq!(prizes[1].get("kind").and_then(Value::as_str), Some("free"));
        assert_eq!(prizes[1].get("itemId"), None);
        assert_eq!(
            prizes[1].get("label").and_then(Value::as_str),
            Some("500 pièces d'or")
        );
    }

    #[test]
    fn tombola_is_omitted_when_absent_or_empty_and_announce_bounded() {
        let (v, _) = to_addon_value(&sample()).unwrap();
        let sv = parse(assignment("GSF_PushedData", &v).as_bytes()).unwrap();
        assert!(sv.get("GSF_PushedData").unwrap().get("tombola").is_none());

        let mut d = sample();
        d.tombola = Some(Tombola {
            id: Some(1),
            announce: Some(TombolaAnnounce {
                text: "é".repeat(71),
                ends_at: 1,
            }), // 142 octets
            to_send: vec![],
        });
        let (v, warnings) = to_addon_value(&d).unwrap();
        assert!(
            warnings.iter().any(|w| w.contains("annonce")),
            "{warnings:?}"
        );
        let sv = parse(assignment("GSF_PushedData", &v).as_bytes()).unwrap();
        assert!(sv.get("GSF_PushedData").unwrap().get("tombola").is_none());
    }

    #[test]
    fn tombola_public_is_written_for_everyone_and_invalid_entries_filtered() {
        let mut d = sample();
        d.tombola_public = Some(
            serde_json::from_str(
                r#"{"id":12,"status":"open","streamer":"Brindille","twitch":"brindille_tv","draw_at":1800000000,
                    "lots":[
                      {"lot_id":34,"kind":"item","label":"Cape de factionnaire","item_id":2059,"count":1,"details":null,
                       "min_level":25,"drawn":true,"entered":true,"won":true},
                      {"lot_id":35,"kind":"free","label":"500 pièces d'or","item_id":null,"count":1,
                       "details":"Remis en jeu par Brindille","min_level":null,"drawn":false,"entered":false,"won":false},
                      {"lot_id":36,"kind":"item","label":"bad|label","count":1},
                      {"lot_id":37,"kind":"boost","label":"Bad kind","count":1},
                      {"lot_id":38,"kind":"item","label":null,"count":1}
                    ],
                    "won":[
                      {"lot_id":34,"kind":"item","label":"Cape de factionnaire","item_id":2059,"count":1,"details":null,
                       "character":"Brindefer","drawn_at":1800000100,"sent":false},
                      {"lot_id":0,"kind":"item","label":"Bad id","count":1}
                    ]}"#,
            )
            .unwrap(),
        );
        let (v, warnings) = to_addon_value(&d).unwrap();
        assert_eq!(warnings.len(), 6, "{warnings:?}"); // 2 bans + 3 lots + 1 gain invalides
        let sv = parse(assignment("GSF_PushedData", &v).as_bytes()).unwrap();
        let t = sv
            .get("GSF_PushedData")
            .unwrap()
            .get("tombolaPublic")
            .unwrap();
        assert_eq!(t.get("id"), Some(&Value::Int(12)));
        assert_eq!(t.get("status").and_then(Value::as_str), Some("open"));
        assert_eq!(t.get("streamer").and_then(Value::as_str), Some("Brindille"));
        assert_eq!(
            t.get("twitch").and_then(Value::as_str),
            Some("brindille_tv")
        );
        assert_eq!(t.get("drawAt"), Some(&Value::Int(1_800_000_000)));
        let lots: Vec<&Value> = t.get("lots").unwrap().array().collect();
        assert_eq!(lots.len(), 2);
        assert_eq!(lots[0].get("minLevel"), Some(&Value::Int(25)));
        assert_eq!(lots[0].get("entered"), Some(&Value::Bool(true)));
        assert_eq!(lots[0].get("won"), Some(&Value::Bool(true)));
        assert_eq!(lots[1].get("entered"), None);
        assert_eq!(lots[1].get("itemId"), None);
        assert_eq!(
            lots[1].get("details").and_then(Value::as_str),
            Some("Remis en jeu par Brindille")
        );
        let won: Vec<&Value> = t.get("won").unwrap().array().collect();
        assert_eq!(won.len(), 1);
        assert_eq!(
            won[0].get("character").and_then(Value::as_str),
            Some("Brindefer")
        );
        assert_eq!(won[0].get("drawnAt"), Some(&Value::Int(1_800_000_100)));
        assert_eq!(won[0].get("sent"), None);
    }

    #[test]
    fn tombola_public_is_omitted_when_empty_and_bad_twitch_or_status_dropped() {
        let (v, _) = to_addon_value(&sample()).unwrap();
        let sv = parse(assignment("GSF_PushedData", &v).as_bytes()).unwrap();
        assert!(sv
            .get("GSF_PushedData")
            .unwrap()
            .get("tombolaPublic")
            .is_none());

        let mut d = sample();
        d.tombola_public = Some(TombolaPublic {
            id: None,
            status: None,
            streamer: None,
            twitch: None,
            draw_at: None,
            lots: vec![],
            won: vec![],
        });
        let (v, _) = to_addon_value(&d).unwrap();
        let sv = parse(assignment("GSF_PushedData", &v).as_bytes()).unwrap();
        assert!(sv
            .get("GSF_PushedData")
            .unwrap()
            .get("tombolaPublic")
            .is_none());

        d.tombola_public = Some(
            serde_json::from_str(
                r#"{"id":3,"status":"done","streamer":"Bad|name","twitch":"https://twitch.tv/x","lots":[],"won":[]}"#,
            )
            .unwrap(),
        );
        let (v, _) = to_addon_value(&d).unwrap();
        let sv = parse(assignment("GSF_PushedData", &v).as_bytes()).unwrap();
        let t = sv
            .get("GSF_PushedData")
            .unwrap()
            .get("tombolaPublic")
            .unwrap();
        assert_eq!(t.get("id"), Some(&Value::Int(3)));
        assert!(
            t.get("status").is_none() && t.get("streamer").is_none() && t.get("twitch").is_none()
        );
    }

    #[test]
    fn tombola_public_lots_and_won_are_capped() {
        let lot = |id: i64| TombolaPublicLot {
            lot_id: id,
            kind: Some("item".into()),
            label: format!("Objet {id}"),
            item_id: Some(id),
            count: 1,
            details: None,
            min_level: None,
            drawn: false,
            entered: false,
            won: false,
            character: None,
            drawn_at: None,
            sent: false,
        };
        let mut d = sample();
        d.tombola_public = Some(TombolaPublic {
            id: Some(1),
            status: Some("open".into()),
            streamer: None,
            twitch: None,
            draw_at: None,
            lots: (1..=60).map(lot).collect(),
            won: (1..=25).map(lot).collect(),
        });
        let (v, warnings) = to_addon_value(&d).unwrap();
        assert!(
            warnings.iter().any(|w| w.contains("10 lot")),
            "{warnings:?}"
        );
        assert!(
            warnings.iter().any(|w| w.contains("5 gain")),
            "{warnings:?}"
        );
        let sv = parse(assignment("GSF_PushedData", &v).as_bytes()).unwrap();
        let t = sv
            .get("GSF_PushedData")
            .unwrap()
            .get("tombolaPublic")
            .unwrap();
        assert_eq!(
            t.get("lots").unwrap().array().count(),
            MAX_TOMBOLA_PUBLIC_LOTS
        );
        assert_eq!(
            t.get("won").unwrap().array().count(),
            MAX_TOMBOLA_PUBLIC_WON
        );
    }

    #[test]
    fn cleared_empty_by_default_and_capped() {
        let (v, warnings) = to_addon_value(&sample()).unwrap();
        let src = assignment("GSF_PushedData", &v);
        let sv = parse(src.as_bytes()).unwrap();
        assert_eq!(
            sv.get("GSF_PushedData")
                .unwrap()
                .get("cleared")
                .unwrap()
                .as_table()
                .unwrap()
                .len(),
            0
        );
        assert_eq!(warnings.len(), 2);

        let mut d = sample();
        for i in 0..(MAX_CLEARED + 5) {
            d.cleared.push(Cleared {
                name: format!("Joueur{i}"),
                at: 1_700_000_000,
            });
        }
        let (v, warnings) = to_addon_value(&d).unwrap();
        let src = assignment("GSF_PushedData", &v);
        let sv = parse(src.as_bytes()).unwrap();
        assert_eq!(
            sv.get("GSF_PushedData")
                .unwrap()
                .get("cleared")
                .unwrap()
                .as_table()
                .unwrap()
                .len(),
            MAX_CLEARED
        );
        assert!(warnings.iter().any(|w| w.contains("joueur(s) blanchi(s)")));
    }

    #[test]
    fn banned_identities_always_present_and_invalid_entries_filtered() {
        let (v, warnings) = to_addon_value(&sample()).unwrap();
        let src = assignment("GSF_PushedData", &v);
        let sv = parse(src.as_bytes()).unwrap();
        assert_eq!(
            sv.get("GSF_PushedData")
                .unwrap()
                .get("bannedIdentities")
                .unwrap()
                .as_table()
                .unwrap()
                .len(),
            0,
            "toujours présente, même vide"
        );
        assert_eq!(warnings.len(), 2, "{warnings:?}"); // 2 bans invalides de sample()

        let mut d = sample();
        d.banned_identities = vec![
            BannedIdentity {
                bt: Some("fa2ce2459aec6c5c0d72ca5991cd4cfd".into()),
                acct: Some("0123456789abcdef".into()),
                reason: Some("Or externe".into()),
                expires_at: Some(1_900_000_000),
            },
            // acct seul valide, bt absent.
            BannedIdentity {
                bt: None,
                acct: Some("0123456789abcdef".into()),
                reason: None,
                expires_at: None,
            },
            // bt invalide (majuscules) et acct invalide (longueur) : écartée.
            BannedIdentity {
                bt: Some("FA2CE2459AEC6C5C0D72CA5991CD4CFD".into()),
                acct: Some("short".into()),
                reason: None,
                expires_at: None,
            },
            // ni bt ni acct : écartée.
            BannedIdentity {
                bt: None,
                acct: None,
                reason: Some("rien de valide".into()),
                expires_at: None,
            },
        ];
        let (v, warnings) = to_addon_value(&d).unwrap();
        assert_eq!(warnings.len(), 4, "{warnings:?}"); // 2 bans + 2 identités invalides
        assert!(warnings
            .iter()
            .any(|w| w.contains("identité bannie ignorée")));
        let src = assignment("GSF_PushedData", &v);
        let sv = parse(src.as_bytes()).unwrap();
        let entries: Vec<&Value> = sv
            .get("GSF_PushedData")
            .unwrap()
            .get("bannedIdentities")
            .unwrap()
            .array()
            .collect();
        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries[0].get("bt").and_then(Value::as_str),
            Some("fa2ce2459aec6c5c0d72ca5991cd4cfd")
        );
        assert_eq!(
            entries[0].get("acct").and_then(Value::as_str),
            Some("0123456789abcdef")
        );
        assert_eq!(
            entries[0].get("reason").and_then(Value::as_str),
            Some("Or externe")
        );
        assert_eq!(
            entries[0].get("expiresAt"),
            Some(&Value::Int(1_900_000_000))
        );
        assert_eq!(entries[1].get("bt"), None, "bt absent, non écrit");
        assert_eq!(
            entries[1].get("acct").and_then(Value::as_str),
            Some("0123456789abcdef")
        );
        assert_eq!(entries[1].get("reason"), None);
        assert_eq!(entries[1].get("expiresAt"), None);
    }

    #[test]
    fn banned_identities_reason_follows_ban_rules_and_is_capped() {
        let mut d = sample();
        d.banned_identities.push(BannedIdentity {
            bt: Some("fa2ce2459aec6c5c0d72ca5991cd4cfd".into()),
            acct: None,
            reason: Some("é".repeat(250)),
            expires_at: Some(-1),
        });
        for i in 0..(MAX_BANNED_IDENTITIES + 5) {
            d.banned_identities.push(BannedIdentity {
                bt: None,
                acct: Some(format!("{i:016x}")),
                reason: None,
                expires_at: None,
            });
        }
        let (v, warnings) = to_addon_value(&d).unwrap();
        let src = assignment("GSF_PushedData", &v);
        let sv = parse(src.as_bytes()).unwrap();
        let entries: Vec<&Value> = sv
            .get("GSF_PushedData")
            .unwrap()
            .get("bannedIdentities")
            .unwrap()
            .array()
            .collect();
        assert_eq!(entries.len(), MAX_BANNED_IDENTITIES);
        let reason = entries[0].get("reason").and_then(Value::as_str).unwrap();
        assert_eq!(
            reason.len(),
            MAX_REASON_BYTES,
            "même troncature que les bans"
        );
        assert_eq!(
            entries[0].get("expiresAt"),
            None,
            "expiresAt négatif écarté, comme pour un ban"
        );
        assert!(warnings.iter().any(|w| w.contains("identité(s) bannie(s)")));
    }

    #[test]
    fn invalid_schema_or_guild_key_is_refused() {
        let mut d = sample();
        d.schema = 2;
        assert!(matches!(to_addon_value(&d), Err(PushedError::Schema(2))));
        let mut d = sample();
        d.guild_key = String::new();
        assert!(matches!(to_addon_value(&d), Err(PushedError::GuildKey)));
    }

    #[test]
    fn requests_and_cleared_json_field_names_match_the_contract() {
        // Forme exacte attendue de GET /api/v1/bridge/pushed-data (contrat
        // 2026-10-15, section C) : `requests: {expiry_days}`, `cleared:
        // [{name, at}]`, en snake_case comme `allied_guilds`/`federation`.
        let d: PushedData = serde_json::from_str(
            r#"{"schema":1,"guild_key":"realm#home",
                "requests":{"expiry_days":7},
                "cleared":[{"name":"Ann Bee","at":1700000000}]}"#,
        )
        .unwrap();
        assert_eq!(
            d.requests,
            Some(RequestSettings {
                expiry_days: Some(7)
            })
        );
        assert_eq!(
            d.cleared,
            vec![Cleared {
                name: "Ann Bee".into(),
                at: 1_700_000_000,
            }]
        );
    }

    fn returns_table(d: &PushedData) -> (Option<Value>, Vec<String>) {
        let (v, warnings) = to_addon_value(d).unwrap();
        let src = assignment("GSF_PushedData", &v);
        let sv = parse(src.as_bytes()).unwrap();
        (
            sv.get("GSF_PushedData").unwrap().get("returns").cloned(),
            warnings,
        )
    }

    #[test]
    fn malformed_returns_never_drop_the_rest_of_the_pushed_data() {
        let json = r#"{"schema":1,"guild_key":"realm#home","bans":[{"name":"Cheater"}],
            "returns":{"allowed":[{"name":null},{"name":"Ann Bee","bt":123},{"name":"Bob","acct":"0123456789abcdef","since":"x"},
                {"name":"Cid","acct":"0123456789abcdef"}],
              "validated":[{"name":"Ann Bee","at":null},{"name":"Cid","at":5}]}}"#;
        let d: PushedData = serde_json::from_str(json).unwrap();
        assert_eq!(d.bans.len(), 1);
        let r = d.returns.unwrap();
        assert_eq!(r.allowed.len(), 1);
        assert_eq!(r.allowed[0].name, "Cid");
        assert_eq!(r.validated.len(), 1);
        assert_eq!(r.validated[0].at, 5);
        for bad in [r#""x""#, "12", "[]", "null"] {
            let json = format!(
                r#"{{"schema":1,"guild_key":"realm#home","bans":[{{"name":"Cheater"}}],"returns":{bad}}}"#
            );
            let d: PushedData = serde_json::from_str(&json).unwrap();
            assert!(d.returns.is_none() && d.bans.len() == 1, "{bad}");
        }
    }

    #[test]
    fn returns_absent_when_the_site_does_not_send_them() {
        let (r, _) = returns_table(&sample());
        assert!(r.is_none());
        let mut d = sample();
        d.returns = Some(Returns::default());
        let (r, _) = returns_table(&d);
        let r = r.unwrap();
        assert_eq!(r.get("allowed").unwrap().as_table().unwrap().len(), 0);
        assert_eq!(r.get("validated").unwrap().as_table().unwrap().len(), 0);
    }

    #[test]
    fn returns_are_written_and_invalid_entries_filtered() {
        let bt = "a".repeat(32);
        let acct = "0123456789abcdef".to_owned();
        let mut d = sample();
        d.returns = Some(Returns {
            allowed: vec![
                ReturnAllowed {
                    name: " Ann Bee ".into(),
                    bt: Some(bt.clone()),
                    acct: Some(acct.clone()),
                    reason: Some("inactivity".into()),
                    since: Some(1_800_000_000),
                },
                // bt invalide mais acct valide : gardée, sans bt ; motif inconnu et date négative omis.
                ReturnAllowed {
                    name: "Bob".into(),
                    bt: Some("A".repeat(32)),
                    acct: Some(acct.clone()),
                    reason: Some("kick".into()),
                    since: Some(-1),
                },
                // aucune identité valide : écartée.
                ReturnAllowed {
                    name: "Carl".into(),
                    bt: Some("abc".into()),
                    acct: None,
                    reason: Some("absence".into()),
                    since: None,
                },
                ReturnAllowed {
                    name: "bad|name".into(),
                    bt: Some(bt.clone()),
                    acct: None,
                    reason: None,
                    since: None,
                },
            ],
            validated: vec![
                ReturnValidated {
                    name: "Ann Bee".into(),
                    at: 1_800_100_000,
                },
                ReturnValidated {
                    name: "Bob".into(),
                    at: -5,
                },
                ReturnValidated {
                    name: "  ".into(),
                    at: 1,
                },
            ],
        });
        let (r, warnings) = returns_table(&d);
        let r = r.unwrap();
        let allowed: Vec<&Value> = r.get("allowed").unwrap().array().collect();
        assert_eq!(allowed.len(), 2);
        assert_eq!(
            allowed[0].get("name").and_then(Value::as_str),
            Some("Ann Bee")
        );
        assert_eq!(
            allowed[0].get("bt").and_then(Value::as_str),
            Some(bt.as_str())
        );
        assert_eq!(
            allowed[0].get("acct").and_then(Value::as_str),
            Some(acct.as_str())
        );
        assert_eq!(
            allowed[0].get("reason").and_then(Value::as_str),
            Some("inactivity")
        );
        assert_eq!(allowed[0].get("since"), Some(&Value::Int(1_800_000_000)));
        assert_eq!(allowed[1].get("name").and_then(Value::as_str), Some("Bob"));
        assert!(allowed[1].get("bt").is_none());
        assert!(allowed[1].get("reason").is_none());
        assert!(allowed[1].get("since").is_none());
        let validated: Vec<&Value> = r.get("validated").unwrap().array().collect();
        assert_eq!(validated.len(), 1);
        assert_eq!(
            validated[0].get("name").and_then(Value::as_str),
            Some("Ann Bee")
        );
        assert_eq!(validated[0].get("at"), Some(&Value::Int(1_800_100_000)));
        assert!(warnings.iter().any(|w| w.contains("identité invalide")));
        assert!(warnings.iter().any(|w| w.contains("retour validé ignoré")));
    }

    #[test]
    fn returns_lists_are_capped() {
        let mut r = Returns::default();
        for i in 0..(MAX_RETURNS + 3) {
            r.allowed.push(ReturnAllowed {
                name: format!("Joueur{i}"),
                bt: None,
                acct: Some("0123456789abcdef".into()),
                reason: None,
                since: None,
            });
            r.validated.push(ReturnValidated {
                name: format!("Joueur{i}"),
                at: 1,
            });
        }
        let mut d = sample();
        d.returns = Some(r);
        let (r, warnings) = returns_table(&d);
        let r = r.unwrap();
        assert_eq!(
            r.get("allowed").unwrap().as_table().unwrap().len(),
            MAX_RETURNS
        );
        assert_eq!(
            r.get("validated").unwrap().as_table().unwrap().len(),
            MAX_RETURNS
        );
        assert!(warnings.iter().any(|w| w.contains("retour(s) autorisé(s)")));
        assert!(warnings.iter().any(|w| w.contains("retour(s) validé(s)")));
    }

    #[test]
    fn returns_json_field_names_match_the_contract() {
        let d: PushedData = serde_json::from_str(
            r#"{"schema":1,"guild_key":"realm#home",
                "returns":{"allowed":[{"name":"Ann Bee","bt":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                  "acct":"0123456789abcdef","reason":"absence","since":1800000000}],
                  "validated":[{"name":"Ann Bee","at":1800100000}]}}"#,
        )
        .unwrap();
        let r = d.returns.unwrap();
        assert_eq!(r.allowed[0].reason.as_deref(), Some("absence"));
        assert_eq!(r.allowed[0].since, Some(1_800_000_000));
        assert_eq!(r.allowed[0].acct.as_deref(), Some("0123456789abcdef"));
        assert_eq!(r.validated[0].at, 1_800_100_000);
        // Liste manquante : vide, jamais une erreur.
        let d: PushedData =
            serde_json::from_str(r#"{"schema":1,"guild_key":"realm#home","returns":{}}"#).unwrap();
        assert_eq!(d.returns, Some(Returns::default()));
    }

    #[test]
    fn toc_declares_interface_and_data_file() {
        let toc = render_toc("1.2.3");
        assert!(toc.starts_with("## Interface: 16001\n"));
        assert!(toc.contains("## Title: GuildSelfForever_Data\n"));
        assert!(toc.contains("## Notes: "));
        assert!(toc.ends_with("\nData.lua\n"));
    }

    #[test]
    fn writes_only_when_game_is_closed() {
        let dir = tempfile::tempdir().unwrap();
        let flavor = dir.path().join("_classic_beta_");
        fs::create_dir_all(flavor.join("Interface/AddOns/GuildSelfForever")).unwrap();
        fs::create_dir_all(dir.path().join("_retail_/Interface/AddOns")).unwrap();
        let targets = addon_targets(&[flavor.clone(), dir.path().join("_retail_")]);
        assert_eq!(targets, vec![flavor.join("Interface/AddOns")]);
        let data_dir = flavor.join("Interface/AddOns").join(DATA_ADDON_DIR);

        let game = FixedGameDetector::new(true);
        let (out, _) = write_pushed_data(&targets, &sample(), &game, "0.1.0").unwrap();
        assert_eq!(out, WriteOutcome::GameRunning);
        assert!(!data_dir.exists(), "rien n'est écrit jeu lancé");

        game.set(false);
        let (out, _) = write_pushed_data(&targets, &sample(), &game, "0.1.0").unwrap();
        assert_eq!(out, WriteOutcome::Written(vec![data_dir.clone()]));
        let toc = fs::read_to_string(data_dir.join("GuildSelfForever_Data.toc")).unwrap();
        assert!(toc.contains("Data.lua"));
        let lua = fs::read(data_dir.join("Data.lua")).unwrap();
        assert!(parse(&lua).unwrap().get("GSF_PushedData").is_some());

        let (out, _) = write_pushed_data(&targets, &sample(), &game, "0.1.0").unwrap();
        assert_eq!(out, WriteOutcome::Unchanged);
        // Même version, nouvel horodatage (le site le change à chaque appel) :
        // rien n'est réécrit, même jeu lancé.
        let mut same = sample();
        same.generated_at = Some(1800009999);
        game.set(true);
        let (out, _) = write_pushed_data(&targets, &same, &game, "0.1.0").unwrap();
        assert_eq!(out, WriteOutcome::Unchanged);
        // Nouvelle version : réécrite une fois le jeu fermé.
        same.version = Some("def456".into());
        let (out, _) = write_pushed_data(&targets, &same, &game, "0.1.0").unwrap();
        assert_eq!(out, WriteOutcome::GameRunning);
        game.set(false);
        let (out, _) = write_pushed_data(&targets, &same, &game, "0.1.0").unwrap();
        assert_eq!(out, WriteOutcome::Written(vec![data_dir.clone()]));
        let lua = fs::read_to_string(data_dir.join("Data.lua")).unwrap();
        assert!(lua.contains("generatedAt = 1800009999"));
        let mut entries: Vec<_> = fs::read_dir(&data_dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        entries.sort();
        assert_eq!(entries, ["Data.lua", "GuildSelfForever_Data.toc"]);

        let (out, _) = write_pushed_data(&[], &sample(), &game, "0.1.0").unwrap();
        assert_eq!(out, WriteOutcome::NoTarget);
    }
}
