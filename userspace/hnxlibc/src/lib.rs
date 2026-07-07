#![no_std]

pub mod syscalls;
pub use syscalls::*;

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

#[no_mangle]
pub extern "C" fn write(fd: i32, buf: *const u8, count: usize) -> isize {
    syscall!(SYSCALL_WRITE, fd as usize, buf as usize, count, 0, 0, 0) as isize
}

#[no_mangle]
pub extern "C" fn read(fd: i32, buf: *mut u8, count: usize) -> isize {
    syscall!(SYSCALL_READ, fd as usize, buf as usize, count, 0, 0, 0) as isize
}

#[no_mangle]
pub extern "C" fn exit(status: i32) -> ! {
    syscall!(SYSCALL_EXIT, status as usize, 0, 0, 0, 0, 0);
    loop {}
}

#[no_mangle]
pub extern "C" fn open(path: *const u8, flags: i32, _mode: i32) -> i32 {
    syscall!(SYSCALL_OPEN, path as usize, flags as usize, 0, 0, 0, 0) as i32
}

#[no_mangle]
pub extern "C" fn close(fd: i32) -> i32 {
    syscall!(SYSCALL_CLOSE, fd as usize, 0, 0, 0, 0, 0) as i32
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
    unsafe {
        self::write(1, s.as_ptr(), s.len());
    }
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
