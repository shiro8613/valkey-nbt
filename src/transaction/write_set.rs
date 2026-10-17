use crate::nbt::path::{NbtPath, PathSegment};

pub use crate::nbt::mutate::WriteOp;

#[derive(Debug, Clone, PartialEq)]
pub struct WriteEntry {
    pub key: String,
    pub path: NbtPath,
    pub op: WriteOp,
}

#[derive(Debug, Default)]
pub struct WriteSet {
    entries: Vec<WriteEntry>,
}

impl WriteSet {
    pub fn push(&mut self, entry: WriteEntry) {
        self.entries.push(entry);
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entries(&self) -> &[WriteEntry] {
        &self.entries
    }

    pub fn into_entries(self) -> Vec<WriteEntry> {
        self.entries
    }
}

pub fn required_lock_path(path: &NbtPath, op: &WriteOp) -> NbtPath {
    match op {
        WriteOp::Remove => match path.last() {
            Some(PathSegment::Index(_)) => path.parent().unwrap_or_else(NbtPath::root),
            _ => path.clone(),
        },
        _ => path.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nbt::tag::Tag;

    fn p(s: &str) -> NbtPath {
        NbtPath::parse(s).unwrap()
    }

    #[test]
    fn list_structure_changes_lock_whole_list() {
        assert_eq!(
            required_lock_path(&p("Inventory[2]"), &WriteOp::Remove),
            p("Inventory")
        );
        assert_eq!(
            required_lock_path(&p("Inventory[2].id"), &WriteOp::Remove),
            p("Inventory[2].id")
        );
        assert_eq!(
            required_lock_path(&p("Inventory"), &WriteOp::Add(Tag::Int(1))),
            p("Inventory")
        );
        assert_eq!(
            required_lock_path(&p("Inventory[2]"), &WriteOp::Set(Tag::Int(1))),
            p("Inventory[2]")
        );
        assert_eq!(
            required_lock_path(&p("[0]"), &WriteOp::Remove),
            NbtPath::root()
        );
    }
}
