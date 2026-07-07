use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use shared::status::{Result, Status};
use shared::types::{HandleValue, ObjectType};
use crate::mm::vmo::Vmo;
use crate::mm::vmar::Vmar;
use crate::ipc::channel::Channel;
use crate::ipc::port::Port;

static HANDLE_VALUE_COUNTER: AtomicU32 = AtomicU32::new(1);

pub const MAX_HANDLES: usize = 32;
const PAGE_SIZE: usize = 4096;

#[derive(Debug)]
pub enum KernelObject {
    Vmo(Vmo),
    Vmar(Vmar),
    Channel(Channel),
    Port(Port),
    Process(u64),
    Thread(usize),
}

impl KernelObject {
    pub fn object_type(&self) -> ObjectType {
        match self {
            KernelObject::Vmo(_) => ObjectType::Vmo,
            KernelObject::Vmar(_) => ObjectType::Vmar,
            KernelObject::Channel(_) => ObjectType::Channel,
            KernelObject::Port(_) => ObjectType::Port,
            KernelObject::Process(_) => ObjectType::Process,
            KernelObject::Thread(_) => ObjectType::Thread,
        }
    }
}

struct Slot {
    value: HandleValue,
    rights: u32,
    object: KernelObject,
}

/// A minimal spinlock to replace `spin::Mutex`.
///
/// The large `HandleSlots` struct (~5 KB) is stored in a phys page to keep
/// stack usage low; the lock is a plain `AtomicBool`.
pub struct HandleTable {
    inner: *mut HandleSlots,
}

static HANDLE_TABLE_LOCK: AtomicBool = AtomicBool::new(false);

fn acquire_lock() {
    while HANDLE_TABLE_LOCK
        .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
}

fn release_lock() {
    HANDLE_TABLE_LOCK.store(false, Ordering::Release);
}

struct HandleSlots {
    slots: [Option<Slot>; MAX_HANDLES],
}

impl HandleSlots {
    /// Allocate phys pages and initialise every slot to `None`.
    fn new() -> *mut Self {
        let slot_size = core::mem::size_of::<Option<Slot>>();
        let total = MAX_HANDLES * slot_size;
        let pages = (total + PAGE_SIZE - 1) / PAGE_SIZE;
        let first_pa = crate::mm::phys::alloc_page().expect("alloc_page failed");
        let base_va = crate::mm::mmu::pa_to_kernel_va(first_pa.as_usize());
        unsafe {
            core::ptr::write_bytes(base_va as *mut u8, 0, PAGE_SIZE);
        }
        for _ in 1..pages {
            let pa = crate::mm::phys::alloc_page().expect("alloc_page failed");
            let va = crate::mm::mmu::pa_to_kernel_va(pa.as_usize());
            unsafe { core::ptr::write_bytes(va as *mut u8, 0, PAGE_SIZE); }
        }
        // Niche-optimised `None` may NOT be all-zero bytes, so we must
        // explicitly write `None` into every slot.
        unsafe {
            let p = base_va as *mut Option<Slot>;
            for i in 0..MAX_HANDLES {
                core::ptr::write(p.add(i), None);
            }
        }
        base_va as *mut Self
    }
}

impl HandleTable {
    fn slots_mut(&self) -> &mut HandleSlots {
        unsafe { &mut *self.inner }
    }
}

impl core::fmt::Debug for HandleTable {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        acquire_lock();
        let live = self.slots_mut().slots.iter().filter(|s| s.is_some()).count();
        release_lock();
        f.debug_struct("HandleTable").field("live", &live).finish()
    }
}

impl HandleTable {
    pub fn new() -> Self {
        let inner = HandleSlots::new();
        HandleTable { inner }
    }

    pub fn add(&self, object: KernelObject, rights: u32) -> Result<HandleValue> {
        let hv = HandleValue::new(HANDLE_VALUE_COUNTER.fetch_add(1, Ordering::Relaxed));
        acquire_lock();
        let slots = self.slots_mut();
        for slot in slots.slots.iter_mut() {
            if slot.is_none() {
                *slot = Some(Slot { value: hv, rights, object });
                release_lock();
                return Ok(hv);
            }
        }
        release_lock();
        Err(Status::NoMemory)
    }

