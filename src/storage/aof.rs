use crate::nbt::parser::to_snbt;
use crate::nbt::tag::Tag;
use crate::transaction::write_set::{WriteEntry, WriteOp};

pub const REWRITE_COMMAND: &str = "NBT.CREATE";

pub fn rewrite_snbt(tag: &Tag) -> String {
    to_snbt(tag)
}

pub fn replication_command(entry: &WriteEntry) -> (&'static str, Vec<String>) {
    let key = entry.key.clone();
    let path = entry.path.to_string();
    match &entry.op {
        WriteOp::Set(v) => ("NBT.SET", vec![key, path, to_snbt(v)]),
        WriteOp::Remove => ("NBT.REMOVE", vec![key, path]),
        WriteOp::Merge(v) => ("NBT.MERGE", vec![key, path, to_snbt(v)]),
        WriteOp::Add(v) => ("NBT.ADD", vec![key, path, to_snbt(v)]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nbt::path::NbtPath;

    #[test]
    fn set_is_replicated_as_plain_command() {
        let e = WriteEntry {
            key: "player:1".to_string(),
            path: NbtPath::parse("Health").unwrap(),
            op: WriteOp::Set(Tag::Float(15.0)),
        };
        let (cmd, args) = replication_command(&e);
        assert_eq!(cmd, "NBT.SET");
        assert_eq!(args, vec!["player:1", "Health", "15.0f"]);
    }
}
