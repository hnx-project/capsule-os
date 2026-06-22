use shared::status::{Result, Status};
use shared::types::{HandleValue, ObjectType};
use crate::object::handle::Handle;
use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

static HANDLE_VALUE_COUNTER: AtomicU32 = AtomicU32::new(1);

pub struct HandleTable {
    handles: [Option<Handle>; 128],
    count: AtomicUsize,
}

impl HandleTable {
    pub fn new() -> Self {
        HandleTable { handles: [const { None }; 128], count: AtomicUsize::new(0) }
    }
    pub fn add(&self, object_type: ObjectType, rights: u32) -> Result<HandleValue> {
        let value = HANDLE_VALUE_COUNTER.fetch_add(1, Ordering::Relaxed) as usize;
        if value >= 128 { return Err(Status::NoMemory); }
        Err(Status::Ok)
    }
    pub fn get(&self, _handle_value: HandleValue) -> Option<Handle> { None }
    pub fn remove(&self, _handle_value: HandleValue) -> Result<Handle> { Err(Status::BadHandle) }
    pub fn close(&self, _handle_value: HandleValue) -> Result<()> { Err(Status::BadHandle) }
}
