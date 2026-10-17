use std::collections::{BTreeMap, HashMap, VecDeque};
use std::time::{Duration, Instant};

use crate::error::{NbtError, Result};
use crate::nbt::mutate;
use crate::nbt::path::NbtPath;
use crate::nbt::query;
use crate::nbt::tag::Tag;
use crate::storage::KeyStore;
use crate::transaction::conflict::LockMode;
use crate::transaction::lock_table::LockTable;
use crate::transaction::write_set::{required_lock_path, WriteEntry, WriteOp, WriteSet};
use crate::transaction::ClientId;

const TOMBSTONE_CAP: usize = 4096;

#[derive(Debug, Clone)]
pub struct Config {
    pub tx_timeout: Duration,
    pub max_transactions: usize,
    pub max_locks_per_tx: usize,
    pub max_write_set_ops: usize,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            tx_timeout: Duration::from_secs(30),
            max_transactions: 1024,
            max_locks_per_tx: 256,
            max_write_set_ops: 1024,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxState {
    Active,
    Committing,
    Committed,
    Aborted,
}

#[derive(Debug)]
pub struct Transaction {
    pub id: String,
    pub owner: ClientId,
    pub state: TxState,
    pub started: Instant,
    pub deadline: Instant,
    pub write_set: WriteSet,
}

#[derive(Debug, Clone)]
pub struct StageRequest<'a> {
    pub txid: &'a str,
    pub owner: ClientId,
    pub key: &'a str,
    pub path: NbtPath,
    pub op: WriteOp,
    pub now: Instant,
}

#[derive(Debug)]
pub struct TransactionManager {
    cfg: Config,
    txs: HashMap<String, Transaction>,
    tombstones: HashMap<String, TxState>,
    tomb_order: VecDeque<String>,
    locks: LockTable,
}

impl Default for TransactionManager {
    fn default() -> Self {
        TransactionManager::new(Config::default())
    }
}

impl TransactionManager {
    pub fn new(cfg: Config) -> Self {
        TransactionManager {
            cfg,
            txs: HashMap::new(),
            tombstones: HashMap::new(),
            tomb_order: VecDeque::new(),
            locks: LockTable::new(),
        }
    }

    pub fn active_count(&self) -> usize {
        self.txs.len()
    }

    pub fn lock_count(&self) -> usize {
        self.locks.total()
    }

    fn remember(&mut self, txid: &str, state: TxState) {
        if self.tombstones.insert(txid.to_string(), state).is_none() {
            self.tomb_order.push_back(txid.to_string());
        }
        while self.tomb_order.len() > TOMBSTONE_CAP {
            if let Some(old) = self.tomb_order.pop_front() {
                self.tombstones.remove(&old);
            }
        }
    }

    fn finish(&mut self, txid: &str, state: TxState) {
        self.txs.remove(txid);
        self.locks.release_all(txid);
        self.remember(txid, state);
    }

    pub fn sweep(&mut self, now: Instant) {
        let expired: Vec<String> = self
            .txs
            .iter()
            .filter(|(_, t)| t.deadline <= now)
            .map(|(id, _)| id.clone())
            .collect();
        for id in expired {
            self.finish(&id, TxState::Aborted);
        }
    }

    fn check(&self, txid: &str, owner: ClientId) -> Result<()> {
        match self.txs.get(txid) {
            None => {
                if self.tombstones.contains_key(txid) {
                    Err(NbtError::TxInvalidState(format!(
                        "transaction '{}' has already finished (committed, aborted or expired)",
                        txid
                    )))
                } else {
                    Err(NbtError::TxNotFound(format!(
                        "transaction '{}' not found",
                        txid
                    )))
                }
            }
            Some(tx) => {
                if tx.owner != owner {
                    Err(NbtError::TxNotOwner(format!(
                        "transaction '{}' belongs to another client",
                        txid
                    )))
                } else if tx.state != TxState::Active {
                    Err(NbtError::TxInvalidState(format!(
                        "transaction '{}' is not active",
                        txid
                    )))
                } else {
                    Ok(())
                }
            }
        }
    }

