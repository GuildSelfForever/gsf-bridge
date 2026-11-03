//! Construction de [`GuildData`] à partir des SavedVariables parsées.

use std::collections::{BTreeMap, BTreeSet};

use super::names::{guild_key_from_parts, normalize_guild_key, CharacterRef, Resolver};
use super::wire::{decode_audit_cleanup_row, decode_request_line};
use super::*;
use crate::sv::{Key, SavedVariables, Value};

// --- Accès tolérants aux champs ------------------------------------------

fn text(v: &Value, k: &str) -> Option<String> {
    v.get(k)?.as_str_lossy().map(|s| s.into_owned())
}

fn int(v: &Value, k: &str) -> Option<i64> {
    v.get(k)?.as_i64()
}

fn boolean(v: &Value, k: &str) -> Option<bool> {
    v.get(k)?.as_bool()
}

/// Champ hexadécimal minuscule de longueur exacte (`bt`/`acct`, contrat
/// 2026-10-17) : `None` si absent ou de forme invalide (champ écarté seul,
/// l'entrée reste).
fn hex_field(v: &Value, k: &str, len: usize) -> Option<String> {
    let s = text(v, k)?;
    (s.len() == len
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
    .then_some(s)
}

/// `btag` en clair (complément 2026-10-17 b `IDB`) : `None` si absent ou de
/// forme invalide (champ seul écarté, l'entrée reste). Gardé seulement si
/// chaîne, rognée (espaces en tête/fin), ≤ 64 octets, sans caractère de
/// contrôle, contenant `#`.
fn btag_field(v: &Value, k: &str) -> Option<String> {
    let s = text(v, k)?;
    let s = s.trim().to_string();
    (s.len() <= 64 && s.contains('#') && !s.chars().any(|c| c.is_control())).then_some(s)
}

/// Champs d'une sous-table, quelles que soient les clés ; vide si absente.
fn entries<'a>(v: Option<&'a Value>) -> impl Iterator<Item = (&'a Key, &'a Value)> + 'a {
    v.and_then(Value::as_table)
        .into_iter()
        .flat_map(|t| t.iter())
}

/// Champs à clé chaîne dont la valeur est une table.
fn table_entries<'a>(v: Option<&'a Value>) -> impl Iterator<Item = (String, &'a Value)> + 'a {
    entries(v).filter_map(|(k, v)| {
        let key = k.as_str_lossy()?.into_owned();
        v.as_table().map(|_| (key, v))
    })
}

/// Valeurs-tables d'une liste ou d'un dictionnaire, dans l'ordre des clés.
fn table_values<'a>(v: Option<&'a Value>) -> impl Iterator<Item = &'a Value> + 'a {
    entries(v)
        .map(|(_, v)| v)
        .filter(|v| v.as_table().is_some())
}

/// Garde, par clé, l'élément au plus grand horodatage. À horodatage égal,
/// le dernier proposé gagne : les profils sont parcourus du plus ancien au
/// plus récent (le profil actif en dernier).
struct Newest<T> {
    map: BTreeMap<String, (i64, T)>,
}

impl<T> Newest<T> {
    fn new() -> Self {
        Newest {
            map: BTreeMap::new(),
        }
    }

    fn offer(&mut self, key: String, stamp: Option<i64>, item: T) {
        let stamp = stamp.unwrap_or(i64::MIN);
        if !matches!(self.map.get(&key), Some((old, _)) if *old > stamp) {
            self.map.insert(key, (stamp, item));
        }
    }

    fn contains(&self, key: &str) -> bool {
        self.map.contains_key(key)
    }

    fn into_vec(self) -> Vec<T> {
        self.map.into_values().map(|(_, v)| v).collect()
    }

    /// Complète avec les clés de `other` absentes d'ici (`self` prioritaire).
    fn fill_from(mut self, other: Newest<T>) -> Self {
        for (k, v) in other.map {
            self.map.entry(k).or_insert(v);
        }
        self
    }
}

// --- Sources ---------------------------------------------------------------

struct Profile<'a> {
    key: String,
    value: &'a Value,
    active: bool,
    freshness: i64,
}

struct Backup<'a> {
    key: String,
    source: String,
    value: &'a Value,
}

struct CharFile<'a> {
    source: String,
    name: Option<String>,
    guild: Option<String>,
    pending: Option<&'a Value>,
}

#[derive(Default)]
struct Group<'a> {
    profiles: Vec<Profile<'a>>,
    backups: Vec<Backup<'a>>,
    chars: Vec<CharFile<'a>>,
}

