#![no_std]
#![no_main]

extern crate libcapsule;
use libcapsule::kprintln;

/// The standard, stateless AArch64 entry trampoline for loader (PID 1).
/// Ohlink-linker binds this verbatim to the entry offset (0x10000 / 65536)
/// defined in xtask.toml.
#[cfg(target_arch = "aarch64")]
core::arch::global_asm!(
    r#"
.section .text
.global _start
_start:
    // Preserve argc (x0) and argv (x1)
    mov     x9,  x0
    mov     x10, x1

    // Align Stack Pointer to 16 bytes (AArch64 hard requirement)
    mov     x8,  sp
    and     x8,  x8, #~0xf
    mov     sp,  x8

    // Zero Frame Pointer and Link Register to terminate unwind stack
    mov     x29, #0
    mov     x30, #0

    // Restore args and jump to our user entry manager
    mov     x0,  x9
    mov     x1,  x10
    bl      _hnx_user_entry
"#
);

#[no_mangle]
pub unsafe extern "C" fn _hnx_user_entry() -> ! {
    // 1. Direct hardware-aligned entry: execute the loader's main function!
    let code = main();

    // Direct system exit syscall to the kernel
    libcapsule::syscall!(
        shared::syscall_nums::SYSCALL_EXIT,
        code as usize,
        0,
        0,
        0,
        0,
        0
    );
    loop {}
}

#[no_mangle]
pub fn main() -> i32 {
    // 2. Focus 100% on lighting up the console debug print!
    kprintln!("Loader: CapsuleOS Pangu Userboot Loader Active");

    loop {
        libcapsule::syscalls::yield_cpu();
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
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
