use shared::status::Status;

const MAX_FILES: usize = 16;
const MAX_NAME: usize = 64;
const FILE_SIZE: usize = 4096;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NodeType {
    File,
    Directory,
}

static mut NAMES: [[u8; MAX_NAME]; MAX_FILES] = [[0u8; MAX_NAME]; MAX_FILES];
static mut NAME_LENS: [u8; MAX_FILES] = [0u8; MAX_FILES];
static mut NTYPES: [u8; MAX_FILES] = [0u8; MAX_FILES];
static mut SIZES: [u32; MAX_FILES] = [0u32; MAX_FILES];
static mut PARENTS: [i16; MAX_FILES] = [-1i16; MAX_FILES];
static mut FIRST_CHILD: [i16; MAX_FILES] = [-1i16; MAX_FILES];
static mut NEXT_SIBLING: [i16; MAX_FILES] = [-1i16; MAX_FILES];
static mut CHILD_COUNTS: [u8; MAX_FILES] = [0u8; MAX_FILES];
static mut DATAS: [[u8; FILE_SIZE]; MAX_FILES] = [[0u8; FILE_SIZE]; MAX_FILES];
static mut ROOT: i16 = -1;
static mut NEXT_IDX: i16 = 0;

pub fn init() {
    unsafe {
        ROOT = 0;
        NEXT_IDX = 1;
        NTYPES[0] = 2;
        let name = b"/";
        NAME_LENS[0] = 1;
        NAMES[0][0] = b'/';
    }
}

fn alloc_idx() -> Option<i16> {
    unsafe {
        let i = NEXT_IDX;
        if (i as usize) < MAX_FILES {
            NEXT_IDX = i + 1;
            Some(i)
        } else {
            for j in 1..MAX_FILES {
                if NTYPES[j] == 0 {
                    NEXT_IDX = (j + 1) as i16;
                    return Some(j as i16);
                }
            }
            None
        }
    }
}

fn set_node(idx: i16, name: &str, ntype: NodeType, parent: i16) -> Option<i16> {
    unsafe {
        let idx = alloc_idx()?;
        let name_len = name.len().min(MAX_NAME - 1);
        NAMES[idx as usize][..name_len].copy_from_slice(&name.as_bytes()[..name_len]);
        NAME_LENS[idx as usize] = name_len as u8;
        NTYPES[idx as usize] = match ntype {
            NodeType::File => 1,
            NodeType::Directory => 2,
        };
        SIZES[idx as usize] = 0;
        PARENTS[idx as usize] = parent;
        FIRST_CHILD[idx as usize] = -1;
        NEXT_SIBLING[idx as usize] = -1;
        CHILD_COUNTS[idx as usize] = 0;

        if parent >= 0 {
            let pu = parent as usize;
            let first = FIRST_CHILD[pu];
            if first < 0 {
                FIRST_CHILD[pu] = idx;
            } else {
                let mut cur = first;
                loop {
                    let next = NEXT_SIBLING[cur as usize];
                    if next < 0 {
                        NEXT_SIBLING[cur as usize] = idx;
                        break;
                    }
                    cur = next;
                }
            }
            CHILD_COUNTS[pu] += 1;
        }
        Some(idx)
    }
}

pub fn create_file(parent: i16, name: &str) -> Option<i16> {
    unsafe {
        if parent < 0 || parent as usize >= MAX_FILES {
            return None;
        }
        if NTYPES[parent as usize] != 2 {
            return None;
        }
        if find_child(parent, name).is_some() {
            return None;
        }
        set_node(-1, name, NodeType::File, parent)
    }
}

pub fn create_dir(parent: i16, name: &str) -> Option<i16> {
    unsafe {
        if parent < 0 || parent as usize >= MAX_FILES {
            return None;
        }
        if NTYPES[parent as usize] != 2 {
            return None;
        }
        if find_child(parent, name).is_some() {
            return None;
        }
        set_node(-1, name, NodeType::Directory, parent)
    }
}

