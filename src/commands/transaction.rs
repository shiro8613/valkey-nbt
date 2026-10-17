use std::time::Instant;

use crate::error::{NbtError, Result};
use crate::nbt::mutate::WriteOp;
use crate::nbt::parser::{parse_snbt, to_snbt};
use crate::nbt::path::NbtPath;
use crate::storage::KeyStore;
use crate::transaction::conflict::LockMode;
use crate::transaction::manager::{StageRequest, TransactionManager};
use crate::transaction::write_set::WriteEntry;
use crate::transaction::ClientId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageKind {
    Set,
    Merge,
    Add,
    Remove,
}

pub fn begin(
    txm: &mut TransactionManager,
    txid: &str,
    owner: ClientId,
    now: Instant,
) -> Result<()> {
    txm.begin(txid, owner, now)
}

pub fn lock(
    txm: &mut TransactionManager,
    txid: &str,
    owner: ClientId,
    key: &str,
    path: &str,
    mode: &str,
    now: Instant,
) -> Result<()> {
    let path = NbtPath::parse_allow_root(path)?;
    let mode = LockMode::parse(mode)?;
    txm.lock(txid, owner, key, &path, mode, now)
}

pub fn get(
    txm: &mut TransactionManager,
    store: &dyn KeyStore,
    txid: &str,
    owner: ClientId,
    key: &str,
    path: &str,
    now: Instant,
) -> Result<String> {
    let path = NbtPath::parse_allow_root(path)?;
    let tag = txm.read_view(txid, owner, key, &path, store, now)?;
    Ok(to_snbt(&tag))
}

#[derive(Debug, Clone, Copy)]
pub struct StageArgs<'a> {
    pub txid: &'a str,
    pub owner: ClientId,
    pub key: &'a str,
    pub path: &'a str,
    pub kind: StageKind,
    /// SNBT value; `None` for `Remove`.
    pub value: Option<&'a str>,
    pub now: Instant,
}

pub fn stage(
    txm: &mut TransactionManager,
    store: &dyn KeyStore,
    args: StageArgs<'_>,
) -> Result<()> {
    let path = NbtPath::parse_allow_root(args.path)?;
    let op = match (args.kind, args.value) {
        (StageKind::Remove, None) => WriteOp::Remove,
        (StageKind::Set, Some(v)) => WriteOp::Set(parse_snbt(v)?),
        (StageKind::Merge, Some(v)) => WriteOp::Merge(parse_snbt(v)?),
        (StageKind::Add, Some(v)) => WriteOp::Add(parse_snbt(v)?),
        _ => {
            return Err(NbtError::InvalidSyntax(
                "wrong number of arguments for this operation".to_string(),
            ))
        }
    };
    txm.stage(
        StageRequest {
            txid: args.txid,
            owner: args.owner,
            key: args.key,
            path,
            op,
            now: args.now,
        },
        store,
    )
}

pub fn commit(
    txm: &mut TransactionManager,
    store: &mut dyn KeyStore,
    txid: &str,
    owner: ClientId,
    now: Instant,
) -> Result<Vec<WriteEntry>> {
    txm.commit(txid, owner, store, now)
}

pub fn abort(
    txm: &mut TransactionManager,
    txid: &str,
    owner: ClientId,
    now: Instant,
) -> Result<()> {
    txm.abort(txid, owner, now)
}
