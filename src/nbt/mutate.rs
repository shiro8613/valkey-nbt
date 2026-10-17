use std::collections::BTreeMap;

use crate::error::{NbtError, Result};
use crate::nbt::path::{resolve_index, NbtPath, PathSegment};
use crate::nbt::tag::{check_list_compat, Tag};

#[derive(Debug, Clone, PartialEq)]
pub enum WriteOp {
    Set(Tag),
    Remove,
    Merge(Tag),
    Add(Tag),
}

pub fn get_mut<'a>(root: &'a mut Tag, path: &NbtPath) -> Result<&'a mut Tag> {
    let mut cur = root;
    for seg in &path.segments {
        cur = step_mut(cur, seg)?;
    }
    Ok(cur)
}

fn step_mut<'a>(tag: &'a mut Tag, seg: &PathSegment) -> Result<&'a mut Tag> {
    match (seg, tag) {
        (PathSegment::Key(k), Tag::Compound(m)) => m
            .get_mut(k)
            .ok_or_else(|| NbtError::PathNotFound(format!("key '{}' not found", k))),
        (PathSegment::Key(k), other) => Err(NbtError::TypeMismatch(format!(
            "cannot use key '{}' on {}",
            k,
            other.type_name()
        ))),
        (PathSegment::Index(i), Tag::List(l)) => {
            let len = l.len();
            match resolve_index(*i, len) {
                Some(idx) => Ok(&mut l[idx]),
                None => Err(NbtError::PathNotFound(format!(
                    "index {} out of range (len {})",
                    i, len
                ))),
            }
        }
        (PathSegment::Index(i), other) => Err(NbtError::TypeMismatch(format!(
            "cannot use index {} on {}",
            i,
            other.type_name()
        ))),
    }
}

pub fn set(root: &mut Tag, path: &NbtPath, value: Tag) -> Result<()> {
    let (last, parent_path) = match path.split_last() {
        Some(x) => x,
        None => {
            if !matches!(value, Tag::Compound(_)) {
                return Err(NbtError::TypeMismatch(
                    "root must be a compound".to_string(),
                ));
            }
            *root = value;
            return Ok(());
        }
    };
    let parent = get_mut(root, &parent_path)?;
    match (last, parent) {
        (PathSegment::Key(k), Tag::Compound(m)) => {
            m.insert(k.clone(), value);
            Ok(())
        }
        (PathSegment::Key(k), other) => Err(NbtError::TypeMismatch(format!(
            "cannot use key '{}' on {}",
            k,
            other.type_name()
        ))),
        (PathSegment::Index(i), Tag::List(l)) => {
            let idx = resolve_index(*i, l.len()).ok_or_else(|| {
                NbtError::PathNotFound(format!("index {} out of range (len {})", i, l.len()))
            })?;
            let other_type = l
                .iter()
                .enumerate()
                .find(|&(j, _)| j != idx)
                .map(|(_, t)| t.type_id());
            if let Some(id) = other_type {
                if id != value.type_id() {
                    return Err(NbtError::TypeMismatch(format!(
                        "list element must be of the list's type, got {}",
                        value.type_name()
                    )));
                }
            }
            l[idx] = value;
            Ok(())
        }
        (PathSegment::Index(i), other) => Err(NbtError::TypeMismatch(format!(
            "cannot use index {} on {}",
            i,
            other.type_name()
        ))),
    }
}

pub fn remove(root: &mut Tag, path: &NbtPath) -> Result<()> {
    let (last, parent_path) = path
        .split_last()
        .ok_or_else(|| NbtError::NbtInvalid("the root cannot be removed".to_string()))?;
    let parent = get_mut(root, &parent_path)?;
    match (last, parent) {
        (PathSegment::Key(k), Tag::Compound(m)) => m
            .remove(k)
            .map(|_| ())
            .ok_or_else(|| NbtError::PathNotFound(format!("key '{}' not found", k))),
        (PathSegment::Key(k), other) => Err(NbtError::TypeMismatch(format!(
            "cannot use key '{}' on {}",
            k,
            other.type_name()
        ))),
        (PathSegment::Index(i), Tag::List(l)) => {
            let idx = resolve_index(*i, l.len()).ok_or_else(|| {
                NbtError::PathNotFound(format!("index {} out of range (len {})", i, l.len()))
            })?;
            l.remove(idx);
            Ok(())
        }
        (PathSegment::Index(i), other) => Err(NbtError::TypeMismatch(format!(
            "cannot use index {} on {}",
            i,
            other.type_name()
        ))),
    }
}