fn account_profiles(account: &SavedVariables) -> Vec<Profile<'_>> {
    let Some(root) = account.get("GuildFoundForeverDB") else {
        return Vec::new();
    };
    let config = account.get("GuildFoundForeverConfigDB");
    let active_key = text(root, "lastActiveGuildKey")
        .or_else(|| config.and_then(|c| text(c, "lastActiveGuildKey")));
    let seen_at = |key: &str| {
        config
            .and_then(|c| c.get("guilds"))
            .and_then(|g| g.get(key))
            .and_then(|e| int(e, "lastSeenAt"))
            .unwrap_or(0)
    };
    let mut out: Vec<Profile> = match root.get("guilds").filter(|g| g.as_table().is_some()) {
        Some(guilds) => table_entries(Some(guilds))
            .map(|(key, value)| Profile {
                active: active_key.as_deref() == Some(key.as_str()),
                freshness: seen_at(&key),
                key,
                value,
            })
            .collect(),
        // Disposition à plat d'avant 0.17.13 : un seul profil.
        None => text(root, "guildName")
            .map(|g| {
                let realm = text(root, "realmName").unwrap_or_default();
                Profile {
                    key: format!("{}#{}", realm.to_ascii_lowercase(), g.to_ascii_lowercase()),
                    value: root,
                    active: true,
                    freshness: 0,
                }
            })
            .into_iter()
            .collect(),
    };
    out.sort_by(|a, b| (a.active, a.freshness, &a.key).cmp(&(b.active, b.freshness, &b.key)));
    out
}

fn character_file<'a>(source: &str, sv: &'a SavedVariables) -> (CharFile<'a>, Vec<Backup<'a>>) {
    let adventure = sv.get("GuildSelfForeverAdventure");
    let baseline = sv.get("GuildSelfForeverAuditBaseline");
    let chardb = sv.get("GuildFoundForeverCharacterDB");

    let mut backups: Vec<Backup> =
        table_entries(chardb.and_then(|c| c.get("criticalStateBackups")))
            .map(|(key, value)| Backup {
                key: text(value, "guildKey").unwrap_or(key),
                source: source.to_string(),
                value,
            })
            .collect();
    // Alias hérité (< 0.17.13) : seulement s'il n'est pas déjà couvert.
    if let Some(legacy) = chardb.and_then(|c| c.get("criticalStateBackup")) {
        if let Some(key) = text(legacy, "guildKey").filter(|k| !k.is_empty()) {
            let norm = normalize_guild_key(&key);
            if !backups.iter().any(|b| normalize_guild_key(&b.key) == norm) {
                backups.push(Backup {
                    key,
                    source: source.to_string(),
                    value: legacy,
                });
            }
        }
    }
    backups.retain(|b| !b.key.is_empty());

    let name = adventure
        .and_then(|a| text(a, "name"))
        .or_else(|| baseline.and_then(|b| text(b, "character")));
    let guild = baseline
        .and_then(|b| text(b, "guildKey"))
        .filter(|k| !k.is_empty())
        .map(|k| normalize_guild_key(&k))
        .or_else(|| {
            let a = adventure?;
            guild_key_from_parts(
                &text(a, "guildName")?,
                &text(a, "realm").unwrap_or_default(),
            )
        })
        .or_else(|| backups.first().map(|b| normalize_guild_key(&b.key)));
    let file = CharFile {
        source: source.to_string(),
        name,
        guild,
        pending: sv
            .get("GuildSelfForeverAuditPending")
            .and_then(|p| p.get("audits")),
    };
    (file, backups)
}

/// Normalise un fichier compte et les fichiers de personnages du même compte
/// WoW (`(indice de chemin, contenu)`, sans les `.bak`) en une entrée par
/// guilde, triées par clé normalisée.
pub fn normalize<P: AsRef<str>>(
    account: &SavedVariables,
    characters: &[(P, SavedVariables)],
) -> Vec<GuildData> {
    let mut groups: BTreeMap<String, Group> = BTreeMap::new();
    for p in account_profiles(account) {
        groups
            .entry(normalize_guild_key(&p.key))
            .or_default()
            .profiles
            .push(p);
    }
    for (path, sv) in characters {
        let (file, backups) = character_file(path.as_ref(), sv);
        for b in backups {
            groups
                .entry(normalize_guild_key(&b.key))
                .or_default()
                .backups
                .push(b);
        }
        if let Some(g) = file.guild.clone() {
            groups.entry(g).or_default().chars.push(file);
        }
    }

    let root = account.get("GuildFoundForeverDB");
    let writer_version = root.and_then(|r| text(r, "writerVersion"));
    let writer_saved_at = root.and_then(|r| int(r, "writerSavedAt"));

    groups
        .into_iter()
        .map(|(key, group)| {
            let mut g = build_guild(key, &group);
            g.writer_version = writer_version.clone();
            g.writer_saved_at = writer_saved_at;
            g
        })
        .collect()
}

/// Noms complets fiables connus de la guilde.
fn learn_names(r: &mut Resolver, group: &Group) {
    for p in &group.profiles {
        let v = p.value;
        for field in [
            "memberPresence",
            "memberIdentity",
            "adventureRoster",
            "bridgeRoster",
            "professions",
            "authorityVersions",
        ] {
            for (key, rec) in table_entries(v.get(field)) {
                r.learn(&key);
                if let Some(n) = text(rec, "name") {
                    r.learn(&n);
                }
            }
        }
        for rep in table_values(v.get("auditReports")) {
            if let Some(n) = text(rep, "player") {
                r.learn(&n);
            }
        }
        for field in [
            "bridgeLinkOutbox",
            "bridgeViolations",
            "bridgeLocalViolations",
        ] {
            for rec in table_values(v.get(field)) {
                for name in ["sender", "senderKey", "player", "playerKey", "reporter"] {
                    if let Some(n) = text(rec, name) {
                        r.learn(&n);
                    }
                }
            }
        }
        for req in table_values(v.get("requests")) {
            for field in ["requester", "decidedBy", "takenBy"] {
                if let Some(n) = text(req, field) {
                    r.learn(&n);
                }
            }
        }
        for (k, _) in entries(v.get("auditTombstones")) {
            if let Some((player, _)) = k.as_str_lossy().as_deref().and_then(|k| k.split_once(':')) {
                r.learn(player);
            }
        }
    }
    for b in &group.backups {
        for line in entries(b.value.get("requests")).filter_map(|(_, l)| l.as_bytes()) {
            if let Some(w) = decode_request_line(line) {
                r.learn(&w.requester);
                r.learn(&w.decided_by);
                r.learn(w.taken_by.as_deref().unwrap_or(""));
            }
        }
        for row in entries(b.value.get("auditCleanupRows")).filter_map(|(_, l)| l.as_bytes()) {
            if let Some(w) = decode_audit_cleanup_row(row) {
                r.learn(&w.player);
            }
        }
    }
    for c in &group.chars {
        if let Some(n) = &c.name {
            r.learn(n);
        }
    }
}

