use std::sync::{Mutex, MutexGuard};
use std::time::Instant;

use valkey_module::alloc::ValkeyAlloc;
use valkey_module::server_events::ClientChangeSubevent;
use valkey_module::{valkey_module, Context, ValkeyError, ValkeyResult, ValkeyString, ValkeyValue};
use valkey_module_macros::client_changed_event_handler;

use crate::commands::transaction::{self as txc, StageArgs, StageKind};
use crate::commands::{add, create, get, merge, remove, set};
use crate::error::NbtError;
use crate::storage::aof;
use crate::storage::data_type::{ValkeyStore, NBT_TYPE};
use crate::transaction::manager::TransactionManager;

static TXM: std::sync::OnceLock<Mutex<TransactionManager>> = OnceLock::new();

fn txm() -> MutexGuard<'static, TransactionManager> {
    TXM.get_or_init(|| Mutex::new(TransactionManager::default()))
        .lock()
        .unwrap_or_else(|p| p.into_inner())
}

fn err(e: NbtError) -> ValkeyError {
    ValkeyError::String(e.to_string())
}

fn arg(args: &[ValkeyString], i: usize) -> String {
    args[i].to_string_lossy()
}

fn ok() -> ValkeyResult {
    Ok(ValkeyValue::SimpleStringStatic("OK"))
}

fn nbt_create(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    if args.len() != 2 && args.len() != 3 {
        return Err(ValkeyError::WrongArity);
    }
    let snbt = if args.len() == 3 {
        Some(arg(&args, 2))
    } else {
        None
    };
    let mut store = ValkeyStore::new(ctx);
    create::execute(&mut store, &arg(&args, 1), snbt.as_deref()).map_err(err)?;
    ctx.replicate_verbatim();
    ok()
}

fn nbt_get(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    if args.len() != 2 && args.len() != 3 {
        return Err(ValkeyError::WrongArity);
    }
    let path = if args.len() == 3 {
        Some(arg(&args, 2))
    } else {
        None
    };
    let store = ValkeyStore::new(ctx);
    let s = get::execute(&store, &arg(&args, 1), path.as_deref()).map_err(err)?;
    Ok(ValkeyValue::BulkString(s))
}

fn nbt_set(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    if args.len() != 4 {
        return Err(ValkeyError::WrongArity);
    }
    let mut store = ValkeyStore::new(ctx);
    set::execute(
        &mut store,
        &mut txm(),
        &arg(&args, 1),
        &arg(&args, 2),
        &arg(&args, 3),
        Instant::now(),
    )
    .map_err(err)?;
    ctx.replicate_verbatim();
    ok()
}

fn nbt_merge(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    if args.len() != 4 {
        return Err(ValkeyError::WrongArity);
    }
    let mut store = ValkeyStore::new(ctx);
    merge::execute(
        &mut store,
        &mut txm(),
        &arg(&args, 1),
        &arg(&args, 2),
        &arg(&args, 3),
        Instant::now(),
    )
    .map_err(err)?;
    ctx.replicate_verbatim();
    ok()
}

fn nbt_add(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    if args.len() != 4 {
        return Err(ValkeyError::WrongArity);
    }
    let mut store = ValkeyStore::new(ctx);
    add::execute(
        &mut store,
        &mut txm(),
        &arg(&args, 1),
        &arg(&args, 2),
        &arg(&args, 3),
        Instant::now(),
    )
    .map_err(err)?;
    ctx.replicate_verbatim();
    ok()
}

fn nbt_remove(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    if args.len() != 3 {
        return Err(ValkeyError::WrongArity);
    }
    let mut store = ValkeyStore::new(ctx);
    remove::execute(
        &mut store,
        &mut txm(),
        &arg(&args, 1),
        &arg(&args, 2),
        Instant::now(),
    )
    .map_err(err)?;
    ctx.replicate_verbatim();
    ok()
}

fn tx_begin(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    if args.len() != 2 {
        return Err(ValkeyError::WrongArity);
    }
    txc::begin(
        &mut txm(),
        &arg(&args, 1),
        ctx.get_client_id(),
        Instant::now(),
    )
    .map_err(err)?;
    ok()
}

fn tx_lock(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    if args.len() != 5 {
        return Err(ValkeyError::WrongArity);
    }
    txc::lock(
        &mut txm(),
        &arg(&args, 1),
        ctx.get_client_id(),
        &arg(&args, 2),
        &arg(&args, 3),
        &arg(&args, 4),
        Instant::now(),
    )
    .map_err(err)?;
    ok()
}

