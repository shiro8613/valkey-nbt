use crate::error::{NbtError, Result};
use crate::nbt::parser::to_snbt;
use crate::nbt::path::NbtPath;
use crate::nbt::query;
use crate::storage::KeyStore;

pub fn execute(store: &dyn KeyStore, key: &str, path: Option<&str>) -> Result<String> {
    let path = match path {
        Some(p) => NbtPath::parse_allow_root(p)?,
        None => NbtPath::root(),
    };
    let root = store
        .get(key)?
        .ok_or_else(|| NbtError::KeyNotFound(key.to_string()))?;
    Ok(to_snbt(query::get(&root, &path)?))
}
