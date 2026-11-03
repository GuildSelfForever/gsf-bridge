//! Photos d'audit (« Je joue sur plusieurs ordinateurs », contrat
//! 2026-11-03) : la référence A que l'addon garde pour chaque perso (sacs,
//! équipement, or, banque), lue dans les SavedVariables pour être partagée
//! entre les PC d'un même compte quand la case « Synchroniser mon addon entre
//! mes ordinateurs » est cochée.
//!
//! Sources (`Audit.lua` de l'addon) : `GuildSelfForeverAuditBaseline` de
//! chaque fichier de personnage (copie dédiée) et
//! `GuildFoundForeverDB.guilds[*].auditBaselineBackups[*]` du fichier compte
//! (copie de secours). La plus récente (`savedAt`) gagne par perso.
//!
//! Le format est strict et typé, identique côté site : seules les valeurs
//! qui entrent dans l'empreinte (`checksum`) de l'addon sont gardées, et un
//! enregistrement que l'addon refuserait (capture incomplète, objet ou or
//! hors limites, clé non entière) est écarté entier. La vérification de
//! l'empreinte elle-même reste faite par l'addon, au chargement.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::names::clean_name;
use crate::sv::{Key, SavedVariables, Value};

/// Photos par envoi (et par réponse du site).
pub const MAX_BASELINES: usize = 50;
/// Lignes d'objets par liste (sacs, banque).
pub const MAX_BASELINE_ITEMS: usize = 1000;
/// Emplacements d'équipement de l'addon (1-19 équipement, 20-23 sacs équipés).
pub const BASELINE_EQUIPMENT_SLOTS: i64 = 23;
const MAX_GOLD: i64 = 2_147_483_647;
const MAX_ITEM_ID: i64 = 9_999_999;
const MAX_COUNT: i64 = 1_000_000;
const MAX_BAG_SLOTS: i64 = 1000;
/// `/played` (contrat 2026-11-03 c) : mêmes bornes que l'addon
/// (`PLAYED_MAX`, `PLAYED_OBSERVED_MAX`, `MAX_LEVEL` d'`Audit.lua`).
pub const MAX_PLAYED: i64 = 2_147_483_647;
pub const MAX_PLAYED_OBSERVED: i64 = 315_360_000;
pub const MAX_LEVEL: i64 = 255;

fn is_false(b: &bool) -> bool {
    !*b
}

/// Champ `/played` reçu du site : toute valeur qui n'est pas un entier i64
/// donne `None` (champ écarté seul, jamais la photo).
fn lenient_i64<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<i64>, D::Error> {
    Ok(serde_json::Value::deserialize(d)?.as_i64())
}

/// Photo d'un perso, au format du contrat (JSON du site).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditBaseline {
    /// Clé du perso (`GSF:CleanName`).
    pub name: String,
    /// Nom affiché (`character` de l'addon).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub character: Option<String>,
    pub saved_at: i64,
    pub checksum: String,
    pub adventure_id: String,
    /// Clé de guilde telle qu'écrite par l'addon (comparée telle quelle en jeu).
    pub guild_key: String,
    pub snapshot: BaselineSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaselineSnapshot {
    pub time: i64,
    pub gold: i64,
    #[serde(default)]
    pub bag_slots: i64,
    #[serde(default, skip_serializing_if = "is_false")]
    pub legacy: bool,
    /// `[identifiant, nombre]`, triés par identifiant.
    pub items: Vec<(i64, i64)>,
    /// `[emplacement, "objet:enchantement"]`, triés par emplacement.
    pub equipment: Vec<(i64, String)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bank: Option<BaselineBank>,
    /// Dernier `/played` connu du serveur, en secondes (addon ≥ 0.22.11).
    /// Hors `checksum` ; toujours avec `played_observed` (voir
    /// [`BaselineSnapshot::sanitize_played`]).
    #[serde(
        default,
        deserialize_with = "lenient_i64",
        skip_serializing_if = "Option::is_none"
    )]
    pub played: Option<i64>,
    /// Secondes où l'addon a vu le perso en ligne depuis `played`.
    #[serde(
        default,
        deserialize_with = "lenient_i64",
        skip_serializing_if = "Option::is_none"
    )]
    pub played_observed: Option<i64>,
    /// Niveau du perso au moment de la photo (1 à 255).
    #[serde(
        default,
        deserialize_with = "lenient_i64",
        skip_serializing_if = "Option::is_none"
    )]
    pub level: Option<i64>,
}