pub fn find_child(parent: i16, name: &str) -> Option<i16> {
    unsafe {
        if parent < 0 || parent as usize >= MAX_FILES {
            return None;
        }
        let mut cur = FIRST_CHILD[parent as usize];
        while cur >= 0 {
            let cu = cur as usize;
            let len = NAME_LENS[cu] as usize;
            if len == name.len() {
                let matches = &NAMES[cu][..len] == name.as_bytes();
                if matches {
                    return Some(cur);
                }
            }
            cur = NEXT_SIBLING[cu];
        }
        None
    }
}

pub fn resolve(path: &str) -> Option<i16> {
    unsafe {
        let clean = if path.starts_with('/') { &path[1..] } else { path };
        if clean.is_empty() {
            return if ROOT >= 0 { Some(ROOT) } else { None };
        }
        let mut current = ROOT;
        if current < 0 {
            return None;
        }
        for component in clean.split('/').filter(|c| !c.is_empty()) {
            let mut found = -1;
            let mut cur = FIRST_CHILD[current as usize];
            while cur >= 0 {
                let cu = cur as usize;
                let len = NAME_LENS[cu] as usize;
                if len == component.len() && &NAMES[cu][..len] == component.as_bytes() {
                    found = cur;
                    break;
                }
                cur = NEXT_SIBLING[cu];
            }
            if found < 0 {
                return None;
            }
            current = found;
        }
        Some(current)
    }
}

pub fn read(idx: i16, buf: &mut [u8], offset: usize) -> i32 {
    unsafe {
        if idx < 0 || idx as usize >= MAX_FILES {
            return -1;
        }
        let iu = idx as usize;
        if NTYPES[iu] == 0 {
            return -1;
        }
        let size = SIZES[iu] as usize;
        if offset >= size {
            return 0;
        }
        let avail = size - offset;
        let read_len = buf.len().min(avail);
        buf[..read_len].copy_from_slice(&DATAS[iu][offset..offset + read_len]);
        read_len as i32
    }
}

pub fn write(idx: i16, data: &[u8], offset: usize) -> i32 {
    unsafe {
        if idx < 0 || idx as usize >= MAX_FILES {
            return -1;
        }
        let iu = idx as usize;
        if NTYPES[iu] != 1 {
            return -1;
        }
        let write_len = data.len().min(FILE_SIZE - offset);
        DATAS[iu][offset..offset + write_len].copy_from_slice(&data[..write_len]);
        let new_size = (offset + write_len) as u32;
        if new_size > SIZES[iu] {
            SIZES[iu] = new_size;
        }
        write_len as i32
    }
}

pub fn stat_size(idx: i16) -> i32 {
    unsafe {
        if idx < 0 || idx as usize >= MAX_FILES || NTYPES[idx as usize] == 0 {
            return -1;
        }
        SIZES[idx as usize] as i32
    }
}

pub fn stat_type(idx: i16) -> Option<NodeType> {
    unsafe {
        if idx < 0 || idx as usize >= MAX_FILES || NTYPES[idx as usize] == 0 {
            return None;
        }
        match NTYPES[idx as usize] {
            1 => Some(NodeType::File),
            2 => Some(NodeType::Directory),
            _ => None,
        }
    }
}

pub fn mkdir_path(path: &str) -> i32 {
    unsafe {
        let clean = if path.starts_with('/') { &path[1..] } else { path };
        if clean.is_empty() {
            return 0;
        }
        let mut current = ROOT;
        if current < 0 {
            return Status::NotFound.to_raw() as i32;
        }
        let mut components = clean.split('/').filter(|c| !c.is_empty()).peekable();
        while let Some(component) = components.next() {
            let mut found = -1;
            let mut cur = FIRST_CHILD[current as usize];
            while cur >= 0 {
                let cu = cur as usize;
                let len = NAME_LENS[cu] as usize;
                if len == component.len() && &NAMES[cu][..len] == component.as_bytes() {
                    found = cur;
                    break;
                }
                cur = NEXT_SIBLING[cu];
            }
            if found >= 0 {
                if components.peek().is_none() {
                    return Status::AlreadyExists.to_raw() as i32;
                }
                if NTYPES[found as usize] != 2 {
                    return Status::WrongType.to_raw() as i32;
                }
                current = found;
            } else {
                let new = create_dir(current, component);
                match new {
                    Some(n) => current = n,
                    None => return Status::NoMemory.to_raw() as i32,
                }
            }
        }
        0
    }
}

