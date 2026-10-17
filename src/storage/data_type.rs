use std::ffi::{c_void, CString};
use std::os::raw::{c_char, c_int};

use valkey_module::native_types::ValkeyType;
use valkey_module::{raw, Context};

use crate::error::{NbtError, Result};
use crate::nbt::tag::Tag;
use crate::storage::{aof, rdb, KeyStore};

pub struct NbtValue {
    pub tag: Tag,
}

pub static NBT_TYPE: ValkeyType = ValkeyType::new(
    "valkeynbt",
    0,
    raw::RedisModuleTypeMethods {
        version: raw::VALKEYMODULE_TYPE_METHOD_VERSION as u64,
        rdb_load: Some(rdb_load),
        rdb_save: Some(rdb_save),
        aof_rewrite: Some(aof_rewrite),
        free: Some(free),
        mem_usage: Some(mem_usage),
        digest: None,
        aux_load: None,
        aux_save: None,
        aux_save_triggers: 0,
        free_effort: None,
        unlink: None,
        copy: None,
        defrag: None,
        mem_usage2: None,
        free_effort2: None,
        unlink2: None,
        copy2: None,
        aux_save2: None,
    },
);

unsafe extern "C" fn free(value: *mut c_void) {
    if !value.is_null() {
        drop(Box::from_raw(value as *mut NbtValue));
    }
}

fn estimated_size(tag: &Tag) -> usize {
    std::mem::size_of::<Tag>()
        + match tag {
            Tag::ByteArray(v) => v.len(),
            Tag::IntArray(v) => v.len() * 4,
            Tag::LongArray(v) => v.len() * 8,
            Tag::String(s) => s.len(),
            Tag::List(items) => items.iter().map(estimated_size).sum(),
            Tag::Compound(m) => m.iter().map(|(k, v)| k.len() + estimated_size(v)).sum(),
            _ => 0,
        }
}

unsafe extern "C" fn mem_usage(value: *const c_void) -> usize {
    let v = &*(value as *const NbtValue);
    estimated_size(&v.tag)
}

unsafe extern "C" fn rdb_save(rdb_io: *mut raw::RedisModuleIO, value: *mut c_void) {
    let v = &*(value as *mut NbtValue);
    // Stored tags are always validated, so encoding cannot fail in practice.
    let bytes = rdb::encode(&v.tag)
        .or_else(|_| rdb::encode(&Tag::empty_compound()))
        .unwrap_or_default();
    raw::save_slice(rdb_io, &bytes);
}

unsafe extern "C" fn rdb_load(rdb_io: *mut raw::RedisModuleIO, encver: c_int) -> *mut c_void {
    if encver != 0 {
        return std::ptr::null_mut();
    }
    let buf = match raw::load_string_buffer(rdb_io) {
        Ok(b) => b,
        Err(_) => return std::ptr::null_mut(),
    };
    match rdb::decode(buf.as_ref()) {
        Ok(tag) => Box::into_raw(Box::new(NbtValue { tag })) as *mut c_void,
        Err(_) => std::ptr::null_mut(),
    }
}

unsafe extern "C" fn aof_rewrite(
    aof_io: *mut raw::RedisModuleIO,
    key: *mut raw::RedisModuleString,
    value: *mut c_void,
) {
    let v = &*(value as *mut NbtValue);
    let snbt = aof::rewrite_snbt(&v.tag);
    let cmd = CString::new(aof::REWRITE_COMMAND).unwrap();
    let fmt = CString::new("sb").unwrap(); // s = module string, b = (ptr, len) buffer
    if let Some(emit) = raw::RedisModule_EmitAOF {
        emit(
            aof_io,
            cmd.as_ptr(),
            fmt.as_ptr(),
            key,
            snbt.as_ptr() as *const c_char,
            snbt.len(),
        );
    }
}

pub struct ValkeyStore<'a> {
    pub ctx: &'a Context,
}

impl<'a> ValkeyStore<'a> {
    pub fn new(ctx: &'a Context) -> Self {
        ValkeyStore { ctx }
    }
}

impl KeyStore for ValkeyStore<'_> {
    fn get(&self, key: &str) -> Result<Option<Tag>> {
        let k = self.ctx.open_key(&self.ctx.create_string(key));
        match k.get_value::<NbtValue>(&NBT_TYPE) {
            Ok(Some(v)) => Ok(Some(v.tag.clone())),
            Ok(None) => Ok(None),
            Err(_) => Err(NbtError::TypeMismatch(
                "WRONGTYPE key holds a different kind of value".to_string(),
            )),
        }
    }

    fn put(&mut self, key: &str, tag: Tag) -> Result<()> {
        let k = self.ctx.open_key_writable(&self.ctx.create_string(key));
        k.set_value(&NBT_TYPE, NbtValue { tag })
            .map_err(|e| NbtError::CommitFailed(e.to_string()))
    }
}
