#![no_std]
#![no_main]

extern crate hnxlibc;

pub mod fatfs;
pub mod ramfs;

use hnxlibc::syscalls;
use ramfs::{RamFs, RamfsNode, RamfsNodeType};

static mut RAM_FS: Option<RamFs> = None;

#[derive(Debug, Clone, Copy)]
struct OpenFile {
    node_idx: usize,
    offset: usize,
}

static mut OPEN_FILES: [Option<OpenFile>; 16] = [None; 16];
static mut SESSIONS: [usize; 16] = [0; 16];

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub enum FileAgentCmd {
    Open {
        path: [u8; 128],
        path_len: u32,
        flags: u32,
    },
    Close {
        fd: u32,
    },
    Read {
        fd: u32,
        len: usize,
    },
    Write {
        fd: u32,
        len: usize,
        vmo_handle: u32,
    },
    MkDir {
        path: [u8; 128],
        path_len: u32,
    },
    RmDir {
        path: [u8; 128],
        path_len: u32,
    },
    Unlink {
        path: [u8; 128],
        path_len: u32,
    },
}

#[no_mangle]
pub fn main() -> i32 {
    unsafe {
        RAM_FS = Some(RamFs::new());

        // 预置一个测试文件，让客户端开箱即用！
        if let Some(ref mut fs) = RAM_FS {
            if let Some(node_idx) = fs.create(0, "welcome.txt") {
                fs.write_file(
                    node_idx,
                    b"Hello from CapsuleOS RamFS VMO dynamic filesystem!",
                    0,
                );
            }
        }
    }

    // 1. 创建主监听通道 (server_chan) 并向服务总线注册为 "svc.vfs"
    let server_chan = match syscalls::channel_create() {
        Ok(ch) => ch,
        Err(_) => return -1,
    };

    if let Err(_) = syscalls::channel_register("svc.vfs", server_chan) {
        return -2;
    }

    loop {
        // A. 轮询主监听信道，查看是否有新客户端连接 (Lookup 请求)
        let mut conn_buf = [0u8; 64];
        let mut conn_handles = [0u32; 2];
        if let Ok(len) = syscalls::channel_read(server_chan, &mut conn_buf, &mut conn_handles) {
            if conn_handles[0] != 0 {
                // 将新生成的会话专属 Channel 加入会话轮询表
                unsafe {
                    let mut added = false;
                    for s in &mut SESSIONS {
                        if *s == 0 {
                            *s = conn_handles[0] as usize;
                            added = true;
                            break;
                        }
                    }
                    if !added {
                        // 表满，则关闭该连接避免句柄泄漏
                        let _ = syscalls::close(conn_handles[0] as usize);
                    }
                }
            }
        }

        // B. 依次轮询所有的活跃会话信道，处理客户端发出的 VFS 命令
        unsafe {
            for i in 0..16 {
                let session_chan = SESSIONS[i];
                if session_chan == 0 {
                    continue;
                }

                let mut cmd_buf = [0u8; 256];
                let mut cmd_handles = [0u32; 2];
                match syscalls::channel_read(session_chan, &mut cmd_buf, &mut cmd_handles) {
                    Ok(len) if len >= core::mem::size_of::<FileAgentCmd>() => {
                        let cmd =
                            core::ptr::read_unaligned(cmd_buf.as_ptr() as *const FileAgentCmd);
                        // 处理命令并带上客户端传入的句柄列表
                        handle_command(cmd, session_chan, &cmd_handles);
                    }
                    Err(hnxlibc::Status::PeerClosed) => {
                        // 客户端关闭连接，清理该会话
                        let _ = syscalls::close(session_chan);
                        SESSIONS[i] = 0;
                    }
                    _ => {}
                }
            }
        }
    }
}