impl BaselineSnapshot {
    /// Écarte les champs `/played` invalides sans toucher au reste de la
    /// photo (ils sont hors `checksum`) : `played`/`played_observed` gardés
    /// seulement ensemble, dans leurs bornes et avec une somme ≤
    /// [`MAX_PLAYED`] (`snapshotPlayed` de l'addon) ; `level` seulement de 1
    /// à [`MAX_LEVEL`].
    pub fn sanitize_played(&mut self) {
        let pair = match (self.played, self.played_observed) {
            (Some(p), Some(o))
                if (0..=MAX_PLAYED).contains(&p)
                    && (0..=MAX_PLAYED_OBSERVED).contains(&o)
                    && p + o <= MAX_PLAYED =>
            {
                Some((p, o))
            }
            _ => None,
        };
        self.played = pair.map(|(p, _)| p);
        self.played_observed = pair.map(|(_, o)| o);
        self.level = self.level.filter(|l| (1..=MAX_LEVEL).contains(l));
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaselineBank {
    pub time: i64,
    #[serde(default, skip_serializing_if = "is_false")]
    pub legacy: bool,
    pub items: Vec<(i64, i64)>,
}

/// Texte de 1 à `max` octets sans caractère de contrôle ni `|` (ni `~` si
/// `strict`).
fn valid_text(s: &str, max: usize, strict: bool) -> bool {
    !s.is_empty()
        && s.len() <= max
        && !s
            .chars()
            .any(|c| c.is_control() || c == '|' || (strict && c == '~'))
}

fn valid_items(items: &[(i64, i64)]) -> bool {
    let mut last = 0;
    items.len() <= MAX_BASELINE_ITEMS
        && items.iter().all(|&(id, count)| {
            let ok = id > last && id <= MAX_ITEM_ID && (0..=MAX_COUNT).contains(&count);
            last = id;
            ok
        })
}

impl AuditBaseline {
    /// Mêmes règles que le site (`App\Support\Bridge\AuditBaselines`), sauf
    /// la date future (horloge du site) : listes triées sans doublon.
    pub fn is_valid(&self) -> bool {
        let s = &self.snapshot;
        let mut last_slot = 0;
        valid_text(&self.name, 64, true)
            && self
                .character
                .as_deref()
                .map_or(true, |c| valid_text(c, 64, false))
            && self.saved_at > 0
            && (1..=32).contains(&self.checksum.len())
            && self.checksum.bytes().all(|b| b.is_ascii_alphanumeric())
            && (1..=64).contains(&self.adventure_id.len())
            && self
                .adventure_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            && valid_text(&self.guild_key, 160, false)
            && self.guild_key.contains('#')
            && s.time >= 0
            && (0..=MAX_GOLD).contains(&s.gold)
            && (0..=MAX_BAG_SLOTS).contains(&s.bag_slots)
            && valid_items(&s.items)
            && s.equipment.len() <= BASELINE_EQUIPMENT_SLOTS as usize
            && s.equipment.iter().all(|(slot, v)| {
                let ok = *slot > last_slot
                    && *slot <= BASELINE_EQUIPMENT_SLOTS
                    && valid_text(v, 64, false);
                last_slot = *slot;
                ok
            })
            && s.bank
                .as_ref()
                .map_or(true, |b| b.time >= 0 && valid_items(&b.items))
    }
}

// --- Lecture des SavedVariables --------------------------------------------

fn text(v: &Value, k: &str) -> Option<String> {
    v.get(k)?.as_str_lossy().map(|s| s.into_owned())
}

fn int(v: &Value, k: &str) -> Option<i64> {
    v.get(k)?.as_i64()
}

/// `{ [id] = nombre }` ; `None` si une clé n'est pas un entier (l'empreinte
/// de l'addon ne lirait pas la même chose) ou une valeur pas un entier.
fn item_map(v: Option<&Value>) -> Option<Vec<(i64, i64)>> {
    let Some(v) = v else {
        return Some(Vec::new());
    };
    let table = v.as_table()?;
    let mut out = Vec::with_capacity(table.len());
    for (k, count) in table {
        let Key::Int(id) = k else { return None };
        out.push((*id, count.as_i64()?));
    }
    out.sort_unstable();
    Some(out)
}

/// `{ [emplacement] = "objet:enchantement" }` ; seuls les emplacements 1 à
/// 23 entrent dans l'empreinte, les autres sont laissés de côté.
fn equipment_map(v: Option<&Value>) -> Option<Vec<(i64, String)>> {
    let Some(v) = v else {
        return Some(Vec::new());
    };
    let mut out = Vec::new();
    for (k, value) in v.as_table()? {
        let Key::Int(slot) = k else { continue };
        if !(1..=BASELINE_EQUIPMENT_SLOTS).contains(slot) {
            continue;
        }
        let s = match value {
            Value::Str(b) => String::from_utf8(b.clone()).ok()?,
            Value::Int(n) => n.to_string(),
            _ => return None,
        };
        out.push((*slot, s));
    }
    out.sort_unstable();
    Some(out)
}

/// Enregistrement `{ snapshot, savedAt, checksum, adventureID, guildKey,
/// character }` de l'addon ; `fallback_name` sert si `character` manque.
fn record(v: &Value, fallback_name: Option<&str>) -> Option<AuditBaseline> {
    let snap = v.get("snapshot").filter(|s| s.as_table().is_some())?;
    if snap.get("captureComplete").and_then(Value::as_bool) == Some(false) {
        return None;
    }
    let time = int(snap, "time")?;
    let character = text(v, "character").filter(|c| !c.trim().is_empty());
    let name = character
        .as_deref()
        .or(fallback_name)
        .and_then(clean_name)?;
    let checksum = match v.get("checksum")? {
        Value::Int(n) => n.to_string(),
        other => other.as_str_lossy()?.into_owned(),
    };
    let bank = match snap.get("bank") {
        Some(b) if b.as_table().is_some() => Some(BaselineBank {
            time: int(b, "time").unwrap_or(time),
            legacy: b.get("legacy").and_then(Value::as_bool) == Some(true),
            items: item_map(b.get("items"))?,
        }),
        _ => None,
    };
    let mut baseline = AuditBaseline {
        name,
        character,
        saved_at: int(v, "savedAt").unwrap_or(time),
        checksum,
        adventure_id: text(v, "adventureID")?,
        guild_key: text(v, "guildKey")?,
        snapshot: BaselineSnapshot {
            time,
            gold: int(snap, "gold")?,
            bag_slots: int(snap, "bagSlots").unwrap_or(0),
            legacy: snap.get("legacy").and_then(Value::as_bool) == Some(true),
            items: item_map(snap.get("items"))?,
            equipment: equipment_map(snap.get("equipment"))?,
            bank,
            played: int(snap, "played"),
            played_observed: int(snap, "playedObserved"),
            level: int(snap, "level"),
        },
    };
    baseline.snapshot.sanitize_played();
    baseline.is_valid().then_some(baseline)
}

/// Nom du dossier du perso dans un chemin
/// `…/<Royaume>/<Perso>/SavedVariables/GuildSelfForever.lua`.
fn character_dir_name(path: &str) -> Option<&str> {
    let parts: Vec<&str> = path.split(['/', '\\']).filter(|p| !p.is_empty()).collect();
    let idx = parts
        .iter()
        .rposition(|p| p.eq_ignore_ascii_case("SavedVariables"))?;
    idx.checked_sub(1).map(|i| parts[i])
}

/// Photos d'un compte WoW : la plus récente par perso, [`MAX_BASELINES`] au
/// plus (les plus récentes), triées par clé de perso.
pub fn audit_baselines<P: AsRef<str>>(
    account: &SavedVariables,
    characters: &[(P, SavedVariables)],
) -> Vec<AuditBaseline> {
    let mut best: BTreeMap<String, AuditBaseline> = BTreeMap::new();
    let mut offer = |b: AuditBaseline| {
        if best
            .get(&b.name)
            .map_or(true, |old| old.saved_at < b.saved_at)
        {
            best.insert(b.name.clone(), b);
        }
    };
    // Copies dédiées d'abord : à date égale, elles gardent la place.
    for (path, sv) in characters {
        if let Some(b) = sv
            .get("GuildSelfForeverAuditBaseline")
            .and_then(|v| record(v, character_dir_name(path.as_ref())))
        {
            offer(b);
        }
    }
    let profiles = account
        .get("GuildFoundForeverDB")
        .and_then(|r| r.get("guilds"))
        .and_then(Value::as_table);
    for profile in profiles.into_iter().flat_map(|t| t.values()) {
        let backups = profile
            .get("auditBaselineBackups")
            .and_then(Value::as_table);
        for rec in backups.into_iter().flat_map(|t| t.values()) {
            if let Some(b) = record(rec, None) {
                offer(b);
            }
        }
    }
    let mut list: Vec<AuditBaseline> = best.into_values().collect();
    if list.len() > MAX_BASELINES {
        list.sort_by(|a, b| b.saved_at.cmp(&a.saved_at).then(a.name.cmp(&b.name)));
        list.truncate(MAX_BASELINES);
        list.sort_by(|a, b| a.name.cmp(&b.name));
    }
    list
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sv::parse;

    fn character(saved_at: i64, gold: i64) -> String {
        format!(
            r#"GuildSelfForeverAuditBaseline = {{
                ["schema"] = 3, ["savedAt"] = {saved_at}, ["checksum"] = "123456",
                ["adventureID"] = "A123", ["guildKey"] = "classicbetapve#home", ["character"] = "Tester",
                ["snapshot"] = {{
                    ["time"] = {saved_at}, ["gold"] = {gold}, ["bagSlots"] = 64, ["captureComplete"] = true,
                    ["items"] = {{ [6948] = 1, [2589] = 20 }},
                    ["equipment"] = {{ [16] = "25:0", [1] = "1234:0", [40] = "ignored:0" }},
                    ["bank"] = {{ ["items"] = {{ [2770] = 5 }} }},
                }},
            }}"#
        )
    }