    pub fn begin(&mut self, txid: &str, owner: ClientId, now: Instant) -> Result<()> {
        self.sweep(now);
        if self.txs.contains_key(txid) || self.tombstones.contains_key(txid) {
            return Err(NbtError::TxInvalidState(format!(
                "transaction id '{}' is in use or was already used",
                txid
            )));
        }
        if self.txs.len() >= self.cfg.max_transactions {
            return Err(NbtError::LimitExceeded(
                "too many concurrent transactions".to_string(),
            ));
        }
        self.txs.insert(
            txid.to_string(),
            Transaction {
                id: txid.to_string(),
                owner,
                state: TxState::Active,
                started: now,
                deadline: now + self.cfg.tx_timeout,
                write_set: WriteSet::default(),
            },
        );
        Ok(())
    }

    pub fn lock(
        &mut self,
        txid: &str,
        owner: ClientId,
        key: &str,
        path: &NbtPath,
        mode: LockMode,
        now: Instant,
    ) -> Result<()> {
        self.sweep(now);
        self.check(txid, owner)?;
        let already = self.locks.holds(txid, key, path, mode);
        if !already && self.locks.count_for(txid) >= self.cfg.max_locks_per_tx {
            return Err(NbtError::LimitExceeded(
                "too many locks in this transaction".to_string(),
            ));
        }
        self.locks.acquire(txid, key, path, mode)
    }

    pub fn read_view(
        &mut self,
        txid: &str,
        owner: ClientId,
        key: &str,
        path: &NbtPath,
        store: &dyn KeyStore,
        now: Instant,
    ) -> Result<Tag> {
        self.sweep(now);
        self.check(txid, owner)?;
        if !self.locks.holds(txid, key, path, LockMode::Read) {
            return Err(NbtError::Locked(format!(
                "{} {} is not locked by this transaction",
                key, path
            )));
        }
        let base = store
            .get(key)?
            .ok_or_else(|| NbtError::KeyNotFound(key.to_string()))?;
        let tx = self.txs.get(txid).expect("checked");
        let ops = tx
            .write_set
            .entries()
            .iter()
            .filter(|e| e.key == key)
            .map(|e| (&e.path, &e.op));
        let view = mutate::apply_ops(&base, ops)?;
        Ok(query::get(&view, path)?.clone())
    }

    pub fn stage(&mut self, req: StageRequest<'_>, store: &dyn KeyStore) -> Result<()> {
        let StageRequest {
            txid,
            owner,
            key,
            path,
            op,
            now,
        } = req;
        self.sweep(now);
        self.check(txid, owner)?;
        let needed = required_lock_path(&path, &op);
        if !self.locks.holds(txid, key, &needed, LockMode::Write) {
            return Err(NbtError::Locked(format!(
                "{} {} requires a WRITE lock held by this transaction",
                key, needed
            )));
        }
        match &op {
            WriteOp::Set(v) | WriteOp::Merge(v) | WriteOp::Add(v) => v.validate()?,
            WriteOp::Remove => {}
        }
        {
            let tx = self.txs.get(txid).expect("checked");
            if tx.write_set.len() >= self.cfg.max_write_set_ops {
                return Err(NbtError::LimitExceeded("write set is full".to_string()));
            }
            let base = store
                .get(key)?
                .ok_or_else(|| NbtError::KeyNotFound(key.to_string()))?;
            let existing = tx
                .write_set
                .entries()
                .iter()
                .filter(|e| e.key == key)
                .map(|e| (&e.path, &e.op));
            mutate::apply_ops(&base, existing.chain(std::iter::once((&path, &op))))?;
        }
        let tx = self.txs.get_mut(txid).expect("checked");
        tx.write_set.push(WriteEntry {
            key: key.to_string(),
            path,
            op,
        });
        Ok(())
    }