    pub fn with<R>(&self, hv: HandleValue, required_rights: u32,
                   f: impl FnOnce(&mut KernelObject) -> Result<R>) -> Result<R> {
        acquire_lock();
        let slots = self.slots_mut();
        let slot = slots.slots.iter_mut()
            .find_map(|s| s.as_mut().filter(|s| s.value == hv))
            .ok_or(Status::BadHandle)?;
        if !Self::check_rights(slot.rights, required_rights) {
            release_lock();
            return Err(Status::AccessDenied);
        }
        let result = f(&mut slot.object);
        release_lock();
        result
    }

    pub fn with_vmo<R>(&self, hv: HandleValue, required_rights: u32,
                       f: impl FnOnce(&mut Vmo) -> R) -> Result<R> {
        self.with(hv, required_rights, |obj| {
            match obj {
                KernelObject::Vmo(vmo) => Ok(f(vmo)),
                _ => Err(Status::WrongType),
            }
        })
    }

    pub fn with_vmar<R>(&self, hv: HandleValue, required_rights: u32,
                        f: impl FnOnce(&mut Vmar) -> R) -> Result<R> {
        self.with(hv, required_rights, |obj| {
            match obj {
                KernelObject::Vmar(vmar) => Ok(f(vmar)),
                _ => Err(Status::WrongType),
            }
        })
    }

    pub fn with_channel<R>(&self, hv: HandleValue, required_rights: u32,
                           f: impl FnOnce(&mut Channel) -> R) -> Result<R> {
        self.with(hv, required_rights, |obj| {
            match obj {
                KernelObject::Channel(chan) => Ok(f(chan)),
                _ => Err(Status::WrongType),
            }
        })
    }

    pub fn with_port<R>(&self, hv: HandleValue, required_rights: u32,
                        f: impl FnOnce(&mut Port) -> R) -> Result<R> {
        self.with(hv, required_rights, |obj| {
            match obj {
                KernelObject::Port(port) => Ok(f(port)),
                _ => Err(Status::WrongType),
            }
        })
    }

    pub fn with_process<R>(&self, hv: HandleValue, required_rights: u32,
                           f: impl FnOnce(u64) -> R) -> Result<R> {
        self.with(hv, required_rights, |obj| {
            match obj {
                KernelObject::Process(pid) => Ok(f(*pid)),
                _ => Err(Status::WrongType),
            }
        })
    }

    pub fn with_thread<R>(&self, hv: HandleValue, required_rights: u32,
                          f: impl FnOnce(usize) -> R) -> Result<R> {
        self.with(hv, required_rights, |obj| {
            match obj {
                KernelObject::Thread(tid) => Ok(f(*tid)),
                _ => Err(Status::WrongType),
            }
        })
    }

    pub fn remove(&self, hv: HandleValue) -> Result<KernelObject> {
        match self.remove_with_rights(hv) {
            Ok((obj, _)) => Ok(obj),
            Err(e) => Err(e),
        }
    }

    pub fn remove_with_rights(&self, hv: HandleValue) -> Result<(KernelObject, u32)> {
        acquire_lock();
        let slots = self.slots_mut();
        let idx = slots.slots.iter().position(|s| {
            s.as_ref().map_or(false, |s| s.value == hv)
        }).ok_or(Status::BadHandle);
        match idx {
            Ok(idx) => {
                let slot = slots.slots[idx].take().unwrap();
                release_lock();
                Ok((slot.object, slot.rights))
            }
            Err(e) => {
                release_lock();
                Err(e)
            }
        }
    }

    pub fn close(&self, hv: HandleValue) -> Result<KernelObject> {
        self.remove(hv)
    }

    pub fn live_count(&self) -> usize {
        acquire_lock();
        let n = self.slots_mut().slots.iter().filter(|s| s.is_some()).count();
        release_lock();
        n
    }

    fn check_rights(have: u32, required: u32) -> bool {
        if required == 0 { return true; }
        (have & required) == required
    }
}
