use std::collections::BTreeMap;
use std::convert::TryFrom;

use crate::error::{NbtError, Result};
use crate::nbt::tag::{Tag, MAX_DEPTH};

fn invalid(msg: &str) -> NbtError {
    NbtError::NbtInvalid(msg.to_string())
}

pub fn encode(root: &Tag) -> Result<Vec<u8>> {
    if !matches!(root, Tag::Compound(_)) {
        return Err(invalid("root tag must be a compound"));
    }
    root.validate()?;
    let mut buf = Vec::new();
    buf.push(10u8);
    buf.extend_from_slice(&0u16.to_be_bytes()); // empty root name
    write_payload(root, &mut buf)?;
    Ok(buf)
}

fn write_len(buf: &mut Vec<u8>, len: usize) -> Result<()> {
    let n = i32::try_from(len).map_err(|_| invalid("collection too long"))?;
    buf.extend_from_slice(&n.to_be_bytes());
    Ok(())
}

fn write_str(buf: &mut Vec<u8>, s: &str) -> Result<()> {
    let n = u16::try_from(s.len()).map_err(|_| invalid("string longer than 65535 bytes"))?;
    buf.extend_from_slice(&n.to_be_bytes());
    buf.extend_from_slice(s.as_bytes());
    Ok(())
}

fn write_payload(tag: &Tag, buf: &mut Vec<u8>) -> Result<()> {
    match tag {
        Tag::End => return Err(invalid("End tag cannot be encoded as a value")),
        Tag::Byte(v) => buf.push(*v as u8),
        Tag::Short(v) => buf.extend_from_slice(&v.to_be_bytes()),
        Tag::Int(v) => buf.extend_from_slice(&v.to_be_bytes()),
        Tag::Long(v) => buf.extend_from_slice(&v.to_be_bytes()),
        Tag::Float(v) => buf.extend_from_slice(&v.to_bits().to_be_bytes()),
        Tag::Double(v) => buf.extend_from_slice(&v.to_bits().to_be_bytes()),
        Tag::ByteArray(v) => {
            write_len(buf, v.len())?;
            buf.extend(v.iter().map(|b| *b as u8));
        }
        Tag::String(s) => write_str(buf, s)?,
        Tag::List(items) => {
            let id = items.first().map_or(0, |t| t.type_id());
            buf.push(id);
            write_len(buf, items.len())?;
            for it in items {
                write_payload(it, buf)?;
            }
        }
        Tag::Compound(map) => {
            for (k, v) in map {
                buf.push(v.type_id());
                write_str(buf, k)?;
                write_payload(v, buf)?;
            }
            buf.push(0);
        }
        Tag::IntArray(v) => {
            write_len(buf, v.len())?;
            for x in v {
                buf.extend_from_slice(&x.to_be_bytes());
            }
        }
        Tag::LongArray(v) => {
            write_len(buf, v.len())?;
            for x in v {
                buf.extend_from_slice(&x.to_be_bytes());
            }
        }
    }
    Ok(())
}