fn build_guild(key: String, group: &Group) -> GuildData {
    let mut r = Resolver::default();
    learn_names(&mut r, group);

    let mut members = Newest::new();
    let mut adventure = Newest::new();
    let mut bridge_roster: Option<Newest<BridgeRosterEntry>> = None;
    let mut requests = Newest::new();
    let mut request_tombstones = Newest::new();
    let mut request_purge_before: Option<i64> = None;
    let mut deaths = Newest::new();
    let mut member_identities = Newest::new();
    let mut bank_item_info = Newest::new();
    let mut collector_sessions = Newest::new();
    let mut blacklist_actions: BTreeMap<String, BlacklistAction> = BTreeMap::new();
    // Audits reçus du joueur lui-même (authentifiés par le jeu) et audits
    // relayés par un autre membre : le direct l'emporte toujours.
    let mut reports = Newest::new();
    let mut relayed_reports = Newest::new();
    let mut tombstones = Newest::new();
    let mut approvals = Newest::new();
    let mut permanent: BTreeMap<String, BankApproval> = BTreeMap::new();
    let mut personal = Newest::new();
    let mut tabs = Newest::new();
    let mut professions = Newest::new();
    let mut allied: Option<(i64, Vec<AlliedGuild>)> = None;
    let mut outbox: Option<BTreeMap<String, LinkOutboxEntry>> = None;
    let mut violations: Option<Newest<Violation>> = None;
    let mut local_violations: Option<Newest<Violation>> = None;
    // Clé = (player, id) : un seul enregistrement par audit porté, le plus
    // récent `carriedAt` gagne (addon côté porteur, un seul exemplaire).
    let mut carried_audits: Option<BTreeMap<(String, String), CarriedAudit>> = None;
    let mut bridge_roster_at: Option<i64> = None;
    // Dernière connexion la plus récente par personnage, quel que soit le
    // relevé (`seenAt`) retenu pour le reste de l'entrée.
    let mut last_online: BTreeMap<String, i64> = BTreeMap::new();
    let mut source_keys = BTreeSet::new();
    let mut guild_name = String::new();
    let mut realm = String::new();
    let mut active = false;

    for p in &group.profiles {
        let v = p.value;
        source_keys.insert(p.key.clone());
        active |= p.active;
        if let Some(n) = text(v, "guildName").filter(|n| !n.is_empty()) {
            guild_name = n;
        }
        if let Some(n) = text(v, "realmName").filter(|n| !n.is_empty()) {
            realm = n;
        }

        for (k, rec) in table_entries(v.get("memberPresence")) {
            let character = r.resolve(&k, "members");
            members.offer(
                character.merge_key(),
                int(rec, "lastSeen"),
                MemberPresence {
                    name: text(rec, "name"),
                    ever_seen: boolean(rec, "everSeen"),
                    last_seen: int(rec, "lastSeen"),
                    last_version: text(rec, "lastVersion"),
                    character,
                },
            );
        }

        for (k, rec) in table_entries(v.get("memberIdentity")) {
            let character = r.resolve(&k, "member_identities");
            let at = int(rec, "at").unwrap_or(0);
            member_identities.offer(
                character.merge_key(),
                Some(at),
                MemberIdentity {
                    bt: hex_field(rec, "bt", 32),
                    acct: hex_field(rec, "acct", 16),
                    at,
                    btag: btag_field(rec, "btag"),
                    character,
                },
            );
        }

        for (k, rec) in entries(v.get("bankItemInfo")) {
            let (Some(id), Some(_)) = (k.as_i64(), rec.as_table()) else {
                continue;
            };
            let Some(name) = text(rec, "n").filter(|n| !n.trim().is_empty()) else {
                continue;
            };
            if id <= 0 {
                continue;
            }
            let at = int(rec, "at");
            bank_item_info.offer(
                format!("{id:010}"),
                at,
                BankItemInfo {
                    id,
                    name,
                    quality: int(rec, "q"),
                    bind: int(rec, "b"),
                    required_level: int(rec, "r"),
                    item_level: int(rec, "l"),
                    at,
                },
            );
        }

        for (_, rec) in table_entries(v.get("collectorSessions")) {
            let (Some(raw), Some(started_at)) = (text(rec, "character"), int(rec, "startedAt"))
            else {
                continue;
            };
            if raw.trim().is_empty() || started_at <= 0 {
                continue;
            }
            let character = r.resolve(&raw, "collector_sessions");
            let seen_at = int(rec, "seenAt");
            let ended_at = int(rec, "endedAt");
            collector_sessions.offer(
                format!("{}:{started_at}", character.merge_key()),
                seen_at.max(ended_at),
                CollectorSession {
                    name: text(rec, "name"),
                    started_at,
                    seen_at,
                    ended_at,
                    character,
                },
            );
        }

        for (id, rec) in table_entries(v.get("blacklistOutbox")) {
            if blacklist_actions.len() >= 100 || id.len() > 64 {
                continue;
            }
            let (Some(operation), Some(target_type), Some(target_key), Some(target_label)) = (
                text(rec, "operation"),
                text(rec, "targetType"),
                text(rec, "targetKey"),
                text(rec, "targetLabel"),
            ) else {
                continue;
            };
            blacklist_actions
                .entry(id.clone())
                .or_insert(BlacklistAction {
                    id,
                    operation,
                    target_type,
                    target_key,
                    target_label,
                    reason: text(rec, "reason"),
                });
        }

        for (k, rec) in table_entries(v.get("adventureRoster")) {
            let character = r.resolve(&k, "adventure_roster");
            let updated_at = int(rec, "updatedAt");
            let last_seen = int(rec, "lastGuildSeenAt");
            adventure.offer(
                character.merge_key(),
                updated_at.max(last_seen),
                AdventureMember {
                    id: text(rec, "id"),
                    name: text(rec, "name"),
                    realm: text(rec, "realm"),
                    status: text(rec, "status"),
                    source: text(rec, "source"),
                    class_file: text(rec, "classFileName").or_else(|| text(rec, "classFile")),
                    enrolled_at: int(rec, "enrolledAt"),
                    updated_at,
                    last_guild_seen_at: last_seen,
                    flagged_at: int(rec, "flaggedAt"),
                    flagged_by: text(rec, "flaggedBy"),
                    flag_source: text(rec, "flagSource"),
                    character,
                },
            );
        }

        if let Some(roster) = v.get("bridgeRoster").filter(|t| t.as_table().is_some()) {
            let merged = bridge_roster.get_or_insert_with(Newest::new);
            for (k, rec) in table_entries(Some(roster)) {
                let character = r.resolve(&k, "bridge_roster");
                let last_online_at = int(rec, "lastOnlineAt");
                if let Some(at) = last_online_at {
                    let best = last_online.entry(character.merge_key()).or_insert(at);
                    *best = (*best).max(at);
                }
                merged.offer(
                    character.merge_key(),
                    int(rec, "seenAt"),
                    BridgeRosterEntry {
                        name: text(rec, "name"),
                        rank_index: int(rec, "rankIndex"),
                        rank_name: text(rec, "rankName"),
                        class_file: text(rec, "classFile"),
                        level: int(rec, "level"),
                        seen_at: int(rec, "seenAt"),
                        last_online_at,
                        character,
                    },
                );
            }
        }

        for (k, rec) in table_entries(v.get("requests")) {
            let req = request_from_table(&k, rec, &mut r);
            requests.offer(req.id.clone(), req.updated_at, req);
        }

        for (k, rec) in table_entries(v.get("requestTombstones")) {
            let updated_at = int(rec, "updatedAt");
            request_tombstones.offer(
                k.clone(),
                updated_at,
                RequestTombstone {
                    id: k,
                    status: text(rec, "status"),
                    updated_at,
                },
            );
        }

        if let Some(p) = int(v, "requestPurgeBefore") {
            request_purge_before = Some(request_purge_before.map_or(p, |old| old.max(p)));
        }

        for (k, rec) in table_entries(v.get("deathLog")) {
            let id = text(rec, "id").unwrap_or_else(|| k.clone());
            let at = int(rec, "at");
            deaths.offer(
                id.clone(),
                at,
                Death {
                    id,
                    name: text(rec, "name"),
                    realm: text(rec, "realm"),
                    guild_key: text(rec, "guildKey"),
                    guild_name: text(rec, "guildName"),
                    class_file: text(rec, "classFile"),
                    race: text(rec, "race"),
                    level: int(rec, "level"),
                    zone: text(rec, "zone"),
                    killer: text(rec, "killer"),
                    killer_kind: text(rec, "killerKind"),
                    at,
                    reporter: text(rec, "reporter"),
                },
            );
        }

        for rec in table_values(v.get("auditReports")) {
            if let Some(rep) = audit_from(rec, None, &mut r, "audit_reports") {
                let key = format!("{}:{}", rep.player.merge_key(), rep.id);
                if rep.relayed_by.is_some() {
                    relayed_reports.offer(key, rep.to, rep);
                } else {
                    reports.offer(key, rep.to, rep);
                }
            }
        }

        for (k, val) in entries(v.get("auditTombstones")) {
            let Some(k) = k.as_str_lossy() else { continue };
            if let Some(t) = tombstone(&k, val, &mut r) {
                let key = format!("{}:{}", t.player.merge_key(), t.report_id);
                tombstones.offer(key, t.at, t);
            }
        }

        for (k, rec) in table_entries(v.get("bankApprovalRecords")) {
            let a = approval(&k, rec, &mut r);
            approvals.offer(a.character.merge_key(), a.changed_at, a);
        }
        for (k, rec) in table_entries(v.get("bankPermanentRegistry")) {
            let a = approval(&k, rec, &mut r);
            let key = a.character.merge_key();
            if permanent
                .get(&key)
                .map_or(true, |old| old.changed_at <= a.changed_at)
            {
                permanent.insert(key, a);
            }
        }

        let snapshots = v.get("bankSnapshots");
        for (k, rec) in table_entries(snapshots.and_then(|s| s.get("personal"))) {
            let owner = r.resolve(&k, "bank_snapshots.personal");
            personal.offer(
                owner.merge_key(),
                int(rec, "time"),
                PersonalBank {
                    owner,
                    time: int(rec, "time"),
                    total_slots: int(rec, "totalSlots"),
                    slots: slots(rec.get("slots")),
                },
            );
        }
        let guild_tabs = snapshots
            .and_then(|s| s.get("guild"))
            .and_then(|g| g.get("tabs"));
        for (k, rec) in entries(guild_tabs) {
            let (Some(tab), Some(_)) = (k.as_i64(), rec.as_table()) else {
                continue;
            };
            tabs.offer(
                format!("{tab:010}"),
                int(rec, "time"),
                GuildBankTab {
                    tab,
                    name: text(rec, "name"),
                    time: int(rec, "time"),
                    total_slots: int(rec, "totalSlots"),
                    by: text(rec, "by"),
                    slots: slots(rec.get("slots")),
                },
            );
        }

        for (k, rec) in table_entries(v.get("professions")) {
            let character = r.resolve(&k, "professions");
            professions.offer(
                character.merge_key(),
                int(rec, "seen"),
                ProfessionRecord {
                    name: text(rec, "name"),
                    origin: text(rec, "origin"),
                    seen: int(rec, "seen"),
                    list: table_values(rec.get("list"))
                        .map(|p| Profession {
                            name: text(p, "name"),
                            skill_line: int(p, "skillLine"),
                            rank: int(p, "rank"),
                            max_rank: int(p, "maxRank"),
                        })
                        .collect(),
                    character,
                },
            );
        }

        if v.get("alliedGuilds").and_then(Value::as_table).is_some() {
            let stamp = int(v, "alliedGuildsStamp").unwrap_or(i64::MIN);
            if allied.as_ref().map_or(true, |(old, _)| *old <= stamp) {
                let list = table_entries(v.get("alliedGuilds"))
                    .map(|(key, rec)| AlliedGuild {
                        name: text(rec, "name"),
                        key,
                    })
                    .collect();
                allied = Some((stamp, list));
            }
        }

        if let Some(at) = int(v, "bridgeRosterAt") {
            bridge_roster_at = bridge_roster_at.max(Some(at));
        }

        if let Some(t) = v.get("bridgeLinkOutbox").filter(|t| t.as_table().is_some()) {
            let merged = outbox.get_or_insert_with(BTreeMap::new);
            for (k, rec) in table_entries(Some(t)) {
                let entry = link_entry(&k, rec, &mut r);
                merge_link(merged, entry);
            }
        }

        for (field, store) in [
            ("bridgeViolations", &mut violations),
            ("bridgeLocalViolations", &mut local_violations),
        ] {
            if let Some(t) = v.get(field).filter(|t| t.as_table().is_some()) {
                let merged = store.get_or_insert_with(Newest::new);
                for (k, rec) in table_entries(Some(t)) {
                    let viol = violation(&k, rec, &mut r, field);
                    let stamp = viol.received_at.or(viol.at);
                    merged.offer(viol.id.clone(), stamp, viol);
                }
            }
        }

        if let Some(t) = v
            .get("bridgeCarriedAudits")
            .filter(|t| t.as_table().is_some())
        {
            let merged = carried_audits.get_or_insert_with(BTreeMap::new);
            for (_, rec) in table_entries(Some(t)) {
                let Some(c) = carried_audit(rec) else {
                    continue;
                };
                let key = (c.player.clone(), c.id.clone());
                let replace = match merged.get(&key) {
                    Some(old) => {
                        c.carried_at.unwrap_or(i64::MIN) >= old.carried_at.unwrap_or(i64::MIN)
                    }
                    None => true,
                };
                if replace {
                    merged.insert(key, c);
                }
            }
        }
    }

    // Sauvegardes des fichiers de personnages.
    let mut backup_requests = Newest::new();
    for b in &group.backups {
        source_keys.insert(b.key.clone());
        if guild_name.is_empty() {
            guild_name = text(b.value, "guildName").unwrap_or_default();
        }
        if realm.is_empty() {
            realm = text(b.value, "realm").unwrap_or_default();
        }
        let saved_at = int(b.value, "savedAt");
        for line in entries(b.value.get("requests")).filter_map(|(_, l)| l.as_bytes()) {
            let Some(w) = decode_request_line(line) else {
                continue;
            };
            let requester = r.resolve(&w.requester, "backup_requests.requester");
            let opt = |s: String| (!s.is_empty()).then_some(s);
            let request = Request {
                id: w.id.clone(),
                kind: Some(w.kind),
                text: Some(w.text),
                requester,
                status: Some(w.status),
                created_at: Some(w.created_at),
                updated_at: Some(w.updated_at),
                decided_at: Some(w.decided_at),
                decided_by: opt(w.decided_by),
                taken_by: w.taken_by.and_then(opt),
                taken_at: w.taken_at,
                craft_skill_line: Some(w.craft_skill_line),
                craft_recipe_id: Some(w.craft_recipe_id),
                craft_category: Some(w.craft_category),
                item_id: Some(w.item_id),
                request_number: request_number(&w.id),
                arrival: None,
                trusted: None,
                released_by: w.released_by.clone().and_then(opt),
                released_at: w.released_at,
                // Absent de la ligne de sauvegarde (13/15/17 champs) : pas
                // de 19ᵉ champ à ce jour, donc toujours `None` ici.
                close_reason: None,
            };
            backup_requests.offer(
                w.id,
                Some(w.updated_at),
                BackupRequest {
                    request,
                    field_count: w.field_count,
                    source: b.source.clone(),
                    backup_saved_at: saved_at,
                },
            );
        }
        for row in entries(b.value.get("auditCleanupRows")).filter_map(|(_, l)| l.as_bytes()) {
            let Some(w) = decode_audit_cleanup_row(row) else {
                continue;
            };
            let player = r.resolve(&w.player, "audit_tombstones");
            let key = format!("{}:{}", player.merge_key(), w.id);
            let opt = |s: String| (!s.is_empty()).then_some(s);
            let t = AuditTombstone {
                key: format!("{}:{}", w.player, w.id),
                player,
                report_id: w.id,
                at: Some(w.at),
                by: opt(w.by),
                adventure_id: opt(w.adventure_id),
                form: TombstoneForm::BackupRow,
            };
            // La décision stockée dans le compte reste prioritaire.
            if !tombstones.contains(&key) {
                tombstones.offer(key, t.at, t);
            }
        }
    }

    let audit_pending = group
        .chars
        .iter()
        .map(|c| {
            let character = c.name.as_deref().map(|n| r.resolve(n, "audit_pending"));
            let audits = table_values(c.pending)
                .filter_map(|a| audit_from(a, character.as_ref(), &mut r, "audit_pending"))
                .collect();
            PendingAudits {
                source: c.source.clone(),
                character,
                audits,
            }
        })
        .collect();

    // Approbations : le registre permanent complète et marque les records.
    let mut bank_approvals = approvals.into_vec();
    for a in &mut bank_approvals {
        a.permanent = permanent.remove(&a.character.merge_key()).is_some();
    }
    bank_approvals.extend(permanent.into_values().map(|mut a| {
        a.permanent = true;
        a
    }));
    bank_approvals.sort_by_key(|a| a.character.merge_key());

    GuildData {
        guild_key_normalized: key,
        guild_name,
        realm,
        source_keys: source_keys.into_iter().collect(),
        active,
        writer_version: None,
        writer_saved_at: None,
        members: members.into_vec(),
        adventure_roster: adventure.into_vec(),
        bridge_roster: bridge_roster.map(|m| {
            let mut v = m.into_vec();
            for e in &mut v {
                e.last_online_at = last_online.get(&e.character.merge_key()).copied();
            }
            v
        }),
        bridge_roster_at,
        requests: requests.into_vec(),
        backup_requests: backup_requests.into_vec(),
        audit_reports: reports.fill_from(relayed_reports).into_vec(),
        audit_pending,
        audit_tombstones: tombstones.into_vec(),
        bank_approvals,
        bank_snapshots: BankSnapshots {
            personal: personal.into_vec(),
            guild_tabs: tabs.into_vec(),
        },
        professions: professions.into_vec(),
        allied_guilds: allied.map(|(_, l)| l).unwrap_or_default(),
        bridge_link_outbox: outbox.map(|m| m.into_values().collect()),
        bridge_violations: violations.map(Newest::into_vec),
        bridge_local_violations: local_violations.map(Newest::into_vec),
        bridge_carried_audits: carried_audits.map(|m| m.into_values().collect()),
        request_tombstones: request_tombstones.into_vec(),
        request_purge_before,
        deaths: deaths.into_vec(),
        member_identities: member_identities.into_vec(),
        bank_item_info: bank_item_info.into_vec(),
        collector_sessions: collector_sessions.into_vec(),
        blacklist_actions: blacklist_actions.into_values().collect(),
        unresolved_aliases: r.into_unresolved(),
    }
}

