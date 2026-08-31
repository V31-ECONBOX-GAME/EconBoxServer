//! Minimal JSON support: a value type, a serializer and a parser.
//!
//! EconBoxServer speaks JSON on the wire but depends on no crates, so this
//! module implements exactly the subset of RFC 8259 the API needs.

use std::fmt;

/// Maximum nesting depth accepted by [`parse`]. Guards against stack overflow
/// on hostile input such as `[[[[[[...`.
const MAX_DEPTH: usize = 64;

static NULL: Json = Json::Null;

/// A JSON value.
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    /// Object fields keep insertion order, which keeps responses stable and
    /// diffable. Lookup is linear, which is fine for the small objects here.
    Obj(Vec<(String, Json)>),
}

impl Json {
    /// Number constructor that keeps the output valid: JSON has no `NaN` or
    /// `Infinity`, so non-finite values collapse to `0`.
    pub fn num(v: f64) -> Json {
        Json::Num(if v.is_finite() { v } else { 0.0 })
    }

    /// Number rounded to two decimals. Frames carry thousands of coordinates;
    /// trimming them keeps payloads small without any visible difference.
    pub fn num2(v: f64) -> Json {
        Json::num((v * 100.0).round() / 100.0)
    }

    pub fn str<S: Into<String>>(s: S) -> Json {
        Json::Str(s.into())
    }

    pub fn obj<I: IntoIterator<Item = (&'static str, Json)>>(fields: I) -> Json {
        Json::Obj(
            fields
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
        )
    }

    pub fn arr<I: IntoIterator<Item = Json>>(items: I) -> Json {
        Json::Arr(items.into_iter().collect())
    }

    /// Field lookup. Missing fields (and lookups on non-objects) read as
    /// `Null`, so callers can chain `get("a").get("b").as_f64()`.
    pub fn get(&self, key: &str) -> &Json {
        match self {
            Json::Obj(fields) => fields
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v)
                .unwrap_or(&NULL),
            _ => &NULL,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Json::Num(v) if v.is_finite() => Some(*v),
            _ => None,
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Json::Num(v) if v.is_finite() && *v >= 0.0 => Some(*v as u64),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Json::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Json::Null)
    }

    fn write(&self, out: &mut String) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(true) => out.push_str("true"),
            Json::Bool(false) => out.push_str("false"),
            Json::Num(v) => {
                if v.is_finite() {
                    // `{}` prints the shortest round-trippable form and never
                    // uses an exponent Rust-side that JSON would reject.
                    out.push_str(&format!("{v}"));
                } else {
                    out.push('0');
                }
            }
            Json::Str(s) => write_string(s, out),
            Json::Arr(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    item.write(out);
                }
                out.push(']');
            }
            Json::Obj(fields) => {
                out.push('{');
                for (i, (k, v)) in fields.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write_string(k, out);
                    out.push(':');
                    v.write(out);
                }
                out.push('}');
            }
        }
    }
}

impl fmt::Display for Json {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut out = String::new();
        self.write(&mut out);
        f.write_str(&out)
    }
}

