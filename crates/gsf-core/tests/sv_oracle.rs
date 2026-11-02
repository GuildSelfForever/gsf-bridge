//! Oracle : chaque fixture réelle est chargée par un vrai Lua 5.1 (mlua,
//! vendored, sans aucune bibliothèque standard, environnement vide) et le
//! résultat doit être identique à celui de `sv::parse`.

mod common;

use std::collections::BTreeMap;

use gsf_core::sv::{parse, Key, Value};
use mlua::{Lua, LuaOptions, StdLib};

/// Structure comparable commune aux deux mondes. Lua 5.1 n'a que des
/// doubles : tous les nombres sont comparés en `f64` (bits normalisés).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Cmp {
    Bool(bool),
    Num(u64),
    Str(Vec<u8>),
    Table(BTreeMap<Cmp, Cmp>),
}

fn num(f: f64) -> Cmp {
    // -0.0 == 0.0 en Lua.
    Cmp::Num(if f == 0.0 {
        0f64.to_bits()
    } else {
        f.to_bits()
    })
}

fn from_ours(v: &Value) -> Option<Cmp> {
    Some(match v {
        Value::Nil => return None,
        Value::Bool(b) => Cmp::Bool(*b),
        Value::Int(i) => num(*i as f64),
        Value::Num(f) => num(*f),
        Value::Str(s) => Cmp::Str(s.clone()),
        Value::Table(t) => Cmp::Table(
            t.iter()
                .filter_map(|(k, v)| {
                    let k = match k {
                        Key::Int(i) => num(*i as f64),
                        Key::Str(s) => Cmp::Str(s.clone()),
                        Key::Bool(b) => Cmp::Bool(*b),
                        Key::Num(f) => num(f.get()),
                    };
                    Some((k, from_ours(v)?))
                })
                .collect(),
        ),
    })
}

fn from_lua(v: &mlua::Value) -> Option<Cmp> {
    Some(match v {
        mlua::Value::Nil => return None,
        mlua::Value::Boolean(b) => Cmp::Bool(*b),
        mlua::Value::Integer(i) => num(*i as f64),
        mlua::Value::Number(f) => num(*f),
        mlua::Value::String(s) => Cmp::Str(s.as_bytes().to_vec()),
        mlua::Value::Table(t) => {
            let mut out = BTreeMap::new();
            for pair in t.clone().pairs::<mlua::Value, mlua::Value>() {
                let (k, v) = pair.expect("paire Lua");
                out.insert(
                    from_lua(&k).expect("clé nil"),
                    from_lua(&v).expect("valeur nil"),
                );
            }
            Cmp::Table(out)
        }
        other => panic!("type Lua inattendu dans une SavedVariable : {other:?}"),
    })
}

/// Globales définies par le fichier, évaluées par Lua 5.1 dans un état sans
/// stdlib et un environnement vide (aucun accès à `_G`).
fn lua_globals(src: &[u8]) -> BTreeMap<Cmp, Cmp> {
    let lua = Lua::new_with(StdLib::NONE, LuaOptions::new()).expect("état Lua");
    let env = lua.create_table().expect("env");
    lua.load(src)
        .set_mode(mlua::ChunkMode::Text)
        .set_environment(env.clone())
        .exec()
        .expect("Lua refuse la fixture");
    match from_lua(&mlua::Value::Table(env)) {
        Some(Cmp::Table(t)) => t,
        _ => unreachable!(),
    }
}

fn our_globals(src: &[u8]) -> BTreeMap<Cmp, Cmp> {
    parse(src)
        .expect("sv::parse refuse la fixture")
        .iter()
        .filter_map(|(name, v)| Some((Cmp::Str(name.as_bytes().to_vec()), from_ours(v)?)))
        .collect()
}

#[test]
fn every_fixture_matches_the_lua51_oracle() {
    let fixtures = common::fixtures();
    assert!(
        fixtures.len() >= 10,
        "fixtures manquantes : {}",
        fixtures.len()
    );
    for (name, bytes) in fixtures {
        let ours = our_globals(&bytes);
        let lua = lua_globals(&bytes);
        assert!(!ours.is_empty(), "{name}: aucune globale");
        assert_eq!(ours, lua, "{name}: divergence avec l'oracle Lua 5.1");
    }
}

#[test]
fn synthetic_edge_cases_match_the_oracle() {
    let cases: &[&[u8]] = &[
        b"X = { nil, nil, nil, \"6096:0\", nil, [11] = \"y\", }",
        b"X = { \"pos\", [1] = \"exp\", [\"k\"] = nil, k2 = -3 }",
        b"X = { [1.5] = 1, [2.0] = 2, [-0.0] = 3, [true] = false, [0x1F] = 1e3 }",
        b"X = \"\\65\\066\\0671\\\\\\\"\\n\\r\\t\\a\\q\\255\\0\"",
        b"X = [==[\r\na]]b\r\nc]==] -- c\r\nY = --[[ x ]] .5",
        b"X = \"a\\\r\nb\" Y = 'q\\'s'; Z = nil",
        // `\x41` n'existe pas en Lua 5.1 (vaut "x41") : garantit que l'oracle
        // est bien Lua 5.1 et non LuaJIT/5.2+.
        b"X = \"\\x41\"",
        b"X = 9223372036854775807 Y = -9223372036854775808 Z = 123456789012345678901234567890",
    ];
    for src in cases {
        assert_eq!(
            our_globals(src),
            lua_globals(src),
            "{}",
            String::from_utf8_lossy(src)
        );
    }
    let items = vec!["0"; 50].join(",");
    let src = format!("X = {{ {items}, [1] = 1, nil, [52] = 2, \"z\" }}");
    assert_eq!(our_globals(src.as_bytes()), lua_globals(src.as_bytes()));
}
