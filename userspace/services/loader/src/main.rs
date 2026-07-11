#![no_std]
#![no_main]

extern crate hnxlibc;

use hnxlibc::syscalls;

/// The EL0 **loader service** is CapsuleOS's `init`-level orchestrator: it
/// brings up the rest of the EL0 service tier in order, waits for the
/// IPC service bus to settle, and then exec's the next-stage bootstrap
/// (`init`, which in turn hands off to `osh`).
///
/// Boot chain (managed by `loader`):
///   1. Spawn `devmgr`      (cosmetic EL0 service; prints a "running" banner)
///   2. Spawn `fileagent`   (registers `svc.vfs`, the IPC endpoint that
///                            every hnxlibc::open / read / write call resolves)
///   3. Spin-poll `channel_lookup("svc.vfs")`, `yield_cpu`-ing between
///      attempts, until fileagent finishes registering.
///   4. exec("init") which exec("osh").
///
/// `devmgr` and `fileagent` are spawned — not `exec`'d — so loader keeps
/// running and can poll the registry between them.  Each `sys_spawn`
/// returns a fresh pid; we ignore them here because we never talk to the
/// children again (they run to completion on their own time slice).
#[no_mangle]
pub fn main() -> i32 {
    let msg = "Loader: bringing up EL0 services (devmgr + fileagent)\n";
    let _ = hnxlibc::write(1, msg.as_ptr(), msg.len());

    // 1. devmgr — purely cosmetic; a no-op service that demonstrates the
    //    EL0 service pattern.
    match syscalls::spawn("devmgr", &[]) {
        Ok(pid) => {
            let ack = "Loader: spawned devmgr (pid=";
            let _ = hnxlibc::write(1, ack.as_ptr(), ack.len());
            // Print pid as raw ASCII bytes.
            let digits = dec_digits(pid);
            let _ = hnxlibc::write(1, digits.as_ptr(), digits.len());
            let tail = ")\n";
            let _ = hnxlibc::write(1, tail.as_ptr(), tail.len());
        }
        Err(_) => {
            let _ = hnxlibc::write(1, b"Loader: devmgr spawn failed\n".as_ptr(), 28);
        }
    }

    // 2. fileagent — the real deal; this is the only process that owns
    //    `svc.vfs`, so hnxlibc open/read/write hang off it.
    match syscalls::spawn("fileagent", &[]) {
        Ok(pid) => {
            let ack = "Loader: spawned fileagent (pid=";
            let _ = hnxlibc::write(1, ack.as_ptr(), ack.len());
            let digits = dec_digits(pid);
            let _ = hnxlibc::write(1, digits.as_ptr(), digits.len());
            let tail = ")\nLoader: waiting for svc.vfs registration...\n";
            let _ = hnxlibc::write(1, tail.as_ptr(), tail.len());
        }
        Err(_) => {
            let _ = hnxlibc::write(1, b"Loader: fileagent spawn failed\n".as_ptr(), 31);
        }
    }

    // 3. Spin-poll `svc.vfs` registration.  Each iteration gives the
    //    scheduler a chance to run fileagent's main loop (channel_create
    //    → channel_register) without us having to introduce a sleep
    //    syscall.  Bail out after 200 failed attempts (~3 seconds of
    //    wall time at 16 ms each) so a buggy fileagent doesn't wedge
    //    the boot.
    let service_name_str = "svc.vfs";
    let mut attempts: usize = 0;
    let max_attempts: usize = 200;
    loop {
        let res = syscalls::channel_lookup(service_name_str);
        match res {
            Ok(_) => {
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
                let _ = syscalls::yield_cpu();
            }
        }
    }

    // 4. Hand off to init (which will in turn exec osh).  We treat this
    //    as a one-way transition — sys_exec replaces us — so we don't
    //    need to do anything afterwards.
    let _ = hnxlibc::exec("init");

    // Unreachable, but kept to satisfy `fn main() -> i32`.
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
    // Reverse in place so digits read left-to-right.
    let mut reversed = [0u8; 20];
    for j in 0..i {
        reversed[j] = buf[i - 1 - j];
    }
    reversed
}
