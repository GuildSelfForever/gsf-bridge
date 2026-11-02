//! Lexer du sous-ensemble Lua 5.1 écrit par WoW dans les SavedVariables.
//!
//! Fidèle à `llex.c` de Lua 5.1 pour ce qui peut apparaître dans un fichier
//! SavedVariables : chaînes courtes (échappements 5.1, octets bruts), chaînes
//! longues `[[ ]]` / `[=[ ]=]`, commentaires `--` et `--[[ ]]`, nombres
//! décimaux / flottants / exposants / hexadécimaux. Les fins de ligne `\r\n`,
//! `\n\r`, `\n` et `\r` comptent chacune pour une ligne.

use super::ParseError;

/// Position (1-based) dans le fichier source. La colonne est en octets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pos {
    pub line: usize,
    pub col: usize,
}

/// Littéral numérique : entier exact si possible, sinon flottant.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Number {
    Int(i64),
    Float(f64),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Ident(String),
    Str(Vec<u8>),
    Num(Number),
    True,
    False,
    Nil,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Assign,
    Comma,
    Semi,
    Minus,
    Eof,
}

impl Tok {
    /// Libellé lisible pour les messages d'erreur.
    pub fn describe(&self) -> String {
        match self {
            Tok::Ident(s) => format!("identifiant `{s}`"),
            Tok::Str(_) => "chaîne".into(),
            Tok::Num(_) => "nombre".into(),
            Tok::True => "`true`".into(),
            Tok::False => "`false`".into(),
            Tok::Nil => "`nil`".into(),
            Tok::LBrace => "`{`".into(),
            Tok::RBrace => "`}`".into(),
            Tok::LBracket => "`[`".into(),
            Tok::RBracket => "`]`".into(),
            Tok::Assign => "`=`".into(),
            Tok::Comma => "`,`".into(),
            Tok::Semi => "`;`".into(),
            Tok::Minus => "`-`".into(),
            Tok::Eof => "fin de fichier".into(),
        }
    }
}