pub fn merge(root: &mut Tag, path: &NbtPath, value: Tag) -> Result<()> {
    let src = match value {
        Tag::Compound(m) => m,
        other => {
            return Err(NbtError::TypeMismatch(format!(
                "merge requires a compound, got {}",
                other.type_name()
            )))
        }
    };
    match get_mut(root, path)? {
        Tag::Compound(dst) => {
            merge_maps(dst, src);
            Ok(())
        }
        other => Err(NbtError::TypeMismatch(format!(
            "merge target must be a compound, found {}",
            other.type_name()
        ))),
    }
}

fn merge_maps(dst: &mut BTreeMap<String, Tag>, src: BTreeMap<String, Tag>) {
    for (k, v) in src {
        let both_compound = matches!(
            (dst.get(&k), &v),
            (Some(Tag::Compound(_)), Tag::Compound(_))
        );
        if both_compound {
            if let (Some(Tag::Compound(d)), Tag::Compound(s)) = (dst.get_mut(&k), v) {
                merge_maps(d, s);
            }
        } else {
            dst.insert(k, v);
        }
    }
}

fn overflow() -> NbtError {
    NbtError::NbtInvalid("numeric overflow".to_string())
}

fn add_numeric(target: &mut Tag, delta: &Tag) -> Result<()> {
    match (target, delta) {
        (Tag::Byte(a), Tag::Byte(b)) => *a = a.checked_add(*b).ok_or_else(overflow)?,
        (Tag::Short(a), Tag::Short(b)) => *a = a.checked_add(*b).ok_or_else(overflow)?,
        (Tag::Int(a), Tag::Int(b)) => *a = a.checked_add(*b).ok_or_else(overflow)?,
        (Tag::Long(a), Tag::Long(b)) => *a = a.checked_add(*b).ok_or_else(overflow)?,
        (Tag::Float(a), Tag::Float(b)) => {
            let r = *a + *b;
            if !r.is_finite() {
                return Err(overflow());
            }
            *a = r;
        }
        (Tag::Double(a), Tag::Double(b)) => {
            let r = *a + *b;
            if !r.is_finite() {
                return Err(overflow());
            }
            *a = r;
        }
        (t, d) => {
            return Err(NbtError::TypeMismatch(format!(
                "cannot add {} to {}",
                d.type_name(),
                t.type_name()
            )))
        }
    }
    Ok(())
}

pub fn add(root: &mut Tag, path: &NbtPath, value: Tag) -> Result<()> {
    let target = get_mut(root, path)?;
    match target {
        Tag::List(items) => {
            check_list_compat(items, &value)?;
            items.push(value);
            Ok(())
        }
        Tag::ByteArray(v) => match value {
            Tag::Byte(b) => {
                v.push(b);
                Ok(())
            }
            other => Err(NbtError::TypeMismatch(format!(
                "cannot append {} to ByteArray",
                other.type_name()
            ))),
        },
        Tag::IntArray(v) => match value {
            Tag::Int(b) => {
                v.push(b);
                Ok(())
            }
            other => Err(NbtError::TypeMismatch(format!(
                "cannot append {} to IntArray",
                other.type_name()
            ))),
        },
        Tag::LongArray(v) => match value {
            Tag::Long(b) => {
                v.push(b);
                Ok(())
            }
            other => Err(NbtError::TypeMismatch(format!(
                "cannot append {} to LongArray",
                other.type_name()
            ))),
        },
        t if t.is_numeric() => add_numeric(t, &value),
        other => Err(NbtError::TypeMismatch(format!(
            "ADD is not supported on {}",
            other.type_name()
        ))),
    }
}

pub fn apply_op(root: &mut Tag, path: &NbtPath, op: &WriteOp) -> Result<()> {
    match op {
        WriteOp::Set(v) => set(root, path, v.clone()),
        WriteOp::Remove => remove(root, path),
        WriteOp::Merge(v) => merge(root, path, v.clone()),
        WriteOp::Add(v) => add(root, path, v.clone()),
    }
}

