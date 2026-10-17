# valkey-nbt

Valkey module that stores Minecraft NBT natively and edits it with
Minecraft-style paths, with **path-level locking** and **atomic multi-path
transactions**

## Commands

Paths: `Health`, `Inventory[0].Count`, `"quoted key".x`; `.` = root. Values: SNBT.

| Command | Notes |
|---|---|
| `NBT.CREATE key [snbt]` | root must be a compound; fails with `KEY_EXISTS` if present |
| `NBT.GET key path` | returns SNBT |
| `NBT.SET / MERGE / ADD key path value`, `NBT.REMOVE key path` | refused with `LOCKED` if the path overlaps a transaction lock |
| `NBT.TX.BEGIN txid` | owner = calling client |
| `NBT.TX.LOCK txid key path READ\|WRITE` | no waiting; conflict → `LOCKED` |
| `NBT.TX.GET txid key path` | sees committed data + own staged writes; needs a covering lock |
| `NBT.TX.SET / MERGE / ADD txid key path value`, `NBT.TX.REMOVE txid key path` | needs a covering WRITE lock |
| `NBT.TX.COMMIT txid` | atomic; returns number of applied ops |
| `NBT.TX.ABORT txid` | |

(TODOs: `TX.MERGE/ADD/REMOVE`)

## Decisions taken for open items

* **ADD**: numeric target → same-type addition (checked for integers, finite for
  floats); List/ByteArray/IntArray/LongArray target → append one element of the right type.
* **SET** creates the final compound key if missing; intermediate paths must exist.
  List elements must keep the list's type. **MERGE** is deep for compounds, replaces everything else.
* **Isolation**: reads inside a transaction need a covering lock; locks are held
  until commit/abort/timeout/disconnect, so locked paths cannot change underneath.
* **Plain writes vs locks**: plain write commands fail with `LOCKED` on overlapping
  locked paths. Direct `DEL`/`SET` on the key by other Valkey commands is *not* blocked; a
  commit then fails with `COMMIT_FAILED` and changes nothing.
* **Multi-key transactions** are supported; candidates for all keys are built before any write.
* **Timeout**: 30 s total lifetime, checked lazily on every command. Limits: 1024 txs,
  256 locks/tx, 1024 staged ops (`Config`). `LIMIT_EXCEEDED` is an added error code.
* **List structure changes** (`REMOVE` of an element) require a WRITE lock on the whole list.
* **Persistence**: RDB = `[version u8][binary NBT]`; AOF rewrite = `NBT.CREATE key snbt`;
  commits replicate as the equivalent plain commands (MULTI/EXEC). Locks/txs are never persisted.
* NaN/Infinity floats are rejected (SNBT cannot round-trip them); strings/keys ≤ 65535 bytes;
  depth ≤ 512. Binary strings are plain UTF-8, not Java modified UTF-8.
* Array tags cannot be indexed element-wise (`Arr[0]`) yet; wildcards/filters are not supported.
* Finished transaction ids are remembered (last 4096) and cannot be reused.

## Build

```sh
cargo build --release --features valkey
```
