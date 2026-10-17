use std::fmt;

use crate::error::{NbtError, Result};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PathSegment {
    Key(String),
    Index(i64),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct NbtPath {
    pub segments: Vec<PathSegment>,
}

pub fn resolve_index(index: i64, len: usize) -> Option<usize> {
    if index >= 0 {
        usize::try_from(index).ok().filter(|&i| i < len)
    } else {
        usize::try_from(index.unsigned_abs())
            .ok()
            .filter(|&back| back <= len)
            .map(|back| len - back)
    }
}

fn segments_may_alias(a: &PathSegment, b: &PathSegment) -> bool {
    match (a, b) {
        (PathSegment::Index(x), PathSegment::Index(y)) => x == y || ((*x < 0) != (*y < 0)),
        _ => a == b,
    }
}

fn is_simple_key_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '+')
}

impl NbtPath {
    pub fn root() -> Self {
        NbtPath {
            segments: Vec::new(),
        }
    }

    pub fn is_root(&self) -> bool {
        self.segments.is_empty()
    }

    pub fn parse(input: &str) -> Result<Self> {
        let chars: Vec<char> = input.chars().collect();
        let err = |m: &str| NbtError::InvalidSyntax(format!("invalid NBT path '{}': {}", input, m));
        let mut segments: Vec<PathSegment> = Vec::new();
        let mut pos = 0usize;
        let mut after_dot = false;

        while pos < chars.len() {
            let c = chars[pos];
            match c {
                '.' => {
                    if after_dot || segments.is_empty() {
                        return Err(err("unexpected '.'"));
                    }
                    after_dot = true;
                    pos += 1;
                }
                '[' => {
                    if after_dot {
                        return Err(err("unexpected '[' after '.'"));
                    }
                    pos += 1;
                    let negative = pos < chars.len() && chars[pos] == '-';
                    if negative {
                        pos += 1;
                    }
                    let start = pos;
                    while pos < chars.len() && chars[pos].is_ascii_digit() {
                        pos += 1;
                    }
                    if pos == start {
                        return Err(err("expected array index"));
                    }
                    if pos >= chars.len() || chars[pos] != ']' {
                        return Err(err("expected ']'"));
                    }
                    let digits: String = chars[start..pos].iter().collect();
                    let mut idx: i64 = digits.parse().map_err(|_| err("index out of range"))?;
                    if negative {
                        idx = -idx;
                    }
                    pos += 1;
                    segments.push(PathSegment::Index(idx));
                    after_dot = false;
                }
                '"' => {
                    if !after_dot && !segments.is_empty() {
                        return Err(err("missing '.' before key"));
                    }
                    pos += 1;
                    let mut key = String::new();
                    let mut closed = false;
                    while pos < chars.len() {
                        let ch = chars[pos];
                        pos += 1;
                        if ch == '"' {
                            closed = true;
                            break;
                        } else if ch == '\\' {
                            if pos >= chars.len() {
                                return Err(err("unterminated escape"));
                            }
                            let n = chars[pos];
                            pos += 1;
                            if n == '"' || n == '\\' {
                                key.push(n);
                            } else {
                                return Err(err("invalid escape sequence"));
                            }
                        } else {
                            key.push(ch);
                        }
                    }
                    if !closed {
                        return Err(err("unterminated quoted key"));
                    }
                    segments.push(PathSegment::Key(key));
                    after_dot = false;
                }
                c if is_simple_key_char(c) => {
                    if !after_dot && !segments.is_empty() {
                        return Err(err("missing '.' before key"));
                    }
                    let start = pos;
                    while pos < chars.len() && is_simple_key_char(chars[pos]) {
                        pos += 1;
                    }
                    let key: String = chars[start..pos].iter().collect();
                    segments.push(PathSegment::Key(key));
                    after_dot = false;
                }
                other => {
                    return Err(err(&format!("unexpected character '{}'", other)));
                }
            }
        }
        if after_dot {
            return Err(err("trailing '.'"));
        }
        if segments.is_empty() {
            return Err(err("empty path"));
        }
        Ok(NbtPath { segments })
    }

    pub fn parse_allow_root(input: &str) -> Result<Self> {
        if input == "." {
            Ok(NbtPath::root())
        } else {
            NbtPath::parse(input)
        }
    }

    pub fn last(&self) -> Option<&PathSegment> {
        self.segments.last()
    }

