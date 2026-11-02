//! Formats « fil » de l'addon stockés tels quels dans les SavedVariables :
//! champs séparés par `~`, encodés `%XX` (`GSF:Encode` / `GSF:Decode`).

/// `GSF:Decode` : remplace chaque `%XX` (hexadécimal) par l'octet
/// correspondant. Le résultat est décodé en UTF-8 avec remplacement.
pub fn percent_decode(value: &[u8]) -> String {
    let mut out = Vec::with_capacity(value.len());
    let mut i = 0;
    while i < value.len() {
        if value[i] == b'%' && i + 2 < value.len() {
            let hex = &value[i + 1..i + 3];
            if let Some(b) = std::str::from_utf8(hex)
                .ok()
                .filter(|h| h.bytes().all(|c| c.is_ascii_hexdigit()))
                .and_then(|h| u8::from_str_radix(h, 16).ok())
            {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(value[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Découpe comme `splitFields` de l'addon : `(payload.."~"):gmatch("(.-)~")`.
pub fn split_fields(payload: &[u8]) -> Vec<&[u8]> {
    payload.split(|&b| b == b'~').collect()
}

/// Entier « fil » : chiffres ASCII uniquement (`^%d+$`).
pub fn wire_int(field: &[u8]) -> Option<i64> {
    if field.is_empty() || !field.iter().all(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(field).ok()?.parse().ok()
}

/// Demande de craft décodée depuis une ligne de sauvegarde
/// (`GSF:DeserializeGuildRequest`, 13 champs, 15 avec `takenBy/takenAt`, ou
/// 17 avec en plus `releasedBy/releasedAt` depuis l'addon 0.19.30).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireRequest {
    pub id: String,
    pub kind: String,
    pub text: String,
    pub requester: String,
    pub created_at: i64,
    pub status: String,
    pub updated_at: i64,
    pub decided_at: i64,
    pub decided_by: String,
    pub craft_skill_line: i64,
    pub craft_recipe_id: i64,
    pub craft_category: String,
    pub item_id: i64,
    pub taken_by: Option<String>,
    pub taken_at: Option<i64>,
    /// `releasedBy`/`releasedAt` (17 champs), reportés dans
    /// `Request`/`BackupRequest` depuis le contrat 2026-10-15 (`released_by`,
    /// `released_at`).
    pub released_by: Option<String>,
    pub released_at: Option<i64>,
    pub field_count: usize,
}

/// Décode une ligne de sauvegarde ; `None` exactement quand l'addon la
/// rejetterait (mauvais nombre de champs, entier invalide).
pub fn decode_request_line(line: &[u8]) -> Option<WireRequest> {
    let f = split_fields(line);
    if f.len() != 13 && f.len() != 15 && f.len() != 17 {
        return None;
    }
    let taken_at = if f.len() >= 15 {
        Some(wire_int(f[14])?)
    } else {
        None
    };
    let released_at = if f.len() == 17 {
        Some(wire_int(f[16])?)
    } else {
        None
    };
    Some(WireRequest {
        id: percent_decode(f[0]),
        kind: percent_decode(f[1]),
        text: percent_decode(f[2]),
        requester: percent_decode(f[3]),
        created_at: wire_int(f[4])?,
        status: percent_decode(f[5]),
        updated_at: wire_int(f[6])?,
        decided_at: wire_int(f[7])?,
        decided_by: percent_decode(f[8]),
        craft_skill_line: wire_int(f[9])?,
        craft_recipe_id: wire_int(f[10])?,
        craft_category: percent_decode(f[11]),
        item_id: wire_int(f[12])?,
        taken_by: (f.len() >= 15).then(|| percent_decode(f[13])),
        taken_at,
        released_by: (f.len() == 17).then(|| percent_decode(f[15])),
        released_at,
        field_count: f.len(),
    })
}

/// Ligne de nettoyage d'audit (`GSF:CollectAuditCleanupRows`) :
/// `player~id~at~by~adventureID`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireAuditCleanup {
    pub player: String,
    pub id: String,
    pub at: i64,
    pub by: String,
    pub adventure_id: String,
}

pub fn decode_audit_cleanup_row(row: &[u8]) -> Option<WireAuditCleanup> {
    let f = split_fields(row);
    if f.len() < 3 {
        return None;
    }
    Some(WireAuditCleanup {
        player: percent_decode(f[0]),
        id: percent_decode(f[1]),
        at: wire_int(f[2])?,
        by: f.get(3).map(|b| percent_decode(b)).unwrap_or_default(),
        adventure_id: f.get(4).map(|b| percent_decode(b)).unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_decoding() {
        assert_eq!(
            percent_decode(b"Enchantement %3A Arme"),
            "Enchantement : Arme"
        );
        assert_eq!(
            percent_decode(b"%7Ccffffffff%7CHitem%3A2775"),
            "|cffffffff|Hitem:2775"
        );
        assert_eq!(percent_decode(b"100%"), "100%");
        assert_eq!(percent_decode(b"%zz%4"), "%zz%4");
        assert_eq!(percent_decode(b"%C3%A9"), "é");
        assert_eq!(percent_decode(b"%FF"), "\u{fffd}");
    }

    #[test]
    fn thirteen_field_line() {
        let line = "gsfr-0043-garou-1790755692-4227~enchant~Enchantement %3A Arme — +55 puissance de soins~Garou~1790755692~cancelled~1790756293~1790756293~Garou~0~0~~0";
        let r = decode_request_line(line.as_bytes()).unwrap();
        assert_eq!(r.id, "gsfr-0043-garou-1790755692-4227");
        assert_eq!(r.kind, "enchant");
        assert_eq!(r.text, "Enchantement : Arme — +55 puissance de soins");
        assert_eq!(r.requester, "Garou");
        assert_eq!(r.created_at, 1_790_755_692);
        assert_eq!(r.status, "cancelled");
        assert_eq!(r.decided_by, "Garou");
        assert_eq!(r.craft_category, "");
        assert_eq!(r.item_id, 0);
        assert_eq!((r.taken_by, r.taken_at, r.field_count), (None, None, 13));
    }

    #[test]
    fn fifteen_field_line() {
        let line =
            b"id%7E1~craft~[Forge] x~Garou Bel~10~active~11~0~~164~3319~Catalogue~3469~Garou~12";
        let r = decode_request_line(line).unwrap();
        assert_eq!(r.id, "id~1");
        assert_eq!(r.taken_by.as_deref(), Some("Garou"));
        assert_eq!(r.taken_at, Some(12));
        assert_eq!(r.field_count, 15);
    }

    /// Ligne réelle produite par `GSF:SerializeGuildRequest` 0.19.30 : une
    /// demande de craft reposée (`GSF:ReleaseGuildRequest`, `takenBy`/`takenAt`
    /// remis à vide/0, `releasedBy`/`releasedAt` renseignés).
    #[test]
    fn seventeen_field_line() {
        let line = b"gsfr-0050-garou-1790800000-1234~craft~Sac en toile~Garou~1790800000~active~1790800550~0~~197~12345~Sacs~6256~~0~Garou~1790800550";
        let r = decode_request_line(line).unwrap();
        assert_eq!(r.id, "gsfr-0050-garou-1790800000-1234");
        assert_eq!(r.kind, "craft");
        assert_eq!(r.text, "Sac en toile");
        assert_eq!(r.requester, "Garou");
        assert_eq!(r.status, "active");
        assert_eq!(r.decided_by, "");
        assert_eq!(r.craft_skill_line, 197);
        assert_eq!(r.craft_recipe_id, 12345);
        assert_eq!(r.craft_category, "Sacs");
        assert_eq!(r.item_id, 6256);
        assert_eq!(r.taken_by.as_deref(), Some(""));
        assert_eq!(r.taken_at, Some(0));
        assert_eq!(r.released_by.as_deref(), Some("Garou"));
        assert_eq!(r.released_at, Some(1_790_800_550));
        assert_eq!(r.field_count, 17);
    }

    #[test]
    fn rejected_lines() {
        assert!(decode_request_line(b"a~b~c").is_none());
        assert!(decode_request_line(b"").is_none());
        // 14 champs.
        assert!(decode_request_line(b"i~k~t~r~1~s~2~3~d~4~5~c~6~x").is_none());
        // 16 champs : ni 15 ni 17.
        assert!(decode_request_line(b"i~k~t~r~1~s~2~3~d~4~5~c~6~b~12~x").is_none());
        // Entier invalide (négatif, vide, `nil`).
        assert!(decode_request_line(b"i~k~t~r~-1~s~2~3~d~4~5~c~6").is_none());
        assert!(decode_request_line(b"i~k~t~r~1~s~2~3~d~4~5~c~").is_none());
        assert!(decode_request_line(b"i~k~t~r~1~s~2~3~d~4~5~c~6~b~nil").is_none());
        // 17 champs, `releasedAt` invalide.
        assert!(decode_request_line(b"i~k~t~r~1~s~2~3~d~4~5~c~6~b~12~rel~nil").is_none());
        assert!(decode_request_line(b"i~k~t~r~1~s~2~3~d~4~5~c~6~b~12~rel~").is_none());
    }

    #[test]
    fn audit_cleanup_rows() {
        let r = decode_audit_cleanup_row(b"garou bel~123-456~1790760506~Garou Bel~A1").unwrap();
        assert_eq!(r.player, "garou bel");
        assert_eq!(r.id, "123-456");
        assert_eq!(r.at, 1_790_760_506);
        assert_eq!(r.adventure_id, "A1");
        assert!(decode_audit_cleanup_row(b"x~y").is_none());
        assert!(decode_audit_cleanup_row(b"x~y~z").is_none());
    }
}
