//! Strict JSON parser: RFC 8259 grammar plus the journal's restrictions (spec §4).

use crate::{Int, Key, MAX_DEPTH, Object, ParseError, ParseErrorKind as Kind, Value};

pub(crate) fn parse(input: &[u8]) -> Result<Value, ParseError> {
    let mut p = Parser {
        input,
        rest: input,
        depth: 0,
    };
    p.skip_ws();
    let value = p.value()?;
    p.skip_ws();
    if !p.rest.is_empty() {
        return Err(p.error(Kind::TrailingData));
    }
    Ok(value)
}

struct Parser<'a> {
    input: &'a [u8],
    rest: &'a [u8],
    depth: usize,
}

impl<'a> Parser<'a> {
    fn offset(&self) -> usize {
        self.input.len().saturating_sub(self.rest.len())
    }

    fn error(&self, kind: Kind) -> ParseError {
        at(kind, self.offset())
    }

    fn peek(&self) -> Option<u8> {
        self.rest.first().copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let (b, rest) = self.rest.split_first()?;
        self.rest = rest;
        Some(*b)
    }

    fn eat(&mut self, b: u8) -> bool {
        let found = self.peek() == Some(b);
        if found {
            self.bump();
        }
        found
    }

    fn expect(&mut self, b: u8) -> Result<(), ParseError> {
        if self.eat(b) {
            Ok(())
        } else {
            Err(self.error(Kind::Syntax))
        }
    }

    fn skip_ws(&mut self) {
        let n = self
            .rest
            .iter()
            .take_while(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r'))
            .count();
        self.rest = self.rest.get(n..).unwrap_or_default();
    }

    fn literal(&mut self, word: &[u8], value: Value) -> Result<Value, ParseError> {
        match self.rest.strip_prefix(word) {
            Some(rest) => {
                self.rest = rest;
                Ok(value)
            }
            None => Err(self.error(Kind::Syntax)),
        }
    }

    fn digits(&mut self) -> &'a [u8] {
        let n = self.rest.iter().take_while(|b| b.is_ascii_digit()).count();
        let (digits, rest) = self.rest.split_at_checked(n).unwrap_or((&[], self.rest));
        self.rest = rest;
        digits
    }

    fn value(&mut self) -> Result<Value, ParseError> {
        match self.peek() {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => self.string().map(Value::Str),
            Some(b't') => self.literal(b"true", Value::Bool(true)),
            Some(b'f') => self.literal(b"false", Value::Bool(false)),
            Some(b'n') => self.literal(b"null", Value::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err(self.error(Kind::Syntax)),
        }
    }

    fn enter(&mut self) -> Result<(), ParseError> {
        self.depth = self
            .depth
            .checked_add(1)
            .filter(|d| *d <= MAX_DEPTH)
            .ok_or_else(|| self.error(Kind::TooDeep))?;
        self.bump();
        self.skip_ws();
        Ok(())
    }

    fn array(&mut self) -> Result<Value, ParseError> {
        self.enter()?;
        let mut items = Vec::new();
        if !self.eat(b']') {
            loop {
                self.skip_ws();
                items.push(self.value()?);
                self.skip_ws();
                if !self.eat(b',') {
                    self.expect(b']')?;
                    break;
                }
            }
        }
        self.depth = self.depth.saturating_sub(1);
        Ok(Value::Array(items))
    }

    fn object(&mut self) -> Result<Value, ParseError> {
        self.enter()?;
        let mut members = Object::new();
        if !self.eat(b'}') {
            loop {
                self.skip_ws();
                let key_at = self.offset();
                if self.peek() != Some(b'"') {
                    return Err(self.error(Kind::Syntax));
                }
                let name = self.string()?;
                let key = Key::new(&name).map_err(|_| at(Kind::InvalidKey, key_at))?;
                if members.contains_key(&key) {
                    return Err(at(Kind::DuplicateKey, key_at));
                }
                self.skip_ws();
                self.expect(b':')?;
                self.skip_ws();
                let value = self.value()?;
                members.insert(key, value);
                self.skip_ws();
                if !self.eat(b',') {
                    self.expect(b'}')?;
                    break;
                }
            }
        }
        self.depth = self.depth.saturating_sub(1);
        Ok(Value::Object(members))
    }

    fn number(&mut self) -> Result<Value, ParseError> {
        let start = self.offset();
        let negative = self.eat(b'-');
        let int = self.digits();
        if int.is_empty() {
            return Err(self.error(Kind::Syntax));
        }
        let mut float = false;
        if self.eat(b'.') {
            if self.digits().is_empty() {
                return Err(self.error(Kind::Syntax));
            }
            float = true;
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.bump();
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.bump();
            }
            if self.digits().is_empty() {
                return Err(self.error(Kind::Syntax));
            }
            float = true;
        }
        if float {
            return Err(at(Kind::Float, start));
        }
        if int.len() > 1 && int.first() == Some(&b'0') {
            return Err(at(Kind::Syntax, start));
        }
        if negative {
            return Err(at(Kind::IntegerRange, start));
        }
        int.iter()
            .try_fold(0u64, |n, d| {
                n.checked_mul(10)?
                    .checked_add(u64::from(d.checked_sub(b'0')?))
            })
            .and_then(Int::new)
            .map(Value::Int)
            .ok_or(at(Kind::IntegerRange, start))
    }

