//! Variations de format entre versions de l'addon, sur des fichiers
//! synthétiques.

use gsf_core::model::{normalize, HolderKind, TombstoneForm};
use gsf_core::sv::{parse, SavedVariables};

fn sv(src: &str) -> SavedVariables {
    parse(src.as_bytes()).unwrap_or_else(|e| panic!("{e}"))
}

const NO_CHARS: &[(&str, SavedVariables)] = &[];

#[test]
fn blacklist_outbox_is_normalized_from_the_guild_profile() {
    let account = sv(r#"
GuildFoundForeverDB = { ["guilds"] = { ["realm#g"] = {
  ["guildName"] = "G", ["realmName"] = "Realm",
  ["blacklistOutbox"] = {
    ["1800000000-12345678"] = { ["operation"] = "ban", ["targetType"] = "battle_tag",
      ["targetKey"] = "fa2ce2459aec6c5c0d72ca5991cd4cfd", ["targetLabel"] = "Davin#54321" },
  },
} } }
"#);
    let guilds = normalize(&account, NO_CHARS);
    assert_eq!(guilds.len(), 1);
    let actions = &guilds[0].blacklist_actions;
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].id, "1800000000-12345678");
    assert_eq!(actions[0].target_type, "battle_tag");
    assert_eq!(actions[0].target_label, "Davin#54321");
}

