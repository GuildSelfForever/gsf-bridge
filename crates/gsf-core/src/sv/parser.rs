//! Parseur à descente récursive des SavedVariables.
//!
//! Grammaire acceptée (sous-ensemble de Lua 5.1) :
//!
//! ```text
//! fichier    := { Nom '=' valeur | ';' }
//! valeur     := nil | true | false | nombre | '-' nombre | chaîne | table
//! table      := '{' [ champ { (',' | ';') champ } [',' | ';'] ] '}'
//! champ      := '[' valeur ']' '=' valeur | Nom '=' valeur | valeur
//! ```
//!
//! Les champs positionnels suivent exactement Lua 5.1 : ils sont mis en
//! attente et écrits par lots de 50 (`LFIELDS_PER_FLUSH`), un `nil`
//! positionnel fait avancer l'index et efface la case, une clé explicite est
//! écrite immédiatement.

use std::collections::BTreeMap;

use super::lexer::{Lexer, Number, Pos, Tok};
use super::{FloatKey, Key, ParseError, SavedVariables, Value};

/// Profondeur maximale d'imbrication des tables (Lua 5.1 : `LUAI_MAXCCALLS`).
const MAX_DEPTH: usize = 200;
/// Taille des lots de champs positionnels (Lua 5.1 : `LFIELDS_PER_FLUSH`).
const FIELDS_PER_FLUSH: usize = 50;

/// Parse le contenu brut d'un fichier SavedVariables.
///
/// Ne panique jamais : toute entrée invalide (fichier tronqué pendant
/// l'écriture, octets parasites…) retourne une [`ParseError`] positionnée.
pub fn parse(src: &[u8]) -> Result<SavedVariables, ParseError> {
    let mut p = Parser::new(src)?;
    let mut out = SavedVariables::default();
    loop {
        match &p.tok {
            Tok::Eof => return Ok(out),
            Tok::Semi => p.advance()?,
            Tok::Ident(name) => {
                let name = name.clone();
                p.advance()?;
                p.expect(&Tok::Assign, "`=` attendu après le nom de la variable")?;
                let value = p.value()?;
                out.set(name, value);
            }
            other => {
                let msg = format!("nom de variable attendu, trouvé {}", other.describe());
                return Err(p.error_here(msg));
            }
        }
    }
}

struct Parser<'a> {
    lx: Lexer<'a>,
    tok: Tok,
    pos: Pos,
    depth: usize,
}

impl<'a> Parser<'a> {
    fn new(src: &'a [u8]) -> Result<Self, ParseError> {
        let mut lx = Lexer::new(src);
        let (tok, pos) = lx.next()?;
        Ok(Parser {
            lx,
            tok,
            pos,
            depth: 0,
        })
    }

    fn advance(&mut self) -> Result<(), ParseError> {
        let (tok, pos) = self.lx.next()?;
        self.tok = tok;
        self.pos = pos;
        Ok(())
    }

    fn error_at(&self, pos: Pos, message: impl Into<String>) -> ParseError {
        ParseError {
            line: pos.line,
            col: pos.col,
            message: message.into(),
        }
    }

    fn error_here(&self, message: impl Into<String>) -> ParseError {
        self.error_at(self.pos, message)
    }

    fn expect(&mut self, want: &Tok, msg: &str) -> Result<(), ParseError> {
        if &self.tok == want {
            self.advance()
        } else {
            Err(self.error_here(format!("{msg}, trouvé {}", self.tok.describe())))
        }
    }

    fn value(&mut self) -> Result<Value, ParseError> {
        let v = match &self.tok {
            Tok::Nil => Value::Nil,
            Tok::True => Value::Bool(true),
            Tok::False => Value::Bool(false),
            Tok::Num(n) => number(*n),
            Tok::Str(s) => Value::Str(s.clone()),
            Tok::Minus => {
                self.advance()?;
                let Tok::Num(n) = self.tok else {
                    let msg = format!("nombre attendu après `-`, trouvé {}", self.tok.describe());
                    return Err(self.error_here(msg));
                };
                negate(n)
            }
            Tok::LBrace => return self.table(),
            other => {
                let msg = format!("valeur attendue, trouvé {}", other.describe());
                return Err(self.error_here(msg));
            }
        };
        self.advance()?;
        Ok(v)
    }