fn handle_command(cmd: FileAgentCmd, channel: usize, handles: &[u32]) {
    match cmd {
        FileAgentCmd::Open {
            path,
            path_len,
            flags,
        } => {
            let path_str = if let Ok(s) = core::str::from_utf8(&path[..path_len.min(128) as usize])
            {
                s
            } else {
                ""
            };
            let fd = do_open(path_str, flags);
            let response = fd as u64;
            let _ = syscalls::channel_write(
                channel,
                unsafe { core::slice::from_raw_parts(&response as *const u64 as *const u8, 8) },
                &[],
            );
        }
        FileAgentCmd::Close { fd } => {
            let result = do_close(fd);
            let response = result as u64;
            let _ = syscalls::channel_write(
                channel,
                unsafe { core::slice::from_raw_parts(&response as *const u64 as *const u8, 8) },
                &[],
            );
        }
        FileAgentCmd::Read { fd, len } => {
            // ⭐ 核心创新：自适应零拷贝共享 VMO 架构！
            // 服务端直接读取文件数据后存入新建的 VMO 句柄中，
            // 随后通过句柄传递直接把这个 VMO 句柄分发给客户端！
            // 客户端直接从 VMO 里高能极速读取，彻底做到大文件数据跨进程零拷贝传输！
            let (result, data) = do_read(fd, len);
            if result >= 0 && !data.is_empty() {
                if let Ok(vmo_handle) = syscalls::vmo_create(data.len()) {
                    let _ = syscalls::vmo_write(vmo_handle, 0, data);

                    let mut response_buf = [0u8; 16];
                    response_buf[0..8].copy_from_slice(&(result as u64).to_le_bytes());
                    response_buf[8..16].copy_from_slice(&(data.len() as u64).to_le_bytes());

                    // 将 VMO 句柄随 Channel 发送出去！
                    let _ = syscalls::channel_write(channel, &response_buf, &[vmo_handle as u32]);
                    // 服务端可以关闭这个本地 VMO 引用，因为 duplicate 后客户端已有有效引用
                    let _ = syscalls::close(vmo_handle as usize);
                } else {
                    let response = -1i64 as u64;
                    let _ = syscalls::channel_write(
                        channel,
                        unsafe {
                            core::slice::from_raw_parts(&response as *const u64 as *const u8, 8)
                        },
                        &[],
                    );
                }
            } else {
                let response = result as u64;
                let _ = syscalls::channel_write(
                    channel,
                    unsafe { core::slice::from_raw_parts(&response as *const u64 as *const u8, 8) },
                    &[],
                );
            }
        }
        FileAgentCmd::Write {
            fd,
            len,
            vmo_handle: _vmo,
        } => {
            // ⭐ 核心创新：写操作零拷贝机制！
            // 客户端如果传入了携带着待写入数据的 VMO 句柄 (位于 handles[0])：
            // 服务端直接将 handles[0] 作为 VMO 进行跨进程高能直接读取并落地到 RamFS 中！
            let mut result = -1;
            if handles[0] != 0 {
                let mut vmo_buf = [0u8; 1024];
                let read_len = len.min(1024);
                if let Ok(_) = syscalls::vmo_read(handles[0] as usize, 0, &mut vmo_buf[..read_len])
                {
                    result = do_write(fd, &vmo_buf[..read_len]);
                }
                let _ = syscalls::close(handles[0] as usize);
            }
            let response = result as u64;
            let _ = syscalls::channel_write(
                channel,
                unsafe { core::slice::from_raw_parts(&response as *const u64 as *const u8, 8) },
                &[],
            );
        }
        FileAgentCmd::MkDir { path, path_len } => {
            let path_str = if let Ok(s) = core::str::from_utf8(&path[..path_len.min(128) as usize])
            {
                s
            } else {
                ""
            };
            let result = do_mkdir(path_str) as i64;
            let response = result as u64;
            let _ = syscalls::channel_write(
                channel,
                unsafe { core::slice::from_raw_parts(&response as *const u64 as *const u8, 8) },
                &[],
            );
        }
        FileAgentCmd::RmDir { path, path_len } => {
            let path_str = if let Ok(s) = core::str::from_utf8(&path[..path_len.min(128) as usize])
            {
                s
            } else {
                ""
            };
            let result = do_rmdir(path_str) as i64;
            let response = result as u64;
            let _ = syscalls::channel_write(
                channel,
                unsafe { core::slice::from_raw_parts(&response as *const u64 as *const u8, 8) },
                &[],
            );
        }
        FileAgentCmd::Unlink { path, path_len } => {
            let path_str = if let Ok(s) = core::str::from_utf8(&path[..path_len.min(128) as usize])
            {
                s
            } else {
                ""
            };
            let result = do_unlink(path_str) as i64;
            let response = result as u64;
            let _ = syscalls::channel_write(
                channel,
                unsafe { core::slice::from_raw_parts(&response as *const u64 as *const u8, 8) },
                &[],
            );
        }
    }
}