    pub fn parent(&self) -> Option<NbtPath> {
        if self.segments.is_empty() {
            None
        } else {
            Some(NbtPath {
                segments: self.segments[..self.segments.len() - 1].to_vec(),
            })
        }
    }

    pub fn split_last(&self) -> Option<(&PathSegment, NbtPath)> {
        let last = self.segments.last()?;
        Some((
            last,
            NbtPath {
                segments: self.segments[..self.segments.len() - 1].to_vec(),
            },
        ))
    }

    pub fn is_prefix_of(&self, other: &NbtPath) -> bool {
        other.segments.starts_with(&self.segments)
    }

    pub fn overlaps(&self, other: &NbtPath) -> bool {
        self.segments
            .iter()
            .zip(other.segments.iter())
            .all(|(a, b)| segments_may_alias(a, b))
    }
}

impl fmt::Display for NbtPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.segments.is_empty() {
            return write!(f, ".");
        }
        for (i, seg) in self.segments.iter().enumerate() {
            match seg {
                PathSegment::Key(k) => {
                    if i > 0 {
                        write!(f, ".")?;
                    }
                    if !k.is_empty() && k.chars().all(is_simple_key_char) {
                        write!(f, "{}", k)?;
                    } else {
                        write!(f, "\"")?;
                        for c in k.chars() {
                            if c == '"' || c == '\\' {
                                write!(f, "\\")?;
                            }
                            write!(f, "{}", c)?;
                        }
                        write!(f, "\"")?;
                    }
                }
                PathSegment::Index(n) => write!(f, "[{}]", n)?,
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> NbtPath {
        NbtPath::parse(s).unwrap()
    }

    #[test]
    fn parses_basic() {
        assert_eq!(
            p("Inventory[0].Count").segments,
            vec![
                PathSegment::Key("Inventory".to_string()),
                PathSegment::Index(0),
                PathSegment::Key("Count".to_string()),
            ]
        );
    }

    #[test]
    fn rejects_bad_syntax() {
        for bad in [
            "", "a..b", ".a", "a.", "a[", "a[]", "a[x]", "a[0]b", "a.[0]", "\"abc", "a[-]",
            "a[--1]", "a[+1]", "a[-1",
        ] {
            assert!(NbtPath::parse(bad).is_err(), "should reject {:?}", bad);
        }
    }

    #[test]
    fn display_roundtrip() {
        for s in [
            "Health",
            "Inventory[0].Count",
            "\"a b\".c[2]",
            "[3]",
            "Tags[-1]",
            "a[-2].b",
        ] {
            let path = p(s);
            assert_eq!(NbtPath::parse(&path.to_string()).unwrap(), path);
        }
        assert_eq!(NbtPath::parse_allow_root(".").unwrap(), NbtPath::root());
    }

    #[test]
    fn parses_negative_index() {
        assert_eq!(
            p("Tags[-1]").segments,
            vec![PathSegment::Key("Tags".to_string()), PathSegment::Index(-1)]
        );
    }

    #[test]
    fn resolves_indices() {
        assert_eq!(resolve_index(0, 3), Some(0));
        assert_eq!(resolve_index(2, 3), Some(2));
        assert_eq!(resolve_index(3, 3), None);
        assert_eq!(resolve_index(-1, 3), Some(2));
        assert_eq!(resolve_index(-3, 3), Some(0));
        assert_eq!(resolve_index(-4, 3), None);
        assert_eq!(resolve_index(-1, 0), None);
        assert_eq!(resolve_index(i64::MIN, 3), None);
    }

    #[test]
    fn negative_indices_overlap_conservatively() {
        assert!(p("l[-1]").overlaps(&p("l[2]")));
        assert!(p("l[-1]").overlaps(&p("l[-1]")));
        assert!(!p("l[-1]").overlaps(&p("l[-2]")));
        assert!(p("l[-1].x").overlaps(&p("l[0]")));
        assert!(p("l").overlaps(&p("l[-1]")));
        // coverage stays strict
        assert!(!p("l[2]").is_prefix_of(&p("l[-1]")));
    }

    #[test]
    fn overlap_rules() {
        assert!(!p("Health").overlaps(&p("Food")));
        assert!(!p("Inventory[0].Count").overlaps(&p("Inventory[0].id")));
        assert!(p("Inventory").overlaps(&p("Inventory[0].Count")));
        assert!(p("Inventory[0]").overlaps(&p("Inventory[0].Count")));
        assert!(p("Health").overlaps(&p("Health")));
        assert!(NbtPath::root().overlaps(&p("Health")));
    }
}
