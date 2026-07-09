#![no_std]

pub mod syscalls;
pub use syscalls::*;
pub use shared::status::Status;

extern "Rust" {
    fn main() -> i32;
}

#[cfg(target_arch = "aarch64")]
core::arch::global_asm!(
    r#"
.section .text
.global _start
_start:
    // 1. Force SP alignment to 16 bytes by masking off low 4 bits
    mov     x0, sp
    and     x0, x0, #~0xf
    mov     sp, x0

    // 2. Zero FP and LR to terminate call-stack unwinding
    mov     x29, #0
    mov     x30, #0

    // 3. Jump to the common Rust-based entry runner
    bl      _hnx_user_entry

.global _hnx_exit_fallback
_hnx_exit_fallback:
    mov     x0, #0
    bl      exit
    b       _hnx_exit_fallback
"#
);

#[cfg(target_arch = "riscv64")]
core::arch::global_asm!(
    r#"
.section .text
.global _start
_start:
    // 1. Align SP on RV64 (must be 16-byte aligned as well)
    andi    sp, sp, -16

    // 2. Zero FP (s0) and RA (ra) to terminate unwinding
    mv      s0, zero
    mv      ra, zero

    // 3. Jump to Rust-based entry
    call    _hnx_user_entry

.global _hnx_exit_fallback
_hnx_exit_fallback:
    li      a0, 0
    call    exit
    j       _hnx_exit_fallback
"#
);

#[no_mangle]
pub unsafe extern "C" fn _hnx_user_entry() -> ! {
    let code = main();
    exit(code);
}

#[no_mangle]
pub extern "C" fn memcpy(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    unsafe {
        let mut i = 0;
        while i < n {
            *dest.add(i) = *src.add(i);
            i += 1;
        }
    }
    dest
}

#[no_mangle]
pub extern "C" fn memmove(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    unsafe {
        if src < dest as *const u8 {
            let mut i = n;
            while i > 0 {
                i -= 1;
                *dest.add(i) = *src.add(i);
            }
        } else {
            let mut i = 0;
            while i < n {
                *dest.add(i) = *src.add(i);
                i += 1;
            }
        }
    }
    dest
}

#[no_mangle]
pub extern "C" fn memcmp(s1: *const u8, s2: *const u8, n: usize) -> i32 {
    unsafe {
        let mut i = 0;
        while i < n {
            let a = *s1.add(i);
            let b = *s2.add(i);
            if a != b {
                return if a < b { -1 } else { 1 };
            }
            i += 1;
        }
    }
    0
}

#[no_mangle]
pub extern "C" fn memset(s: *mut u8, c: i32, n: usize) -> *mut u8 {
    unsafe {
        let mut i = 0;
        while i < n {
            *s.add(i) = c as u8;
            i += 1;
        }
    }
    s
}

#[no_mangle]
pub extern "C" fn putchar(c: u8) {
    let _ = write(1, &c as *const u8, 1);
}

#[no_mangle]
pub extern "C" fn getchar() -> Option<u8> {
    let mut c = 0u8;
    let n = read(0, &mut c as *mut u8, 1);
    if n > 0 {
        Some(c)
    } else {
        None
    }
}

// ----------------------------------------------------
// 🌟 跨进程 VFS 的本地虚拟描述符和 IPC 定义
// ----------------------------------------------------

#[derive(Debug, Clone, Copy)]
struct LibcFile {
    session_chan: usize,  // 通往 fileagent svc.vfs 的客户端专属通道句柄
    remote_fd: u32,       // fileagent 侧维护的打开文件描述符索引
}

static mut LIBC_FILES: [Option<LibcFile>; 16] = [None; 16];

#[derive(Debug, Clone, Copy)]
#[repr(C)]
enum FileAgentCmd {
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
}

