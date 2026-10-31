//! A small JSON reader with `JSON.parse` semantics, for the few wire fields
//! that carry JSON (`match_state`, the `weapon_state` lists, `agent_directives`).
//!
//! Standard library only. It is not a general JSON library: it reads values
//! exactly as V8's `JSON.parse` would and keeps object keys in JavaScript's
//! property order, which is what the TypeScript iterates.

use std::fmt;

/// A parsed JSON value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    /// JSON numbers are f64 in JavaScript, parsed with correct rounding, as
    /// Rust's `str::parse::<f64>` also does.
    Number(f64),
    String(String),
    Array(Vec<Value>),
    /// Keys in JavaScript property order: integer-like keys ascending first,
    /// then the rest in insertion order. A repeated key keeps its first
    /// position and its last value, as `JSON.parse` does.
    Object(Vec<(String, Value)>),
}

impl Value {
    /// The member named `key`, if this is an object that has it.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Object(members) => members.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Number(n) => Some(*n),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(items) => Some(items),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }
}

/// Why a text is not JSON.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// Byte offset in the input.
    pub at: usize,
    pub message: &'static str,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid JSON at byte {}: {}", self.at, self.message)
    }
}

impl std::error::Error for ParseError {}

/// `JSON.parse(text)`.
///
/// One divergence: a `\\u` escape that leaves a lone surrogate becomes U+FFFD,
/// because a Rust `String` cannot hold one. The wire never carries such text
/// (strings arrive through `TextDecoder`, which already replaced them).
pub fn parse(text: &str) -> Result<Value, ParseError> {
    let mut parser = Parser {
        bytes: text.as_bytes(),
        at: 0,
        depth: 0,
    };
    parser.whitespace();
    let value = parser.value()?;
    parser.whitespace();
    if parser.at != parser.bytes.len() {
        return Err(parser.error("unexpected text after the value"));
    }
    Ok(value)
}

/// Nesting deeper than this is refused rather than overflowing the stack.
const MAX_DEPTH: usize = 512;

struct Parser<'a> {
    bytes: &'a [u8],
    at: usize,
    depth: usize,
}