    pub fn commit(
        &mut self,
        txid: &str,
        owner: ClientId,
        store: &mut dyn KeyStore,
        now: Instant,
    ) -> Result<Vec<WriteEntry>> {
        self.sweep(now);
        self.check(txid, owner)?;
        let mut tx = self.txs.remove(txid).expect("checked");
        tx.state = TxState::Committing;

        let outcome = match self.prepare_commit(&tx, &*store) {
            Ok(items) => store.put_all(items),
            Err(e) => Err(e),
        };

        self.locks.release_all(txid);
        match outcome {
            Ok(()) => {
                tx.state = TxState::Committed;
                self.remember(txid, TxState::Committed);
                Ok(tx.write_set.into_entries())
            }
            Err(e) => {
                self.remember(txid, TxState::Aborted);
                Err(NbtError::CommitFailed(e.to_string()))
            }
        }
    }

    fn prepare_commit(&self, tx: &Transaction, store: &dyn KeyStore) -> Result<Vec<(String, Tag)>> {
        for e in tx.write_set.entries() {
            let needed = required_lock_path(&e.path, &e.op);
            if !self.locks.holds(&tx.id, &e.key, &needed, LockMode::Write) {
                return Err(NbtError::Locked(format!(
                    "{} {} is no longer write-locked",
                    e.key, needed
                )));
            }
        }
        let mut grouped: BTreeMap<&str, Vec<(&NbtPath, &WriteOp)>> = BTreeMap::new();
        for e in tx.write_set.entries() {
            grouped
                .entry(e.key.as_str())
                .or_default()
                .push((&e.path, &e.op));
        }
        let mut out = Vec::new();
        for (key, ops) in grouped {
            let base = store
                .get(key)?
                .ok_or_else(|| NbtError::KeyNotFound(key.to_string()))?;
            let updated = mutate::apply_ops(&base, ops)?;
            out.push((key.to_string(), updated));
        }
        Ok(out)
    }

    pub fn abort(&mut self, txid: &str, owner: ClientId, now: Instant) -> Result<()> {
        self.sweep(now);
        self.check(txid, owner)?;
        self.finish(txid, TxState::Aborted);
        Ok(())
    }

    pub fn abort_client(&mut self, owner: ClientId) -> usize {
        let ids: Vec<String> = self
            .txs
            .values()
            .filter(|t| t.owner == owner)
            .map(|t| t.id.clone())
            .collect();
        let n = ids.len();
        for id in ids {
            self.finish(&id, TxState::Aborted);
        }
        n
    }

    pub fn check_external_write(&mut self, key: &str, path: &NbtPath, now: Instant) -> Result<()> {
        self.sweep(now);
        self.locks.check_external_write(key, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nbt::parser::parse_snbt;
    use crate::storage::MemoryStore;

    const A: ClientId = 1;
    const B: ClientId = 2;

    fn p(s: &str) -> NbtPath {
        NbtPath::parse(s).unwrap()
    }

    fn req<'a>(
        txid: &'a str,
        owner: ClientId,
        key: &'a str,
        path: &str,
        op: WriteOp,
        now: Instant,
    ) -> StageRequest<'a> {
        StageRequest {
            txid,
            owner,
            key,
            path: p(path),
            op,
            now,
        }
    }

    fn setup() -> (TransactionManager, MemoryStore, Instant) {
        let mut store = MemoryStore::default();
        store.map.insert(
            "player".to_string(),
            parse_snbt(
                "{Health:20.0f,Food:20,Inventory:[{id:\"a\",Count:1b},{id:\"b\",Count:2b}]}",
            )
            .unwrap(),
        );
        (TransactionManager::default(), store, Instant::now())
    }

