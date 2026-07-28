use crate::ipc::channel::Channel;
use core::sync::atomic::{AtomicBool, Ordering};
use shared::status::{Result, Status};

pub const MAX_SERVICES: usize = 16;

pub struct ServiceEntry {
    pub name: [u8; 16],
    pub name_len: usize,
    pub channel_ptr: *mut Channel,
}

struct Registry {
    services: [Option<ServiceEntry>; MAX_SERVICES],
}

static mut REGISTRY: Registry = Registry {
    services: [
        None, None, None, None, None, None, None, None,
        None, None, None, None, None, None, None, None,
    ],
};

static REGISTRY_LOCK: AtomicBool = AtomicBool::new(false);

fn acquire_lock() {
    while REGISTRY_LOCK
        .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
}

fn release_lock() {
    REGISTRY_LOCK.store(false, Ordering::Release);
}

pub fn register_service(name_bytes: &[u8], channel_ptr: *mut Channel) -> Result<()> {
    if name_bytes.len() > 16 || name_bytes.is_empty() {
        return Err(Status::InvalidArgs);
    }
    acquire_lock();
    let registry = unsafe { &mut REGISTRY };
    
    // Check if already exists
    for entry in registry.services.iter() {
        if let Some(e) = entry {
            if &e.name[..e.name_len] == name_bytes {
                release_lock();
                return Err(Status::AlreadyExists);
            }
        }
    }

    // Insert into a free slot
    for slot in registry.services.iter_mut() {
        if slot.is_none() {
            let mut name = [0u8; 16];
            name[..name_bytes.len()].copy_from_slice(name_bytes);
            *slot = Some(ServiceEntry {
                name,
                name_len: name_bytes.len(),
                channel_ptr,
            });
            release_lock();
            return Ok(());
        }
    }

    release_lock();
    Err(Status::NoMemory)
}

pub fn lookup_service(name_bytes: &[u8]) -> Result<*mut Channel> {
    if name_bytes.len() > 16 || name_bytes.is_empty() {
        return Err(Status::InvalidArgs);
    }
    acquire_lock();
    let registry = unsafe { &REGISTRY };

    for entry in registry.services.iter() {
        if let Some(e) = entry {
            if &e.name[..e.name_len] == name_bytes {
                let ptr = e.channel_ptr;
                release_lock();
                return Ok(ptr);
            }
        }
    }

    release_lock();
    Err(Status::NotFound)
}
