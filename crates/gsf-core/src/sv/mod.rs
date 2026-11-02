//! SavedVariables : sous-ensemble Lua écrit par le client WoW.
//!
//! [`parse`] lit un fichier `WTF/…/SavedVariables/*.lua` (octets bruts, sans
//! hypothèse d'encodage) et retourne les variables globales dans l'ordre du
//! fichier. Les tables suivent la sémantique Lua 5.1 : `nil` positionnels qui
//! font avancer l'index implicite, clés `[n]=` / `["k"]=` / `k=` mêlées,
//! valeurs `nil` absentes de la table.
//!
//! L'ordre des clés change à chaque écriture du jeu (ordre de hachage) : ne
//! jamais comparer deux fichiers octet par octet, comparer les [`Value`].

mod lexer;
mod parser;

use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt;
use std::hash::{Hash, Hasher};

pub use parser::parse;

/// Erreur de parsing avec position (ligne et colonne 1-based, colonne en
/// octets).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub line: usize,
    pub col: usize,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: {}", self.line, self.col, self.message)
    }
}

impl std::error::Error for ParseError {}

/// Clé flottante non entière (`[1.5] = …`), ordonnée totalement.
///
/// Les clés flottantes entières sont normalisées en [`Key::Int`] comme en Lua
/// (`t[2.0]` et `t[2]` sont la même case).
#[derive(Debug, Clone, Copy)]
pub struct FloatKey(f64);

impl FloatKey {
    pub fn get(self) -> f64 {
        self.0
    }
}

impl PartialEq for FloatKey {
    fn eq(&self, other: &Self) -> bool {
        self.0.to_bits() == other.0.to_bits()
    }
}

impl Eq for FloatKey {}

impl PartialOrd for FloatKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for FloatKey {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.total_cmp(&other.0)
    }
}

impl Hash for FloatKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.to_bits().hash(state);
    }
}

/// Clé d'une table Lua telle qu'écrite dans les SavedVariables.
///
/// WoW n'écrit en pratique que des clés entières ou chaînes ; booléens et
/// flottants sont acceptés par prudence. L'ordre (`Int` avant `Str`) donne
/// une sortie déterministe.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Key {
    Int(i64),
    /// Octets bruts : les SavedVariables peuvent contenir du non UTF-8.
    Str(Vec<u8>),
    Bool(bool),
    Num(FloatKey),
}

/// Valeur Lua d'une SavedVariable.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Nil,
    Bool(bool),
    Int(i64),
    Num(f64),
    /// Octets bruts : les SavedVariables peuvent contenir du non UTF-8.
    Str(Vec<u8>),
    Table(BTreeMap<Key, Value>),
}

impl From<&str> for Key {
    fn from(s: &str) -> Self {
        Key::Str(s.as_bytes().to_vec())
    }
}

impl From<&[u8]> for Key {
    fn from(s: &[u8]) -> Self {
        Key::Str(s.to_vec())
    }
}

impl From<i64> for Key {
    fn from(n: i64) -> Self {
        Key::Int(n)
    }
}

impl Key {
    /// Clé chaîne (octets bruts), si c'en est une.
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Key::Str(b) => Some(b),
            _ => None,
        }
    }

    /// Clé chaîne décodée en UTF-8 avec remplacement des octets invalides.
    pub fn as_str_lossy(&self) -> Option<Cow<'_, str>> {
        self.as_bytes().map(String::from_utf8_lossy)
    }

    /// Clé entière, si c'en est une.
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Key::Int(n) => Some(*n),
            _ => None,
        }
    }
}

impl Value {
    /// Accès à un champ si la valeur est une table.
    pub fn get(&self, key: impl Into<Key>) -> Option<&Value> {
        self.as_table()?.get(&key.into())
    }

    /// Table sous-jacente, si la valeur est une table.
    pub fn as_table(&self) -> Option<&BTreeMap<Key, Value>> {
        match self {
            Value::Table(t) => Some(t),
            _ => None,
        }
    }