    fn account(backup_saved_at: i64) -> String {
        format!(
            r#"GuildFoundForeverDB = {{ ["guilds"] = {{ ["classicbetapve#home"] = {{
                ["auditBaselineBackups"] = {{ ["A123"] = {{
                    ["savedAt"] = {backup_saved_at}, ["checksum"] = "999", ["adventureID"] = "A123",
                    ["guildKey"] = "classicbetapve#home", ["character"] = "Tester",
                    ["snapshot"] = {{ ["time"] = {backup_saved_at}, ["gold"] = 1, ["items"] = {{}}, ["equipment"] = {{}} }},
                }} }},
            }} }} }}"#
        )
    }

    fn path() -> &'static str {
        "WTF/Account/ACC/Realm/Tester/SavedVariables/GuildSelfForever.lua"
    }

    #[test]
    fn reads_the_dedicated_baseline_in_the_contract_format() {
        let chars = vec![(path(), parse(character(1800, 500).as_bytes()).unwrap())];
        let list = audit_baselines(&parse(account(1700).as_bytes()).unwrap(), &chars);
        assert_eq!(list.len(), 1);
        let b = &list[0];
        assert_eq!(b.name, "tester");
        assert_eq!(b.checksum, "123456");
        assert_eq!(b.snapshot.items, vec![(2589, 20), (6948, 1)]);
        assert_eq!(
            b.snapshot.equipment,
            vec![(1, "1234:0".to_owned()), (16, "25:0".to_owned())]
        );
        // Banque sans heure : heure de la photo, comme `cloneSnapshot`.
        assert_eq!(b.snapshot.bank.as_ref().unwrap().time, 1800);
        let json = serde_json::to_value(b).unwrap();
        assert_eq!(json["snapshot"]["items"][0], serde_json::json!([2589, 20]));
        assert!(json["snapshot"].get("legacy").is_none());
    }

    #[test]
    fn the_newest_copy_wins_between_character_and_account_files() {
        let chars = vec![(path(), parse(character(1800, 500).as_bytes()).unwrap())];
        let list = audit_baselines(&parse(account(1900).as_bytes()).unwrap(), &chars);
        assert_eq!(list[0].checksum, "999");
        assert_eq!(list[0].saved_at, 1900);
    }

    #[test]
    fn rejects_records_the_addon_would_refuse() {
        let incomplete = character(1800, 500).replace(
            r#"["captureComplete"] = true"#,
            r#"["captureComplete"] = false"#,
        );
        let string_key = character(1800, 500).replace("[6948] = 1", r#"["6948"] = 1"#);
        let no_adventure = character(1800, 500).replace(r#"["adventureID"] = "A123","#, "");
        let too_rich = character(1800, 3_000_000_000);
        for src in [incomplete, string_key, no_adventure, too_rich] {
            let chars = vec![(path(), parse(src.as_bytes()).unwrap())];
            assert!(
                audit_baselines(&parse(b"").unwrap(), &chars).is_empty(),
                "{src}"
            );
        }
    }

    #[test]
    fn falls_back_to_the_character_folder_name() {
        let src = character(1800, 500).replace(r#"["character"] = "Tester","#, "");
        let chars = vec![(path(), parse(src.as_bytes()).unwrap())];
        let list = audit_baselines(&parse(b"").unwrap(), &chars);
        assert_eq!(list[0].name, "tester");
        assert_eq!(list[0].character, None);
        assert_eq!(
            character_dir_name(r"WTF\Account\A\R\Dävin\SavedVariables\x.lua"),
            Some("Dävin")
        );
    }

    #[test]
    fn played_fields_are_carried_outside_the_checksum() {
        // Contrat 2026-11-03 c : `played`/`playedObserved`/`level` (addon ≥ 0.22.11).
        let with = |extra: &str| {
            let src = character(1800, 500).replace(
                r#"["bagSlots"] = 64,"#,
                &format!(r#"["bagSlots"] = 64, {extra}"#),
            );
            let chars = vec![(path(), parse(src.as_bytes()).unwrap())];
            audit_baselines(&parse(b"").unwrap(), &chars).remove(0)
        };
        let plain = with("");
        let b = with(r#"["played"] = 51210, ["playedObserved"] = 120, ["level"] = 20,"#);
        assert_eq!(
            (
                b.snapshot.played,
                b.snapshot.played_observed,
                b.snapshot.level
            ),
            (Some(51_210), Some(120), Some(20))
        );
        assert_eq!(b.checksum, plain.checksum, "empreinte inchangée");
        let json = serde_json::to_value(&b).unwrap();
        assert_eq!(json["snapshot"]["played"], 51_210);
        assert_eq!(json["snapshot"]["played_observed"], 120);
        assert_eq!(json["snapshot"]["level"], 20);
        // Sans eux : JSON identique à celui d'un bridge 0.7.2.
        let json = serde_json::to_value(&plain).unwrap();
        for k in ["played", "played_observed", "level"] {
            assert!(json["snapshot"].get(k).is_none(), "{k}");
        }
        // Back-compat : une photo sans ces clés se relit.
        let back: AuditBaseline = serde_json::from_value(json).unwrap();
        assert_eq!(back, plain);

        // L'un sans l'autre, hors bornes : écartés seuls, la photo reste.
        for extra in [
            r#"["played"] = 51210, ["level"] = 0,"#,
            r#"["playedObserved"] = 5, ["level"] = 256,"#,
            r#"["played"] = -1, ["playedObserved"] = 5,"#,
            r#"["played"] = 5, ["playedObserved"] = 315360001,"#,
            r#"["played"] = 2147483647, ["playedObserved"] = 1,"#,
            r#"["played"] = "5", ["playedObserved"] = 5, ["level"] = "20","#,
        ] {
            let b = with(extra);
            assert_eq!(
                (
                    b.snapshot.played,
                    b.snapshot.played_observed,
                    b.snapshot.level
                ),
                (None, None, None),
                "{extra}"
            );
            assert!(b.is_valid());
        }
    }

    #[test]
    fn validation_matches_the_site_rules() {
        let chars = vec![(path(), parse(character(1800, 500).as_bytes()).unwrap())];
        let good = audit_baselines(&parse(b"").unwrap(), &chars).remove(0);
        assert!(good.is_valid());
        let mut bad = good.clone();
        bad.checksum = "not hex!".into();
        assert!(!bad.is_valid());
        let mut bad = good.clone();
        bad.snapshot.items = vec![(5, 1), (5, 2)];
        assert!(!bad.is_valid());
        let mut bad = good.clone();
        bad.name = "a~b".into();
        assert!(!bad.is_valid());
        let mut bad = good;
        bad.snapshot.items = (1..=1001).map(|i| (i, 1)).collect();
        assert!(!bad.is_valid());
    }
}
