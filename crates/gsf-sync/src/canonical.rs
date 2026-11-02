//! JSON canonique et `snapshot_hash`.
//!
//! Le hash ne doit dépendre que du contenu : clés d'objets triées (ordre des
//! octets UTF-8), aucun espace, nombres et chaînes au format serde_json. On
//! n'utilise pas l'ordre de `serde_json::Map`, qui change si une dépendance
//! active la fonctionnalité `preserve_order`.

use serde_json::Value;
use sha2::{Digest, Sha256};

/// Sérialisation canonique d'une valeur JSON.
pub fn canonical_json(value: &Value) -> String {
    let mut out = String::new();
    write(value, &mut out);
    out
}

fn write(value: &Value, out: &mut String) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => out.push_str(&n.to_string()),
        Value::String(s) => out.push_str(&Value::String(s.clone()).to_string()),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write(item, out);
            }
            out.push(']');
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (i, key) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&Value::String(key.clone()).to_string());
                out.push(':');
                write(&map[key], out);
            }
            out.push('}');
        }
    }
}

/// SHA-256 en hexadécimal minuscule.
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// `snapshot_hash` du contrat : sha256 du JSON canonique de `guilds`.
pub fn snapshot_hash(guilds: &Value) -> String {
    sha256_hex(canonical_json(guilds).as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn keys_are_sorted_recursively_without_spaces() {
        let v = json!({"b": [1, {"z": null, "a": true}], "a": "é\"\n", "c": 1.5});
        assert_eq!(
            canonical_json(&v),
            r#"{"a":"é\"\n","b":[1,{"a":true,"z":null}],"c":1.5}"#
        );
    }

    #[test]
    fn hash_ignores_key_order_but_not_content() {
        let a = json!([{"x": 1, "y": 2}]);
        let b: Value = serde_json::from_str(r#"[{"y":2,"x":1}]"#).unwrap();
        let c = json!([{"x": 1, "y": 3}]);
        assert_eq!(snapshot_hash(&a), snapshot_hash(&b));
        assert_ne!(snapshot_hash(&a), snapshot_hash(&c));
        assert_eq!(snapshot_hash(&a).len(), 64);
    }

    #[test]
    fn sha256_known_vector() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
