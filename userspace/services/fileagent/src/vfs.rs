use crate::ramfs;
use shared::status::Status;

const MAX_SESSIONS: usize = 8;
const MAX_FDS: usize = 16;

pub struct OpenFile {
    pub node_idx: i16,
    pub offset: u32,
}

pub struct Session {
    pub server_chan: usize,
    pub fds: [Option<OpenFile>; MAX_FDS],
}

static mut SESSIONS: [Option<Session>; MAX_SESSIONS] = [const { None }; MAX_SESSIONS];

pub fn sessions_mut() -> &'static mut [Option<Session>; MAX_SESSIONS] {
    unsafe { &mut SESSIONS }
}

pub const fn tty_fd() -> u32 {
    100
}

pub fn do_open(session_idx: usize, path: &str, flags: u32) -> i32 {
    unsafe {
        let session = match &SESSIONS[session_idx] {
            Some(_) => true,
            None => return -1,
        };
        if !session {
            return -1;
        }

        if path == "dev/tty" || path == "/dev/tty" {
            return tty_fd() as i32;
        }

        let clean = if path.starts_with('/') { &path[1..] } else { path };
        let mut node = ramfs::resolve(clean);
        if node.is_none() && (flags & 0x40 != 0) {
            node = ramfs::create_file_path(clean);
        }
        match node {
            Some(idx) => {
                let sess = SESSIONS[session_idx].as_mut().unwrap();
                for fd in 0..MAX_FDS {
                    if sess.fds[fd].is_none() {
                        sess.fds[fd] = Some(OpenFile {
                            node_idx: idx,
                            offset: 0,
                        });
                        return fd as i32;
                    }
                }
                Status::NoMemory.to_raw() as i32
            }
            None => Status::NotFound.to_raw() as i32,
        }
    }
}

pub fn do_close(session_idx: usize, fd: u32) -> i32 {
    unsafe {
        if fd == tty_fd() {
            return 0;
        }
        let sess = match SESSIONS[session_idx].as_mut() {
            Some(s) => s,
            None => return -1,
        };
        if (fd as usize) < MAX_FDS {
            sess.fds[fd as usize] = None;
            return 0;
        }
        -1
    }
}

pub fn do_read(session_idx: usize, fd: u32, buf: &mut [u8]) -> i32 {
    unsafe {
        if fd == tty_fd() {
            return 0;
        }
        let sess = match SESSIONS[session_idx].as_mut() {
            Some(s) => s,
            None => return -1,
        };
        let of = match &sess.fds[fd as usize] {
            Some(f) => f,
            None => return -1,
        };
        let node_idx = of.node_idx;
        let offset = of.offset;
        let result = ramfs::read(node_idx, buf, offset as usize);
        if result > 0 {
            if let Some(ref mut of_mut) = sess.fds[fd as usize] {
                of_mut.offset += result as u32;
            }
        }
        result
    }
}

pub fn do_write(session_idx: usize, fd: u32, data: &[u8]) -> i32 {
    unsafe {
        if fd == tty_fd() {
            return data.len() as i32;
        }
        let sess = match SESSIONS[session_idx].as_mut() {
            Some(s) => s,
            None => return -1,
        };
        let of = match &sess.fds[fd as usize] {
            Some(f) => f,
            None => return -1,
        };
        let node_idx = of.node_idx;
        let offset = of.offset;
        let result = ramfs::write(node_idx, data, offset as usize);
        if result > 0 {
            if let Some(ref mut of_mut) = sess.fds[fd as usize] {
                of_mut.offset += result as u32;
            }
        }
        result
    }
}

pub fn do_mkdir(path: &str) -> i32 {
    ramfs::mkdir_path(path)
}

pub fn do_rmdir(path: &str) -> i32 {
    let child = ramfs::resolve(path);
    match child {
        Some(idx) => {
            let parent = ramfs::parent_of(idx);
            if parent < 0 {
                Status::InvalidArgs.to_raw() as i32
            } else if ramfs::remove(parent, idx) {
                0
            } else {
                Status::NotEmpty.to_raw() as i32
            }
        }
        None => Status::NotFound.to_raw() as i32,
    }
}

pub fn do_unlink(path: &str) -> i32 {
    let child = ramfs::resolve(path);
    match child {
        Some(idx) => {
            let parent = ramfs::parent_of(idx);
            if parent < 0 {
                Status::InvalidArgs.to_raw() as i32
            } else if ramfs::remove(parent, idx) {
                0
            } else {
                -1
            }
        }
        None => Status::NotFound.to_raw() as i32,
    }
}

pub fn do_readdir(session_idx: usize, fd: u32, buf: &mut [u8]) -> i32 {
    unsafe {
        let sess = match SESSIONS[session_idx].as_mut() {
            Some(s) => s,
            None => return -1,
        };
        let of = match &sess.fds[fd as usize] {
            Some(f) => f,
            None => return -1,
        };
        let node_idx = of.node_idx;
        let result = ramfs::readdir_names(node_idx, buf);
        if result > 0 {
            if let Some(ref mut of_mut) = sess.fds[fd as usize] {
                of_mut.offset += result as u32;
            }
        }
        result
    }
}

pub fn do_stat(path: &str) -> (i32, u64) {
    let clean = if path.starts_with('/') { &path[1..] } else { path };
    let node = ramfs::resolve(clean);
    match node {
        Some(idx) => {
            let size = ramfs::stat_size(idx);
            let ntype = ramfs::stat_type(idx);
            let type_val = match ntype {
                Some(ramfs::NodeType::File) => 1u64,
                Some(ramfs::NodeType::Directory) => 2u64,
                None => 0u64,
            };
            (size, type_val)
        }
        None => (-1, 0),
    }
}
