pub mod add;
pub mod create;
pub mod get;
pub mod merge;
pub mod remove;
pub mod set;
pub mod transaction;

use std::time::Instant;

use crate::error::{NbtError, Result};
use crate::nbt::mutate::{self, WriteOp};
use crate::nbt::path::NbtPath;
use crate::storage::KeyStore;
use crate::transaction::manager::TransactionManager;
use crate::transaction::write_set::required_lock_path;

pub(crate) fn apply_direct(
    store: &mut dyn KeyStore,
    txm: &mut TransactionManager,
    key: &str,
    path: &NbtPath,
    op: WriteOp,
    now: Instant,
) -> Result<()> {
    txm.check_external_write(key, &required_lock_path(path, &op), now)?;
    let base = store
        .get(key)?
        .ok_or_else(|| NbtError::KeyNotFound(key.to_string()))?;
    let updated = mutate::apply_ops(&base, [(path, &op)])?;
    store.put(key, updated)
}
