use crate::error::{NbtError, Result};
use crate::nbt::parser::parse_snbt;
use crate::nbt::tag::Tag;
use crate::storage::KeyStore;

pub fn execute(store: &mut dyn KeyStore, key: &str, snbt: Option<&str>) -> Result<()> {
    if store.exists(key)? {
        return Err(NbtError::KeyExists(key.to_string()));
    }
    let tag = match snbt {
        Some(s) => parse_snbt(s)?,
        None => Tag::empty_compound(),
    };
    if !matches!(tag, Tag::Compound(_)) {
        return Err(NbtError::TypeMismatch(
            "the root of an NBT value must be a compound".to_string(),
        ));
    }
    store.put(key, tag)
}
