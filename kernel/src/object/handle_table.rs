use shared::status::{Result, Status};
use shared::types::{HandleValue, ObjectType};
use crate::object::handle::Handle;
use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

static HANDLE_VALUE_COUNTER: AtomicU32 = AtomicU32::new(1);

pub struct HandleTable {
    _handles: [Option<Handle>; 128],
    _count: AtomicUsize,
}

impl HandleTable {
    pub fn new() -> Self {
        HandleTable { _handles: [const { None }; 128], _count: AtomicUsize::new(0) }
    }
    pub fn add(&self, _object_type: ObjectType, _rights: u32) -> Result<HandleValue> {
        let value = HANDLE_VALUE_COUNTER.fetch_add(1, Ordering::Relaxed);
        if value as usize >= 128 { return Err(Status::NoMemory); }
        Ok(HandleValue::new(value))
    }
    pub fn get(&self, _handle_value: HandleValue) -> Option<Handle> { None }
    pub fn remove(&self, _handle_value: HandleValue) -> Result<Handle> { Err(Status::BadHandle) }
    pub fn close(&self, _handle_value: HandleValue) -> Result<()> { Err(Status::BadHandle) }
}