    #[test]
    fn spec_example_commits_all_changes() {
        let (mut m, mut s, now) = setup();
        m.begin("t", A, now).unwrap();
        m.lock("t", A, "player", &p("Health"), LockMode::Write, now)
            .unwrap();
        m.lock("t", A, "player", &p("Food"), LockMode::Write, now)
            .unwrap();
        m.stage(
            req(
                "t",
                A,
                "player",
                "Health",
                WriteOp::Set(Tag::Float(15.0)),
                now,
            ),
            &s,
        )
        .unwrap();
        m.stage(
            req("t", A, "player", "Food", WriteOp::Set(Tag::Int(18)), now),
            &s,
        )
        .unwrap();
        // not visible before commit
        assert_eq!(
            s.map["player"],
            parse_snbt(
                "{Health:20.0f,Food:20,Inventory:[{id:\"a\",Count:1b},{id:\"b\",Count:2b}]}"
            )
            .unwrap()
        );
        // read-your-writes
        assert_eq!(
            m.read_view("t", A, "player", &p("Health"), &s, now)
                .unwrap(),
            Tag::Float(15.0)
        );
        let applied = m.commit("t", A, &mut s, now).unwrap();
        assert_eq!(applied.len(), 2);
        assert_eq!(
            query::get(&s.map["player"], &p("Health")).unwrap(),
            &Tag::Float(15.0)
        );
        assert_eq!(
            query::get(&s.map["player"], &p("Food")).unwrap(),
            &Tag::Int(18)
        );
        assert_eq!(m.lock_count(), 0);
        // finished ids cannot be reused or operated on
        assert!(matches!(
            m.begin("t", A, now),
            Err(NbtError::TxInvalidState(_))
        ));
        assert!(matches!(
            m.abort("t", A, now),
            Err(NbtError::TxInvalidState(_))
        ));
    }

    #[test]
    fn disjoint_transactions_do_not_clobber_each_other() {
        let (mut m, mut s, now) = setup();
        m.begin("t1", A, now).unwrap();
        m.begin("t2", B, now).unwrap();
        m.lock("t1", A, "player", &p("Health"), LockMode::Write, now)
            .unwrap();
        m.lock("t2", B, "player", &p("Food"), LockMode::Write, now)
            .unwrap();
        m.stage(
            req(
                "t1",
                A,
                "player",
                "Health",
                WriteOp::Set(Tag::Float(1.0)),
                now,
            ),
            &s,
        )
        .unwrap();
        m.stage(
            req("t2", B, "player", "Food", WriteOp::Set(Tag::Int(2)), now),
            &s,
        )
        .unwrap();
        m.commit("t1", A, &mut s, now).unwrap();
        m.commit("t2", B, &mut s, now).unwrap();
        assert_eq!(
            query::get(&s.map["player"], &p("Health")).unwrap(),
            &Tag::Float(1.0)
        );
        assert_eq!(
            query::get(&s.map["player"], &p("Food")).unwrap(),
            &Tag::Int(2)
        );
    }

    #[test]
    fn conflicting_lock_is_rejected_and_freed_on_abort() {
        let (mut m, _s, now) = setup();
        m.begin("t1", A, now).unwrap();
        m.begin("t2", B, now).unwrap();
        m.lock(
            "t1",
            A,
            "player",
            &p("Inventory[0].Count"),
            LockMode::Write,
            now,
        )
        .unwrap();
        assert!(matches!(
            m.lock("t2", B, "player", &p("Inventory"), LockMode::Write, now),
            Err(NbtError::Locked(_))
        ));
        m.abort("t1", A, now).unwrap();
        m.lock("t2", B, "player", &p("Inventory"), LockMode::Write, now)
            .unwrap();
    }