    /// Chaîne en UTF-8 si la valeur est une chaîne valide.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(b) => std::str::from_utf8(b).ok(),
            _ => None,
        }
    }

    /// Octets bruts si la valeur est une chaîne.
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Value::Str(b) => Some(b),
            _ => None,
        }
    }

    /// Chaîne décodée en UTF-8 avec remplacement des octets invalides.
    pub fn as_str_lossy(&self) -> Option<Cow<'_, str>> {
        self.as_bytes().map(String::from_utf8_lossy)
    }

    /// Entier : `Int`, ou `Num` sans partie fractionnaire et dans les bornes.
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Int(n) => Some(*n),
            Value::Num(f) if f.fract() == 0.0 && *f >= i64::MIN as f64 && *f < i64::MAX as f64 => {
                Some(*f as i64)
            }
            _ => None,
        }
    }

    /// Nombre (entier ou flottant) en `f64`.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Int(n) => Some(*n as f64),
            Value::Num(f) => Some(*f),
            _ => None,
        }
    }

    /// Booléen, si la valeur en est un.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// Vrai si la valeur est `nil`.
    pub fn is_nil(&self) -> bool {
        matches!(self, Value::Nil)
    }

    /// Partie tableau, comme `ipairs` : `t[1]`, `t[2]`… jusqu'au premier trou.
    /// Vide si la valeur n'est pas une table.
    pub fn array(&self) -> impl Iterator<Item = &Value> + '_ {
        let table = self.as_table();
        (1i64..).map_while(move |i| table?.get(&Key::Int(i)))
    }

    /// Champs à clé chaîne (octets bruts) d'une table, dans l'ordre des clés.
    pub fn str_entries(&self) -> impl Iterator<Item = (&[u8], &Value)> + '_ {
        self.as_table()
            .into_iter()
            .flat_map(|t| t.iter())
            .filter_map(|(k, v)| k.as_bytes().map(|b| (b, v)))
    }
}

/// Contenu d'un fichier SavedVariables : variables globales dans l'ordre du
/// fichier. Une variable écrite `Nom = nil` est présente avec [`Value::Nil`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SavedVariables {
    globals: Vec<(String, Value)>,
}

impl SavedVariables {
    /// Valeur d'une variable globale (`None` si absente du fichier).
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.globals.iter().find(|(n, _)| n == name).map(|(_, v)| v)
    }

    /// Affecte une variable ; une réaffectation garde la première position.
    pub fn set(&mut self, name: String, value: Value) {
        match self.globals.iter_mut().find(|(n, _)| *n == name) {
            Some(slot) => slot.1 = value,
            None => self.globals.push((name, value)),
        }
    }

    /// Variables dans l'ordre du fichier.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Value)> + '_ {
        self.globals.iter().map(|(n, v)| (n.as_str(), v))
    }

    pub fn len(&self) -> usize {
        self.globals.len()
    }

    pub fn is_empty(&self) -> bool {
        self.globals.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_lookup_by_string_and_int_keys() {
        let mut t = BTreeMap::new();
        t.insert(Key::from("version"), Value::Str(b"0.17.46".to_vec()));
        t.insert(Key::Int(1), Value::Int(-3));
        t.insert(Key::Int(2), Value::Num(1.5));
        t.insert(Key::from("enabled"), Value::Bool(true));
        let db = Value::Table(t);

        assert_eq!(db.get("version").and_then(Value::as_str), Some("0.17.46"));
        assert_eq!(db.get(1), Some(&Value::Int(-3)));
        assert_eq!(db.get(2), Some(&Value::Num(1.5)));
        assert_eq!(db.get("enabled"), Some(&Value::Bool(true)));
        assert!(db.get("missing").is_none());
        assert!(Value::Nil.is_nil());
        assert_eq!(db.array().count(), 2);
        assert_eq!(db.str_entries().count(), 2);
    }

    #[test]
    fn keys_are_ordered_ints_before_strings() {
        let mut keys = [Key::from("a"), Key::Int(2), Key::Int(-1)];
        keys.sort();
        assert_eq!(keys, [Key::Int(-1), Key::Int(2), Key::from("a")]);
    }

    #[test]
    fn non_utf8_string_is_kept_but_not_exposed_as_str() {
        let v = Value::Str(vec![0xff, 0xfe]);
        assert_eq!(v.as_str(), None);
        assert_eq!(v.as_str_lossy().as_deref(), Some("\u{fffd}\u{fffd}"));
    }

    #[test]
    fn numeric_helpers() {
        assert_eq!(Value::Num(3.0).as_i64(), Some(3));
        assert_eq!(Value::Num(3.5).as_i64(), None);
        assert_eq!(Value::Int(-2).as_f64(), Some(-2.0));
        assert_eq!(Value::Str(b"1".to_vec()).as_i64(), None);
    }

    #[test]
    fn saved_variables_keep_file_order_and_last_assignment() {
        let mut sv = SavedVariables::default();
        sv.set("B".into(), Value::Int(1));
        sv.set("A".into(), Value::Nil);
        sv.set("B".into(), Value::Int(2));
        let names: Vec<_> = sv.iter().map(|(n, _)| n).collect();
        assert_eq!(names, ["B", "A"]);
        assert_eq!(sv.get("B"), Some(&Value::Int(2)));
        assert_eq!(sv.get("A"), Some(&Value::Nil));
        assert_eq!(sv.get("C"), None);
    }
}
