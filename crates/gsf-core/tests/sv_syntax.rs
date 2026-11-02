//! Chaque élément de syntaxe des SavedVariables, isolément.

use gsf_core::sv::{parse, Key, ParseError, Value};

fn one(src: &str) -> Value {
    let sv = parse(src.as_bytes()).unwrap_or_else(|e| panic!("{src:?}: {e}"));
    sv.get("X").cloned().expect("X absent")
}

fn err(src: &[u8]) -> ParseError {
    parse(src).expect_err("une erreur était attendue")
}

fn s(v: &str) -> Value {
    Value::Str(v.as_bytes().to_vec())
}

#[test]
fn wow_layout_crlf_leading_empty_line_and_nil_globals() {
    let src = "\r\nA = {\r\n[\"k\"] = 1,\r\n}\r\nB = nil\r\nC = \"x\"\r\n";
    let sv = parse(src.as_bytes()).unwrap();
    let names: Vec<_> = sv.iter().map(|(n, _)| n).collect();
    assert_eq!(names, ["A", "B", "C"]);
    assert_eq!(sv.get("A").unwrap().get("k"), Some(&Value::Int(1)));
    assert_eq!(sv.get("B"), Some(&Value::Nil));
    assert_eq!(sv.get("C"), Some(&s("x")));
}

#[test]
fn scalars() {
    assert_eq!(one("X = true"), Value::Bool(true));
    assert_eq!(one("X = false"), Value::Bool(false));
    assert_eq!(one("X = 42"), Value::Int(42));
    assert_eq!(one("X = -42"), Value::Int(-42));
    assert_eq!(one("X = - 7"), Value::Int(-7));
    assert_eq!(one("X = 1.25"), Value::Num(1.25));
    assert_eq!(one("X = -0.5"), Value::Num(-0.5));
    assert_eq!(one("X = 1e3"), Value::Num(1000.0));
    assert_eq!(one("X = 1.5E-3"), Value::Num(0.0015));
    assert_eq!(one("X = 2e+2"), Value::Num(200.0));
    assert_eq!(one("X = .5"), Value::Num(0.5));
    assert_eq!(one("X = 0x10"), Value::Int(16));
    assert_eq!(
        one("X = -9223372036854775808"),
        Value::Num(-9_223_372_036_854_775_808.0)
    );
    assert_eq!(one("X = 1790760035"), Value::Int(1_790_760_035));
}

