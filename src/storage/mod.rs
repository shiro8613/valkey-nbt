pub mod aof;
pub mod rdb;

#[cfg(feature = "valkey")]
pub mod data_type;

use std::collections::HashMap;

use crate::error::Result;
use crate::nbt::tag::Tag;

pub trait KeyStore {
    fn get(&self, key: &str) -> Result<Option<Tag>>;
    fn put(&mut self, key: &str, tag: Tag) -> Result<()>;

    fn exists(&self, key: &str) -> Result<bool> {
        Ok(self.get(key)?.is_some())
    }

    fn put_all(&mut self, items: Vec<(String, Tag)>) -> Result<()> {
        for (k, t) in items {
            self.put(&k, t)?;
        }
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct MemoryStore {
    pub map: HashMap<String, Tag>,
}

impl KeyStore for MemoryStore {
    fn get(&self, key: &str) -> Result<Option<Tag>> {
        Ok(self.map.get(key).cloned())
    }

    fn put(&mut self, key: &str, tag: Tag) -> Result<()> {
        self.map.insert(key.to_string(), tag);
        Ok(())
    }
}
