//! Normalisation des vraies SavedVariables (addon 0.17.0 → 0.17.32).

mod common;

use std::collections::BTreeMap;

use gsf_core::model::{normalize, GuildData, TombstoneForm};
use gsf_core::sv::{parse, SavedVariables};

const ACTIVE_KEY: &str = "classic beta pve#mirabel france";
const STALE_KEY: &str = "classicbetapve#mirabel france";
const CHARACTERS: &[&str] = &[
    "char_Astral-Davin.lua",
    "char_Davin-Douze.lua",
    "char_Davin-Test.lua",
    "char_Dävin-Abysses.lua",
    "char_Fumée-Ravine.lua",
];

fn sv(name: &str) -> SavedVariables {
    parse(&common::fixture(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn characters() -> Vec<(String, SavedVariables)> {
    CHARACTERS.iter().map(|n| (n.to_string(), sv(n))).collect()
}

fn guild() -> GuildData {
    let mut guilds = normalize(&sv("account.lua"), &characters());
    assert_eq!(guilds.len(), 1, "une seule guilde après fusion");
    guilds.remove(0)
}

fn count<'a>(it: impl Iterator<Item = &'a str>) -> BTreeMap<&'a str, usize> {
    let mut m = BTreeMap::new();
    for s in it {
        *m.entry(s).or_default() += 1;
    }
    m
}

#[test]
fn raw_active_profile_has_45_requests() {
    let account = sv("account.lua");
    let profile = account
        .get("GuildFoundForeverDB")
        .and_then(|db| db.get("guilds"))
        .and_then(|g| g.get(ACTIVE_KEY))
        .expect("profil actif");
    let requests = profile.get("requests").and_then(|r| r.as_table()).unwrap();
    assert_eq!(requests.len(), 45);
    let status = count(requests.values().filter_map(|r| r.get("status")?.as_str()));
    assert_eq!(status, BTreeMap::from([("cancelled", 30), ("done", 15)]));
}

#[test]
fn duplicate_profiles_merge_into_one_guild() {
    let g = guild();
    assert_eq!(g.guild_key_normalized, "classicbetapve#mirabel france");
    assert_eq!(g.source_keys, [ACTIVE_KEY, STALE_KEY]);
    assert!(g.active);
    assert_eq!(g.guild_name, "Mirabel France");
    // Nom de royaume du profil actif (le plus récent).
    assert_eq!(g.realm, "Classic Beta PvE");
    // Champs ≥ 0.17.47 absents de ces fichiers 0.17.32.
    assert_eq!(g.writer_version, None);
    assert_eq!(g.bridge_roster, None);
    assert_eq!(g.bridge_link_outbox, None);
    assert_eq!(g.bridge_violations, None);
    assert_eq!(g.bridge_local_violations, None);
    assert_eq!(g.bridge_roster_at, None);
}

#[test]
fn requests_counts_by_status_and_kind() {
    let g = guild();
    assert_eq!(g.requests.len(), 45);
    let status = count(g.requests.iter().filter_map(|r| r.status.as_deref()));
    assert_eq!(status, BTreeMap::from([("cancelled", 30), ("done", 15)]));
    let kinds = count(g.requests.iter().filter_map(|r| r.kind.as_deref()));
    assert_eq!(
        kinds,
        BTreeMap::from([
            ("craft", 17),
            ("enchant", 8),
            ("specific", 6),
            ("starter_caster", 5),
            ("starter_physical", 9),
        ])
    );
    let r = g
        .requests
        .iter()
        .find(|r| r.id == "gsfr-0031-garou-1790664025-3549")
        .unwrap();
    assert_eq!(
        r.text.as_deref(),
        Some("[Ingénierie] Agrandisseur de monde")
    );
    assert_eq!(r.craft_category.as_deref(), Some("Catalogue intégré"));
    assert_eq!(r.request_number, Some(31));
    assert_eq!(r.item_id, Some(18660));
}

#[test]
fn garou_and_garou_dan_are_the_same_character() {
    let g = guild();
    // Demandeurs écrits « Garou » (26) ou « Garou Bel » (19).
    let raw = count(g.requests.iter().map(|r| r.requester.raw.as_str()));
    assert_eq!(raw, BTreeMap::from([("Garou", 26), ("Garou Bel", 19)]));
    assert!(g
        .requests
        .iter()
        .all(|r| r.requester.character.as_deref() == Some("garou bel")));
    // Approbation de banque et banque perso sous la clé « garou ».
    assert_eq!(g.bank_approvals.len(), 1);
    let a = &g.bank_approvals[0];
    assert_eq!(a.character.raw, "garou");
    assert_eq!(a.character.character.as_deref(), Some("garou bel"));
    assert_eq!(a.approved, Some(true));
    assert!(a.permanent);
}

#[test]
fn ambiguous_davin_is_not_attached() {
    let g = guild();
    let p = g
        .professions
        .iter()
        .find(|p| p.character.raw == "davin")
        .expect("métiers « davin »");
    assert_eq!(p.character.character, None);
    assert_eq!(p.character.candidates, ["davin douze", "davin test"]);
    assert_eq!(g.unresolved_aliases.len(), 1);
    assert_eq!(g.unresolved_aliases[0].alias, "davin");
    assert_eq!(g.unresolved_aliases[0].contexts, ["professions"]);
    // « fumée » est unique : rattaché.
    let f = g
        .professions
        .iter()
        .find(|p| p.character.raw == "fumée")
        .unwrap();
    assert_eq!(f.character.character.as_deref(), Some("fumée ravine"));
    // Aucun enregistrement n'est rattaché à « davin » seul.
    assert!(g
        .members
        .iter()
        .all(|m| m.character.character.as_deref() != Some("davin")));
}

#[test]
fn personal_bank_of_garou_has_32_occupied_slots() {
    let g = guild();
    let banks = &g.bank_snapshots.personal;
    assert_eq!(banks.len(), 1);
    assert_eq!(banks[0].owner.raw, "garou");
    assert_eq!(banks[0].owner.character.as_deref(), Some("garou bel"));
    assert_eq!(banks[0].slots.len(), 32);
    assert_eq!(banks[0].total_slots, Some(76));
    assert_eq!(banks[0].time, Some(1_790_716_755));
    assert!(g.bank_snapshots.guild_tabs.is_empty());
}

#[test]
fn rosters_merge_with_newest_record_winning() {
    let g = guild();
    let members: Vec<_> = g
        .members
        .iter()
        .map(|m| m.character.character.as_deref().unwrap())
        .collect();
    assert_eq!(
        members,
        [
            "astral",
            "davin douze",
            "davin test",
            "fumée ravine",
            "garou bel"
        ]
    );
    let garou = g
        .members
        .iter()
        .find(|m| m.character.raw == "garou bel")
        .unwrap();
    // Profil actif : lastSeen 1790761964 / 0.17.33 (périmé : 1790760035 / 0.17.32).
    assert_eq!(garou.last_seen, Some(1_790_761_964));
    assert_eq!(garou.last_version.as_deref(), Some("0.17.33"));

    assert_eq!(g.adventure_roster.len(), 4);
    let fumee = g
        .adventure_roster
        .iter()
        .find(|a| a.id.as_deref() == Some("A1929936150"))
        .unwrap();
    assert_eq!(fumee.class_file.as_deref(), Some("SHAMAN"));
    assert_eq!(fumee.last_guild_seen_at, Some(1_790_761_503));

    let prof = g
        .professions
        .iter()
        .find(|p| p.character.raw == "garou bel")
        .unwrap();
    assert_eq!(prof.seen, Some(1_790_760_582));
    assert_eq!(prof.list.len(), 4);
    assert_eq!(prof.list[0].name.as_deref(), Some("Blacksmithing"));
    assert_eq!(prof.list[0].rank, Some(79));

    assert_eq!(g.allied_guilds.len(), 1);
    assert_eq!(
        g.allied_guilds[0].name.as_deref(),
        Some("Beta Self Forever")
    );
}

#[test]
fn audits_reports_tombstones_and_pending() {
    let g = guild();
    // 5 rapports du profil actif + 6 du profil périmé, identifiants distincts.
    assert_eq!(g.audit_reports.len(), 11);
    let r = g
        .audit_reports
        .iter()
        .find(|r| r.id == "2074-857186")
        .unwrap();
    assert_eq!(r.player.character.as_deref(), Some("davin test"));
    assert_eq!(r.equipment.len(), 13);
    // Format stocké `afterItem`.
    let slot4 = r.equipment.iter().find(|e| e.slot == Some(4)).unwrap();
    assert_eq!(slot4.after.as_deref(), Some("6096:0"));
    assert_eq!(slot4.before, None);

    assert_eq!(g.audit_tombstones.len(), 23);
    assert!(g
        .audit_tombstones
        .iter()
        .all(|t| t.form == TombstoneForm::Number && t.at.is_some()));
    let by_player = count(
        g.audit_tombstones
            .iter()
            .filter_map(|t| t.player.character.as_deref()),
    );
    assert_eq!(
        by_player,
        BTreeMap::from([("davin test", 1), ("fumée ravine", 2), ("garou bel", 20)])
    );

    // File d'attente des persos (format `after`, sans `Item`).
    assert_eq!(g.audit_pending.len(), 2);
    let fumee = g
        .audit_pending
        .iter()
        .find(|p| p.source == "char_Fumée-Ravine.lua")
        .unwrap();
    assert_eq!(fumee.audits.len(), 4);
    let first = fumee.audits.iter().find(|a| a.id == "24210-74264").unwrap();
    assert_eq!(first.player.character.as_deref(), Some("fumée ravine"));
    assert_eq!(first.equipment.len(), 3);
    assert_eq!(first.equipment[0].after.as_deref(), Some("154:0"));
    assert_eq!(first.acked_by.get("garou bel"), Some(&1_790_673_263));
    let davin = g
        .audit_pending
        .iter()
        .find(|p| p.source == "char_Davin-Test.lua")
        .unwrap();
    assert_eq!(davin.audits.len(), 2);
}

#[test]
fn backup_lines_decode_and_agree_with_the_account() {
    let g = guild();
    assert_eq!(g.backup_requests.len(), 45);
    assert!(g.backup_requests.iter().all(|b| b.field_count == 13));
    for b in &g.backup_requests {
        let a = g
            .requests
            .iter()
            .find(|r| r.id == b.request.id)
            .unwrap_or_else(|| panic!("{} absent du compte", b.request.id));
        assert_eq!(a.status, b.request.status, "{}", a.id);
        assert_eq!(a.kind, b.request.kind, "{}", a.id);
        assert_eq!(a.text, b.request.text, "{}", a.id);
        assert_eq!(a.updated_at, b.request.updated_at, "{}", a.id);
        assert_eq!(a.requester.character, b.request.requester.character);
    }
    let enchant = g
        .backup_requests
        .iter()
        .find(|b| b.request.id == "gsfr-0043-garou-1790755692-4227")
        .unwrap();
    assert_eq!(
        enchant.request.text.as_deref(),
        Some("Enchantement : Arme — +55 puissance de soins")
    );
}

#[test]
fn backup_account_file_normalizes_too() {
    let guilds = normalize(&sv("account.lua.bak"), &characters());
    assert_eq!(guilds.len(), 1);
    assert_eq!(guilds[0].source_keys.len(), 2);
    assert!(!guilds[0].requests.is_empty());
}

#[test]
fn character_files_alone_still_yield_the_guild() {
    let guilds = normalize(&SavedVariables::default(), &characters());
    assert_eq!(guilds.len(), 1);
    let g = &guilds[0];
    assert_eq!(g.guild_key_normalized, "classicbetapve#mirabel france");
    assert!(!g.active);
    assert!(g.requests.is_empty());
    assert_eq!(g.backup_requests.len(), 45);
    assert_eq!(g.audit_pending.len(), 2);
}

#[test]
fn json_serialization_is_stable() {
    let a = serde_json::to_string(&guild()).unwrap();
    let b = serde_json::to_string(&guild()).unwrap();
    assert_eq!(a, b);
    let v: serde_json::Value = serde_json::from_str(&a).unwrap();
    assert_eq!(v["guild_key_normalized"], "classicbetapve#mirabel france");
    assert!(v["writer_version"].is_null());
}

#[test]
fn any_fixture_in_any_role_never_panics() {
    let all: Vec<(String, SavedVariables)> = common::fixtures()
        .into_iter()
        .map(|(n, b)| (n, parse(&b).unwrap()))
        .collect();
    for (_, account) in &all {
        let _ = normalize(account, &all);
        let _ = normalize(account, &[] as &[(&str, SavedVariables)]);
    }
}
