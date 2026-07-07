#![no_std]
#![no_main]

extern crate hnxlibc;

use hnxlibc::syscalls;

const FILE_AGENT_CHANNEL: u32 = 100;

#[derive(Debug, Clone, Copy)]
pub enum FileAgentCmd {
    Open { path: &'static str, flags: u32 },
    Close { fd: u32 },
    Read { fd: u32, len: usize },
    Write { fd: u32, data: &'static [u8] },
    Seek { fd: u32, offset: i64, whence: i32 },
    MkDir { path: &'static str },
    RmDir { path: &'static str },
    Unlink { path: &'static str },
    Readdir { fd: u32 },
}

static mut FILE_AGENT_RUNNING: bool = false;

#[no_mangle]
pub extern "C" fn main() -> i32 {
    unsafe { FILE_AGENT_RUNNING = true; }

    let channel = match syscalls::channel_create() {
        Ok(ch) => ch,
        Err(e) => {
            return -1;
        }
    };

    loop {
        let mut buf = [0u8; 256];
        let mut handles = [0u32; 4];

        match syscalls::channel_read(channel, &mut buf, &mut handles) {
            Ok(len) if len > 0 => {
                let cmd = unsafe { core::ptr::read_unaligned(buf.as_ptr() as *const FileAgentCmd) };
                handle_command(cmd, channel);
            }
            _ => {
                continue;
            }
        }
    }
}

fn handle_command(cmd: FileAgentCmd, channel: u32) {
    match cmd {
        FileAgentCmd::Open { path, flags } => {
            let fd = do_open(path, flags);
            let response = fd as u64;
            let _ = syscalls::channel_write(channel, unsafe {
                core::slice::from_raw_parts(&response as *const u64 as *const u8, 8)
            }, &[]);
        }
        FileAgentCmd::Close { fd } => {
            let result = do_close(fd);
            let response = result as u64;
            let _ = syscalls::channel_write(channel, unsafe {
                core::slice::from_raw_parts(&response as *const u64 as *const u8, 8)
            }, &[]);
        }
        FileAgentCmd::Read { fd, len } => {
            let (result, data) = do_read(fd, len);
            let mut buf = [0u8; 512];
            buf[0..8].copy_from_slice(&(result as u64).to_le_bytes());
            if !data.is_empty() {
                let copy_len = data.len().min(504);
                buf[8..8+copy_len].copy_from_slice(&data[..copy_len]);
            }
            let _ = syscalls::channel_write(channel, &buf, &[]);
        }
        FileAgentCmd::Write { fd, data } => {
            let result = do_write(fd, data);
            let response = result as u64;
            let _ = syscalls::channel_write(channel, unsafe {
                core::slice::from_raw_parts(&response as *const u64 as *const u8, 8)
            }, &[]);
        }
        FileAgentCmd::Seek { fd, offset, whence } => {
            let result = do_seek(fd, offset, whence);
            let response = result as u64;
            let _ = syscalls::channel_write(channel, unsafe {
                core::slice::from_raw_parts(&response as *const u64 as *const u8, 8)
            }, &[]);
        }
        FileAgentCmd::MkDir { path } => {
            let result = do_mkdir(path);
            let response = result as u64;
            let _ = syscalls::channel_write(channel, unsafe {
                core::slice::from_raw_parts(&response as *const u64 as *const u8, 8)
            }, &[]);
        }
        FileAgentCmd::RmDir { path } => {
            let result = do_rmdir(path);
            let response = result as u64;
            let _ = syscalls::channel_write(channel, unsafe {
                core::slice::from_raw_parts(&response as *const u64 as *const u8, 8)
            }, &[]);
        }
        FileAgentCmd::Unlink { path } => {
            let result = do_unlink(path);
            let response = result as u64;
            let _ = syscalls::channel_write(channel, unsafe {
                core::slice::from_raw_parts(&response as *const u64 as *const u8, 8)
            }, &[]);
        }
        FileAgentCmd::Readdir { fd } => {
            let mut buf = [0u8; 256];
            let result = do_readdir(fd, &mut buf);
            let mut response_buf = [0u8; 264];
            response_buf[0..8].copy_from_slice(&(result as u64).to_le_bytes());
            let copy_len = result.min(256);
            response_buf[8..8+copy_len].copy_from_slice(&buf[..copy_len]);
            let _ = syscalls::channel_write(channel, &response_buf, &[]);
        }
    }
}

fn do_open(path: &str, _flags: u32) -> i32 {
    -1
}

fn do_close(_fd: u32) -> i32 {
    0
}

fn do_read(_fd: u32, _len: usize) -> (i32, &'static [u8]) {
    (0, &[])
}

fn do_write(_fd: u32, _data: &[u8]) -> i32 {
    0
}

fn do_seek(_fd: u32, _offset: i64, _whence: i32) -> i32 {
    0
}

fn do_mkdir(_path: &str) -> i32 {
    -1
}

fn do_rmdir(_path: &str) -> i32 {
    -1
}

fn do_unlink(_path: &str) -> i32 {
    -1
}

fn do_readdir(_fd: u32, _buf: &mut [u8]) -> usize {
    0
}
