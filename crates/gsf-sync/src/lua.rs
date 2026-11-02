//! Sérialiseur Lua minimal et sûr pour `GSF_PushedData`.
//!
//! Les chaînes sont toujours écrites entre guillemets doubles avec des
//! échappements explicites : `\\`, `\"`, `\n`, `\r`, `\t`, et `\ddd` (trois
//! chiffres) pour tout autre caractère de contrôle. Aucune chaîne longue
//! `[[…]]` : un `]]` dans les données ne peut donc rien fermer. Les
//! caractères non ASCII sont écrits tels quels en UTF-8 (WoW lit l'UTF-8).

use std::fmt::Write;

/// Valeur Lua sérialisable.
#[derive(Debug, Clone, PartialEq)]
pub enum LuaValue {
    Nil,
    Bool(bool),
    Int(i64),
    Str(String),
    /// Champs dans l'ordre d'écriture.
    Table(Vec<(LuaKey, LuaValue)>),
}

/// Clé de table.
#[derive(Debug, Clone, PartialEq)]
pub enum LuaKey {
    /// `[n] = …`
    Index(i64),
    /// `nom = …` si identifiant valide, sinon `["nom"] = …`.
    Name(String),
}

const KEYWORDS: &[&str] = &[
    "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "if", "in", "local",
    "nil", "not", "or", "repeat", "return", "then", "true", "until", "while", "goto",
];

/// `true` si `s` peut s'écrire sans crochets.
pub fn is_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !KEYWORDS.contains(&s)
}

/// Littéral chaîne Lua échappé, guillemets compris.
pub fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                let _ = write!(out, "\\{:03}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Sérialise une valeur (tables indentées de 4 espaces).
pub fn to_lua(value: &LuaValue) -> String {
    let mut out = String::new();
    write_value(value, 0, &mut out);
    out
}

/// `nom = valeur` suivi d'un saut de ligne.
pub fn assignment(name: &str, value: &LuaValue) -> String {
    debug_assert!(is_identifier(name));
    format!("{name} = {}\n", to_lua(value))
}

fn write_value(value: &LuaValue, depth: usize, out: &mut String) {
    match value {
        LuaValue::Nil => out.push_str("nil"),
        LuaValue::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        LuaValue::Int(n) => {
            let _ = write!(out, "{n}");
        }
        LuaValue::Str(s) => out.push_str(&quote(s)),
        LuaValue::Table(fields) => {
            if fields.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push_str("{\n");
            let pad = "    ".repeat(depth + 1);
            for (key, v) in fields {
                out.push_str(&pad);
                match key {
                    LuaKey::Index(i) => {
                        let _ = write!(out, "[{i}] = ");
                    }
                    LuaKey::Name(n) if is_identifier(n) => {
                        let _ = write!(out, "{n} = ");
                    }
                    LuaKey::Name(n) => {
                        let _ = write!(out, "[{}] = ", quote(n));
                    }
                }
                write_value(v, depth + 1, out);
                out.push_str(",\n");
            }
            out.push_str(&"    ".repeat(depth));
            out.push('}');
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gsf_core::sv::{parse, Value};

    #[test]
    fn weird_strings_round_trip_through_the_parser() {
        let weird = [
            "simple",
            "guillemets \" et 'apostrophes'",
            "antislash \\ fin\\",
            "ligne\nsuivante\r\n",
            "tab\tbell\u{7}nul\u{0}del\u{7f}",
            "]] fin de chaîne longue ]==] [[",
            "Dävin-Fumée — ✓ 漢字 🐉",
            "\\\"",
            "chiffres après échappement : \u{1}23",
            "",
        ];
        for s in weird {
            let src = assignment("X", &LuaValue::Str(s.to_owned()));
            assert!(
                !src[4..src.len() - 1].contains('\n'),
                "une seule ligne : {src:?}"
            );
            let sv = parse(src.as_bytes()).unwrap_or_else(|e| panic!("{s:?} : {e}"));
            assert_eq!(sv.get("X").and_then(Value::as_str), Some(s), "{src}");
        }
    }

    #[test]
    fn tables_keys_and_scalars() {
        let v = LuaValue::Table(vec![
            (LuaKey::Name("schema".into()), LuaValue::Int(1)),
            (LuaKey::Name("end".into()), LuaValue::Bool(true)),
            (LuaKey::Name("clé espacée".into()), LuaValue::Int(-5)),
            (
                LuaKey::Name("list".into()),
                LuaValue::Table(vec![
                    (LuaKey::Index(1), LuaValue::Str("a".into())),
                    (LuaKey::Index(2), LuaValue::Table(vec![])),
                ]),
            ),
            (LuaKey::Name("none".into()), LuaValue::Nil),
        ]);
        let src = assignment("GSF_PushedData", &v);
        assert!(src.contains("    schema = 1,\n"));
        assert!(src.contains("[\"end\"] = true"));
        let sv = parse(src.as_bytes()).unwrap();
        let t = sv.get("GSF_PushedData").unwrap();
        assert_eq!(t.get("schema"), Some(&Value::Int(1)));
        assert_eq!(t.get("end"), Some(&Value::Bool(true)));
        assert_eq!(t.get("clé espacée"), Some(&Value::Int(-5)));
        assert_eq!(t.get("list").unwrap().array().count(), 2);
        assert_eq!(t.get("none"), None);
    }

    #[test]
    fn identifiers() {
        assert!(is_identifier("guildKey"));
        assert!(is_identifier("_x1"));
        assert!(!is_identifier("1x"));
        assert!(!is_identifier("a-b"));
        assert!(!is_identifier("nil"));
        assert!(!is_identifier("é"));
        assert!(!is_identifier(""));
    }
}
