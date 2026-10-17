use std::time::Instant;

use crate::error::Result;
use crate::nbt::mutate::WriteOp;
use crate::nbt::parser::parse_snbt;
use crate::nbt::path::NbtPath;
use crate::storage::KeyStore;
use crate::transaction::manager::TransactionManager;

pub fn execute(
    store: &mut dyn KeyStore,
    txm: &mut TransactionManager,
    key: &str,
    path: &str,
    value: &str,
    now: Instant,
) -> Result<()> {
    let path = NbtPath::parse_allow_root(path)?;
    let value = parse_snbt(value)?;
    super::apply_direct(store, txm, key, &path, WriteOp::Merge(value), now)
}