#[no_mangle]
pub extern "C" fn open(path: *const u8, flags: i32, _mode: i32) -> i32 {
    if path.is_null() {
        return -1;
    }

    // 1. 解析传入的路径
    let mut len = 0;
    unsafe {
        while *path.add(len) != 0 && len < 127 {
            len += 1;
        }
    }
    
    let path_bytes = unsafe { core::slice::from_raw_parts(path, len) };

    // 2. Lookup 建立到服务端 "svc.vfs" 的对偶通信信道
    let session_chan = match syscalls::channel_lookup("svc.vfs") {
        Ok(ch) => ch,
        Err(_) => return -1,
    };

    // 3. 构建 Open 命令，以进程间绝对安全的方式传输
    let mut cmd = FileAgentCmd::Open {
        path: [0u8; 128],
        path_len: len as u32,
        flags: flags as u32,
    };
    if let FileAgentCmd::Open { ref mut path, .. } = cmd {
        path[..len].copy_from_slice(path_bytes);
    }

    // 4. 将 Open 请求通过专属信道推送给服务端
    let cmd_slice = unsafe {
        core::slice::from_raw_parts(&cmd as *const FileAgentCmd as *const u8, core::mem::size_of::<FileAgentCmd>())
    };
    if let Err(_) = syscalls::channel_write(session_chan, cmd_slice, &[]) {
        let _ = syscalls::close(session_chan);
        return -1;
    }

    // 5. 等待服务器回执
    let mut resp_buf = [0u8; 16];
    let mut resp_handles = [0u32; 2];
    match syscalls::channel_read(session_chan, &mut resp_buf, &mut resp_handles) {
        Ok(read_len) if read_len >= 8 => {
            let remote_fd = unsafe { core::ptr::read_unaligned(resp_buf.as_ptr() as *const i64) } as i32;
            if remote_fd < 0 {
                let _ = syscalls::close(session_chan);
                return -1;
            }

            // 6. 分配本地客户端虚拟 FD 并绑定
            unsafe {
                for i in 3..16 {
                    if LIBC_FILES[i].is_none() {
                        LIBC_FILES[i] = Some(LibcFile {
                            session_chan,
                            remote_fd: remote_fd as u32,
                        });
                        return i as i32;
                    }
                }
            }
            // 本地 FD 表满，清理
            let _ = syscalls::close(session_chan);
            -1
        }
        _ => {
            let _ = syscalls::close(session_chan);
            -1
        }
    }
}

#[no_mangle]
pub extern "C" fn read(fd: i32, buf: *mut u8, count: usize) -> isize {
    if fd < 3 {
        // 标准 I/O 回退至底层的物理串口直接系统调用
        return syscall!(SYSCALL_READ, fd as usize, buf as usize, count, 0, 0, 0) as isize;
    }

    unsafe {
        if fd >= 16 || LIBC_FILES[fd as usize].is_none() {
            return -1;
        }

        let file = LIBC_FILES[fd as usize].unwrap();

        // 1. 构建 Read 请求并发送
        let cmd = FileAgentCmd::Read {
            fd: file.remote_fd,
            len: count,
        };
        let cmd_slice = core::slice::from_raw_parts(
            &cmd as *const FileAgentCmd as *const u8,
            core::mem::size_of::<FileAgentCmd>(),
        );

        if let Err(_) = syscalls::channel_write(file.session_chan, cmd_slice, &[]) {
            return -1;
        }

        // 2. 阻塞接收，带 VMO 句柄传递
        let mut resp_buf = [0u8; 16];
        let mut resp_handles = [0u32; 2];
        match syscalls::channel_read(file.session_chan, &mut resp_buf, &mut resp_handles) {
            Ok(read_len) if read_len >= 16 => {
                let result = core::ptr::read_unaligned(resp_buf.as_ptr() as *const i64) as isize;
                let data_len = core::ptr::read_unaligned(resp_buf[8..16].as_ptr() as *const u64) as usize;

                if result >= 0 && resp_handles[0] != 0 && data_len > 0 {
                    // ⭐ 零拷贝读取：从接收到的 VMO 句柄直接读取数据到客户端的 buf 中！
                    let target_slice = core::slice::from_raw_parts_mut(buf, count.min(data_len));
                    if let Ok(_) = syscalls::vmo_read(resp_handles[0] as usize, 0, target_slice) {
                        let _ = syscalls::close(resp_handles[0] as usize);
                        return result;
                    }
                    let _ = syscalls::close(resp_handles[0] as usize);
                }
                result
            }
            Ok(read_len) if read_len >= 8 => {
                core::ptr::read_unaligned(resp_buf.as_ptr() as *const i64) as isize
            }
            _ => -1,
        }
    }
}