/// `bridgeCarriedAudits[key]` : ignore l'entrée si `player`, `id` ou
/// `payload` est absent, mal typé ou vide.
fn carried_audit(rec: &Value) -> Option<CarriedAudit> {
    let player = text(rec, "player").filter(|s| !s.is_empty())?;
    let id = text(rec, "id").filter(|s| !s.is_empty())?;
    let payload = text(rec, "payload").filter(|s| !s.is_empty())?;
    Some(CarriedAudit {
        player,
        id,
        payload,
        carried_at: int(rec, "carriedAt"),
    })
}

/// Numéro embarqué dans l'identifiant `gsfr-0031-…`.
fn request_number(id: &str) -> Option<i64> {
    let rest = id.strip_prefix("gsfr-")?;
    rest.split('-').next()?.parse().ok()
}

fn request_from_table(key: &str, rec: &Value, r: &mut Resolver) -> Request {
    let id = text(rec, "id").unwrap_or_else(|| key.to_string());
    Request {
        kind: text(rec, "kind"),
        text: text(rec, "text"),
        requester: r.resolve(
            &text(rec, "requester").unwrap_or_default(),
            "requests.requester",
        ),
        status: text(rec, "status"),
        created_at: int(rec, "createdAt"),
        updated_at: int(rec, "updatedAt"),
        decided_at: int(rec, "decidedAt"),
        decided_by: text(rec, "decidedBy"),
        taken_by: text(rec, "takenBy"),
        taken_at: int(rec, "takenAt"),
        craft_skill_line: int(rec, "craftSkillLine"),
        craft_recipe_id: int(rec, "craftRecipeID"),
        craft_category: text(rec, "craftCategory"),
        item_id: int(rec, "itemID"),
        request_number: int(rec, "requestNumber").or_else(|| request_number(&id)),
        arrival: int(rec, "arrival"),
        trusted: boolean(rec, "trusted"),
        released_by: text(rec, "releasedBy"),
        released_at: int(rec, "releasedAt"),
        close_reason: text(rec, "closeReason"),
        id,
    }
}