pub struct Lexer<'a> {
    src: &'a [u8],
    i: usize,
    line: usize,
    line_start: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(src: &'a [u8]) -> Self {
        // BOM UTF-8 toléré par prudence (WoW n'en écrit pas).
        let i = if src.starts_with(&[0xEF, 0xBB, 0xBF]) {
            3
        } else {
            0
        };
        Lexer {
            src,
            i,
            line: 1,
            line_start: 0,
        }
    }

    pub fn pos(&self) -> Pos {
        Pos {
            line: self.line,
            col: self.i - self.line_start + 1,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.src.get(self.i).copied()
    }

    fn peek_at(&self, off: usize) -> Option<u8> {
        self.src.get(self.i + off).copied()
    }

    fn err(&self, pos: Pos, msg: impl Into<String>) -> ParseError {
        ParseError {
            line: pos.line,
            col: pos.col,
            message: msg.into(),
        }
    }

    /// Consomme une fin de ligne (`\n`, `\r`, `\r\n` ou `\n\r`) à la position
    /// courante, comme `inclinenumber` de Lua 5.1.
    fn newline(&mut self) {
        let first = self.src[self.i];
        self.i += 1;
        if let Some(next) = self.peek() {
            if (next == b'\n' || next == b'\r') && next != first {
                self.i += 1;
            }
        }
        self.line += 1;
        self.line_start = self.i;
    }

    fn skip_trivia(&mut self) -> Result<(), ParseError> {
        loop {
            match self.peek() {
                Some(b'\n' | b'\r') => self.newline(),
                Some(b' ' | b'\t' | 0x0B | 0x0C) => self.i += 1,
                Some(b'-') if self.peek_at(1) == Some(b'-') => {
                    let start = self.pos();
                    self.i += 2;
                    if self.peek() == Some(b'[') {
                        if let Some(level) = self.long_bracket_level() {
                            self.read_long(level, start, "commentaire long")?;
                            continue;
                        }
                    }
                    while let Some(c) = self.peek() {
                        if c == b'\n' || c == b'\r' {
                            break;
                        }
                        self.i += 1;
                    }
                }
                _ => return Ok(()),
            }
        }
    }

    /// Si la position courante est `[` suivi de `=`* puis `[`, retourne le
    /// niveau (nombre de `=`) sans rien consommer.
    fn long_bracket_level(&self) -> Option<usize> {
        let mut j = self.i + 1;
        while self.src.get(j) == Some(&b'=') {
            j += 1;
        }
        (self.src.get(j) == Some(&b'[')).then(|| j - self.i - 1)
    }

    /// Lit une chaîne longue dont l'ouverture commence à la position courante.
    fn read_long(&mut self, level: usize, start: Pos, what: &str) -> Result<Vec<u8>, ParseError> {
        self.i += level + 2;
        // Un saut de ligne juste après l'ouverture est ignoré.
        if matches!(self.peek(), Some(b'\n' | b'\r')) {
            self.newline();
        }
        let mut out = Vec::new();
        loop {
            match self.peek() {
                None => return Err(self.err(start, format!("{what} non terminé"))),
                Some(b']') => {
                    let mut j = self.i + 1;
                    while self.src.get(j) == Some(&b'=') {
                        j += 1;
                    }
                    if j - self.i - 1 == level && self.src.get(j) == Some(&b']') {
                        self.i = j + 1;
                        return Ok(out);
                    }
                    out.push(b']');
                    self.i += 1;
                }
                Some(b'\n' | b'\r') => {
                    out.push(b'\n');
                    self.newline();
                }
                Some(c) => {
                    out.push(c);
                    self.i += 1;
                }
            }
        }
    }

    fn read_short_string(&mut self, quote: u8, start: Pos) -> Result<Vec<u8>, ParseError> {
        self.i += 1;
        let mut out = Vec::new();
        loop {
            let Some(c) = self.peek() else {
                return Err(self.err(start, "chaîne non terminée"));
            };
            match c {
                b'\n' | b'\r' => return Err(self.err(start, "chaîne non terminée")),
                b'\\' => {
                    let esc_pos = self.pos();
                    self.i += 1;
                    let Some(e) = self.peek() else {
                        return Err(self.err(start, "chaîne non terminée"));
                    };
                    match e {
                        b'n' => out.push(b'\n'),
                        b't' => out.push(b'\t'),
                        b'r' => out.push(b'\r'),
                        b'a' => out.push(0x07),
                        b'b' => out.push(0x08),
                        b'f' => out.push(0x0C),
                        b'v' => out.push(0x0B),
                        b'\n' | b'\r' => {
                            out.push(b'\n');
                            self.newline();
                            continue;
                        }
                        b'0'..=b'9' => {
                            let mut n: u32 = 0;
                            let mut k = 0;
                            while k < 3 {
                                match self.peek() {
                                    Some(d @ b'0'..=b'9') => {
                                        n = n * 10 + u32::from(d - b'0');
                                        self.i += 1;
                                        k += 1;
                                    }
                                    _ => break,
                                }
                            }
                            let byte = u8::try_from(n)
                                .map_err(|_| self.err(esc_pos, "échappement décimal trop grand"))?;
                            out.push(byte);
                            continue;
                        }
                        // Lua 5.1 : tout autre caractère échappé vaut lui-même
                        // (`\\`, `\"`, `\'`, mais aussi `\q` → `q`).
                        other => out.push(other),
                    }
                    self.i += 1;
                }
                _ if c == quote => {
                    self.i += 1;
                    return Ok(out);
                }
                _ => {
                    out.push(c);
                    self.i += 1;
                }
            }
        }
    }

    fn read_number(&mut self, start: Pos) -> Result<Number, ParseError> {
        let begin = self.i;
        // Comme `read_numeral` de Lua 5.1 : chiffres, points, exposant signé,
        // puis tout caractère alphanumérique ou `_` collé.
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || c == b'.' || c == b'_' {
                let seen = &self.src[begin..self.i];
                let hex = seen.len() >= 2 && seen[0] == b'0' && (seen[1] | 0x20) == b'x';
                let exp = (c == b'e' || c == b'E') && !hex;
                self.i += 1;
                if exp && matches!(self.peek(), Some(b'+' | b'-')) {
                    self.i += 1;
                }
            } else {
                break;
            }
        }
        let text = std::str::from_utf8(&self.src[begin..self.i]).unwrap_or("");
        parse_number(text).ok_or_else(|| self.err(start, format!("nombre mal formé `{text}`")))
    }

    pub fn next(&mut self) -> Result<(Tok, Pos), ParseError> {
        self.skip_trivia()?;
        let pos = self.pos();
        let Some(c) = self.peek() else {
            return Ok((Tok::Eof, pos));
        };
        let tok = match c {
            b'{' => {
                self.i += 1;
                Tok::LBrace
            }
            b'}' => {
                self.i += 1;
                Tok::RBrace
            }
            b']' => {
                self.i += 1;
                Tok::RBracket
            }
            b'=' => {
                self.i += 1;
                Tok::Assign
            }
            b',' => {
                self.i += 1;
                Tok::Comma
            }
            b';' => {
                self.i += 1;
                Tok::Semi
            }
            b'-' => {
                self.i += 1;
                Tok::Minus
            }
            b'[' => match self.long_bracket_level() {
                Some(level) => Tok::Str(self.read_long(level, pos, "chaîne longue")?),
                None => {
                    self.i += 1;
                    Tok::LBracket
                }
            },
            b'"' | b'\'' => Tok::Str(self.read_short_string(c, pos)?),
            b'0'..=b'9' => Tok::Num(self.read_number(pos)?),
            b'.' if matches!(self.peek_at(1), Some(b'0'..=b'9')) => {
                Tok::Num(self.read_number(pos)?)
            }
            c if c.is_ascii_alphabetic() || c == b'_' => {
                let begin = self.i;
                while matches!(self.peek(), Some(c) if c.is_ascii_alphanumeric() || c == b'_') {
                    self.i += 1;
                }
                // Que de l'ASCII : la conversion ne peut pas échouer.
                let word = String::from_utf8_lossy(&self.src[begin..self.i]).into_owned();
                match word.as_str() {
                    "true" => Tok::True,
                    "false" => Tok::False,
                    "nil" => Tok::Nil,
                    _ => Tok::Ident(word),
                }
            }
            other => {
                return Err(self.err(pos, format!("caractère inattendu 0x{other:02X}")));
            }
        };
        Ok((tok, pos))
    }
}