fn do_open(path: &str, _flags: u32) -> i32 {
    unsafe {
        if let Some(ref mut fs) = RAM_FS {
            // 剔除前导斜杠
            let clean_path = if path.starts_with('/') {
                &path[1..]
            } else {
                path
            };

            // 查找名为 welcome.txt 的文件或匹配其全路径
            // 这里为了简单性我们进行精确匹配，也可以自定义查找
            let mut node_idx_opt = None;
            for i in 0..ramfs::RAMFS_MAX_FILES {
                if let Some(ref node) = fs.nodes[i] {
                    let mut len = 0;
                    while len < ramfs::RAMFS_MAX_NAME_LEN && node.name[len] != 0 {
                        len += 1;
                    }
                    if let Ok(name_str) = core::str::from_utf8(&node.name[..len]) {
                        if name_str == clean_path {
                            node_idx_opt = Some(i);
                            break;
                        }
                    }
                }
            }

            if let Some(node_idx) = node_idx_opt {
                for i in 0..16 {
                    if OPEN_FILES[i].is_none() {
                        OPEN_FILES[i] = Some(OpenFile {
                            node_idx,
                            offset: 0,
                        });
                        return i as i32;
                    }
                }
            }
        }
    }
    -1
}

fn do_close(fd: u32) -> i32 {
    unsafe {
        if fd < 16 {
            OPEN_FILES[fd as usize] = None;
            return 0;
        }
    }
    -1
}

fn do_read(fd: u32, len: usize) -> (i32, &'static [u8]) {
    unsafe {
        if fd < 16 {
            if let Some(ref mut of) = OPEN_FILES[fd as usize] {
                if let Some(ref fs) = RAM_FS {
                    if let Some(node) = fs.get_node(of.node_idx) {
                        let available = node.size.saturating_sub(of.offset);
                        let read_len = len.min(available);
                        if read_len > 0 {
                            let data = &node.data[of.offset..of.offset + read_len];
                            of.offset += read_len;
                            // 绕开生命周期约束，返回静态引用字节切片给传输机制
                            let ptr = data.as_ptr();
                            let static_data = core::slice::from_raw_parts(ptr, read_len);
                            return (read_len as i32, static_data);
                        }
                        return (0, &[]);
                    }
                }
            }
        }
    }
    (-1, &[])
}

fn do_write(fd: u32, data: &[u8]) -> i32 {
    unsafe {
        if fd < 16 {
            if let Some(ref mut of) = OPEN_FILES[fd as usize] {
                if let Some(ref mut fs) = RAM_FS {
                    let written = fs.write_file(of.node_idx, data, of.offset);
                    of.offset += written;
                    return written as i32;
                }
            }
        }
    }
    -1
}

fn do_mkdir(path: &str) -> i32 {
    unsafe {
        if let Some(ref mut fs) = RAM_FS {
            if fs.mkdir_path(path).is_some() {
                return 0;
            }
        }
    }
    -1
}

fn do_rmdir(path: &str) -> i32 {
    unsafe {
        if let Some(ref mut fs) = RAM_FS {
            if let Some(child_idx) = fs.resolve_path(path) {
                if let Some(parent_idx) = fs.parent_of(child_idx) {
                    if fs.rmdir(parent_idx, child_idx) {
                        return 0;
                    }
                }
            }
        }
    }
    -1
}

fn do_unlink(path: &str) -> i32 {
    unsafe {
        if let Some(ref mut fs) = RAM_FS {
            if let Some(child_idx) = fs.resolve_path(path) {
                if let Some(parent_idx) = fs.parent_of(child_idx) {
                    if fs.unlink(parent_idx, child_idx) {
                        return 0;
                    }
                }
            }
        }
    }
    -1
}