fn item_deltas(v: Option<&Value>) -> Vec<ItemDelta> {
    table_values(v)
        .map(|x| ItemDelta {
            id: int(x, "id"),
            delta: int(x, "delta"),
        })
        .collect()
}

/// Rapport d'audit stocké (`before/after` ou `beforeItem/afterItem`).
fn audit_from(
    rec: &Value,
    owner: Option<&CharacterRef>,
    r: &mut Resolver,
    context: &str,
) -> Option<AuditReport> {
    let id = text(rec, "id").or_else(|| rec.get("id")?.as_i64().map(|n| n.to_string()))?;
    let player = match (text(rec, "player"), owner) {
        (Some(p), _) => r.resolve(&p, context),
        (None, Some(o)) => o.clone(),
        (None, None) => r.resolve("", context),
    };
    let equipment = table_values(rec.get("equipment"))
        .map(|x| EquipmentChange {
            slot: int(x, "slot"),
            before: text(x, "beforeItem").or_else(|| text(x, "before")),
            after: text(x, "afterItem").or_else(|| text(x, "after")),
        })
        .collect();
    let acked_by = entries(rec.get("ackedBy"))
        .filter_map(|(k, v)| Some((k.as_str_lossy()?.into_owned(), v.as_i64()?)))
        .collect();
    // Couple tout ou rien, comme le jeton `P` de l'addon :
    // `playedAfter` doit dépasser `playedBefore`.
    let (played_before, played_after) = match (
        int_in(rec, "playedBefore", 0, MAX_AUDIT_PLAYED),
        int_in(rec, "playedAfter", 0, MAX_AUDIT_PLAYED),
    ) {
        (Some(before), Some(after)) if after > before => (Some(before), Some(after)),
        _ => (None, None),
    };
    Some(AuditReport {
        id,
        player,
        class_file: text(rec, "classFile"),
        created_at: int(rec, "createdAt"),
        from: int(rec, "from"),
        to: int(rec, "to"),
        gold_before: int(rec, "goldBefore"),
        gold_after: int(rec, "goldAfter"),
        gold_delta: int(rec, "goldDelta")
            .or_else(|| Some(int(rec, "goldAfter")? - int(rec, "goldBefore")?)),
        change_count: int(rec, "changeCount"),
        missing_baseline: boolean(rec, "missingBaseline"),
        truncated: boolean(rec, "truncated"),
        bank_known: boolean(rec, "bankKnown"),
        items: item_deltas(rec.get("items")),
        bank: item_deltas(rec.get("bank")),
        equipment,
        acked_by,
        attempts: int(rec, "attempts"),
        next_send_at: int(rec, "nextSendAt"),
        relayed_by: text(rec, "relayedBy").filter(|n| !n.trim().is_empty()),
        played_before,
        played_after,
        level_before: int_in(rec, "levelBefore", 1, MAX_AUDIT_LEVEL),
        level_after: int_in(rec, "levelAfter", 1, MAX_AUDIT_LEVEL),
    })
}