pub fn remove(parent: i16, child: i16) -> bool {
    unsafe {
        if parent < 0 || parent as usize >= MAX_FILES || child < 0 || child as usize >= MAX_FILES
        {
            return false;
        }
        let cu = child as usize;
        if NTYPES[cu] == 2 && CHILD_COUNTS[cu] > 0 {
            return false;
        }
        if NTYPES[cu] == 0 {
            return false;
        }
        let pu = parent as usize;
        let mut prev = -1i16;
        let mut cur = FIRST_CHILD[pu];
        while cur >= 0 {
            if cur == child {
                if prev < 0 {
                    FIRST_CHILD[pu] = NEXT_SIBLING[cu];
                } else {
                    NEXT_SIBLING[prev as usize] = NEXT_SIBLING[cu];
                }
                CHILD_COUNTS[pu] -= 1;
                NTYPES[cu] = 0;
                NAME_LENS[cu] = 0;
                SIZES[cu] = 0;
                PARENTS[cu] = -1;
                FIRST_CHILD[cu] = -1;
                NEXT_SIBLING[cu] = -1;
                CHILD_COUNTS[cu] = 0;
                DATAS[cu] = [0u8; FILE_SIZE];
                return true;
            }
            prev = cur;
            cur = NEXT_SIBLING[cur as usize];
        }
        false
    }
}

pub fn parent_of(child: i16) -> i16 {
    unsafe {
        if child < 0 || child as usize >= MAX_FILES {
            return -1;
        }
        PARENTS[child as usize]
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Dirent {
    pub ino: u64,
    pub size: u64,
    pub ftype: u8,
    pub name_len: u8,
    pub name: [u8; 110],
}

pub fn readdir_entry(parent: i16, index: usize) -> Option<Dirent> {
    unsafe {
        if parent < 0 || parent as usize >= MAX_FILES || NTYPES[parent as usize] != 2 {
            return None;
        }
        let mut cur = FIRST_CHILD[parent as usize];
        let mut i = 0;
        while cur >= 0 {
            if i == index {
                let cu = cur as usize;
                let mut dirent = Dirent {
                    ino: cur as u64,
                    size: SIZES[cu] as u64,
                    ftype: NTYPES[cu],
                    name_len: NAME_LENS[cu],
                    name: [0u8; 110],
                };
                let len = (NAME_LENS[cu] as usize).min(110);
                dirent.name[..len].copy_from_slice(&NAMES[cu][..len]);
                return Some(dirent);
            }
            i += 1;
            cur = NEXT_SIBLING[cur as usize];
        }
        None
    }
}

pub fn readdir_names(parent: i16, buf: &mut [u8]) -> i32 {
    unsafe {
        if parent < 0 || parent as usize >= MAX_FILES || NTYPES[parent as usize] != 2 {
            return -1;
        }
        let mut written = 0;
        let mut cur = FIRST_CHILD[parent as usize];
        let mut first = true;
        while cur >= 0 {
            if !first {
                if written >= buf.len() { break; }
                buf[written] = b'\n';
                written += 1;
            }
            first = false;
            let cu = cur as usize;
            let len = NAME_LENS[cu] as usize;
            let remaining = buf.len() - written;
            let copy_len = len.min(remaining);
            buf[written..written + copy_len].copy_from_slice(&NAMES[cu][..copy_len]);
            written += copy_len;
            cur = NEXT_SIBLING[cu];
        }
        written as i32
    }
}

pub fn create_file_path(path: &str) -> Option<i16> {
    unsafe {
        let clean = if path.starts_with('/') { &path[1..] } else { path };
        if clean.is_empty() {
            return None;
        }

        let mut parent_node = ROOT;
        let mut file_name = clean;

        if let Some(slash_idx) = clean.rfind('/') {
            let parent_path = &clean[..slash_idx];
            file_name = &clean[slash_idx + 1..];
            if let Some(p_idx) = resolve(parent_path) {
                parent_node = p_idx;
            } else {
                return None;
            }
        }

        create_file(parent_node, file_name)
    }
}
