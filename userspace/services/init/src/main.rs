#![no_std]
#![no_main]

extern crate hnxlibc;

fn print(s: &str) {
    unsafe {
        hnxlibc::write(1, s.as_ptr(), s.len());
    }
}

fn println(s: &str) {
    print(s);
    print("\n");
}

fn print_hex(n: i32) {
    // Minimal 1-8 hex-digit printer for the negative `Status` code
    // returned by `exec` on failure.  Avoids dragging in a full
    // `core::fmt` formatter into a #![no_std] userspace program.
    let mut buf = [0u8; 9];
    let mut v: u32 = n as u32;
    let mut i: usize = 0;
    if v == 0 {
        buf[0] = b'0';
        i = 1;
    } else {
        while v > 0 && i < buf.len() {
            let digit = (v & 0xF) as u8;
            buf[i] = if digit < 10 {
                b'0' + digit
            } else {
                b'a' + digit - 10
            };
            v >>= 4;
            i += 1;
        }
        // Reverse the digit slice in place.
        let mut lo = 0usize;
        let mut hi = i - 1;
        while lo < hi {
            let t = buf[lo];
            buf[lo] = buf[hi];
            buf[hi] = t;
            lo += 1;
            hi -= 1;
        }
    }
    print("0x");
    print(core::str::from_utf8(&buf[..i]).unwrap_or("?"));
}

#[no_mangle]
pub fn main() -> i32 {
    println("init: starting...");
    println("init: handing off to osh");
    // `exec` is a *replacing* syscall: on success the kernel
    // marks the current thread Dead and `eret`s into the new
    // process, so this call never returns.  On failure it
    // returns a negative `Status` (a `shared::status::Status`
    // value cast to `i32`); the old behaviour silently dropped
    // the error code, exited the init process, and let the
    // kernel halt in `SCHED No runnable threads left` — which
    // is the *worst* possible failure mode for a boot service.
    //
    // If exec fails, surface the status to the UART and exit
    // with a non-zero code so the kernel at least logs a
    // distinct `Process exited with code N` line (vs. the
    // successful clean-exit `code 0` we use for "init ran to
    // completion but the exec target was missing").
    let rc = hnxlibc::exec("osh");
    if rc < 0 {
        println("init: exec(\"osh\") FAILED with status");
        print_hex(rc);
        println("");
        return 1;
    }
    // Unreachable on success: a successful exec never returns.
    0
}
