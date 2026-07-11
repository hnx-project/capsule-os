#![no_std]
#![no_main]

extern crate hnxlibc;

use hnxlibc::syscalls;

/// Tracepoint helper used by `loader` to mark each user-mode statement
/// boundary while we diagnose the boot-chain stall after the first
/// `sys_spawn`.  Each `tp!("FOO")` writes the literal `FOO\n` to the
/// UART — no allocation, no syscall-table bloat, just enough visibility
/// to see exactly which loader line was the last to run before the
/// chain froze.  Strip the diagnostic and these calls when the chain
/// is healthy again.
macro_rules! tp {
    ($lit:expr) => {{
        let s = concat!($lit, "\n");
        let _ = hnxlibc::write(1, s.as_ptr(), s.len());
    }};
}

/// The EL0 **loader service** is CapsuleOS's `init`-level orchestrator:
/// brings up the rest of the EL0 service tier in order, waits for the
/// IPC service bus to settle, then exec's the next-stage bootstrap.
#[no_mangle]
pub fn main() -> i32 {
    tp!("T00: enter main");
    let msg = "Loader: bringing up EL0 services (devmgr + fileagent)\n";
    let _ = hnxlibc::write(1, msg.as_ptr(), msg.len());
    tp!("T01: wrote banner");

    // 1. devmgr
    tp!("T10: about to call syscalls::spawn(devmgr)");
    let spawn_devmgr_res = syscalls::spawn("devmgr", &[]);
    tp!("T11: returned from spawn(devmgr)");
    match spawn_devmgr_res {
        Ok(pid) => {
            tp!("T12: spawn(devmgr) Ok");
            let ack = "Loader: spawned devmgr (pid=";
            let _ = hnxlibc::write(1, ack.as_ptr(), ack.len());
            tp!("T13: wrote ack str");
            let digits = dec_digits(pid);
            let _ = hnxlibc::write(1, digits.as_ptr(), digits.len());
            tp!("T14: wrote digits");
            let tail = ")\n";
            let _ = hnxlibc::write(1, tail.as_ptr(), tail.len());
            tp!("T15: wrote tail");
        }
        Err(_) => {
            tp!("T1E: spawn(devmgr) Err");
            let _ = hnxlibc::write(1, b"Loader: devmgr spawn failed\n".as_ptr(), 28);
        }
    }
    tp!("T20: devmgr block done");

    // 2. fileagent
    tp!("T30: about to call syscalls::spawn(fileagent)");
    let spawn_fa_res = syscalls::spawn("fileagent", &[]);
    tp!("T31: returned from spawn(fileagent)");
    match spawn_fa_res {
        Ok(pid) => {
            tp!("T32: spawn(fileagent) Ok");
            let ack = "Loader: spawned fileagent (pid=";
            let _ = hnxlibc::write(1, ack.as_ptr(), ack.len());
            let digits = dec_digits(pid);
            let _ = hnxlibc::write(1, digits.as_ptr(), digits.len());
            let tail = ")\nLoader: waiting for svc.vfs registration...\n";
            let _ = hnxlibc::write(1, tail.as_ptr(), tail.len());
        }
        Err(_) => {
            tp!("T3E: spawn(fileagent) Err");
            let _ = hnxlibc::write(1, b"Loader: fileagent spawn failed\n".as_ptr(), 31);
        }
    }
    tp!("T40: fileagent block done");

    // 3. Poll for svc.vfs
    let service_name_str = "svc.vfs";
    let mut attempts: usize = 0;
    let max_attempts: usize = 200;
    tp!("T50: enter svc.vfs polling loop");
    loop {
        tp!("T51: top of loop iter");
        let res = syscalls::channel_lookup(service_name_str);
        match res {
            Ok(_) => {
                tp!("T52: svc.vfs Ok");
                let ack = "Loader: svc.vfs is up\n";
                let _ = hnxlibc::write(1, ack.as_ptr(), ack.len());
                break;
            }
            Err(_) => {
                attempts += 1;
                if attempts >= max_attempts {
                    let _ = hnxlibc::write(
                        1,
                        b"Loader: svc.vfs did not register, exec'ing init anyway\n".as_ptr(),
                        52,
                    );
                    break;
                }
                tp!("T53: yield");
                let _ = syscalls::yield_cpu();
                tp!("T54: post-yield");
            }
        }
    }
    tp!("T60: polling loop done");

    // 4. exec init
    tp!("T70: about to exec(init)");
    let _ = hnxlibc::exec("init");
    tp!("T99: post-exec (UNREACHABLE)");

    0
}

/// Render a small non-negative integer as its decimal ASCII digits
/// (no allocation).  Returns the buffer slice with valid bytes.
fn dec_digits(mut n: u64) -> [u8; 20] {
    if n == 0 {
        return [
            b'0', 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
    }
    let mut buf = [0u8; 20];
    let mut i = 0;
    while n > 0 {
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        i += 1;
    }
    let mut reversed = [0u8; 20];
    for j in 0..i {
        reversed[j] = buf[i - 1 - j];
    }
    reversed
}