    fn string(&mut self) -> Result<String, ParseError> {
        let start = self.offset();
        self.bump();
        let mut buf = Vec::new();
        loop {
            let here = self.offset();
            match self.bump() {
                None => return Err(self.error(Kind::Syntax)),
                Some(b'"') => break,
                Some(b'\\') => {
                    let c = self.escape(here)?;
                    buf.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes());
                }
                Some(0x00..=0x1f) => return Err(at(Kind::Syntax, here)),
                Some(b) => buf.push(b),
            }
        }
        String::from_utf8(buf).map_err(|_| at(Kind::InvalidUtf8, start))
    }

    fn escape(&mut self, here: usize) -> Result<char, ParseError> {
        match self.bump() {
            Some(b'"') => Ok('"'),
            Some(b'\\') => Ok('\\'),
            Some(b'/') => Ok('/'),
            Some(b'b') => Ok('\u{8}'),
            Some(b'f') => Ok('\u{c}'),
            Some(b'n') => Ok('\n'),
            Some(b'r') => Ok('\r'),
            Some(b't') => Ok('\t'),
            Some(b'u') => self.unicode_escape(here),
            _ => Err(at(Kind::Syntax, here)),
        }
    }

    fn hex4(&mut self, here: usize) -> Result<u32, ParseError> {
        (0..4).try_fold(0u32, |v, _| {
            let digit = self
                .bump()
                .and_then(|b| char::from(b).to_digit(16))
                .ok_or(at(Kind::Syntax, here))?;
            v.checked_mul(16)
                .and_then(|v| v.checked_add(digit))
                .ok_or(at(Kind::Syntax, here))
        })
    }

    fn unicode_escape(&mut self, here: usize) -> Result<char, ParseError> {
        let lone = at(Kind::LoneSurrogate, here);
        let unit = self.hex4(here)?;
        let code = match unit {
            0xD800..=0xDBFF => {
                let Some(rest) = self.rest.strip_prefix(b"\\u") else {
                    return Err(lone);
                };
                self.rest = rest;
                let low = self.hex4(here)?;
                if !(0xDC00..=0xDFFF).contains(&low) {
                    return Err(lone);
                }
                unit.checked_sub(0xD800)
                    .and_then(|h| h.checked_mul(0x400))
                    .and_then(|h| h.checked_add(low.checked_sub(0xDC00)?))
                    .and_then(|c| c.checked_add(0x1_0000))
                    .ok_or(lone)?
            }
            _ => unit,
        };
        // A lone low surrogate (U+DC00–U+DFFF) is not a `char`.
        char::from_u32(code).ok_or(lone)
    }
}

fn at(kind: Kind, offset: usize) -> ParseError {
    ParseError { kind, offset }
}
