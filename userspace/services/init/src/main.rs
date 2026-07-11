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

/// Stream the contents of `path` to stdout, one byte at a time.
/// Returns the number of bytes copied on success, or a negative
/// hnxlibc status on failure.  Mirrors `cat(1)` from coreutils.
fn cat_file(path: &str) -> i32 {
    let fd = hnxlibc::open_str(path, 0, 0);
    if fd < 0 {
        return fd;
    }
    let mut buf = [0u8; 1024];
    let mut total: i32 = 0;
    loop {
        let n = hnxlibc::read(fd, buf.as_mut_ptr(), buf.len()) as i32;
        if n < 0 {
            let _ = hnxlibc::close(fd);
            return n;
        }
        if n == 0 {
            break;
        }
        // Stream to stdout byte-by-byte (no offset_of / hwrite in
        // hnxlibc yet).  Slow but correct; once `hnxlibc::write` is
        // extended to honour any fd we can switch to bulk.
        for &b in &buf[..n as usize] {
            print(core::str::from_utf8(&[b]).unwrap_or("?"));
        }
        total += n;
    }
    let _ = hnxlibc::close(fd);
    total
}

#[no_mangle]
pub fn main() -> i32 {
    println("init: starting...");
    println("init: testing hnxlibc::open/read/close (cat welcome.txt)");
    let n = cat_file("welcome.txt");
    if n > 0 {
        println("");
        println("init: cat read");
        print_dec(n as u32);
        println(" bytes from welcome.txt");
    } else {
        println("init: cat FAILED, status=");
        print_hex(n);
        println("");
    }
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
    // Use the execve variant (not plain exec) so we also
    // exercise the `sys_execve` -> kernel argv copy ->
    // user stack -> `_hnx_user_entry` handoff end-to-end.
    // argv[0] is conventionally the program name, argv[1] is
    // an arbitrary marker that should appear verbatim in the
    // osh boot log; if it does not, the argv path is broken.
    let argv: [&[u8]; 2] = [b"osh" as &[u8], b"init-was-here" as &[u8]];
    let rc = hnxlibc::execve("osh", &argv);
    if rc < 0 {
        println("init: execve(\"osh\") FAILED with status");
        print_hex(rc);
        println("");
        return 1;
    }
    // Unreachable on success: a successful execve never returns.
    0
}

/// Minimal decimal printer (mirrors `print_hex` but for u32 in
/// base 10).  Used by `cat_file` to log the byte count without
/// dragging in a full `core::fmt` formatter.
fn print_dec(mut n: u32) {
    if n == 0 {
        print("0");
        return;
    }
    let mut buf = [0u8; 12];
    let mut i: usize = 0;
    while n > 0 {
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        i += 1;
    }
    let mut lo: usize = 0;
    let mut hi = i - 1;
    while lo < hi {
        let t = buf[lo];
        buf[lo] = buf[hi];
        buf[hi] = t;
        lo += 1;
        hi -= 1;
    }
    print(core::str::from_utf8(&buf[..i]).unwrap_or("?"));
}