/// Bornes du rapport « joué sans l'addon » (contrat 2026-11-03 c).
const MAX_AUDIT_PLAYED: i64 = 2_147_483_647;
const MAX_AUDIT_LEVEL: i64 = 255;

/// Entier entre `min` et `max`, sinon `None`.
fn int_in(v: &Value, k: &str, min: i64, max: i64) -> Option<i64> {
    int(v, k).filter(|n| (min..=max).contains(n))
}

/// `auditTombstones["joueur:id"]` : un nombre (≤ 0.17.40) ou une table
/// `{at, by, adventureID, player}` (≥ 0.17.41).
fn tombstone(key: &str, val: &Value, r: &mut Resolver) -> Option<AuditTombstone> {
    let (player, report_id) = key.split_once(':')?;
    if player.is_empty() || report_id.is_empty() {
        return None;
    }
    let (at, by, adventure_id, form, who) = match val {
        Value::Table(_) => (
            int(val, "at"),
            text(val, "by").filter(|s| !s.is_empty()),
            text(val, "adventureID").filter(|s| !s.is_empty()),
            TombstoneForm::Table,
            text(val, "player").unwrap_or_else(|| player.to_string()),
        ),
        _ => (
            val.as_i64(),
            None,
            None,
            TombstoneForm::Number,
            player.to_string(),
        ),
    };
    Some(AuditTombstone {
        key: key.to_string(),
        player: r.resolve(&who, "audit_tombstones"),
        report_id: report_id.to_string(),
        at,
        by,
        adventure_id,
        form,
    })
}

