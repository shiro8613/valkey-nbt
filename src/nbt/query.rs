use crate::error::{NbtError, Result};
use crate::nbt::path::{resolve_index, NbtPath, PathSegment};
use crate::nbt::tag::Tag;

pub fn get<'a>(root: &'a Tag, path: &NbtPath) -> Result<&'a Tag> {
    let mut cur = root;
    for seg in &path.segments {
        cur = match (seg, cur) {
            (PathSegment::Key(k), Tag::Compound(m)) => m
                .get(k)
                .ok_or_else(|| NbtError::PathNotFound(format!("key '{}' not found", k)))?,
            (PathSegment::Key(k), other) => {
                return Err(NbtError::TypeMismatch(format!(
                    "cannot read key '{}' of {}",
                    k,
                    other.type_name()
                )))
            }
            (PathSegment::Index(i), Tag::List(l)) => resolve_index(*i, l.len())
                .and_then(|idx| l.get(idx))
                .ok_or_else(|| {
                    NbtError::PathNotFound(format!("index {} out of range (len {})", i, l.len()))
                })?,
            (PathSegment::Index(i), other) => {
                return Err(NbtError::TypeMismatch(format!(
                    "cannot read index {} of {}",
                    i,
                    other.type_name()
                )))
            }
        };
    }
    Ok(cur)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nbt::parser::parse_snbt;

    #[test]
    fn resolves_paths() {
        let t = parse_snbt("{Inventory:[{id:\"a\",Count:1b},{id:\"b\",Count:2b}],Health:20.0f}")
            .unwrap();
        let p = |s: &str| NbtPath::parse(s).unwrap();
        assert_eq!(get(&t, &p("Inventory[1].Count")).unwrap(), &Tag::Byte(2));
        assert_eq!(get(&t, &NbtPath::root()).unwrap(), &t);
        assert!(matches!(
            get(&t, &p("Nope")),
            Err(NbtError::PathNotFound(_))
        ));
        assert!(matches!(
            get(&t, &p("Inventory[5]")),
            Err(NbtError::PathNotFound(_))
        ));
        assert_eq!(get(&t, &p("Inventory[-1].Count")).unwrap(), &Tag::Byte(2));
        assert_eq!(
            get(&t, &p("Inventory[-2].id")).unwrap(),
            &Tag::String("a".to_string())
        );
        assert!(matches!(
            get(&t, &p("Inventory[-3]")),
            Err(NbtError::PathNotFound(_))
        ));
        assert!(matches!(
            get(&t, &p("Health.x")),
            Err(NbtError::TypeMismatch(_))
        ));
        assert!(matches!(
            get(&t, &p("Health[0]")),
            Err(NbtError::TypeMismatch(_))
        ));
    }
}