    #[test]
    fn ownership_is_enforced() {
        let (mut m, mut s, now) = setup();
        m.begin("t", A, now).unwrap();
        assert!(matches!(
            m.lock("t", B, "player", &p("Food"), LockMode::Read, now),
            Err(NbtError::TxNotOwner(_))
        ));
        assert!(matches!(
            m.commit("t", B, &mut s, now),
            Err(NbtError::TxNotOwner(_))
        ));
        assert!(matches!(m.abort("t", B, now), Err(NbtError::TxNotOwner(_))));
        assert!(matches!(
            m.abort("nope", A, now),
            Err(NbtError::TxNotFound(_))
        ));
        // the owner's transaction survived the foreign attempts
        m.abort("t", A, now).unwrap();
    }

    #[test]
    fn stage_requires_write_lock_and_validates() {
        let (mut m, s, now) = setup();
        m.begin("t", A, now).unwrap();
        assert!(matches!(
            m.stage(
                req("t", A, "player", "Food", WriteOp::Set(Tag::Int(1)), now),
                &s
            ),
            Err(NbtError::Locked(_))
        ));
        m.lock("t", A, "player", &p("Food"), LockMode::Read, now)
            .unwrap();
        assert!(matches!(
            m.stage(
                req("t", A, "player", "Food", WriteOp::Set(Tag::Int(1)), now),
                &s
            ),
            Err(NbtError::Locked(_))
        ));
        m.lock("t", A, "player", &p("Food"), LockMode::Write, now)
            .unwrap();
        assert!(matches!(
            m.stage(
                req("t", A, "player", "Food", WriteOp::Add(Tag::Long(1)), now),
                &s
            ),
            Err(NbtError::TypeMismatch(_))
        ));
        assert!(matches!(
            m.stage(req("t", A, "ghost", "Food", WriteOp::Remove, now), &s),
            Err(NbtError::Locked(_)) | Err(NbtError::KeyNotFound(_))
        ));
    }

    #[test]
    fn list_removal_needs_whole_list_lock() {
        let (mut m, mut s, now) = setup();
        m.begin("t", A, now).unwrap();
        m.lock("t", A, "player", &p("Inventory[0]"), LockMode::Write, now)
            .unwrap();
        assert!(matches!(
            m.stage(
                req("t", A, "player", "Inventory[0]", WriteOp::Remove, now),
                &s
            ),
            Err(NbtError::Locked(_))
        ));
        m.lock("t", A, "player", &p("Inventory"), LockMode::Write, now)
            .unwrap();
        m.stage(
            req("t", A, "player", "Inventory[0]", WriteOp::Remove, now),
            &s,
        )
        .unwrap();
        m.commit("t", A, &mut s, now).unwrap();
        assert_eq!(
            query::get(&s.map["player"], &p("Inventory[0].id")).unwrap(),
            &Tag::String("b".to_string())
        );
    }

    #[test]
    fn negative_index_locks_conflict_with_positive_ones() {
        let (mut m, mut s, now) = setup();
        m.begin("t1", A, now).unwrap();
        m.begin("t2", B, now).unwrap();
        m.lock(
            "t1",
            A,
            "player",
            &p("Inventory[-1].Count"),
            LockMode::Write,
            now,
        )
        .unwrap();
        assert!(matches!(
            m.lock(
                "t2",
                B,
                "player",
                &p("Inventory[1].Count"),
                LockMode::Write,
                now
            ),
            Err(NbtError::Locked(_))
        ));
        m.stage(
            req(
                "t1",
                A,
                "player",
                "Inventory[-1].Count",
                WriteOp::Set(Tag::Byte(9)),
                now,
            ),
            &s,
        )
        .unwrap();
        m.commit("t1", A, &mut s, now).unwrap();
        assert_eq!(
            query::get(&s.map["player"], &p("Inventory[1].Count")).unwrap(),
            &Tag::Byte(9)
        );
    }

