use std::collections::HashMap;

use crate::error::{NbtError, Result};
use crate::nbt::path::NbtPath;
use crate::transaction::conflict::{locks_conflict, LockMode};

#[derive(Debug, Clone)]
pub struct LockEntry {
    pub txid: String,
    pub path: NbtPath,
    pub mode: LockMode,
}

#[derive(Debug, Default)]
pub struct LockTable {
    by_key: HashMap<String, Vec<LockEntry>>,
}

impl LockTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn acquire(&mut self, txid: &str, key: &str, path: &NbtPath, mode: LockMode) -> Result<()> {
        if let Some(entries) = self.by_key.get(key) {
            for e in entries {
                if e.txid != txid && locks_conflict(&e.path, e.mode, path, mode) {
                    return Err(NbtError::Locked(format!(
                        "{} {} conflicts with a {} lock held by another transaction",
                        key,
                        path,
                        e.mode.as_str()
                    )));
                }
            }
        }
        let entries = self.by_key.entry(key.to_string()).or_default();
        if let Some(e) = entries
            .iter_mut()
            .find(|e| e.txid == txid && e.path == *path)
        {
            if mode == LockMode::Write {
                e.mode = LockMode::Write;
            }
            return Ok(());
        }
        entries.push(LockEntry {
            txid: txid.to_string(),
            path: path.clone(),
            mode,
        });
        Ok(())
    }

    pub fn holds(&self, txid: &str, key: &str, path: &NbtPath, need: LockMode) -> bool {
        self.by_key.get(key).is_some_and(|v| {
            v.iter().any(|e| {
                e.txid == txid
                    && e.path.is_prefix_of(path)
                    && (need == LockMode::Read || e.mode == LockMode::Write)
            })
        })
    }

    pub fn check_external_write(&self, key: &str, path: &NbtPath) -> Result<()> {
        if let Some(entries) = self.by_key.get(key) {
            for e in entries {
                if e.path.overlaps(path) {
                    return Err(NbtError::Locked(format!(
                        "{} {} is locked by a transaction",
                        key, path
                    )));
                }
            }
        }
        Ok(())
    }

    pub fn release_all(&mut self, txid: &str) {
        for entries in self.by_key.values_mut() {
            entries.retain(|e| e.txid != txid);
        }
        self.by_key.retain(|_, v| !v.is_empty());
    }

    pub fn count_for(&self, txid: &str) -> usize {
        self.by_key
            .values()
            .flatten()
            .filter(|e| e.txid == txid)
            .count()
    }

    pub fn total(&self) -> usize {
        self.by_key.values().map(|v| v.len()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> NbtPath {
        NbtPath::parse(s).unwrap()
    }

    #[test]
    fn disjoint_writes_coexist_and_overlaps_conflict() {
        let mut t = LockTable::new();
        t.acquire("a", "k", &p("Health"), LockMode::Write).unwrap();
        t.acquire("b", "k", &p("Food"), LockMode::Write).unwrap();
        assert!(matches!(
            t.acquire("b", "k", &p("Health"), LockMode::Read),
            Err(NbtError::Locked(_))
        ));
        t.acquire("b", "other", &p("Health"), LockMode::Write)
            .unwrap();
    }

    #[test]
    fn parent_child_conflict() {
        let mut t = LockTable::new();
        t.acquire("a", "k", &p("Inventory[0].Count"), LockMode::Write)
            .unwrap();
        assert!(t
            .acquire("b", "k", &p("Inventory"), LockMode::Read)
            .is_err());
        assert!(t
            .acquire("b", "k", &p("Inventory[1]"), LockMode::Write)
            .is_ok());
    }

    #[test]
    fn reads_share_and_upgrade_needs_exclusivity() {
        let mut t = LockTable::new();
        t.acquire("a", "k", &p("x"), LockMode::Read).unwrap();
        t.acquire("b", "k", &p("x"), LockMode::Read).unwrap();
        assert!(t.acquire("a", "k", &p("x"), LockMode::Write).is_err());
        t.release_all("b");
        t.acquire("a", "k", &p("x"), LockMode::Write).unwrap();
        assert!(t.holds("a", "k", &p("x"), LockMode::Write));
        assert!(t.holds("a", "k", &p("x.y"), LockMode::Write));
        assert!(!t.holds("a", "k", &p("z"), LockMode::Read));
    }

    #[test]
    fn release_and_external_write() {
        let mut t = LockTable::new();
        t.acquire("a", "k", &p("x"), LockMode::Read).unwrap();
        assert!(t.check_external_write("k", &p("x.y")).is_err());
        assert!(t.check_external_write("k", &p("z")).is_ok());
        t.release_all("a");
        assert_eq!(t.total(), 0);
        assert!(t.check_external_write("k", &p("x")).is_ok());
    }
}
