use std::time::Instant;

use crate::error::Result;
use crate::nbt::mutate::WriteOp;
use crate::nbt::path::NbtPath;
use crate::storage::KeyStore;
use crate::transaction::manager::TransactionManager;

pub fn execute(
    store: &mut dyn KeyStore,
    txm: &mut TransactionManager,
    key: &str,
    path: &str,
    now: Instant,
) -> Result<()> {
    let path = NbtPath::parse_allow_root(path)?;
    super::apply_direct(store, txm, key, &path, WriteOp::Remove, now)
}