impl Parser<'_> {
    fn error(&self, message: &'static str) -> ParseError {
        ParseError { at: self.at, message }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn whitespace(&mut self) {
        while let Some(b' ' | b'\t' | b'\n' | b'\r') = self.peek() {
            self.at += 1;
        }
    }

    fn literal(&mut self, word: &'static str, value: Value) -> Result<Value, ParseError> {
        if self.bytes[self.at..].starts_with(word.as_bytes()) {
            self.at += word.len();
            Ok(value)
        } else {
            Err(self.error("unknown literal"))
        }
    }

    fn value(&mut self) -> Result<Value, ParseError> {
        match self.peek() {
            Some(b'n') => self.literal("null", Value::Null),
            Some(b't') => self.literal("true", Value::Bool(true)),
            Some(b'f') => self.literal("false", Value::Bool(false)),
            Some(b'"') => Ok(Value::String(self.string()?)),
            Some(b'[') => self.array(),
            Some(b'{') => self.object(),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(_) => Err(self.error("unexpected character")),
            None => Err(self.error("unexpected end of input")),
        }
    }

    fn enter(&mut self) -> Result<(), ParseError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            Err(self.error("nested too deeply"))
        } else {
            Ok(())
        }
    }

    fn array(&mut self) -> Result<Value, ParseError> {
        self.enter()?;
        self.at += 1;
        let mut items = Vec::new();
        self.whitespace();
        if self.peek() == Some(b']') {
            self.at += 1;
            self.depth -= 1;
            return Ok(Value::Array(items));
        }
        loop {
            self.whitespace();
            items.push(self.value()?);
            self.whitespace();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b']') => {
                    self.at += 1;
                    self.depth -= 1;
                    return Ok(Value::Array(items));
                }
                _ => return Err(self.error("expected ',' or ']'")),
            }
        }
    }

    fn object(&mut self) -> Result<Value, ParseError> {
        self.enter()?;
        self.at += 1;
        let mut members: Vec<(String, Value)> = Vec::new();
        self.whitespace();
        if self.peek() == Some(b'}') {
            self.at += 1;
            self.depth -= 1;
            return Ok(Value::Object(members));
        }
        loop {
            self.whitespace();
            if self.peek() != Some(b'"') {
                return Err(self.error("expected a string key"));
            }
            let key = self.string()?;
            self.whitespace();
            if self.peek() != Some(b':') {
                return Err(self.error("expected ':'"));
            }
            self.at += 1;
            self.whitespace();
            let value = self.value()?;
            match members.iter_mut().find(|(k, _)| *k == key) {
                Some(existing) => existing.1 = value,
                None => members.push((key, value)),
            }
            self.whitespace();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b'}') => {
                    self.at += 1;
                    self.depth -= 1;
                    return Ok(Value::Object(js_property_order(members)));
                }
                _ => return Err(self.error("expected ',' or '}'")),
            }
        }
    }

    fn string(&mut self) -> Result<String, ParseError> {
        self.at += 1; // opening quote
        let mut out = String::new();
        let mut pending_high: Option<u16> = None;
        loop {
            let start = self.at;
            while let Some(b) = self.peek() {
                if b == b'"' || b == b'\\' || b < 0x20 {
                    break;
                }
                self.at += 1;
            }
            if self.at > start {
                if pending_high.take().is_some() {
                    out.push('\u{FFFD}');
                }
                // The input is a &str and we split only at ASCII bytes.
                out.push_str(std::str::from_utf8(&self.bytes[start..self.at]).expect("utf-8 input"));
            }
            match self.peek() {
                Some(b'"') => {
                    self.at += 1;
                    if pending_high.is_some() {
                        out.push('\u{FFFD}');
                    }
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.at += 1;
                    let escape = self.peek().ok_or_else(|| self.error("unterminated escape"))?;
                    self.at += 1;
                    let simple = match escape {
                        b'"' => Some('"'),
                        b'\\' => Some('\\'),
                        b'/' => Some('/'),
                        b'b' => Some('\u{8}'),
                        b'f' => Some('\u{c}'),
                        b'n' => Some('\n'),
                        b'r' => Some('\r'),
                        b't' => Some('\t'),
                        b'u' => None,
                        _ => return Err(self.error("unknown escape")),
                    };
                    if let Some(c) = simple {
                        if pending_high.take().is_some() {
                            out.push('\u{FFFD}');
                        }
                        out.push(c);
                        continue;
                    }
                    let unit = self.hex4()?;
                    match (pending_high.take(), unit) {
                        (Some(high), 0xDC00..=0xDFFF) => {
                            let code = 0x10000 + ((u32::from(high) - 0xD800) << 10) + (u32::from(unit) - 0xDC00);
                            out.push(char::from_u32(code).expect("valid pair"));
                        }
                        (previous, 0xD800..=0xDBFF) => {
                            if previous.is_some() {
                                out.push('\u{FFFD}');
                            }
                            pending_high = Some(unit);
                        }
                        (previous, _) => {
                            if previous.is_some() {
                                out.push('\u{FFFD}');
                            }
                            out.push(char::from_u32(u32::from(unit)).unwrap_or('\u{FFFD}'));
                        }
                    }
                }
                Some(_) => return Err(self.error("control character in string")),
                None => return Err(self.error("unterminated string")),
            }
        }
    }

    fn hex4(&mut self) -> Result<u16, ParseError> {
        let digits = self
            .bytes
            .get(self.at..self.at + 4)
            .ok_or_else(|| self.error("short \\u escape"))?;
        let text = std::str::from_utf8(digits).map_err(|_| self.error("bad \\u escape"))?;
        if !text.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(self.error("bad \\u escape"));
        }
        self.at += 4;
        Ok(u16::from_str_radix(text, 16).expect("hex digits"))
    }

    fn number(&mut self) -> Result<Value, ParseError> {
        let start = self.at;
        if self.peek() == Some(b'-') {
            self.at += 1;
        }
        match self.peek() {
            Some(b'0') => self.at += 1,
            Some(b'1'..=b'9') => self.digits(),
            _ => return Err(self.error("expected a digit")),
        }
        if self.peek() == Some(b'.') {
            self.at += 1;
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(self.error("expected a digit after '.'"));
            }
            self.digits();
        }
        if let Some(b'e' | b'E') = self.peek() {
            self.at += 1;
            if let Some(b'+' | b'-') = self.peek() {
                self.at += 1;
            }
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(self.error("expected an exponent digit"));
            }
            self.digits();
        }
        let text = std::str::from_utf8(&self.bytes[start..self.at]).expect("ascii");
        text.parse::<f64>()
            .map(Value::Number)
            .map_err(|_| self.error("bad number"))
    }

    fn digits(&mut self) {
        while let Some(b'0'..=b'9') = self.peek() {
            self.at += 1;
        }
    }
}

/// An array index key: the canonical decimal form of an integer below 2^32 − 1.
fn array_index(key: &str) -> Option<u32> {
    let n: u32 = key.parse().ok()?;
    (n != u32::MAX && n.to_string() == key).then_some(n)
}

/// JavaScript's own-property order: array-index keys ascending, then the
/// others in insertion order.
fn js_property_order(members: Vec<(String, Value)>) -> Vec<(String, Value)> {
    if !members.iter().any(|(k, _)| array_index(k).is_some()) {
        return members;
    }
    let (mut indexed, named): (Vec<_>, Vec<_>) = members.into_iter().partition(|(k, _)| array_index(k).is_some());
    indexed.sort_by_key(|(k, _)| array_index(k));
    indexed.extend(named);
    indexed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_like_json_parse() {
        let v = parse(r#" {"b":1,"2":true,"a":[null,"x\u00e9\ud83d\ude00"],"1":-0.5e1,"b":2} "#).unwrap();
        let Value::Object(members) = &v else { panic!() };
        let keys: Vec<&str> = members.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["1", "2", "b", "a"]);
        assert_eq!(v.get("b"), Some(&Value::Number(2.0)));
        assert_eq!(v.get("1"), Some(&Value::Number(-5.0)));
        assert_eq!(v.get("a").unwrap().as_array().unwrap()[1].as_str(), Some("xé😀"));
    }

    #[test]
    fn refuses_what_json_parse_refuses() {
        for bad in [
            "",
            "01",
            "1.",
            "+1",
            "[1,]",
            "{\"a\" 1}",
            "'a'",
            "\"\t\"",
            "NaN",
            "[1] x",
            "\"\\x\"",
        ] {
            assert!(parse(bad).is_err(), "{bad:?} should fail");
        }
    }
}
