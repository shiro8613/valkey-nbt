use crate::error::{NbtError, Result};
use crate::nbt::path::NbtPath;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockMode {
    Read,
    Write,
}

impl LockMode {
    pub fn parse(s: &str) -> Result<LockMode> {
        if s.eq_ignore_ascii_case("READ") {
            Ok(LockMode::Read)
        } else if s.eq_ignore_ascii_case("WRITE") {
            Ok(LockMode::Write)
        } else {
            Err(NbtError::InvalidSyntax(format!(
                "lock mode must be READ or WRITE, got '{}'",
                s
            )))
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            LockMode::Read => "READ",
            LockMode::Write => "WRITE",
        }
    }
}

pub fn modes_conflict(a: LockMode, b: LockMode) -> bool {
    a == LockMode::Write || b == LockMode::Write
}

pub fn locks_conflict(pa: &NbtPath, ma: LockMode, pb: &NbtPath, mb: LockMode) -> bool {
    pa.overlaps(pb) && modes_conflict(ma, mb)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> NbtPath {
        NbtPath::parse(s).unwrap()
    }

    #[test]
    fn table_from_spec() {
        use LockMode::*;
        assert!(!locks_conflict(&p("Health"), Write, &p("Food"), Write));
        assert!(!locks_conflict(
            &p("Inventory[0].Count"),
            Write,
            &p("Inventory[0].id"),
            Write
        ));
        assert!(locks_conflict(
            &p("Inventory"),
            Write,
            &p("Inventory[0].Count"),
            Write
        ));
        assert!(locks_conflict(
            &p("Inventory[0]"),
            Write,
            &p("Inventory[0].Count"),
            Read
        ));
        assert!(locks_conflict(&p("Health"), Write, &p("Health"), Write));
        assert!(locks_conflict(&p("Health"), Read, &p("Health"), Write));
        assert!(!locks_conflict(&p("Health"), Read, &p("Health"), Read));
    }

    #[test]
    fn parse_modes() {
        assert_eq!(LockMode::parse("write").unwrap(), LockMode::Write);
        assert_eq!(LockMode::parse("READ").unwrap(), LockMode::Read);
        assert!(LockMode::parse("x").is_err());
    }
}