    fn table(&mut self) -> Result<Value, ParseError> {
        let open = self.pos;
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(self.error_at(open, "tables imbriquées trop profondément"));
        }
        self.advance()?;

        let mut map = BTreeMap::new();
        let mut pending: Vec<Value> = Vec::new();
        let mut flushed: i64 = 0;

        while self.tok != Tok::RBrace {
            if pending.len() == FIELDS_PER_FLUSH {
                flush(&mut map, &mut pending, &mut flushed);
            }
            match &self.tok {
                Tok::LBracket => {
                    let key_pos = self.pos;
                    self.advance()?;
                    let key = self.value()?;
                    self.expect(&Tok::RBracket, "`]` attendu")?;
                    self.expect(&Tok::Assign, "`=` attendu après la clé")?;
                    let value = self.value()?;
                    let key = to_key(key).map_err(|m| self.error_at(key_pos, m))?;
                    assign(&mut map, key, value);
                }
                Tok::Ident(name) => {
                    let key = Key::Str(name.as_bytes().to_vec());
                    self.advance()?;
                    self.expect(&Tok::Assign, "`=` attendu après le nom du champ")?;
                    let value = self.value()?;
                    assign(&mut map, key, value);
                }
                Tok::Eof => {
                    return Err(self.error_here(format!(
                        "fin de fichier inattendue : `}}` attendu (table ouverte en {}:{})",
                        open.line, open.col
                    )));
                }
                _ => {
                    let value = self.value()?;
                    pending.push(value);
                }
            }
            match self.tok {
                Tok::Comma | Tok::Semi => self.advance()?,
                Tok::RBrace => {}
                Tok::Eof => {
                    return Err(self.error_here(format!(
                        "fin de fichier inattendue : `}}` attendu (table ouverte en {}:{})",
                        open.line, open.col
                    )));
                }
                _ => {
                    let msg = format!("`,` ou `}}` attendu, trouvé {}", self.tok.describe());
                    return Err(self.error_here(msg));
                }
            }
        }
        flush(&mut map, &mut pending, &mut flushed);
        self.advance()?;
        self.depth -= 1;
        Ok(Value::Table(map))
    }
}

fn number(n: Number) -> Value {
    match n {
        Number::Int(i) => Value::Int(i),
        Number::Float(f) => Value::Num(f),
    }
}

fn negate(n: Number) -> Value {
    match n {
        Number::Int(i) => match i.checked_neg() {
            Some(v) => Value::Int(v),
            None => Value::Num(-(i as f64)),
        },
        Number::Float(f) => Value::Num(-f),
    }
}

/// `t[k] = v` : une valeur `nil` retire la clé, comme en Lua.
fn assign(map: &mut BTreeMap<Key, Value>, key: Key, value: Value) {
    if value.is_nil() {
        map.remove(&key);
    } else {
        map.insert(key, value);
    }
}

fn flush(map: &mut BTreeMap<Key, Value>, pending: &mut Vec<Value>, flushed: &mut i64) {
    for value in pending.drain(..) {
        *flushed += 1;
        assign(map, Key::Int(*flushed), value);
    }
}

fn to_key(v: Value) -> Result<Key, &'static str> {
    match v {
        Value::Nil => Err("clé de table `nil`"),
        Value::Bool(b) => Ok(Key::Bool(b)),
        Value::Int(i) => Ok(Key::Int(i)),
        Value::Num(f) if f.is_nan() => Err("clé de table NaN"),
        Value::Num(f) if f.fract() == 0.0 && f >= i64::MIN as f64 && f < i64::MAX as f64 => {
            Ok(Key::Int(f as i64))
        }
        // -0.0 et 0.0 sont la même clé en Lua ; entier déjà traité ci-dessus.
        Value::Num(f) => Ok(Key::Num(FloatKey(f))),
        Value::Str(s) => Ok(Key::Str(s)),
        Value::Table(_) => Err("clé de table de type table non supportée"),
    }
}