#[test]
fn string_escapes() {
    assert_eq!(one(r#"X = "a\"b\\c""#), s("a\"b\\c"));
    assert_eq!(one(r#"X = "l1\nl2\r\t""#), s("l1\nl2\r\t"));
    assert_eq!(one(r#"X = "\65\066\0671""#), s("ABC1"));
    assert_eq!(one(r#"X = "\0""#), Value::Str(vec![0]));
    assert_eq!(one(r#"X = "\a\b\f\v""#), Value::Str(vec![7, 8, 12, 11]));
    assert_eq!(one(r#"X = 'it\'s'"#), s("it's"));
    assert_eq!(one(r#"X = "\q""#), s("q"));
    assert_eq!(one(r#"X = "\x41""#), s("x41"), "pas de \\x en Lua 5.1");
    assert_eq!(one("X = \"a\\\r\nb\""), s("a\nb"));
    assert_eq!(
        one(r#"X = "|cffffffff|Hitem:2775::::|h[x]|h|r""#),
        s("|cffffffff|Hitem:2775::::|h[x]|h|r")
    );
}

#[test]
fn non_utf8_and_utf8_bytes_are_kept_raw() {
    let mut src = b"X = { [\"fum\xC3\xA9e ravine\"] = \"".to_vec();
    src.extend_from_slice(&[0xFF, 0xFE, 0x80]);
    src.extend_from_slice(b"\" }");
    let sv = parse(&src).unwrap();
    let t = sv.get("X").unwrap();
    assert_eq!(
        t.get("fumée ravine"),
        Some(&Value::Str(vec![0xFF, 0xFE, 0x80]))
    );
    // Échappement décimal produisant un octet invalide en UTF-8.
    assert_eq!(one(r#"X = "\255""#), Value::Str(vec![255]));
}

#[test]
fn long_strings() {
    assert_eq!(one("X = [[abc]]"), s("abc"));
    assert_eq!(one("X = [[\r\nl1\r\nl2]]"), s("l1\nl2"));
    assert_eq!(one("X = [==[a]]b]=]c]==]"), s("a]]b]=]c"));
    assert_eq!(one("X = { [ [[k]] ] = [=[v]=] }").get("k"), Some(&s("v")));
}

#[test]
fn comments() {
    let src = "-- en-tête\r\nX = { -- fin de ligne\r\n --[[ bloc\r\n sur deux lignes ]] 1, --[==[ ]] ]==] 2 }\r\n-- fin";
    let v = one(src);
    assert_eq!(
        v.array().cloned().collect::<Vec<_>>(),
        [Value::Int(1), Value::Int(2)]
    );
}

#[test]
fn keys_string_int_negative_ident_and_trailing_commas() {
    let v = one("X = { [\"a\"] = 1, [2] = 2, [-3] = 3, b = 4, [5.0] = 5, [1.5] = 6, [true] = 7, }");
    let t = v.as_table().unwrap();
    assert_eq!(t.get(&Key::from("a")), Some(&Value::Int(1)));
    assert_eq!(t.get(&Key::Int(2)), Some(&Value::Int(2)));
    assert_eq!(t.get(&Key::Int(-3)), Some(&Value::Int(3)));
    assert_eq!(t.get(&Key::from("b")), Some(&Value::Int(4)));
    assert_eq!(t.get(&Key::Int(5)), Some(&Value::Int(5)));
    assert_eq!(t.get(&Key::Bool(true)), Some(&Value::Int(7)));
    assert_eq!(t.len(), 7);
    assert_eq!(one("X = {}"), Value::Table(Default::default()));
    // Comme Lua 5.1 : un séparateur seul n'est pas une liste de champs.
    assert!(parse(b"X = { ; }").is_err());
    assert!(parse(b"X = { 1,, }").is_err());
    assert_eq!(one("X = { 1; 2; }").array().count(), 2);
}

#[test]
fn positional_nils_advance_the_implicit_index() {
    // Forme réelle de l'équipement d'une baseline d'audit.
    let v =
        one("X = {\r\nnil,\r\nnil,\r\nnil,\r\n\"6096:0\",\r\nnil,\r\n\"x\",\r\n[11] = \"y\",\r\n}");
    let t = v.as_table().unwrap();
    assert_eq!(t.len(), 3);
    assert_eq!(t.get(&Key::Int(4)), Some(&s("6096:0")));
    assert_eq!(t.get(&Key::Int(6)), Some(&s("x")));
    assert_eq!(t.get(&Key::Int(11)), Some(&s("y")));
    assert_eq!(v.array().count(), 0, "ipairs s'arrête au premier trou");
}

#[test]
fn explicit_nil_value_removes_the_key() {
    let v = one("X = { [\"a\"] = 1, [\"a\"] = nil, [\"b\"] = 2, [\"b\"] = 3 }");
    assert_eq!(v.get("a"), None);
    assert_eq!(v.get("b"), Some(&Value::Int(3)));
}

#[test]
fn positional_vs_explicit_follows_lua51_flush_order() {
    // Positionnel en attente : écrit à la fin, il gagne sur la clé explicite.
    assert_eq!(
        one("X = { \"pos\", [1] = \"exp\" }").get(1),
        Some(&s("pos"))
    );
    // Après un lot complet de 50, la clé explicite suivante gagne.
    let items = vec!["0"; 50].join(", ");
    let v = one(&format!("X = {{ {items}, [1] = \"exp\" }}"));
    assert_eq!(v.get(1), Some(&s("exp")));
    assert_eq!(v.as_table().unwrap().len(), 50);
    let items = vec!["7"; 120].join(",");
    assert_eq!(one(&format!("X = {{ {items} }}")).array().count(), 120);
}

#[test]
fn nested_tables() {
    let v = one("X = { [\"a\"] = { [\"b\"] = { 1, { 2 } } } }");
    let inner = v.get("a").unwrap().get("b").unwrap();
    assert_eq!(inner.get(1), Some(&Value::Int(1)));
    assert_eq!(inner.get(2).unwrap().get(1), Some(&Value::Int(2)));
}

#[test]
fn bom_is_tolerated() {
    let sv = parse(b"\xEF\xBB\xBFX = 1").unwrap();
    assert_eq!(sv.get("X"), Some(&Value::Int(1)));
}

#[test]
fn errors_carry_positions() {
    let e = err(b"\r\nX = {\r\n[\"a\"] = 1,\r\n");
    assert_eq!((e.line, e.col), (4, 1));
    assert!(e.message.contains("fin de fichier"), "{e}");
    assert!(e.message.contains("2:5"), "{e}");

    let e = err(b"X = {\n  [\"a\"] 1 }");
    assert_eq!((e.line, e.col), (2, 9));

    let e = err(b"X = \"abc");
    assert_eq!((e.line, e.col), (1, 5));
    assert!(e.message.contains("chaîne non terminée"));

    let e = err(b"X = \"a\nb\"");
    assert!(e.message.contains("chaîne non terminée"));

    let e = err(b"X = [[abc");
    assert!(e.message.contains("chaîne longue"));

    let e = err(b"--[[ jamais ferme");
    assert!(e.message.contains("commentaire long"));

    let e = err(b"X = { 1 2 }");
    assert_eq!((e.line, e.col), (1, 9));

    let e = err(b"X = 1e");
    assert!(e.message.contains("nombre"));

    let e = err(b"X = \"\\256\"");
    assert!(e.message.contains("décimal"));

    let e = err(b"X = { [nil] = 1 }");
    assert!(e.message.contains("nil"));

    let e = err(b"X = { [{}] = 1 }");
    assert!(e.message.contains("table"));

    let e = err(b"X =");
    assert!(e.message.contains("valeur attendue"));

    let e = err(b"X = -\"a\"");
    assert!(e.message.contains("nombre attendu"));

    let e = err(b"1 = 2");
    assert!(e.message.contains("nom de variable"));

    let e = err(b"X = @");
    assert!(e.message.contains("0x40"));

    let e = err(b"X = { y }");
    assert!(e.message.contains("`=`"));

    assert_eq!(
        err(b"X = 1\nY").to_string(),
        "2:2: `=` attendu après le nom de la variable, trouvé fin de fichier"
    );
}

#[test]
fn nesting_is_bounded() {
    let deep = format!("X = {}{}", "{".repeat(10_000), "}".repeat(10_000));
    let e = err(deep.as_bytes());
    assert!(e.message.contains("profond"));
    let ok = format!("X = {}{}", "{".repeat(150), "}".repeat(150));
    assert!(parse(ok.as_bytes()).is_ok());
}

#[test]
fn empty_and_whitespace_files() {
    assert!(parse(b"").unwrap().is_empty());
    assert!(parse(b"\r\n\r\n").unwrap().is_empty());
    assert_eq!(parse(b"A = 1; B = 2;").unwrap().len(), 2);
}