fn write_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Parse a JSON document. Returns a human-readable message on failure; the
/// message is safe to hand back to the client.
pub fn parse(input: &str) -> Result<Json, String> {
    let mut p = Parser {
        bytes: input.as_bytes(),
        pos: 0,
    };
    p.skip_ws();
    let value = p.value(0)?;
    p.skip_ws();
    if p.pos != p.bytes.len() {
        return Err(format!("trailing input at byte {}", p.pos));
    }
    Ok(value)
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn skip_ws(&mut self) {
        while matches!(
            self.peek(),
            Some(b' ') | Some(b'\t') | Some(b'\n') | Some(b'\r')
        ) {
            self.pos += 1;
        }
    }

    fn expect(&mut self, byte: u8) -> Result<(), String> {
        if self.peek() == Some(byte) {
            self.pos += 1;
            Ok(())
        } else {
            Err(format!("expected '{}' at byte {}", byte as char, self.pos))
        }
    }

    fn literal(&mut self, word: &str, value: Json) -> Result<Json, String> {
        if self.bytes[self.pos..].starts_with(word.as_bytes()) {
            self.pos += word.len();
            Ok(value)
        } else {
            Err(format!("invalid literal at byte {}", self.pos))
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, String> {
        if depth > MAX_DEPTH {
            return Err("nesting too deep".to_string());
        }
        match self.peek() {
            None => Err("unexpected end of input".to_string()),
            Some(b'n') => self.literal("null", Json::Null),
            Some(b't') => self.literal("true", Json::Bool(true)),
            Some(b'f') => self.literal("false", Json::Bool(false)),
            Some(b'"') => Ok(Json::Str(self.string()?)),
            Some(b'[') => self.array(depth),
            Some(b'{') => self.object(depth),
            Some(_) => self.number(),
        }
    }

    fn array(&mut self, depth: usize) -> Result<Json, String> {
        self.expect(b'[')?;
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(Json::Arr(items));
        }
        loop {
            self.skip_ws();
            items.push(self.value(depth + 1)?);
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    return Ok(Json::Arr(items));
                }
                _ => return Err(format!("expected ',' or ']' at byte {}", self.pos)),
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<Json, String> {
        self.expect(b'{')?;
        let mut fields = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(Json::Obj(fields));
        }
        loop {
            self.skip_ws();
            let key = self.string()?;
            self.skip_ws();
            self.expect(b':')?;
            self.skip_ws();
            let value = self.value(depth + 1)?;
            fields.push((key, value));
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(Json::Obj(fields));
                }
                _ => return Err(format!("expected ',' or '}}' at byte {}", self.pos)),
            }
        }
    }

    fn string(&mut self) -> Result<String, String> {
        self.expect(b'"')?;
        let mut out = String::new();
        loop {
            let byte = self.peek().ok_or("unterminated string".to_string())?;
            self.pos += 1;
            match byte {
                b'"' => return Ok(out),
                b'\\' => {
                    let esc = self.peek().ok_or("unterminated escape".to_string())?;
                    self.pos += 1;
                    match esc {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{0008}'),
                        b'f' => out.push('\u{000C}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => out.push(self.unicode_escape()?),
                        _ => return Err(format!("invalid escape at byte {}", self.pos)),
                    }
                }
                _ => {
                    // Copy the whole UTF-8 sequence starting at this byte.
                    let start = self.pos - 1;
                    let len = utf8_len(byte);
                    let end = start + len;
                    if end > self.bytes.len() {
                        return Err("truncated UTF-8 sequence".to_string());
                    }
                    let chunk = std::str::from_utf8(&self.bytes[start..end])
                        .map_err(|_| "invalid UTF-8 in string".to_string())?;
                    out.push_str(chunk);
                    self.pos = end;
                }
            }
        }
    }

    /// Reads the four hex digits of a `\u` escape, joining surrogate pairs.
    fn unicode_escape(&mut self) -> Result<char, String> {
        let high = self.hex4()?;
        if (0xD800..0xDC00).contains(&high) {
            if self.peek() == Some(b'\\') {
                self.pos += 1;
                self.expect(b'u')?;
                let low = self.hex4()?;
                if (0xDC00..0xE000).contains(&low) {
                    let code = 0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00);
                    return char::from_u32(code).ok_or("invalid code point".to_string());
                }
            }
            return Err("unpaired surrogate".to_string());
        }
        char::from_u32(high).ok_or("invalid code point".to_string())
    }

    fn hex4(&mut self) -> Result<u32, String> {
        if self.pos + 4 > self.bytes.len() {
            return Err("truncated \\u escape".to_string());
        }
        let text = std::str::from_utf8(&self.bytes[self.pos..self.pos + 4])
            .map_err(|_| "invalid \\u escape".to_string())?;
        let value = u32::from_str_radix(text, 16).map_err(|_| "invalid \\u escape".to_string())?;
        self.pos += 4;
        Ok(value)
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        while matches!(
            self.peek(),
            Some(b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-')
        ) {
            self.pos += 1;
        }
        let text = std::str::from_utf8(&self.bytes[start..self.pos])
            .map_err(|_| "invalid number".to_string())?;
        text.parse::<f64>()
            .map(Json::Num)
            .map_err(|_| format!("invalid number at byte {start}"))
    }
}

fn utf8_len(first: u8) -> usize {
    match first {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        _ => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_scalars() {
        assert_eq!(Json::Null.to_string(), "null");
        assert_eq!(Json::Bool(true).to_string(), "true");
        assert_eq!(Json::num(1.5).to_string(), "1.5");
        assert_eq!(Json::num(f64::NAN).to_string(), "0");
        assert_eq!(Json::str("a\"b\n").to_string(), "\"a\\\"b\\n\"");
    }

    #[test]
    fn round_trips_documents() {
        let text = r#"{"a":[1,-2.5,true,null],"b":{"c":"é😀"}}"#;
        let value = parse(text).expect("parses");
        assert_eq!(value.get("a").as_f64(), None);
        assert_eq!(value.get("b").get("c").as_str(), Some("é😀"));
        assert_eq!(parse(&value.to_string()).unwrap(), value);
    }

    #[test]
    fn rejects_bad_input() {
        assert!(parse("{").is_err());
        assert!(parse("[1,]").is_err());
        assert!(parse("1 2").is_err());
        assert!(parse(&"[".repeat(200)).is_err());
    }

    #[test]
    fn missing_fields_read_as_null() {
        let value = parse(r#"{"x":1}"#).unwrap();
        assert!(value.get("nope").is_null());
        assert_eq!(value.get("x").as_u64(), Some(1));
    }
}
