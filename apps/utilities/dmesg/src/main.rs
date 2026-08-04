//! `dmesg` — dump the kernel ring buffer to stdout.
//!
//! Mirrors the Linux/FreeBSD `dmesg` user-mode tool.  Repeatedly
//! invokes `SYSCALL_LOGBUF_READ` with the latest sequence number
//! returned by the previous call so we drain until the kernel has
//! nothing new to offer, then exit.

#![cfg_attr(not(feature = "host"), no_std)]
#![cfg_attr(not(feature = "host"), no_main)]

#[cfg(not(feature = "host"))]
extern crate libc;
#[cfg(not(feature = "host"))]
extern crate libcapsule;

#[cfg(feature = "host")]
fn main() {
    // host build: nothing to do (no kernel ring buffer here).
    eprintln!("dmesg: only meaningful on the capsule target");
}

#[cfg(not(feature = "host"))]
#[no_mangle]
pub fn main() -> i32 {
    use libcapsule::{SYSCALL_LOGBUF_READ, SYSCALL_WRITE};
    let mut seq: u64 = 0;
    let mut buf = [0u8; 1024];
    // 64 iterations cover the full 64KiB ring with 1KiB reads.
    for _ in 0..64 {
        let ret = libcapsule::syscall!(
            SYSCALL_LOGBUF_READ,
            seq as usize,
            buf.as_mut_ptr() as usize,
            buf.len(),
            0,
            0,
            0
        );
        let next_seq = (ret >> 32) as u64;
        let n = (ret & 0xFFFFFFFF) as usize;
        if n == 0 {
            break;
        }
        let _ = libcapsule::syscall!(
            SYSCALL_WRITE,
            1,
            buf.as_ptr() as usize,
            n,
            0,
            0,
            0
        );
        if next_seq == seq {
            break;
        }
        seq = next_seq;
    }
    0
}