/// Convertit un littéral numérique Lua 5.1 (sans signe).
fn parse_number(text: &str) -> Option<Number> {
    if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        if hex.is_empty() || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        return Some(match i64::from_str_radix(hex, 16) {
            Ok(n) => Number::Int(n),
            Err(_) => Number::Float(u128::from_str_radix(hex, 16).ok()? as f64),
        });
    }
    let valid = {
        let b = text.as_bytes();
        let mut k = 0;
        let mut digits = 0;
        while k < b.len() && b[k].is_ascii_digit() {
            k += 1;
            digits += 1;
        }
        if k < b.len() && b[k] == b'.' {
            k += 1;
            while k < b.len() && b[k].is_ascii_digit() {
                k += 1;
                digits += 1;
            }
        }
        if digits > 0 && k < b.len() && (b[k] == b'e' || b[k] == b'E') {
            k += 1;
            if k < b.len() && (b[k] == b'+' || b[k] == b'-') {
                k += 1;
            }
            let exp_start = k;
            while k < b.len() && b[k].is_ascii_digit() {
                k += 1;
            }
            if k == exp_start {
                return None;
            }
        }
        digits > 0 && k == b.len()
    };
    if !valid {
        return None;
    }
    if text.bytes().all(|b| b.is_ascii_digit()) {
        if let Ok(n) = text.parse::<i64>() {
            return Some(Number::Int(n));
        }
    }
    text.parse::<f64>().ok().map(Number::Float)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(src: &str) -> Vec<Tok> {
        let mut lx = Lexer::new(src.as_bytes());
        let mut out = Vec::new();
        loop {
            let (t, _) = lx.next().unwrap();
            if t == Tok::Eof {
                return out;
            }
            out.push(t);
        }
    }

    #[test]
    fn numbers() {
        assert_eq!(parse_number("42"), Some(Number::Int(42)));
        assert_eq!(parse_number("1.5"), Some(Number::Float(1.5)));
        assert_eq!(parse_number(".5"), Some(Number::Float(0.5)));
        assert_eq!(parse_number("5."), Some(Number::Float(5.0)));
        assert_eq!(parse_number("1e3"), Some(Number::Float(1000.0)));
        assert_eq!(parse_number("2.5E-2"), Some(Number::Float(0.025)));
        assert_eq!(parse_number("0x1F"), Some(Number::Int(31)));
        assert_eq!(
            parse_number("9223372036854775808"),
            Some(Number::Float(9_223_372_036_854_775_808.0))
        );
        assert_eq!(parse_number("1e"), None);
        assert_eq!(parse_number("12abc"), None);
        assert_eq!(parse_number("0x"), None);
    }

    #[test]
    fn comments_are_skipped() {
        assert_eq!(
            toks("-- line\r\nx --[[ block\r\n ]] = --[==[ x ]] ]==] 1"),
            vec![
                Tok::Ident("x".into()),
                Tok::Assign,
                Tok::Num(Number::Int(1))
            ]
        );
    }

    #[test]
    fn positions_count_crlf_as_one_line() {
        let mut lx = Lexer::new(b"\r\n\r\n  x");
        let (_, pos) = lx.next().unwrap();
        assert_eq!(pos, Pos { line: 3, col: 3 });
    }
}
