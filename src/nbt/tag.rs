use std::collections::BTreeMap;

use crate::error::{NbtError, Result};

pub const MAX_DEPTH: usize = 512;

#[derive(Debug, Clone, PartialEq)]
pub enum Tag {
    End,
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    ByteArray(Vec<i8>),
    String(String),
    List(Vec<Tag>),
    Compound(BTreeMap<String, Tag>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}

impl Tag {
    pub fn empty_compound() -> Tag {
        Tag::Compound(BTreeMap::new())
    }

    pub fn type_id(&self) -> u8 {
        match self {
            Tag::End => 0,
            Tag::Byte(_) => 1,
            Tag::Short(_) => 2,
            Tag::Int(_) => 3,
            Tag::Long(_) => 4,
            Tag::Float(_) => 5,
            Tag::Double(_) => 6,
            Tag::ByteArray(_) => 7,
            Tag::String(_) => 8,
            Tag::List(_) => 9,
            Tag::Compound(_) => 10,
            Tag::IntArray(_) => 11,
            Tag::LongArray(_) => 12,
        }
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Tag::End => "End",
            Tag::Byte(_) => "Byte",
            Tag::Short(_) => "Short",
            Tag::Int(_) => "Int",
            Tag::Long(_) => "Long",
            Tag::Float(_) => "Float",
            Tag::Double(_) => "Double",
            Tag::ByteArray(_) => "ByteArray",
            Tag::String(_) => "String",
            Tag::List(_) => "List",
            Tag::Compound(_) => "Compound",
            Tag::IntArray(_) => "IntArray",
            Tag::LongArray(_) => "LongArray",
        }
    }

    pub fn is_numeric(&self) -> bool {
        matches!(
            self,
            Tag::Byte(_)
                | Tag::Short(_)
                | Tag::Int(_)
                | Tag::Long(_)
                | Tag::Float(_)
                | Tag::Double(_)
        )
    }

    pub fn validate(&self) -> Result<()> {
        self.validate_at(0)
    }

    fn validate_at(&self, depth: usize) -> Result<()> {
        if depth > MAX_DEPTH {
            return Err(NbtError::NbtInvalid(format!(
                "nesting deeper than {} levels",
                MAX_DEPTH
            )));
        }
        match self {
            Tag::End => Err(NbtError::NbtInvalid(
                "End tag cannot be used as a value".to_string(),
            )),
            Tag::Float(v) if !v.is_finite() => {
                Err(NbtError::NbtInvalid("non-finite Float".to_string()))
            }
            Tag::Double(v) if !v.is_finite() => {
                Err(NbtError::NbtInvalid("non-finite Double".to_string()))
            }
            Tag::String(s) => check_str_len(s),
            Tag::List(items) => {
                if let Some(first) = items.first() {
                    let id = first.type_id();
                    for item in items {
                        if item.type_id() != id {
                            return Err(NbtError::NbtInvalid(
                                "list elements must all have the same type".to_string(),
                            ));
                        }
                        item.validate_at(depth + 1)?;
                    }
                }
                Ok(())
            }
            Tag::Compound(map) => {
                for (k, v) in map {
                    check_str_len(k)?;
                    v.validate_at(depth + 1)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

fn check_str_len(s: &str) -> Result<()> {
    if s.len() > u16::MAX as usize {
        Err(NbtError::NbtInvalid(
            "string longer than 65535 bytes".to_string(),
        ))
    } else {
        Ok(())
    }
}

pub fn check_list_compat(list: &[Tag], value: &Tag) -> Result<()> {
    if let Some(first) = list.first() {
        if first.type_id() != value.type_id() {
            return Err(NbtError::TypeMismatch(format!(
                "list holds {}, cannot add {}",
                first.type_name(),
                value.type_name()
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_end_and_mixed_lists() {
        assert!(Tag::End.validate().is_err());
        let mixed = Tag::List(vec![Tag::Int(1), Tag::Long(2)]);
        assert!(mixed.validate().is_err());
        let ok = Tag::List(vec![Tag::Int(1), Tag::Int(2)]);
        assert!(ok.validate().is_ok());
    }

    #[test]
    fn rejects_non_finite() {
        assert!(Tag::Float(f32::NAN).validate().is_err());
        assert!(Tag::Double(f64::INFINITY).validate().is_err());
    }
}