fn tx_get(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    if args.len() != 4 {
        return Err(ValkeyError::WrongArity);
    }
    let store = ValkeyStore::new(ctx);
    let s = txc::get(
        &mut txm(),
        &store,
        &arg(&args, 1),
        ctx.get_client_id(),
        &arg(&args, 2),
        &arg(&args, 3),
        Instant::now(),
    )
    .map_err(err)?;
    Ok(ValkeyValue::BulkString(s))
}

fn stage(ctx: &Context, args: Vec<ValkeyString>, kind: StageKind) -> ValkeyResult {
    let want = if kind == StageKind::Remove { 4 } else { 5 };
    if args.len() != want {
        return Err(ValkeyError::WrongArity);
    }
    let value = if kind == StageKind::Remove {
        None
    } else {
        Some(arg(&args, 4))
    };
    let store = ValkeyStore::new(ctx);
    let (txid, key, path) = (arg(&args, 1), arg(&args, 2), arg(&args, 3));
    txc::stage(
        &mut txm(),
        &store,
        StageArgs {
            txid: &txid,
            owner: ctx.get_client_id(),
            key: &key,
            path: &path,
            kind,
            value: value.as_deref(),
            now: Instant::now(),
        },
    )
    .map_err(err)?;
    ok()
}

fn tx_set(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    stage(ctx, args, StageKind::Set)
}
fn tx_merge(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    stage(ctx, args, StageKind::Merge)
}
fn tx_add(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    stage(ctx, args, StageKind::Add)
}
fn tx_remove(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    stage(ctx, args, StageKind::Remove)
}

fn tx_commit(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    if args.len() != 2 {
        return Err(ValkeyError::WrongArity);
    }
    let mut store = ValkeyStore::new(ctx);
    let applied = txc::commit(
        &mut txm(),
        &mut store,
        &arg(&args, 1),
        ctx.get_client_id(),
        Instant::now(),
    )
    .map_err(err)?;
    for entry in &applied {
        let (cmd, cmd_args) = aof::replication_command(entry);
        let refs: Vec<&str> = cmd_args.iter().map(|s| s.as_str()).collect();
        ctx.replicate(cmd, refs.as_slice());
    }
    Ok(ValkeyValue::Integer(applied.len() as i64))
}

fn tx_abort(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    if args.len() != 2 {
        return Err(ValkeyError::WrongArity);
    }
    txc::abort(
        &mut txm(),
        &arg(&args, 1),
        ctx.get_client_id(),
        Instant::now(),
    )
    .map_err(err)?;
    ok()
}

#[client_changed_event_handler]
fn on_client_change(ctx: &Context, event: ClientChangeSubevent) {
    if let ClientChangeSubevent::Disconnected = event {
        txm().abort_client(ctx.get_client_id());
    }
}

valkey_module! {
    name: "nbt",
    version: 1,
    allocator: (ValkeyAlloc, ValkeyAlloc),
    data_types: [NBT_TYPE],
    commands: [
        ["nbt.create", nbt_create, "write deny-oom", 1, 1, 1],
        ["nbt.get", nbt_get, "readonly", 1, 1, 1],
        ["nbt.set", nbt_set, "write deny-oom", 1, 1, 1],
        ["nbt.merge", nbt_merge, "write deny-oom", 1, 1, 1],
        ["nbt.add", nbt_add, "write deny-oom", 1, 1, 1],
        ["nbt.remove", nbt_remove, "write", 1, 1, 1],
        ["nbt.tx.begin", tx_begin, "fast", 0, 0, 0],
        ["nbt.tx.lock", tx_lock, "fast", 2, 2, 1],
        ["nbt.tx.get", tx_get, "readonly", 2, 2, 1],
        ["nbt.tx.set", tx_set, "fast", 2, 2, 1],
        ["nbt.tx.merge", tx_merge, "fast", 2, 2, 1],
        ["nbt.tx.add", tx_add, "fast", 2, 2, 1],
        ["nbt.tx.remove", tx_remove, "fast", 2, 2, 1],
        ["nbt.tx.commit", tx_commit, "write deny-oom", 0, 0, 0],
        ["nbt.tx.abort", tx_abort, "fast", 0, 0, 0],
    ],
}
