use std::collections::BTreeMap;
use std::fmt::Write;

use crate::error::{NbtError, Result};
use crate::nbt::tag::{check_list_compat, Tag, MAX_DEPTH};

pub fn parse_snbt(input: &str) -> Result<Tag> {
    let mut p = Parser {
        chars: input.chars().collect(),
        pos: 0,
    };
    let tag = p.parse_value(0)?;
    p.skip_ws();
    if p.pos != p.chars.len() {
        return Err(p.err("trailing characters"));
    }
    tag.validate()?;
    Ok(tag)
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
}

fn is_unquoted_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '+')
}

impl Parser {
    fn err(&self, msg: &str) -> NbtError {
        NbtError::InvalidSyntax(format!("invalid SNBT at position {}: {}", self.pos, msg))
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn next(&mut self) -> Option<char> {
        let c = self.chars.get(self.pos).copied();
        if c.is_some() {
            self.pos += 1;
        }
        c
    }

    fn skip_ws(&mut self) {
        while let Some(c) = self.peek() {
            if c.is_whitespace() {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn expect(&mut self, want: char) -> Result<()> {
        match self.next() {
            Some(c) if c == want => Ok(()),
            _ => Err(self.err(&format!("expected '{}'", want))),
        }
    }

    fn parse_value(&mut self, depth: usize) -> Result<Tag> {
        if depth > MAX_DEPTH {
            return Err(self.err("nesting too deep"));
        }
        self.skip_ws();
        match self.peek() {
            None => Err(self.err("unexpected end of input")),
            Some('{') => self.parse_compound(depth),
            Some('[') => self.parse_list(depth),
            Some('"') | Some('\'') => Ok(Tag::String(self.parse_quoted()?)),
            Some(_) => {
                let tok = self.parse_unquoted()?;
                Ok(resolve_token(&tok))
            }
        }
    }

    fn parse_quoted(&mut self) -> Result<String> {
        let quote = self.next().ok_or_else(|| self.err("expected quote"))?;
        let mut out = String::new();
        loop {
            let c = self.next().ok_or_else(|| self.err("unterminated string"))?;
            if c == quote {
                return Ok(out);
            } else if c == '\\' {
                let n = self.next().ok_or_else(|| self.err("unterminated escape"))?;
                if n == quote || n == '\\' {
                    out.push(n);
                } else {
                    return Err(self.err("invalid escape sequence"));
                }
            } else {
                out.push(c);
            }
        }
    }

    fn parse_unquoted(&mut self) -> Result<String> {
        let start = self.pos;
        while let Some(c) = self.peek() {
            if is_unquoted_char(c) {
                self.pos += 1;
            } else {
                break;
            }
        }
        if self.pos == start {
            return Err(self.err("unexpected character"));
        }
        Ok(self.chars[start..self.pos].iter().collect())
    }

    fn parse_key(&mut self) -> Result<String> {
        self.skip_ws();
        match self.peek() {
            Some('"') | Some('\'') => self.parse_quoted(),
            _ => self.parse_unquoted(),
        }
    }

    fn parse_compound(&mut self, depth: usize) -> Result<Tag> {
        self.expect('{')?;
        let mut map = BTreeMap::new();
        self.skip_ws();
        if self.peek() == Some('}') {
            self.pos += 1;
            return Ok(Tag::Compound(map));
        }
        loop {
            let key = self.parse_key()?;
            self.skip_ws();
            self.expect(':')?;
            let value = self.parse_value(depth + 1)?;
            map.insert(key, value);
            self.skip_ws();
            match self.next() {
                Some(',') => {}
                Some('}') => break,
                _ => return Err(self.err("expected ',' or '}'")),
            }
        }
        Ok(Tag::Compound(map))
    }

    fn parse_list(&mut self, depth: usize) -> Result<Tag> {
        self.expect('[')?;
        if let (Some(&t), Some(&';')) = (self.chars.get(self.pos), self.chars.get(self.pos + 1)) {
            if matches!(t, 'B' | 'I' | 'L') {
                self.pos += 2;
                return self.parse_typed_array(t, depth);
            }
        }
        self.skip_ws();
        let mut items: Vec<Tag> = Vec::new();
        if self.peek() == Some(']') {
            self.pos += 1;
            return Ok(Tag::List(items));
        }
        loop {
            let v = self.parse_value(depth + 1)?;
            check_list_compat(&items, &v)?;
            items.push(v);
            self.skip_ws();
            match self.next() {
                Some(',') => {}
                Some(']') => break,
                _ => return Err(self.err("expected ',' or ']'")),
            }
        }
        Ok(Tag::List(items))
    }

    fn parse_typed_array(&mut self, kind: char, depth: usize) -> Result<Tag> {
        let mut items: Vec<Tag> = Vec::new();
        self.skip_ws();
        if self.peek() == Some(']') {
            self.pos += 1;
        } else {
            loop {
                let v = self.parse_value(depth + 1)?;
                items.push(v);
                self.skip_ws();
                match self.next() {
                    Some(',') => {}
                    Some(']') => break,
                    _ => return Err(self.err("expected ',' or ']'")),
                }
            }
        }
        match kind {
            'B' => {
                let mut out = Vec::with_capacity(items.len());
                for t in items {
                    match t {
                        Tag::Byte(b) => out.push(b),
                        _ => return Err(self.err("byte array requires Byte elements")),
                    }
                }
                Ok(Tag::ByteArray(out))
            }
            'I' => {
                let mut out = Vec::with_capacity(items.len());
                for t in items {
                    match t {
                        Tag::Int(v) => out.push(v),
                        _ => return Err(self.err("int array requires Int elements")),
                    }
                }
                Ok(Tag::IntArray(out))
            }
            _ => {
                let mut out = Vec::with_capacity(items.len());
                for t in items {
                    match t {
                        Tag::Long(v) => out.push(v),
                        _ => return Err(self.err("long array requires Long elements")),
                    }
                }
                Ok(Tag::LongArray(out))
            }
        }
    }
}

fn is_int(s: &str) -> bool {
    let digits = s
        .strip_prefix('+')
        .or_else(|| s.strip_prefix('-'))
        .unwrap_or(s);
    !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit())
}

fn is_float(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-'))
        && s.chars()
            .next()
            .is_some_and(|c| c.is_ascii_digit() || matches!(c, '+' | '-' | '.'))
        && s.chars().any(|c| c.is_ascii_digit())
        && s.parse::<f64>().is_ok()
}

fn resolve_token(tok: &str) -> Tag {
    if tok.eq_ignore_ascii_case("true") {
        return Tag::Byte(1);
    }
    if tok.eq_ignore_ascii_case("false") {
        return Tag::Byte(0);
    }
    // Unquoted tokens are ASCII only, so byte slicing is safe.
    let last = tok.chars().last().unwrap_or(' ');
    let body = &tok[..tok.len() - 1];
    match last {
        'b' | 'B' if is_int(body) => {
            if let Ok(v) = body.parse::<i8>() {
                return Tag::Byte(v);
            }
        }
        's' | 'S' if is_int(body) => {
            if let Ok(v) = body.parse::<i16>() {
                return Tag::Short(v);
            }
        }
        'l' | 'L' if is_int(body) => {
            if let Ok(v) = body.parse::<i64>() {
                return Tag::Long(v);
            }
        }
        'f' | 'F' if is_float(body) => {
            if let Ok(v) = body.parse::<f32>() {
                return Tag::Float(v);
            }
        }
        'd' | 'D' if is_float(body) => {
            if let Ok(v) = body.parse::<f64>() {
                return Tag::Double(v);
            }
        }
        _ => {}
    }
    if is_int(tok) {
        if let Ok(v) = tok.parse::<i32>() {
            return Tag::Int(v);
        }
    } else if is_float(tok) {
        if let Ok(v) = tok.parse::<f64>() {
            return Tag::Double(v);
        }
    }
    Tag::String(tok.to_string())
}

pub fn to_snbt(tag: &Tag) -> String {
    let mut out = String::new();
    write_tag(tag, &mut out);
    out
}

fn fmt_float(s: String) -> String {
    if s.contains('.')
        || s.contains('e')
        || s.contains('E')
        || s.contains("inf")
        || s.contains("NaN")
    {
        s
    } else {
        format!("{}.0", s)
    }
}

fn write_quoted(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        if c == '"' || c == '\\' {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
}

fn write_key(k: &str, out: &mut String) {
    if !k.is_empty() && k.chars().all(is_unquoted_char) {
        out.push_str(k);
    } else {
        write_quoted(k, out);
    }
}

fn write_tag(tag: &Tag, out: &mut String) {
    match tag {
        Tag::End => {}
        Tag::Byte(v) => {
            let _ = write!(out, "{}b", v);
        }
        Tag::Short(v) => {
            let _ = write!(out, "{}s", v);
        }
        Tag::Int(v) => {
            let _ = write!(out, "{}", v);
        }
        Tag::Long(v) => {
            let _ = write!(out, "{}L", v);
        }
        Tag::Float(v) => {
            out.push_str(&fmt_float(v.to_string()));
            out.push('f');
        }
        Tag::Double(v) => {
            out.push_str(&fmt_float(v.to_string()));
            out.push('d');
        }
        Tag::String(s) => write_quoted(s, out),
        Tag::ByteArray(v) => {
            out.push_str("[B;");
            for (i, x) in v.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                let _ = write!(out, "{}b", x);
            }
            out.push(']');
        }
        Tag::IntArray(v) => {
            out.push_str("[I;");
            for (i, x) in v.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                let _ = write!(out, "{}", x);
            }
            out.push(']');
        }
        Tag::LongArray(v) => {
            out.push_str("[L;");
            for (i, x) in v.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                let _ = write!(out, "{}L", x);
            }
            out.push(']');
        }
        Tag::List(items) => {
            out.push('[');
            for (i, t) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_tag(t, out);
            }
            out.push(']');
        }
        Tag::Compound(map) => {
            out.push('{');
            for (i, (k, v)) in map.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_key(k, out);
                out.push(':');
                write_tag(v, out);
            }
            out.push('}');
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_spec_example() {
        let t = parse_snbt("{Health:20.0f,Food:20}").unwrap();
        match &t {
            Tag::Compound(m) => {
                assert_eq!(m.get("Health"), Some(&Tag::Float(20.0)));
                assert_eq!(m.get("Food"), Some(&Tag::Int(20)));
            }
            _ => panic!("expected compound"),
        }
        assert_eq!(to_snbt(&t), "{Food:20,Health:20.0f}");
    }

    #[test]
    fn numeric_suffixes() {
        assert_eq!(parse_snbt("1b").unwrap(), Tag::Byte(1));
        assert_eq!(parse_snbt("2S").unwrap(), Tag::Short(2));
        assert_eq!(parse_snbt("3").unwrap(), Tag::Int(3));
        assert_eq!(parse_snbt("4L").unwrap(), Tag::Long(4));
        assert_eq!(parse_snbt("1.5").unwrap(), Tag::Double(1.5));
        assert_eq!(parse_snbt("1.5d").unwrap(), Tag::Double(1.5));
        assert_eq!(parse_snbt("true").unwrap(), Tag::Byte(1));
        assert_eq!(parse_snbt("abc").unwrap(), Tag::String("abc".to_string()));
        assert_eq!(parse_snbt("300b").unwrap(), Tag::String("300b".to_string()));
    }

    #[test]
    fn arrays_and_lists() {
        assert_eq!(parse_snbt("[B;1b,2b]").unwrap(), Tag::ByteArray(vec![1, 2]));
        assert_eq!(parse_snbt("[I;1,2]").unwrap(), Tag::IntArray(vec![1, 2]));
        assert_eq!(parse_snbt("[L;1L]").unwrap(), Tag::LongArray(vec![1]));
        assert!(parse_snbt("[I;1b]").is_err());
        assert!(parse_snbt("[1,2L]").is_err());
        assert_eq!(parse_snbt("[]").unwrap(), Tag::List(vec![]));
    }

    #[test]
    fn rejects_garbage() {
        for bad in ["{", "{a:}", "{a:1,}", "[1,", "{a:1} x", "\"abc", "{:1}"] {
            assert!(parse_snbt(bad).is_err(), "should reject {:?}", bad);
        }
    }

    #[test]
    fn roundtrip() {
        let src = "{a:[1,2,3],b:{c:\"x\\\"y\",d:1.5f},e:[B;1b],f:[L;5L],\"k k\":2s}";
        let t = parse_snbt(src).unwrap();
        let again = parse_snbt(&to_snbt(&t)).unwrap();
        assert_eq!(t, again);
    }
}