pub fn decode(data: &[u8]) -> Result<Tag> {
    let mut r = Reader { data, pos: 0 };
    let id = r.byte()?;
    if id != 10 {
        return Err(invalid("root tag must be a compound"));
    }
    let _root_name = r.string()?;
    let tag = r.payload(10, 0)?;
    if r.pos != data.len() {
        return Err(invalid("trailing bytes after NBT data"));
    }
    tag.validate()?;
    Ok(tag)
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|&e| e <= self.data.len())
            .ok_or_else(|| invalid("unexpected end of NBT data"))?;
        let s = &self.data[self.pos..end];
        self.pos = end;
        Ok(s)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        let s = self.take(N)?;
        let mut a = [0u8; N];
        a.copy_from_slice(s);
        Ok(a)
    }

    fn byte(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn len(&mut self) -> Result<usize> {
        let n = i32::from_be_bytes(self.array()?);
        if n < 0 {
            Err(invalid("negative length"))
        } else {
            Ok(n as usize)
        }
    }

    fn string(&mut self) -> Result<String> {
        let n = u16::from_be_bytes(self.array()?) as usize;
        let b = self.take(n)?;
        String::from_utf8(b.to_vec()).map_err(|_| invalid("invalid UTF-8 string"))
    }

    fn payload(&mut self, id: u8, depth: usize) -> Result<Tag> {
        if depth > MAX_DEPTH {
            return Err(invalid("nesting too deep"));
        }
        match id {
            1 => Ok(Tag::Byte(self.byte()? as i8)),
            2 => Ok(Tag::Short(i16::from_be_bytes(self.array()?))),
            3 => Ok(Tag::Int(i32::from_be_bytes(self.array()?))),
            4 => Ok(Tag::Long(i64::from_be_bytes(self.array()?))),
            5 => Ok(Tag::Float(f32::from_bits(u32::from_be_bytes(
                self.array()?,
            )))),
            6 => Ok(Tag::Double(f64::from_bits(u64::from_be_bytes(
                self.array()?,
            )))),
            7 => {
                let n = self.len()?;
                let bytes = self.take(n)?;
                Ok(Tag::ByteArray(bytes.iter().map(|b| *b as i8).collect()))
            }
            8 => Ok(Tag::String(self.string()?)),
            9 => {
                let elem = self.byte()?;
                let n = self.len()?;
                if elem > 12 || (elem == 0 && n > 0) {
                    return Err(invalid("invalid list element type"));
                }
                if n > self.remaining() {
                    return Err(invalid("list length exceeds available data"));
                }
                let mut items = Vec::with_capacity(n);
                for _ in 0..n {
                    items.push(self.payload(elem, depth + 1)?);
                }
                Ok(Tag::List(items))
            }
            10 => {
                let mut map = BTreeMap::new();
                loop {
                    let tid = self.byte()?;
                    if tid == 0 {
                        break;
                    }
                    let key = self.string()?;
                    let value = self.payload(tid, depth + 1)?;
                    map.insert(key, value);
                }
                Ok(Tag::Compound(map))
            }
            11 => {
                let n = self.len()?;
                if n > self.remaining() / 4 {
                    return Err(invalid("array length exceeds available data"));
                }
                let mut v = Vec::with_capacity(n);
                for _ in 0..n {
                    v.push(i32::from_be_bytes(self.array()?));
                }
                Ok(Tag::IntArray(v))
            }
            12 => {
                let n = self.len()?;
                if n > self.remaining() / 8 {
                    return Err(invalid("array length exceeds available data"));
                }
                let mut v = Vec::with_capacity(n);
                for _ in 0..n {
                    v.push(i64::from_be_bytes(self.array()?));
                }
                Ok(Tag::LongArray(v))
            }
            _ => Err(invalid("unknown tag type")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nbt::parser::parse_snbt;

    #[test]
    fn roundtrip_all_types() {
        let t = parse_snbt(
            "{b:1b,s:2s,i:3,l:4L,f:1.5f,d:2.5d,str:\"hi\",ba:[B;1b,2b],ia:[I;1,2],la:[L;1L],\
             list:[{x:1},{x:2}],empty:[],nested:{a:{b:{c:1}}}}",
        )
        .unwrap();
        let bytes = encode(&t).unwrap();
        assert_eq!(decode(&bytes).unwrap(), t);
    }

    #[test]
    fn rejects_malformed() {
        assert!(decode(&[]).is_err());
        assert!(decode(&[9, 0, 0]).is_err());
        let mut bytes = encode(&Tag::empty_compound()).unwrap();
        bytes.push(0);
        assert!(decode(&bytes).is_err()); // trailing byte
        bytes.truncate(bytes.len() - 2);
        assert!(decode(&bytes).is_err()); // truncated
                                          // int array claiming a huge length must not allocate / panic
        let evil = [10u8, 0, 0, 11, 0, 1, b'a', 0x7f, 0xff, 0xff, 0xff, 0];
        assert!(decode(&evil).is_err());
    }

    #[test]
    fn encode_requires_compound_root() {
        assert!(encode(&Tag::Int(1)).is_err());
    }
}
