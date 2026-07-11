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
    println("init: cat test done; entering yield loop (osh exec removed for kernel audit)");
    // We intentionally do NOT execve("osh") here yet.  The boot chain
    // currently halts at the loader (EL0-FAULT EC=0x24 during
    // `syscalls::spawn("devmgr")`) so fileagent is never brought up,
    // which means the `cat welcome.txt` smoke test above always fails
    // with `init: cat FAILED, status=`.  Before re-enabling the osh
    // hand-off we want to:
    //   1. Fix the loader's spawn-time data abort (FAR=0x92003d68) so
    //      fileagent actually launches and `svc.vfs` gets registered.
    //   2. Audit the kernel for stubs / dead code / missing paths.
    //   3. Tighten the EL0 -> kernel contract on the way through.
    // Until then, yield forever so the kernel stays alive and we can
    // observe scheduler / IPC / fault behaviour from the log.
    loop {
        hnxlibc::yield_cpu();
    }
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