    #[test]
    fn failed_commit_leaves_no_partial_update() {
        let (mut m, mut s, now) = setup();
        s.map
            .insert("other".to_string(), parse_snbt("{x:1}").unwrap());
        let before = s.map["player"].clone();
        m.begin("t", A, now).unwrap();
        m.lock("t", A, "player", &p("Food"), LockMode::Write, now)
            .unwrap();
        m.lock("t", A, "other", &p("x"), LockMode::Write, now)
            .unwrap();
        m.stage(
            req("t", A, "player", "Food", WriteOp::Set(Tag::Int(1)), now),
            &s,
        )
        .unwrap();
        m.stage(
            req("t", A, "other", "x", WriteOp::Set(Tag::Int(2)), now),
            &s,
        )
        .unwrap();
        s.map.remove("other"); // key vanished behind the transaction's back
        assert!(matches!(
            m.commit("t", A, &mut s, now),
            Err(NbtError::CommitFailed(_))
        ));
        assert_eq!(s.map["player"], before);
        assert_eq!(m.lock_count(), 0);
        assert!(matches!(
            m.abort("t", A, now),
            Err(NbtError::TxInvalidState(_))
        ));
    }

    #[test]
    fn timeout_releases_locks_and_blocks_late_commit() {
        let (mut m, mut s, now) = setup();
        m.begin("t", A, now).unwrap();
        m.lock("t", A, "player", &p("Food"), LockMode::Write, now)
            .unwrap();
        m.stage(
            req("t", A, "player", "Food", WriteOp::Set(Tag::Int(1)), now),
            &s,
        )
        .unwrap();
        let later = now + Duration::from_secs(31);
        assert!(m.check_external_write("player", &p("Food"), later).is_ok());
        assert_eq!(m.lock_count(), 0);
        assert!(matches!(
            m.commit("t", A, &mut s, later),
            Err(NbtError::TxInvalidState(_))
        ));
        assert_eq!(
            query::get(&s.map["player"], &p("Food")).unwrap(),
            &Tag::Int(20)
        );
    }

    #[test]
    fn client_disconnect_releases_everything() {
        let (mut m, _s, now) = setup();
        m.begin("t1", A, now).unwrap();
        m.begin("t2", A, now).unwrap();
        m.begin("t3", B, now).unwrap();
        m.lock("t1", A, "player", &p("Food"), LockMode::Write, now)
            .unwrap();
        m.lock("t3", B, "player", &p("Health"), LockMode::Write, now)
            .unwrap();
        assert_eq!(m.abort_client(A), 2);
        assert_eq!(m.active_count(), 1);
        assert_eq!(m.lock_count(), 1);
    }

    #[test]
    fn external_writes_respect_locks() {
        let (mut m, _s, now) = setup();
        m.begin("t", A, now).unwrap();
        m.lock(
            "t",
            A,
            "player",
            &p("Inventory[0].Count"),
            LockMode::Read,
            now,
        )
        .unwrap();
        assert!(matches!(
            m.check_external_write("player", &p("Inventory"), now),
            Err(NbtError::Locked(_))
        ));
        assert!(m.check_external_write("player", &p("Health"), now).is_ok());
    }

    #[test]
    fn limits_are_enforced() {
        let cfg = Config {
            max_transactions: 1,
            max_locks_per_tx: 1,
            max_write_set_ops: 1,
            ..Config::default()
        };
        let mut m = TransactionManager::new(cfg);
        let (_, s, now) = setup();
        m.begin("t", A, now).unwrap();
        assert!(matches!(
            m.begin("u", B, now),
            Err(NbtError::LimitExceeded(_))
        ));
        m.lock("t", A, "player", &p("Food"), LockMode::Write, now)
            .unwrap();
        assert!(matches!(
            m.lock("t", A, "player", &p("Health"), LockMode::Write, now),
            Err(NbtError::LimitExceeded(_))
        ));
        m.stage(
            req("t", A, "player", "Food", WriteOp::Set(Tag::Int(1)), now),
            &s,
        )
        .unwrap();
        assert!(matches!(
            m.stage(
                req("t", A, "player", "Food", WriteOp::Set(Tag::Int(2)), now),
                &s
            ),
            Err(NbtError::LimitExceeded(_))
        ));
    }
}