fn approval(key: &str, rec: &Value, r: &mut Resolver) -> BankApproval {
    BankApproval {
        character: r.resolve(key, "bank_approvals"),
        name: text(rec, "name"),
        approved: boolean(rec, "approved"),
        changed_at: int(rec, "changedAt"),
        changed_by: text(rec, "changedBy"),
        permanent: false,
    }
}

fn slots(v: Option<&Value>) -> Vec<BankSlot> {
    entries(v)
        .filter_map(|(k, s)| {
            s.as_table()?;
            Some(BankSlot {
                slot: k.as_i64()?,
                id: int(s, "id"),
                count: int(s, "count"),
            })
        })
        .collect()
}

/// Référence identifiée par la clé de roster (`senderKey`/`playerKey`,
/// fixée par l'addon), à défaut par le nom affiché.
fn keyed_ref(
    rec: &Value,
    key_field: &str,
    name_field: &str,
    r: &mut Resolver,
    ctx: &str,
) -> CharacterRef {
    let raw = text(rec, key_field)
        .filter(|k| !k.is_empty())
        .or_else(|| text(rec, name_field))
        .unwrap_or_default();
    r.resolve(&raw, ctx)
}

fn link_entry(key: &str, rec: &Value, r: &mut Resolver) -> LinkOutboxEntry {
    LinkOutboxEntry {
        code: text(rec, "code").unwrap_or_else(|| key.to_string()),
        sender: keyed_ref(rec, "senderKey", "sender", r, "bridge_link_outbox"),
        sender_name: text(rec, "sender"),
        guild_key: text(rec, "guildKey"),
        at: int(rec, "at"),
        conflict: boolean(rec, "conflict") == Some(true),
    }
}

