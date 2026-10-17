use crate::error::{NbtError, Result};
use crate::nbt::codec;
use crate::nbt::tag::Tag;

pub const FORMAT_VERSION: u8 = 1;

pub fn encode(tag: &Tag) -> Result<Vec<u8>> {
    let mut out = vec![FORMAT_VERSION];
    out.extend(codec::encode(tag)?);
    Ok(out)
}

pub fn decode(data: &[u8]) -> Result<Tag> {
    match data.split_first() {
        Some((&FORMAT_VERSION, rest)) => codec::decode(rest),
        Some((v, _)) => Err(NbtError::NbtInvalid(format!(
            "unsupported RDB format version {}",
            v
        ))),
        None => Err(NbtError::NbtInvalid("empty RDB payload".to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nbt::parser::parse_snbt;

    #[test]
    fn roundtrip_and_version_check() {
        let t = parse_snbt("{a:1,b:[1,2]}").unwrap();
        let bytes = encode(&t).unwrap();
        assert_eq!(bytes[0], FORMAT_VERSION);
        assert_eq!(decode(&bytes).unwrap(), t);
        let mut bad = bytes.clone();
        bad[0] = 99;
        assert!(decode(&bad).is_err());
        assert!(decode(&[]).is_err());
    }
}
