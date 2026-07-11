use shared::status::{Result, Status};
use shared::types::HandleValue;
use crate::object::{HandleTable, KernelObject, Rights};

pub const MAX_VNODES: usize = 256;
pub const MAX_OPEN_FILES: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VnodeType {
    File,
    Directory,
    Device,
    Symlink,
    Socket,
    Pipe,
}

#[derive(Debug)]
pub struct Vnode {
    pub id: u64,
    pub vtype: VnodeType,
    pub ref_count: u32,
    pub size: u64,
    pub flags: u32,
}

impl Vnode {
    pub fn new(id: u64, vtype: VnodeType) -> Self {
        Vnode {
            id,
            vtype,
            ref_count: 1,
            size: 0,
            flags: 0,
        }
    }

    pub fn inc_ref(&mut self) {
        self.ref_count += 1;
    }

    pub fn dec_ref(&mut self) -> i32 {
        self.ref_count = self.ref_count.saturating_sub(1);
        self.ref_count as i32
    }
}

pub struct VnodeTable {
    entries: [Option<Vnode>; MAX_VNODES],
    next_id: u64,
}

impl VnodeTable {
    pub const fn new() -> Self {
        VnodeTable {
            entries: [const { None }; MAX_VNODES],
            next_id: 1,
        }
    }

    pub fn alloc_vnode(&mut self, vtype: VnodeType) -> Result<u64> {
        for i in 0..MAX_VNODES {
            if self.entries[i].is_none() {
                let id = self.next_id;
                self.next_id += 1;
                self.entries[i] = Some(Vnode::new(id, vtype));
                return Ok(id);
            }
        }
        Err(Status::NoMemory)
    }

    pub fn get_vnode(&self, id: u64) -> Option<&Vnode> {
        for entry in &self.entries {
            if let Some(ref vnode) = entry {
                if vnode.id == id {
                    return Some(vnode);
                }
            }
        }
        None
    }

    pub fn get_vnode_mut(&mut self, id: u64) -> Option<&mut Vnode> {
        for entry in &mut self.entries {
            if let Some(ref mut vnode) = entry {
                if vnode.id == id {
                    return Some(vnode);
                }
            }
        }
        None
    }

    pub fn release_vnode(&mut self, id: u64) -> bool {
        for entry in &mut self.entries {
            if let Some(ref mut vnode) = entry {
                if vnode.id == id {
                    if vnode.dec_ref() <= 0 {
                        *entry = None;
                    }
                    return true;
                }
            }
        }
        false
    }
}

pub struct FileDescriptor {
    pub vnode_id: u64,
    pub offset: u64,
    pub flags: u32,
    pub rights: u32,
}

impl FileDescriptor {
    pub fn new(vnode_id: u64, rights: u32) -> Self {
        FileDescriptor {
            vnode_id,
            offset: 0,
            flags: 0,
            rights,
        }
    }
}

pub struct FileDescriptorTable {
    entries: [Option<FileDescriptor>; MAX_OPEN_FILES],
}

impl FileDescriptorTable {
    pub const fn new() -> Self {
        FileDescriptorTable {
            entries: [const { None }; MAX_OPEN_FILES],
        }
    }

    pub fn alloc_fd(&mut self, vnode_id: u64, rights: u32) -> Result<u32> {
        for i in 0..MAX_OPEN_FILES {
            if self.entries[i].is_none() {
                self.entries[i] = Some(FileDescriptor::new(vnode_id, rights));
                return Ok(i as u32);
            }
        }
        Err(Status::NoMemory)
    }

    pub fn get_fd(&self, fd: u32) -> Option<&FileDescriptor> {
        if (fd as usize) < MAX_OPEN_FILES {
            self.entries[fd as usize].as_ref()
        } else {
            None
        }
    }

    pub fn get_fd_mut(&mut self, fd: u32) -> Option<&mut FileDescriptor> {
        if (fd as usize) < MAX_OPEN_FILES {
            self.entries[fd as usize].as_mut()
        } else {
            None
        }
    }

    pub fn close_fd(&mut self, fd: u32) -> bool {
        if (fd as usize) < MAX_OPEN_FILES {
            if self.entries[fd as usize].is_some() {
                self.entries[fd as usize] = None;
                return true;
            }
        }
        false
    }
}

static mut VNODE_TABLE: VnodeTable = VnodeTable::new();
static mut FD_TABLE: FileDescriptorTable = FileDescriptorTable::new();

pub fn alloc_vnode(vtype: VnodeType) -> Result<u64> {
    unsafe { VNODE_TABLE.alloc_vnode(vtype) }
}

pub fn get_vnode(id: u64) -> Option<u64> {
    unsafe { VNODE_TABLE.get_vnode(id).map(|v| v.id) }
}

pub fn release_vnode(id: u64) -> bool {
    unsafe { VNODE_TABLE.release_vnode(id) }
}

pub fn alloc_fd(vnode_id: u64, rights: u32) -> Result<u32> {
    unsafe { FD_TABLE.alloc_fd(vnode_id, rights) }
}

pub fn get_fd(fd: u32) -> Option<u64> {
    unsafe { FD_TABLE.get_fd(fd).map(|f| f.vnode_id) }
}

pub fn close_fd(fd: u32) -> bool {
    unsafe { FD_TABLE.close_fd(fd) }
}

pub fn read_fd_offset(fd: u32) -> Option<u64> {
    unsafe { FD_TABLE.get_fd(fd).map(|f| f.offset) }
}

pub fn write_fd_offset(fd: u32, offset: u64) -> bool {
    if let Some(f) = unsafe { FD_TABLE.get_fd_mut(fd) } {
        f.offset = offset;
        true
    } else {
        false
    }
}