#[no_mangle]
pub extern "C" fn write(fd: i32, buf: *const u8, count: usize) -> isize {
    if fd < 3 {
        // 标准 I/O 回退至底层的串口直接系统调用
        return syscall!(SYSCALL_WRITE, fd as usize, buf as usize, count, 0, 0, 0) as isize;
    }

    unsafe {
        if fd >= 16 || LIBC_FILES[fd as usize].is_none() {
            return -1;
        }

        let file = LIBC_FILES[fd as usize].unwrap();

        // ⭐ 零拷贝写入：客户端创建一个临时 VMO，并填充待写入的数据
        let vmo_handle = match syscalls::vmo_create(count) {
            Ok(h) => h,
            Err(_) => return -1,
        };

        let src_slice = core::slice::from_raw_parts(buf, count);
        if let Err(_) = syscalls::vmo_write(vmo_handle, 0, src_slice) {
            let _ = syscalls::close(vmo_handle as usize);
            return -1;
        }

        // 1. 构建 Write 命令并将临时 VMO 句柄通过管道发送出去！
        let cmd = FileAgentCmd::Write {
            fd: file.remote_fd,
            len: count,
            vmo_handle: vmo_handle as u32,
        };

        let cmd_slice = core::slice::from_raw_parts(
            &cmd as *const FileAgentCmd as *const u8,
            core::mem::size_of::<FileAgentCmd>(),
        );

        if let Err(_) = syscalls::channel_write(file.session_chan, cmd_slice, &[vmo_handle as u32]) {
            let _ = syscalls::close(vmo_handle as usize);
            return -1;
        }

        // 客户端在 duplicate 传递后即可关闭本地 VMO 引用，由服务端独占读取
        let _ = syscalls::close(vmo_handle as usize);

        // 2. 接收服务端写入结果
        let mut resp_buf = [0u8; 16];
        let mut resp_handles = [0u32; 2];
        match syscalls::channel_read(file.session_chan, &mut resp_buf, &mut resp_handles) {
            Ok(read_len) if read_len >= 8 => {
                core::ptr::read_unaligned(resp_buf.as_ptr() as *const i64) as isize
            }
            _ => -1,
        }
    }
}

#[no_mangle]
pub extern "C" fn close(fd: i32) -> i32 {
    if fd < 3 {
        return syscall!(SYSCALL_CLOSE, fd as usize, 0, 0, 0, 0, 0) as i32;
    }

    unsafe {
        if fd >= 16 || LIBC_FILES[fd as usize].is_none() {
            return -1;
        }

        let file = LIBC_FILES[fd as usize].unwrap();

        // 1. 通知服务端关闭该文件描述符
        let cmd = FileAgentCmd::Close { fd: file.remote_fd };
        let cmd_slice = core::slice::from_raw_parts(
            &cmd as *const FileAgentCmd as *const u8,
            core::mem::size_of::<FileAgentCmd>(),
        );

        let _ = syscalls::channel_write(file.session_chan, cmd_slice, &[]);

        // 2. 阻塞接收关闭响应
        let mut resp_buf = [0u8; 16];
        let mut resp_handles = [0u32; 2];
        let _ = syscalls::channel_read(file.session_chan, &mut resp_buf, &mut resp_handles);

        // 3. 彻底释放客户端专属通道句柄并清除本地映射
        let _ = syscalls::close(file.session_chan);
        LIBC_FILES[fd as usize] = None;
        0
    }
}

#[no_mangle]
pub extern "C" fn exit(status: i32) -> ! {
    syscall!(SYSCALL_EXIT, status as usize, 0, 0, 0, 0, 0);
    loop {}
}

#[no_mangle]
pub extern "C" fn nanosleep(_req: *const u8, _rem: *mut u8) -> i32 {
    0
}

#[no_mangle]
pub extern "C" fn getpid() -> i32 {
    1
}

fn print(s: &str) {
    write(1, s.as_ptr(), s.len());
}

#[no_mangle]
pub extern "C" fn exec(name: &str) -> i32 {
    if name.is_empty() {
        return -1;
    }
    syscalls::exec_impl(name) as i32
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