#[test]
fn newest_record_wins_across_duplicate_profiles() {
    let account = sv(r#"
GuildFoundForeverDB = {
  ["lastActiveGuildKey"] = "realm one#g",
  ["guilds"] = {
    ["realm one#g"] = {
      ["guildName"] = "G", ["realmName"] = "Realm One",
      ["requests"] = {
        ["r1"] = { ["id"] = "r1", ["status"] = "active", ["updatedAt"] = 10, ["requester"] = "Ann Bee" },
        ["r2"] = { ["id"] = "r2", ["status"] = "done", ["updatedAt"] = 5, ["requester"] = "Ann" },
      },
      ["memberPresence"] = { ["ann bee"] = { ["name"] = "Ann Bee", ["lastSeen"] = 100 } },
    },
    ["realmone#g"] = {
      ["guildName"] = "G", ["realmName"] = "RealmOne",
      ["requests"] = {
        ["r1"] = { ["id"] = "r1", ["status"] = "done", ["updatedAt"] = 20, ["requester"] = "Ann" },
        ["r2"] = { ["id"] = "r2", ["status"] = "cancelled", ["updatedAt"] = 4 },
        ["r3"] = { ["id"] = "r3", ["status"] = "active", ["updatedAt"] = 1 },
      },
      ["memberPresence"] = { ["ann bee"] = { ["name"] = "Ann Bee", ["lastSeen"] = 50 } },
    },
  },
}
"#);
    let g = normalize(&account, NO_CHARS);
    assert_eq!(g.len(), 1);
    let g = &g[0];
    assert_eq!(g.guild_key_normalized, "realmone#g");
    assert_eq!(g.realm, "Realm One", "profil actif traité en dernier");
    let status: Vec<_> = g
        .requests
        .iter()
        .map(|r| (r.id.as_str(), r.status.as_deref().unwrap()))
        .collect();
    assert_eq!(status, [("r1", "done"), ("r2", "done"), ("r3", "active")]);
    assert_eq!(
        g.requests[0].requester.character.as_deref(),
        Some("ann bee")
    );
    assert_eq!(g.requests[2].requester.character, None, "demandeur absent");
    assert_eq!(g.members.len(), 1);
    assert_eq!(g.members[0].last_seen, Some(100));
}

#[test]
fn tombstones_number_and_table_forms() {
    let account = sv(r#"
GuildFoundForeverDB = { ["guilds"] = { ["r#g"] = {
  ["auditTombstones"] = {
    ["ann bee:1-2"] = 100,
    ["ann bee:3-4"] = { ["at"] = 200, ["by"] = "Off Icer", ["adventureID"] = "A9", ["player"] = "Ann Bee" },
    ["bogus"] = 5,
    ["ann bee:5-6"] = { ["by"] = "" },
  },
} } }
"#);
    let g = &normalize(&account, NO_CHARS)[0];
    assert_eq!(g.audit_tombstones.len(), 3);
    let t1 = g
        .audit_tombstones
        .iter()
        .find(|t| t.report_id == "1-2")
        .unwrap();
    assert_eq!(
        (t1.form, t1.at, t1.by.as_deref()),
        (TombstoneForm::Number, Some(100), None)
    );
    let t2 = g
        .audit_tombstones
        .iter()
        .find(|t| t.report_id == "3-4")
        .unwrap();
    assert_eq!(t2.form, TombstoneForm::Table);
    assert_eq!(t2.at, Some(200));
    assert_eq!(t2.by.as_deref(), Some("Off Icer"));
    assert_eq!(t2.adventure_id.as_deref(), Some("A9"));
    assert_eq!(t2.player.character.as_deref(), Some("ann bee"));
    let t3 = g
        .audit_tombstones
        .iter()
        .find(|t| t.report_id == "5-6")
        .unwrap();
    assert_eq!((t3.at, t3.by.as_deref()), (None, None));
}

#[test]
fn equipment_before_after_and_before_item_after_item() {
    let account = sv(r#"
GuildFoundForeverDB = { ["guilds"] = { ["r#g"] = {
  ["auditReports"] = {
    { ["id"] = "a", ["player"] = "Ann Bee", ["goldBefore"] = 10, ["goldAfter"] = 4,
      ["equipment"] = { { ["slot"] = 1, ["beforeItem"] = "1:0", ["afterItem"] = "2:0" } } },
    { ["id"] = "b", ["player"] = "Ann Bee",
      ["equipment"] = { { ["slot"] = 2, ["before"] = "3:0", ["after"] = "4:5" }, { ["slot"] = 3 } } },
  },
} } }
"#);
    let g = &normalize(&account, NO_CHARS)[0];
    let a = &g.audit_reports[0];
    assert_eq!(a.equipment[0].before.as_deref(), Some("1:0"));
    assert_eq!(a.equipment[0].after.as_deref(), Some("2:0"));
    assert_eq!(a.gold_delta, Some(-6), "calculé si absent");
    let b = &g.audit_reports[1];
    assert_eq!(b.equipment[0].before.as_deref(), Some("3:0"));
    assert_eq!(b.equipment[0].after.as_deref(), Some("4:5"));
    assert_eq!(b.equipment[1].before, None);
    assert_eq!(b.gold_delta, None);
}

#[test]
fn backup_lines_with_13_and_15_fields() {
    let character = sv(r#"
GuildFoundForeverCharacterDB = {
  ["criticalStateBackups"] = {
    ["realm#g"] = {
      ["guildKey"] = "realm#g", ["guildName"] = "G", ["realm"] = "Realm", ["savedAt"] = 99,
      ["requests"] = {
        "r1~craft~%5BForge%5D x~Ann~10~active~11~0~~164~3319~Cat~3469~Ann Bee~12",
        "r2~specific~a%7Eb~Ann Bee~10~done~11~11~Off~0~0~~0",
        "r3~broken",
        "r4~k~t~r~x~s~1~2~d~3~4~c~5",
      },
      ["auditCleanupRows"] = { "ann bee~9-9~300~Off~A1", "bad" },
    },
  },
}
"#);
    let chars = [("perso.lua", character)];
    let g = &normalize(&SavedVariables::default(), &chars)[0];
    assert_eq!(g.guild_name, "G");
    assert_eq!(g.backup_requests.len(), 2);
    let r1 = &g.backup_requests[0];
    assert_eq!(r1.field_count, 15);
    assert_eq!(r1.request.text.as_deref(), Some("[Forge] x"));
    assert_eq!(r1.request.taken_by.as_deref(), Some("Ann Bee"));
    assert_eq!(r1.request.taken_at, Some(12));
    assert_eq!(r1.request.decided_by, None);
    assert_eq!(r1.request.requester.character.as_deref(), Some("ann bee"));
    assert_eq!(r1.source, "perso.lua");
    assert_eq!(r1.backup_saved_at, Some(99));
    let r2 = &g.backup_requests[1];
    assert_eq!(r2.field_count, 13);
    assert_eq!(r2.request.text.as_deref(), Some("a~b"));
    assert_eq!(r2.request.taken_by, None);
    assert_eq!(g.audit_tombstones.len(), 1);
    assert_eq!(g.audit_tombstones[0].form, TombstoneForm::BackupRow);
    assert_eq!(g.audit_tombstones[0].at, Some(300));
}

#[test]
fn bridge_exports_real_shapes_from_addon_0_17_47() {
    // Formes exactes de BridgeExport.lua : tables indexées par clé de roster,
    // par code ou par identifiant de violation.
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/synthetic_account_0.17.47.lua"
    );
    let account = parse(&std::fs::read(path).unwrap()).unwrap();
    let g = normalize(&account, NO_CHARS);
    assert_eq!(g.len(), 1, "profils doublés fusionnés");
    let g = &g[0];
    assert_eq!(g.writer_version.as_deref(), Some("0.17.47"));
    assert_eq!(g.writer_saved_at, Some(1_800_000_500));
    assert_eq!(g.bridge_roster_at, Some(1_800_000_400));

    // Roster : le plus récent `seenAt` gagne par personnage.
    let roster = g.bridge_roster.as_ref().unwrap();
    let ranks: Vec<_> = roster
        .iter()
        .map(|e| {
            (
                e.character.character.as_deref().unwrap(),
                e.rank_index.unwrap(),
            )
        })
        .collect();
    assert_eq!(
        ranks,
        [
            ("davin test", 3),
            ("garou bel", 0),
            ("lunérie delacourt", 4)
        ]
    );
    let garou = &roster[1];
    assert_eq!(garou.name.as_deref(), Some("Garou Bel"));
    assert_eq!(garou.rank_name.as_deref(), Some("Maître de guilde"));
    assert_eq!(garou.class_file.as_deref(), Some("PALADIN"));
    assert_eq!(garou.level, Some(60));
    assert_eq!(roster[0].rank_name, None, "champ facultatif absent");

    // Dernière connexion (addon ≥ 0.17.53) : la plus récente de tous les
    // relevés, même si le reste de l'entrée vient d'un relevé plus récent.
    assert_eq!(garou.last_online_at, Some(1_800_000_300));
    assert_eq!(roster[0].last_online_at, Some(1_800_000_450));
    assert_eq!(roster[2].last_online_at, None, "champ absent");

    // Audits relayés (addon ≥ 0.17.53) : le direct l'emporte sur le relayé
    // même plus récent ; un audit seulement relayé est gardé avec son relais.
    let audits: Vec<_> = g
        .audit_reports
        .iter()
        .map(|a| {
            (
                a.player.character.as_deref().unwrap(),
                a.id.as_str(),
                a.to.unwrap(),
                a.relayed_by.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        audits,
        [
            ("davin test", "1800000100-1", 1_800_000_100, None),
            (
                "lunérie delacourt",
                "1800000150-2",
                1_800_000_150,
                Some("Davin Test")
            ),
        ]
    );

    // Liaisons : par code, premier émetteur gardé, émetteur différent = conflit.
    let outbox = g.bridge_link_outbox.as_ref().unwrap();
    assert_eq!(outbox.len(), 2);
    let abc = &outbox[0];
    assert_eq!(abc.code, "GSF-ABC123");
    assert_eq!(abc.at, Some(1_800_000_050));
    assert_eq!(abc.sender_name.as_deref(), Some("Garou"));
    assert_eq!(
        abc.sender.character.as_deref(),
        Some("garou bel"),
        "senderKey prime sur le nom affiché"
    );
    assert!(abc.conflict);
    let zzz = &outbox[1];
    assert!(zzz.conflict, "drapeau `conflict` de l'addon conservé");
    assert_eq!(zzz.sender.character.as_deref(), Some("fumée ravine"));

    // Violations reçues : dédoublonnées par identifiant, `receivedAt` le plus récent.
    let v = g.bridge_violations.as_ref().unwrap();
    assert_eq!(v.len(), 2);
    let recruit = &v[0];
    assert_eq!(recruit.kind.as_deref(), Some("recruit_level"));
    // Seul « Davin Test » est connu dans ce fichier : l'alias est unique.
    assert_eq!(recruit.player.character.as_deref(), Some("davin test"));
    assert!(g.unresolved_aliases.is_empty());
    let trade = &v[1];
    assert_eq!(trade.id, "trade_gold-1800000300-123456");
    assert_eq!(trade.player.character.as_deref(), Some("davin test"));
    assert_eq!(trade.player_name.as_deref(), Some("Davin Test"));
    assert_eq!(trade.details.as_deref(), Some("50g vers Inconnu"));
    assert_eq!(trade.received_at, Some(1_800_000_310));
    assert_eq!(trade.reporter.as_deref(), Some("Davin Test"));
    assert_eq!(trade.relayed, None);

    // Journal local du compte.
    let local = g.bridge_local_violations.as_ref().unwrap();
    assert_eq!(local.len(), 1);
    assert_eq!(local[0].kind.as_deref(), Some("mail_send"));
    assert_eq!(local[0].player.character.as_deref(), Some("garou bel"));
    assert_eq!(local[0].relayed, Some(false));
    assert_eq!(local[0].received_at, None);

    let json = serde_json::to_value(g).unwrap();
    assert_eq!(json["bridge_roster"][1]["last_online_at"], 1_800_000_300);
    assert!(json["audit_reports"][0]["relayed_by"].is_null());
    assert_eq!(json["audit_reports"][1]["relayed_by"], "Davin Test");
    assert!(json["bridge_violations"][0].get("relayed").is_none());
    assert_eq!(json["bridge_local_violations"][0]["relayed"], false);
}

#[test]
fn direct_audit_beats_relayed_copies_whatever_the_order() {
    // Le profil actif est traité en dernier : on essaie les deux ordres.
    for (direct_in, relayed_in) in [("r one#g", "rone#g"), ("rone#g", "r one#g")] {
        let account = sv(&format!(
            r#"
GuildFoundForeverDB = {{
  ["lastActiveGuildKey"] = "r one#g",
  ["guilds"] = {{
    ["{direct_in}"] = {{ ["auditReports"] = {{
      {{ ["id"] = "a", ["player"] = "Ann Bee", ["to"] = 10 }},
    }} }},
    ["{relayed_in}"] = {{ ["auditReports"] = {{
      {{ ["id"] = "a", ["player"] = "Ann Bee", ["to"] = 99, ["relayedBy"] = "Bob" }},
      {{ ["id"] = "b", ["player"] = "Ann Bee", ["to"] = 5, ["relayedBy"] = "Bob" }},
      {{ ["id"] = "b", ["player"] = "Ann Bee", ["to"] = 7, ["relayedBy"] = "Cid" }},
      {{ ["id"] = "c", ["player"] = "Ann Bee", ["to"] = 1, ["relayedBy"] = "" }},
    }} }},
  }},
}}
"#
        ));
        let g = &normalize(&account, NO_CHARS)[0];
        let got: Vec<_> = g
            .audit_reports
            .iter()
            .map(|a| (a.id.as_str(), a.to, a.relayed_by.as_deref()))
            .collect();
        assert_eq!(
            got,
            [
                ("a", Some(10), None),
                ("b", Some(7), Some("Cid")),
                ("c", Some(1), None),
            ],
            "direct dans {direct_in}"
        );
    }
}

#[test]
fn played_fields_of_audit_reports_are_read_and_checked() {
    // Contrat 2026-11-03 c : rapport « joué sans l'addon » (addon ≥ 0.22.11).
    let account = sv(r#"
GuildFoundForeverDB = { ["guilds"] = { ["r#g"] = {
  ["auditReports"] = {
    { ["id"] = "p", ["player"] = "Ann Bee", ["changeCount"] = 1,
      ["playedBefore"] = 51210, ["playedAfter"] = 53910, ["levelBefore"] = 20, ["levelAfter"] = 22 },
    { ["id"] = "old", ["player"] = "Ann Bee", ["changeCount"] = 2 },
    { ["id"] = "down", ["player"] = "Ann Bee", ["playedBefore"] = 500, ["playedAfter"] = 500,
      ["levelBefore"] = 0, ["levelAfter"] = 256 },
    { ["id"] = "alone", ["player"] = "Ann Bee", ["playedAfter"] = 900, ["levelAfter"] = "22" },
    { ["id"] = "huge", ["player"] = "Ann Bee", ["playedBefore"] = -1, ["playedAfter"] = 2147483648 },
  },
  ["bridgeCarriedAudits"] = {
    ["ann bee:p"] = { ["player"] = "Ann Bee", ["id"] = "p",
      ["payload"] = "1800000000,1800090000,562,562,,0|P,51210,53910,20,22" },
  },
} } }
"#);
    let g = &normalize(&account, NO_CHARS)[0];
    let got: Vec<_> = g
        .audit_reports
        .iter()
        .map(|a| {
            (
                a.id.as_str(),
                a.played_before,
                a.played_after,
                a.level_before,
                a.level_after,
            )
        })
        .collect();
    assert_eq!(
        got,
        [
            // Triés par identifiant.
            ("alone", None, None, None, None),
            ("down", None, None, None, None),
            ("huge", None, None, None, None),
            ("old", None, None, None, None),
            ("p", Some(51_210), Some(53_910), Some(20), Some(22)),
        ]
    );
    // Le jeton `P` des audits portés voyage tel quel (le site le décode).
    let carried = g.bridge_carried_audits.as_ref().unwrap();
    assert!(carried[0].payload.ends_with("|P,51210,53910,20,22"));

    let json = serde_json::to_value(g).unwrap();
    let p = &json["audit_reports"][4];
    assert_eq!(p["played_before"], 51_210);
    assert_eq!(p["played_after"], 53_910);
    assert_eq!(p["level_before"], 20);
    assert_eq!(p["level_after"], 22);
    // Ancien rapport : aucune clé ajoutée (même JSON, même hash qu'avant).
    let old = json["audit_reports"][3].as_object().unwrap();
    for k in [
        "played_before",
        "played_after",
        "level_before",
        "level_after",
    ] {
        assert!(!old.contains_key(k), "{k}");
    }
}

#[test]
fn unexpected_types_are_tolerated() {
    let account = sv(r#"
GuildFoundForeverDB = { ["guilds"] = {
  ["r#g"] = {
    ["guildName"] = 5, ["requests"] = 5, ["memberPresence"] = "x",
    ["auditReports"] = { 1, "x", { ["player"] = {} }, { ["id"] = 42 } },
    ["auditTombstones"] = { [1] = 2, ["a:b"] = "x" },
    ["bankSnapshots"] = { ["personal"] = { ["x"] = { ["slots"] = { [1] = 3, ["k"] = {} } } }, ["guild"] = 1 },
    ["professions"] = { ["x"] = { ["list"] = { 1, { ["rank"] = "high" } } } },
    ["alliedGuilds"] = { ["a"] = 1 },
    ["bridgeRoster"] = { 1, 2 },
    ["bridgeCarriedAudits"] = 5,
  },
  ["bad"] = 1,
  [3] = {},
} }
"#);
    let g = normalize(&account, NO_CHARS);
    assert_eq!(g.len(), 1);
    let g = &g[0];
    assert_eq!(g.guild_name, "");
    assert!(g.requests.is_empty() && g.members.is_empty());
    assert_eq!(g.audit_reports.len(), 1);
    assert_eq!(g.audit_reports[0].id, "42");
    assert_eq!(g.audit_tombstones.len(), 1);
    assert_eq!(g.audit_tombstones[0].at, None);
    assert!(g.bank_snapshots.personal[0].slots.is_empty());
    assert_eq!(g.professions[0].list.len(), 1);
    assert_eq!(g.professions[0].list[0].rank, None);
    assert!(g.allied_guilds.is_empty());
    assert_eq!(g.bridge_roster.as_deref().map(<[_]>::len), Some(0));
    assert_eq!(
        g.bridge_carried_audits, None,
        "type racine invalide, ignoré"
    );

    // Racine d'un mauvais type, fichier vide.
    assert!(normalize(&sv("GuildFoundForeverDB = 3"), NO_CHARS).is_empty());
    assert!(normalize(&SavedVariables::default(), NO_CHARS).is_empty());
}

#[test]
fn carried_audits_are_collected_and_tolerant_to_malformed_entries() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/synthetic_bridge_carried_audits.lua"
    );
    let account = parse(&std::fs::read(path).unwrap()).unwrap();
    let g = &normalize(&account, NO_CHARS)[0];
    let carried = g.bridge_carried_audits.as_ref().unwrap();
    assert_eq!(carried.len(), 1, "{carried:?}");
    let c = &carried[0];
    assert_eq!(c.player, "Davin Test");
    assert_eq!(c.id, "1800000100-1");
    assert_eq!(c.payload, "opaque-serialized-audit-blob");
    assert_eq!(c.carried_at, Some(1_800_000_500));

    let json = serde_json::to_value(g).unwrap();
    assert_eq!(json["bridge_carried_audits"][0]["player"], "Davin Test");
    assert_eq!(
        json["bridge_carried_audits"][0]["carried_at"],
        1_800_000_500
    );
}

#[test]
fn legacy_flat_account_layout() {
    let account = sv(r#"
GuildFoundForeverDB = { ["guildName"] = "G", ["realmName"] = "Old Realm",
  ["memberPresence"] = { ["ann bee"] = { ["lastSeen"] = 1 } } }
"#);
    let g = &normalize(&account, NO_CHARS)[0];
    assert_eq!(g.guild_key_normalized, "oldrealm#g");
    assert_eq!(g.members.len(), 1);
}

#[test]
fn requests_carry_released_and_close_reason_fields() {
    let account = sv(r#"
GuildFoundForeverDB = { ["guilds"] = { ["r#g"] = {
  ["requests"] = {
    ["r1"] = { ["id"] = "r1", ["status"] = "cancelled", ["updatedAt"] = 10,
      ["requester"] = "Ann", ["releasedBy"] = "Ann", ["releasedAt"] = 9,
      ["closeReason"] = "expired" },
    ["r2"] = { ["id"] = "r2", ["status"] = "active", ["updatedAt"] = 1 },
  },
} } }
"#);
    let g = &normalize(&account, NO_CHARS)[0];
    let r1 = g.requests.iter().find(|r| r.id == "r1").unwrap();
    assert_eq!(r1.released_by.as_deref(), Some("Ann"));
    assert_eq!(r1.released_at, Some(9));
    assert_eq!(r1.close_reason.as_deref(), Some("expired"));
    let r2 = g.requests.iter().find(|r| r.id == "r2").unwrap();
    assert_eq!(
        (
            r2.released_by.as_ref(),
            r2.released_at,
            r2.close_reason.as_ref()
        ),
        (None, None, None),
        "champs facultatifs absents"
    );
}

#[test]
fn backup_request_line_carries_released_fields_but_never_close_reason() {
    // Ligne à 17 champs (`GSF:ReleaseGuildRequest`, addon ≥ 0.19.30) :
    // `releasedBy`/`releasedAt` décodés, mais `closeReason` n'existe pas dans
    // ce format, donc toujours `None` côté sauvegarde.
    let character = sv(r#"
GuildFoundForeverCharacterDB = { ["criticalStateBackups"] = { ["realm#g"] = {
  ["guildKey"] = "realm#g",
  ["requests"] = {
    "gsfr-0050-garou-1790800000-1234~craft~Sac en toile~Garou~1790800000~active~1790800550~0~~197~12345~Sacs~6256~~0~Garou~1790800550",
  },
} } }
"#);
    let chars = [("perso.lua", character)];
    let g = &normalize(&SavedVariables::default(), &chars)[0];
    assert_eq!(g.backup_requests.len(), 1);
    let r = &g.backup_requests[0].request;
    assert_eq!(r.released_by.as_deref(), Some("Garou"));
    assert_eq!(r.released_at, Some(1_790_800_550));
    assert_eq!(r.close_reason, None);
}

#[test]
fn request_tombstones_and_purge_before_merge_across_profiles() {
    let account = sv(r#"
GuildFoundForeverDB = {
  ["lastActiveGuildKey"] = "r one#g",
  ["guilds"] = {
    ["r one#g"] = {
      ["requestTombstones"] = {
        ["r1"] = { ["status"] = "cancelled", ["updatedAt"] = 20 },
      },
      ["requestPurgeBefore"] = 5,
    },
    ["rone#g"] = {
      ["requestTombstones"] = {
        ["r1"] = { ["status"] = "expired", ["updatedAt"] = 50 },
        ["r2"] = { ["status"] = "done", ["updatedAt"] = 1 },
      },
      ["requestPurgeBefore"] = 99,
    },
  },
}
"#);
    let g = &normalize(&account, NO_CHARS)[0];
    assert_eq!(g.request_purge_before, Some(99), "le plus récent des deux");
    let r1 = g.request_tombstones.iter().find(|t| t.id == "r1").unwrap();
    assert_eq!(
        r1.status.as_deref(),
        Some("expired"),
        "updatedAt le plus récent gagne"
    );
    assert_eq!(r1.updated_at, Some(50));
    assert_eq!(g.request_tombstones.len(), 2);
}

#[test]
fn death_log_is_deduplicated_by_id_keeping_the_latest() {
    let account = sv(r#"
GuildFoundForeverDB = {
  ["lastActiveGuildKey"] = "r one#g",
  ["guilds"] = {
    ["r one#g"] = {
      ["deathLog"] = {
        ["realm-100"] = { ["id"] = "realm-100", ["name"] = "Ann Bee", ["realm"] = "Realm",
          ["guildKey"] = "r one#g", ["guildName"] = "G", ["classFile"] = "MAGE", ["race"] = "Human",
          ["level"] = 60, ["zone"] = "Orgrimmar", ["killer"] = "Falling",
          ["killerKind"] = "environment", ["at"] = 100, ["reporter"] = "Ann Bee" },
      },
    },
    ["rone#g"] = {
      ["deathLog"] = {
        ["realm-100"] = { ["id"] = "realm-100", ["name"] = "Ann Bee", ["at"] = 100, ["killer"] = "Bob",
          ["killerKind"] = "player" },
        ["realm-200"] = { ["id"] = "realm-200", ["name"] = "Cid", ["at"] = 200, ["killer"] = "Rawr",
          ["killerKind"] = "npc" },
      },
    },
  },
}
"#);
    let g = &normalize(&account, NO_CHARS)[0];
    assert_eq!(g.deaths.len(), 2, "{:?}", g.deaths);
    let d100 = g.deaths.iter().find(|d| d.id == "realm-100").unwrap();
    // Égalité de `at` : le profil actif (traité en dernier) gagne.
    assert_eq!(d100.killer.as_deref(), Some("Falling"));
    assert_eq!(d100.killer_kind.as_deref(), Some("environment"));
    assert_eq!(d100.zone.as_deref(), Some("Orgrimmar"));
    let d200 = g.deaths.iter().find(|d| d.id == "realm-200").unwrap();
    assert_eq!(d200.name.as_deref(), Some("Cid"));
    assert_eq!(d200.zone, None, "champ facultatif absent");
}

#[test]
fn member_identity_merges_by_latest_at_and_validates_hex_fields() {
    let account = sv(r#"
GuildFoundForeverDB = {
  ["lastActiveGuildKey"] = "r one#g",
  ["guilds"] = {
    ["r one#g"] = {
      ["memberIdentity"] = {
        ["ann bee"] = { ["bt"] = "fa2ce2459aec6c5c0d72ca5991cd4cfd", ["acct"] = "0123456789abcdef", ["at"] = 50 },
        ["cid"] = { ["bt"] = "FA2CE2459AEC6C5C0D72CA5991CD4CFD", ["acct"] = "0123456789abcdef", ["at"] = 10 },
        ["dee"] = { ["acct"] = "short", ["at"] = 5 },
      },
    },
    ["rone#g"] = {
      ["memberIdentity"] = {
        ["ann bee"] = { ["at"] = 100 },
      },
    },
  },
}
"#);
    let g = &normalize(&account, NO_CHARS)[0];
    assert_eq!(g.member_identities.len(), 3, "{:?}", g.member_identities);

    let ann = g
        .member_identities
        .iter()
        .find(|m| m.character.character.as_deref() == Some("ann bee"))
        .unwrap();
    assert_eq!(
        ann.at, 100,
        "le plus récent `at` gagne, tout l'enregistrement"
    );
    assert_eq!(ann.bt, None, "le relevé le plus récent n'a pas de bt");
    assert_eq!(ann.acct, None);

    let cid = g
        .member_identities
        .iter()
        .find(|m| m.character.character.as_deref() == Some("cid"))
        .unwrap();
    assert_eq!(cid.bt, None, "majuscules refusées, champ seul écarté");
    assert_eq!(
        cid.acct.as_deref(),
        Some("0123456789abcdef"),
        "l'entrée reste malgré un bt invalide"
    );
    assert_eq!(cid.at, 10);

    let dee = g
        .member_identities
        .iter()
        .find(|m| m.character.character.as_deref() == Some("dee"))
        .unwrap();
    assert_eq!(dee.acct, None, "longueur invalide, champ seul écarté");
    assert_eq!(dee.bt, None, "absent");
    assert_eq!(dee.at, 5);
}

#[test]
fn member_identity_at_defaults_to_zero_when_missing() {
    let account = sv(r#"
GuildFoundForeverDB = { ["guilds"] = { ["r#g"] = {
  ["memberIdentity"] = {
    ["ann bee"] = { ["bt"] = "fa2ce2459aec6c5c0d72ca5991cd4cfd" },
  },
} } }
"#);
    let g = &normalize(&account, NO_CHARS)[0];
    assert_eq!(g.member_identities.len(), 1);
    assert_eq!(g.member_identities[0].at, 0);
    assert_eq!(
        g.member_identities[0].bt.as_deref(),
        Some("fa2ce2459aec6c5c0d72ca5991cd4cfd")
    );
    assert_eq!(g.member_identities[0].acct, None);
}

#[test]
fn member_identity_btag_is_no_longer_forwarded() {
    // Contrat 2026-11-10 : `memberIdentity[].btag` n'est plus transmis au site (remplacé par
    // `self_identity`, le propre BattleTag envoyé par le propre bridge du joueur). Le champ reste `None`
    // même si la SavedVariables porte un `btag` valide (gardé une version pour compatibilité descendante).
    let account = sv(r#"
GuildFoundForeverDB = { ["guilds"] = { ["r#g"] = {
  ["memberIdentity"] = {
    ["ann bee"] = { ["bt"] = "fa2ce2459aec6c5c0d72ca5991cd4cfd", ["btag"] = "Davin#54321", ["at"] = 1 },
  },
} } }
"#);
    let g = &normalize(&account, NO_CHARS)[0];
    assert_eq!(g.member_identities.len(), 1);
    assert_eq!(g.member_identities[0].btag, None);
    assert_eq!(
        g.member_identities[0].bt.as_deref(),
        Some("fa2ce2459aec6c5c0d72ca5991cd4cfd"),
        "bt continue d'être transmis"
    );
    let json = serde_json::to_value(g).unwrap();
    assert!(json["member_identities"][0]["btag"].is_null());
}

#[test]
fn adventure_roster_flagged_fields_for_cheater_and_to_ban() {
    let account = sv(r#"
GuildFoundForeverDB = { ["guilds"] = { ["r#g"] = {
  ["adventureRoster"] = {
    ["ann bee"] = { ["name"] = "Ann Bee", ["status"] = "CHEATER",
      ["flaggedAt"] = 10, ["flaggedBy"] = "Off Icer", ["flagSource"] = "audit" },
    ["cid"] = { ["name"] = "Cid", ["status"] = "TO_BAN", ["flagSource"] = "site-ban" },
  },
} } }
"#);
    let g = &normalize(&account, NO_CHARS)[0];
    let ann = g
        .adventure_roster
        .iter()
        .find(|m| m.name.as_deref() == Some("Ann Bee"))
        .unwrap();
    assert_eq!(ann.status.as_deref(), Some("CHEATER"));
    assert_eq!(ann.flagged_at, Some(10));
    assert_eq!(ann.flagged_by.as_deref(), Some("Off Icer"));
    assert_eq!(ann.flag_source.as_deref(), Some("audit"));
    let cid = g
        .adventure_roster
        .iter()
        .find(|m| m.name.as_deref() == Some("Cid"))
        .unwrap();
    assert_eq!(cid.status.as_deref(), Some("TO_BAN"));
    assert_eq!(cid.flag_source.as_deref(), Some("site-ban"));
    assert_eq!(cid.flagged_at, None);
}

#[test]
fn bank_item_info_merged_newest_wins_and_skips_nameless() {
    let account = sv(r#"
GuildFoundForeverDB = {
  ["lastActiveGuildKey"] = "r one#g",
  ["guilds"] = {
    ["r one#g"] = {
      ["bankItemInfo"] = {
        [288841] = { ["n"] = "Ancien nom", ["q"] = 3, ["b"] = 1, ["r"] = 25, ["l"] = 30, ["at"] = 10 },
        [288842] = { ["n"] = "Lame du givre", ["q"] = 4, ["b"] = 2, ["r"] = 30, ["l"] = 35, ["at"] = 20 },
        [288843] = { ["q"] = 4, ["at"] = 20 },
        [288844] = { ["n"] = "   ", ["at"] = 20 },
      },
    },
    ["rone#g"] = {
      ["bankItemInfo"] = {
        [288841] = { ["n"] = "Épée de la tempête", ["q"] = 4, ["b"] = 2, ["r"] = 30, ["l"] = 36, ["at"] = 99 },
      },
    },
  },
}
"#);
    let g = &normalize(&account, NO_CHARS)[0];
    assert_eq!(g.bank_item_info.len(), 2, "{:?}", g.bank_item_info);
    let first = g.bank_item_info.iter().find(|i| i.id == 288841).unwrap();
    assert_eq!(
        first.name, "Épée de la tempête",
        "le plus récent `at` gagne"
    );
    assert_eq!(first.quality, Some(4));
    assert_eq!(first.bind, Some(2));
    assert_eq!(first.required_level, Some(30));
    assert_eq!(first.item_level, Some(36));
    assert_eq!(first.at, Some(99));
    assert!(g
        .bank_item_info
        .iter()
        .any(|i| i.id == 288842 && i.name == "Lame du givre"));
}

#[test]
fn collector_sessions_merged_by_character_and_start() {
    let account = sv(r#"
GuildFoundForeverDB = {
  ["lastActiveGuildKey"] = "r one#g",
  ["guilds"] = {
    ["r one#g"] = {
      ["collectorSessions"] = {
        ["offi cier:100"] = { ["character"] = "offi cier", ["name"] = "Offi Cier", ["startedAt"] = 100, ["seenAt"] = 400 },
        ["vigie:500"] = { ["character"] = "vigie", ["startedAt"] = 500, ["seenAt"] = 900, ["endedAt"] = 900 },
        ["nobody:600"] = { ["startedAt"] = 600, ["seenAt"] = 700 },
        ["nostart"] = { ["character"] = "vigie", ["seenAt"] = 700 },
      },
    },
    ["rone#g"] = {
      ["collectorSessions"] = {
        ["offi cier:100"] = { ["character"] = "offi cier", ["name"] = "Offi Cier", ["startedAt"] = 100, ["seenAt"] = 800, ["endedAt"] = 800 },
      },
    },
  },
}
"#);
    let g = &normalize(&account, NO_CHARS)[0];
    assert_eq!(g.collector_sessions.len(), 2, "{:?}", g.collector_sessions);
    let offi = g
        .collector_sessions
        .iter()
        .find(|s| s.started_at == 100)
        .unwrap();
    assert_eq!(offi.character.character.as_deref(), Some("offi cier"));
    assert_eq!(offi.name.as_deref(), Some("Offi Cier"));
    assert_eq!(offi.seen_at, Some(800), "le plus récent `seenAt` gagne");
    assert_eq!(offi.ended_at, Some(800));
    let vigie = g
        .collector_sessions
        .iter()
        .find(|s| s.started_at == 500)
        .unwrap();
    assert_eq!(vigie.character.character.as_deref(), Some("vigie"));
    assert_eq!(vigie.ended_at, Some(900));
    let json = serde_json::to_value(g).unwrap();
    assert_eq!(json["collector_sessions"][0]["started_at"], 100);
}

#[test]
fn violation_handoffs_come_from_handed_to_and_sent_to() {
    let account = sv(r#"
GuildFoundForeverDB = { ["guilds"] = { ["realm#g"] = {
  ["guildName"] = "G", ["realmName"] = "Realm",
  ["bridgeViolations"] = {
    ["trade_gold-10-1"] = { ["id"] = "trade_gold-10-1", ["kind"] = "trade_gold", ["player"] = "Ann Bee",
      ["playerKey"] = "ann bee", ["at"] = 10,
      ["handedTo"] = {
        { ["name"] = "vigie une", ["at"] = 20 },
        { ["name"] = "", ["at"] = 30 },
        { ["at"] = 31 },
        "pas une table",
        { ["name"] = "vigie une", ["at"] = 25 },
        { ["name"] = "sans date" },
      } },
    ["trade_gold-11-2"] = { ["id"] = "trade_gold-11-2", ["kind"] = "trade_gold", ["player"] = "Ann Bee",
      ["playerKey"] = "ann bee", ["at"] = 11, ["handedTo"] = "invalide" },
  },
  ["bridgeLocalViolations"] = {
    ["mail_send-12-3"] = { ["id"] = "mail_send-12-3", ["kind"] = "mail_send", ["player"] = "Ann Bee",
      ["playerKey"] = "ann bee", ["at"] = 12, ["relayed"] = true,
      ["sentTo"] = { ["c1"] = 41, ["c2"] = 42, ["c3"] = 43, ["c4"] = 44, ["c5"] = 45,
                     ["c6"] = 46, ["c7"] = 47, ["c8"] = 48, ["c9"] = 49, ["c0"] = "x" } },
  },
} } }
"#);
    let g = &normalize(&account, NO_CHARS)[0];
    let received = g.bridge_violations.as_ref().unwrap();
    let with = received.iter().find(|v| v.id == "trade_gold-10-1").unwrap();
    let names: Vec<(&str, Option<i64>)> = with
        .handed_to
        .iter()
        .map(|h| (h.name.as_str(), h.at))
        .collect();
    assert_eq!(names, vec![("vigie une", Some(25)), ("sans date", None)]);
    let without = received.iter().find(|v| v.id == "trade_gold-11-2").unwrap();
    assert!(without.handed_to.is_empty());

    let local = &g.bridge_local_violations.as_ref().unwrap()[0];
    assert_eq!(local.handed_to.len(), 8);
    assert_eq!(local.handed_to[0].name, "c9");
    assert_eq!(local.handed_to[0].at, Some(49));
    assert!(local
        .handed_to
        .iter()
        .all(|h| h.name != "c0" && h.name != "c1"));

    let json = serde_json::to_value(g).unwrap();
    let v = json["bridge_violations"].as_array().unwrap();
    let with_json = v.iter().find(|x| x["id"] == "trade_gold-10-1").unwrap();
    assert_eq!(with_json["handed_to"][0]["name"], "vigie une");
    assert_eq!(with_json["handed_to"][0]["at"], 25);
    let without_json = v.iter().find(|x| x["id"] == "trade_gold-11-2").unwrap();
    assert!(without_json.get("handed_to").is_none(), "omis si vide");
}

#[test]
fn bridge_roster_guid_is_read_and_validated() {
    let account = sv(r#"
GuildFoundForeverDB = { ["guilds"] = { ["r#g"] = {
  ["bridgeRoster"] = {
    ["garou bel"] = { ["name"] = "Garou Bel", ["seenAt"] = 10, ["guid"] = "Player-6937-0049F204" },
    ["ann bee"] = { ["name"] = "Ann Bee", ["seenAt"] = 10, ["guid"] = "not-a-guid" },
  },
} } }
"#);
    let g = &normalize(&account, NO_CHARS)[0];
    let roster = g.bridge_roster.as_ref().unwrap();
    let garou = roster
        .iter()
        .find(|e| e.name.as_deref() == Some("Garou Bel"))
        .unwrap();
    assert_eq!(garou.guid.as_deref(), Some("Player-6937-0049F204"));
    let ann = roster
        .iter()
        .find(|e| e.name.as_deref() == Some("Ann Bee"))
        .unwrap();
    assert_eq!(ann.guid, None, "forme invalide, champ seul écarté");
}

#[test]
fn presence_gaps_cover_valid_invalid_merge_and_alias() {
    let account = sv(r#"
GuildFoundForeverDB = {
  ["lastActiveGuildKey"] = "r one#g",
  ["guilds"] = {
    ["r one#g"] = {
      ["memberPresence"] = {
        ["kael ombre"] = { ["name"] = "Kael Ombre" },
        ["kael brute"] = { ["name"] = "Kael Brute" },
        ["torvi quatre"] = { ["name"] = "Torvi Quatre" },
      },
      ["presenceGaps"] = {
        ["torvi quatre:1794500000"] = { ["suspect"] = "Torvi Quatre", ["by"] = "Kael Ombre",
          ["s"] = 1794500000, ["e"] = 1794501800, ["lc"] = 1794501800, ["m"] = 30,
          ["why"] = "addon", ["rank"] = 1, ["rem"] = 1, ["enr"] = true, ["ever"] = true,
          ["ver"] = "0.22.50" },
        ["kael:1"] = { ["suspect"] = "Kael", ["by"] = "Torvi Quatre", ["s"] = 1,
          ["why"] = "bogus", ["rank"] = 9 },
      },
    },
    ["rone#g"] = {
      ["presenceGaps"] = {
        ["torvi quatre:1794500000"] = { ["suspect"] = "Torvi Quatre", ["by"] = "Kael Ombre",
          ["s"] = 1794500000, ["lc"] = 1794501900, ["why"] = "open", ["rank"] = 1 },
      },
    },
  },
}
"#);
    let g = &normalize(&account, NO_CHARS)[0];
    assert_eq!(g.presence_gaps.len(), 2, "{:?}", g.presence_gaps);

    let torvi = g
        .presence_gaps
        .iter()
        .find(|p| p.started_at == 1_794_500_000)
        .unwrap();
    assert_eq!(torvi.suspect.character.as_deref(), Some("torvi quatre"));
    assert_eq!(torvi.witness.character.as_deref(), Some("kael ombre"));
    assert_eq!(
        torvi.last_check_at,
        Some(1_794_501_900),
        "profil le plus récent (lc) gagne"
    );
    assert_eq!(
        torvi.end_reason.as_deref(),
        Some("open"),
        "fusionné en entier, pas champ par champ"
    );
    assert_eq!(torvi.ended_at, None, "pas hérité de l'ancien profil");
    assert_eq!(torvi.minutes, None, "pas hérité de l'ancien profil");
    assert_eq!(
        torvi.id, "torvi quatre:1794500000:kael ombre",
        "identifiant = suspect:début:témoin"
    );

    let kael = g.presence_gaps.iter().find(|p| p.started_at == 1).unwrap();
    assert_eq!(
        kael.suspect.character, None,
        "alias ambigu (Kael Ombre / Kael Brute)"
    );
    assert_eq!(kael.suspect.candidates, ["kael brute", "kael ombre"]);
    assert_eq!(
        kael.end_reason, None,
        "hors liste fermée, champ seul écarté"
    );
    assert_eq!(
        kael.witness_rank, None,
        "hors bornes 1|2, champ seul écarté"
    );
    assert_eq!(kael.witness.character.as_deref(), Some("torvi quatre"));
}

#[test]
fn crafted_sightings_cover_valid_invalid_merge_and_alias() {
    let account = sv(r#"
GuildFoundForeverDB = {
  ["lastActiveGuildKey"] = "r one#g",
  ["guilds"] = {
    ["r one#g"] = {
      ["craftedSightings"] = {
        ["2059|Player-6937-0049F204|fumée ravine|trade"] = {
          ["item"] = 2059, ["guid"] = "Player-6937-0049F204", ["realm"] = 6937,
          ["cname"] = "Fumée Ravine", ["ckind"] = "unknown", ["src"] = "trade",
          ["holder"] = "Fumée Ravine", ["by"] = "Torvi Quatre", ["first"] = 1794500000,
          ["last"] = 1794500000, ["n"] = 1,
        },
        ["2060|-|guildbank:3|guild_bank"] = {
          ["item"] = 2060, ["guid"] = "not-a-guid", ["realm"] = 99999999, ["ckind"] = "bogus",
          ["src"] = "guild_bank", ["holder"] = "guildbank:3", ["by"] = "Torvi Quatre",
        },
        ["2061|-|fumée|mail"] = {
          ["item"] = 2061, ["src"] = "mail", ["holder"] = "Fumée", ["by"] = "Torvi",
        },
      },
    },
    ["rone#g"] = {
      ["craftedSightings"] = {
        ["2059|Player-6937-0049F204|fumée ravine|trade"] = {
          ["item"] = 2059, ["guid"] = "Player-6937-0049F204", ["realm"] = 6937,
          ["holder"] = "Fumée Ravine", ["by"] = "Torvi Quatre", ["first"] = 1794500000,
          ["last"] = 1794600000, ["n"] = 3,
        },
      },
    },
  },
}
"#);
    let g = &normalize(&account, NO_CHARS)[0];
    assert_eq!(g.crafted_sightings.len(), 3, "{:?}", g.crafted_sightings);

    let fumee = g
        .crafted_sightings
        .iter()
        .find(|c| c.item_id == 2059)
        .unwrap();
    assert_eq!(fumee.holder.character.as_deref(), Some("fumée ravine"));
    assert_eq!(fumee.observer.character.as_deref(), Some("torvi quatre"));
    assert_eq!(fumee.holder_kind, HolderKind::Character);
    assert_eq!(fumee.count, Some(3), "le plus récent `last` gagne");
    assert_eq!(fumee.last_seen_at, Some(1_794_600_000));
    assert_eq!(
        fumee.crafter_name, None,
        "absent du relevé gagnant, pas hérité"
    );

    let bank = g
        .crafted_sightings
        .iter()
        .find(|c| c.item_id == 2060)
        .unwrap();
    assert_eq!(bank.holder_kind, HolderKind::GuildBank);
    assert_eq!(
        bank.holder.character, None,
        "onglet de banque, pas un perso"
    );
    assert_eq!(bank.crafter_guid, None, "GUID mal formé, champ seul écarté");
    assert_eq!(
        bank.crafter_realm_id, None,
        "hors bornes, champ seul écarté"
    );
    assert_eq!(
        bank.crafter_kind, None,
        "hors liste fermée, champ seul écarté"
    );

    let alias = g
        .crafted_sightings
        .iter()
        .find(|c| c.item_id == 2061)
        .unwrap();
    assert_eq!(
        alias.holder.character.as_deref(),
        Some("fumée ravine"),
        "alias résolu via `holder`/`by` appris ailleurs"
    );
    assert_eq!(alias.observer.character.as_deref(), Some("torvi quatre"));
}

#[test]
fn guild_bank_log_cover_valid_invalid_merge_and_alias_collector_only() {
    let account = sv(r#"
GuildFoundForeverDB = { ["guilds"] = { ["r#g"] = {
  ["guildBankLog"] = {
    ["3:1794500000:7"] = { ["tab"] = 3, ["k"] = "deposit", ["who"] = "Kael Ombre", ["item"] = 2059,
      ["n"] = 5, ["guid"] = "Player-6937-0049F204", ["at"] = 1794496400, ["age"] = 1,
      ["read"] = 1794500000, ["by"] = "Torvi Quatre" },
    ["0:1794500000:8"] = { ["tab"] = 0, ["k"] = "bogus", ["who"] = "Kael", ["amt"] = 1000,
      ["read"] = 1794500000, ["by"] = "Torvi" },
    ["9:1:1"] = { ["tab"] = 9, ["who"] = "X", ["read"] = 1, ["by"] = "Torvi Quatre" },
  },
} } }
"#);
    let g = &normalize(&account, NO_CHARS)[0];
    let log = g
        .guild_bank_log
        .as_ref()
        .expect("un collecteur a lu le journal");
    assert_eq!(
        log.len(),
        2,
        "tab hors bornes : enregistrement abandonné en entier, {:?}",
        log
    );

    let deposit = log.iter().find(|e| e.item_id == Some(2059)).unwrap();
    assert_eq!(deposit.player.character.as_deref(), Some("kael ombre"));
    assert_eq!(deposit.player_name.as_deref(), Some("Kael Ombre"));
    assert_eq!(deposit.reader.character.as_deref(), Some("torvi quatre"));
    assert_eq!(deposit.kind.as_deref(), Some("deposit"));
    assert_eq!(
        deposit.crafter_guid.as_deref(),
        Some("Player-6937-0049F204")
    );

    let gold = log.iter().find(|e| e.tab == 0).unwrap();
    assert_eq!(gold.kind, None, "hors liste fermée, champ seul écarté");
    assert_eq!(
        gold.player.character.as_deref(),
        Some("kael ombre"),
        "alias résolu via `who`/`by` appris"
    );
    assert_eq!(gold.reader.character.as_deref(), Some("torvi quatre"));
    assert_eq!(gold.amount, Some(1000));
}

#[test]
fn self_identity_cover_valid_invalid_merge_and_alias() {
    let account = sv(r#"
GuildFoundForeverDB = {
  ["lastActiveGuildKey"] = "r one#g",
  ["guilds"] = {
    ["r one#g"] = {
      ["memberPresence"] = {
        ["kael ombre"] = { ["name"] = "Kael Ombre" },
        ["torvi quatre"] = { ["name"] = "Torvi Quatre" },
        ["torvi cinq"] = { ["name"] = "Torvi Cinq" },
      },
      ["selfIdentity"] = {
        ["kael ombre"] = { ["bt"] = "fa2ce2459aec6c5c0d72ca5991cd4cfd", ["acct"] = "0123456789abcdef",
          ["btag"] = "Kael#1234", ["at"] = 50 },
        ["torvi"] = { ["bt"] = "fa2ce2459aec6c5c0d72ca5991cd4cfd", ["at"] = 1 },
      },
    },
    ["rone#g"] = {
      ["selfIdentity"] = {
        ["kael ombre"] = { ["bt"] = "BAD", ["btag"] = "sans dièse", ["at"] = 100 },
      },
    },
  },
}
"#);
    let g = &normalize(&account, NO_CHARS)[0];
    assert_eq!(g.self_identity.len(), 2, "{:?}", g.self_identity);

    let kael = g
        .self_identity
        .iter()
        .find(|s| s.character.merge_key() == "kael ombre")
        .unwrap();
    assert_eq!(
        kael.at, 100,
        "le plus récent `at` gagne, tout l'enregistrement"
    );
    assert_eq!(kael.bt, None, "invalide (pas hex 32), champ seul écarté");
    assert_eq!(kael.btag, None, "pas de `#`, champ seul écarté");
    assert_eq!(kael.acct, None, "absent du relevé gagnant, pas hérité");

    let torvi = g
        .self_identity
        .iter()
        .find(|s| s.character.merge_key() == "?torvi")
        .unwrap();
    assert_eq!(
        torvi.character.character, None,
        "alias ambigu (Torvi Quatre / Torvi Cinq)"
    );
    assert_eq!(torvi.character.candidates, ["torvi cinq", "torvi quatre"]);
    assert_eq!(
        torvi.bt.as_deref(),
        Some("fa2ce2459aec6c5c0d72ca5991cd4cfd")
    );
}

#[test]
fn server_stats_cover_valid_invalid_and_merge_newest() {
    let account = sv(r#"
GuildFoundForeverDB = {
  ["lastActiveGuildKey"] = "r one#g",
  ["guilds"] = {
    ["r one#g"] = {
      ["memberPresence"] = {
        ["kael ombre"] = { ["name"] = "Kael Ombre" },
      },
      ["serverStats"] = {
        ["kael ombre"] = { ["at"] = 200, ["lvl"] = 26,
          ["s"] = { ["329"] = 20, ["919"] = 1000, ["330"] = -5, ["9999"] = 123, ["334"] = 950000 } },
      },
    },
    ["rone#g"] = {
      ["serverStats"] = {
        ["kael ombre"] = { ["at"] = 100, ["lvl"] = 99,
          ["s"] = { ["329"] = 999999, ["98"] = 272 } },
      },
    },
  },
}
"#);
    let g = &normalize(&account, NO_CHARS)[0];
    assert_eq!(g.server_stats.len(), 1, "{:?}", g.server_stats);

    let kael = &g.server_stats[0];
    assert_eq!(kael.character.character.as_deref(), Some("kael ombre"));
    assert_eq!(kael.at, 200, "le plus récent `at` gagne");
    assert_eq!(
        kael.level,
        Some(26),
        "fusionné en entier, pas champ par champ (pas 99 de l'ancien profil)"
    );
    assert_eq!(
        kael.stats.get("329"),
        Some(&20),
        "pas 999999 de l'ancien profil"
    );
    assert_eq!(kael.stats.get("919"), Some(&1000));
    assert_eq!(
        kael.stats.get("334"),
        Some(&950000),
        "334 (plus grande quantité d'argent possédée) gardé"
    );
    assert!(
        !kael.stats.contains_key("330"),
        "valeur négative, clé abandonnée seule"
    );
    assert!(
        !kael.stats.contains_key("9999"),
        "identifiant hors liste fermée, clé abandonnée seule"
    );
    assert!(
        !kael.stats.contains_key("98"),
        "absent du relevé gagnant, pas hérité de l'ancien profil"
    );
    assert_eq!(kael.stats.len(), 3);
}

#[test]
fn stat_checks_cover_valid_invalid_merge_by_id_and_alias() {
    let account = sv(r#"
GuildFoundForeverDB = {
  ["lastActiveGuildKey"] = "r one#g",
  ["guilds"] = {
    ["r one#g"] = {
      ["memberPresence"] = {
        ["dorian"] = { ["name"] = "Dorian" },
        ["kael ombre"] = { ["name"] = "Kael Ombre" },
      },
      ["statChecks"] = {
        ["dorian:1794500000"] = { ["target"] = "Dorian", ["by"] = "Kael Ombre",
          ["at"] = 1794500000, ["how"] = "auto", ["lvl"] = 25,
          ["s"] = { ["329"] = 20, ["330"] = -5, ["9999"] = 1 } },
        ["kael ombre:5"] = { ["by"] = "Dorian", ["at"] = 5, ["how"] = "bogus" },
      },
    },
    ["rone#g"] = {
      ["statChecks"] = {
        ["dorian:1794500000"] = { ["target"] = "Dorian", ["by"] = "Kael Ombre",
          ["at"] = 1794500000, ["how"] = "bogus", ["lvl"] = 99,
          ["s"] = { ["329"] = 999 } },
      },
    },
  },
}
"#);
    let g = &normalize(&account, NO_CHARS)[0];
    assert_eq!(g.stat_checks.len(), 2, "{:?}", g.stat_checks);

    let dorian = g
        .stat_checks
        .iter()
        .find(|c| c.id == "dorian:1794500000")
        .unwrap();
    assert_eq!(dorian.target.character.as_deref(), Some("dorian"));
    assert_eq!(dorian.officer.character.as_deref(), Some("kael ombre"));
    assert_eq!(
        dorian.how.as_deref(),
        Some("auto"),
        "fusionné par identifiant, en entier (pas \"bogus\" de l'autre profil)"
    );
    assert_eq!(dorian.level, Some(25));
    assert_eq!(dorian.stats.get("329"), Some(&20));
    assert!(
        !dorian.stats.contains_key("330"),
        "valeur négative, clé abandonnée seule"
    );
    assert!(
        !dorian.stats.contains_key("9999"),
        "identifiant hors liste fermée, clé abandonnée seule"
    );
    assert_eq!(dorian.stats.len(), 1);

    let fallback = g
        .stat_checks
        .iter()
        .find(|c| c.id == "kael ombre:5")
        .unwrap();
    assert_eq!(
        fallback.target.character.as_deref(),
        Some("kael ombre"),
        "`target` absent du relevé, recalé sur la clé"
    );
    assert_eq!(fallback.officer.character.as_deref(), Some("dorian"));
    assert_eq!(fallback.how, None, "hors liste fermée, champ seul écarté");
    assert_eq!(fallback.level, None);
    assert!(fallback.stats.is_empty());
}