pub fn apply_ops<'a, I>(base: &Tag, ops: I) -> Result<Tag>
where
    I: IntoIterator<Item = (&'a NbtPath, &'a WriteOp)>,
{
    let mut candidate = base.clone();
    for (path, op) in ops {
        apply_op(&mut candidate, path, op)?;
    }
    candidate.validate()?;
    Ok(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nbt::parser::parse_snbt;
    use crate::nbt::query;

    fn p(s: &str) -> NbtPath {
        NbtPath::parse(s).unwrap()
    }
    fn t(s: &str) -> Tag {
        parse_snbt(s).unwrap()
    }

    #[test]
    fn set_creates_final_key_only() {
        let mut r = t("{a:{b:1}}");
        set(&mut r, &p("a.c"), Tag::Int(2)).unwrap();
        assert_eq!(query::get(&r, &p("a.c")).unwrap(), &Tag::Int(2));
        assert!(matches!(
            set(&mut r, &p("x.y"), Tag::Int(1)),
            Err(NbtError::PathNotFound(_))
        ));
    }

    #[test]
    fn set_list_element_type_checked() {
        let mut r = t("{l:[1,2,3]}");
        set(&mut r, &p("l[1]"), Tag::Int(9)).unwrap();
        assert!(matches!(
            set(&mut r, &p("l[1]"), Tag::Long(9)),
            Err(NbtError::TypeMismatch(_))
        ));
        assert!(matches!(
            set(&mut r, &p("l[7]"), Tag::Int(1)),
            Err(NbtError::PathNotFound(_))
        ));
    }

    #[test]
    fn negative_indices_for_set_remove_add() {
        let mut r = t("{l:[1,2,3]}");
        set(&mut r, &p("l[-1]"), Tag::Int(9)).unwrap();
        assert_eq!(r, t("{l:[1,2,9]}"));
        remove(&mut r, &p("l[-3]")).unwrap();
        assert_eq!(r, t("{l:[2,9]}"));
        assert!(matches!(
            set(&mut r, &p("l[-3]"), Tag::Int(1)),
            Err(NbtError::PathNotFound(_))
        ));
        assert!(matches!(
            remove(&mut r, &p("l[-3]")),
            Err(NbtError::PathNotFound(_))
        ));
        let mut n = t("{l:[{c:1},{c:2}]}");
        add(&mut n, &p("l[-1].c"), Tag::Int(10)).unwrap();
        assert_eq!(n, t("{l:[{c:1},{c:12}]}"));
    }

    #[test]
    fn remove_key_and_index() {
        let mut r = t("{a:1,l:[1,2,3]}");
        remove(&mut r, &p("a")).unwrap();
        remove(&mut r, &p("l[0]")).unwrap();
        assert_eq!(r, t("{l:[2,3]}"));
        assert!(remove(&mut r, &NbtPath::root()).is_err());
        assert!(matches!(
            remove(&mut r, &p("zzz")),
            Err(NbtError::PathNotFound(_))
        ));
    }

    #[test]
    fn merge_is_deep_and_replaces_lists() {
        let mut r = t("{a:{x:1,y:2},l:[1,2]}");
        merge(&mut r, &NbtPath::root(), t("{a:{y:3,z:4},l:[9]}")).unwrap();
        assert_eq!(r, t("{a:{x:1,y:3,z:4},l:[9]}"));
        assert!(merge(&mut r, &p("a"), Tag::Int(1)).is_err());
    }

    #[test]
    fn add_numeric_and_list() {
        let mut r = t("{n:1,f:1.5f,l:[1],big:127b}");
        add(&mut r, &p("n"), Tag::Int(4)).unwrap();
        add(&mut r, &p("f"), Tag::Float(0.5)).unwrap();
        add(&mut r, &p("l"), Tag::Int(2)).unwrap();
        assert_eq!(r, t("{n:5,f:2.0f,l:[1,2],big:127b}"));
        assert!(matches!(
            add(&mut r, &p("n"), Tag::Long(1)),
            Err(NbtError::TypeMismatch(_))
        ));
        assert!(matches!(
            add(&mut r, &p("l"), Tag::Long(1)),
            Err(NbtError::TypeMismatch(_))
        ));
        assert!(matches!(
            add(&mut r, &p("big"), Tag::Byte(1)),
            Err(NbtError::NbtInvalid(_))
        ));
    }

    #[test]
    fn apply_ops_is_atomic() {
        let base = t("{a:1}");
        let ok = (p("a"), WriteOp::Set(Tag::Int(2)));
        let bad = (p("missing.x"), WriteOp::Set(Tag::Int(3)));
        let res = apply_ops(&base, vec![(&ok.0, &ok.1), (&bad.0, &bad.1)]);
        assert!(res.is_err());
        assert_eq!(base, t("{a:1}"));
    }
}