/// Même règle que l'addon (`storeLink`) : par code, le premier émetteur est
/// gardé ; un émetteur différent marque le code en conflit.
fn merge_link(store: &mut BTreeMap<String, LinkOutboxEntry>, entry: LinkOutboxEntry) {
    match store.get_mut(&entry.code) {
        None => {
            store.insert(entry.code.clone(), entry);
        }
        Some(old) => {
            let conflict = old.conflict
                || entry.conflict
                || old.sender.merge_key() != entry.sender.merge_key();
            if entry.at.unwrap_or(i64::MAX) < old.at.unwrap_or(i64::MAX) {
                *old = entry;
            }
            old.conflict = conflict;
        }
    }
}

fn violation(key: &str, rec: &Value, r: &mut Resolver, field: &str) -> Violation {
    let ctx = if field == "bridgeViolations" {
        "bridge_violations"
    } else {
        "bridge_local_violations"
    };
    Violation {
        id: text(rec, "id").unwrap_or_else(|| key.to_string()),
        kind: text(rec, "kind"),
        player: keyed_ref(rec, "playerKey", "player", r, ctx),
        player_name: text(rec, "player"),
        details: text(rec, "details"),
        at: int(rec, "at"),
        reporter: text(rec, "reporter"),
        received_at: int(rec, "receivedAt"),
        guild_key: text(rec, "guildKey"),
        relayed: boolean(rec, "relayed"),
        handed_to: handoffs(rec),
    }
}

/// Remises en jeu d'une violation (contrat 2026-11-02 b), au plus
/// [`MAX_HANDOFFS`], les plus récentes ; un même nom n'apparaît qu'une fois
/// (remise la plus récente).
fn handoffs(rec: &Value) -> Vec<Handoff> {
    let mut by_name: BTreeMap<String, Option<i64>> = BTreeMap::new();
    let mut add = |name: Option<String>, at: Option<i64>| {
        let Some(name) = name.map(|n| n.trim().to_owned()).filter(|n| !n.is_empty()) else {
            return;
        };
        let slot = by_name.entry(name).or_insert(at);
        if at > *slot {
            *slot = at;
        }
    };
    // Collecteur : liste `{ { name = …, at = … }, … }`.
    if let Some(list) = rec.get("handedTo") {
        for entry in list.array() {
            add(text(entry, "name"), int(entry, "at"));
        }
    }
    // Signaleur : table `{ ["<clé>"] = <epoch> }`.
    if let Some(sent) = rec.get("sentTo") {
        for (name, at) in sent.str_entries() {
            add(
                Some(String::from_utf8_lossy(name).into_owned()),
                at.as_i64(),
            );
        }
    }
    let mut out: Vec<Handoff> = by_name
        .into_iter()
        .map(|(name, at)| Handoff { name, at })
        .collect();
    // Les plus récentes d'abord (sans date en dernier), puis par nom.
    out.sort_by(|a, b| b.at.cmp(&a.at).then_with(|| a.name.cmp(&b.name)));
    out.truncate(MAX_HANDOFFS);
    out
}

/// Remises gardées par violation (contrat 2026-11-02 b).
const MAX_HANDOFFS: usize = 8;
