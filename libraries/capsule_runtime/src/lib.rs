#![no_std]

extern crate capsule as libcapsule;

#[cfg(target_arch = "aarch64")]
core::arch::global_asm!(
    r#"
.section .text
.global _start
_start:
    mov     x9,  x0
    mov     x10, x1

    mov     x8,  sp
    and     x8,  x8, #~0xf
    mov     sp,  x8

    mov     x29, #0
    mov     x30, #0

    mov     x0,  x9
    mov     x1,  x10
    bl      _hnx_user_entry
"#
);

extern "Rust" {
    fn main() -> i32;
}

#[no_mangle]
pub unsafe extern "C" fn _hnx_user_entry() -> ! {
    let code = main();

    libcapsule::syscall!(libcapsule::SYSCALL_EXIT, code as usize, 0, 0, 0, 0, 0);
    loop {}
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
