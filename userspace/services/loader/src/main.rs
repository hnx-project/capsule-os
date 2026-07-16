#![no_std]
#![no_main]

extern crate libstd;

mod log;

use libstd::os::capsule::{Channel, Command};
use log::Logger;

#[no_mangle]
pub fn main() -> i32 {
    libstd::io::print(
        "Loader: bringing up EL0 services (devmgr, fileagent, procmgr) via ServiceLauncher!\n",
    );

    // Stage 1: Spawn DevMgr (Device Manager)
    let devmgr_res = Command::new("devmgr", "system/bin/devmgr").spawn();
    Logger::print_spawn_status("devmgr", devmgr_res);

    if let Ok(pid) = devmgr_res {
        // Wait for devmgr to complete its launch run or exit normally.
        // Doing wait4 prevents EL0-FAULT when devmgr exits asynchronously,
        // and safely reaps its process zombie context!
        let mut status = 0i32;
        while let Err(shared::status::Status::TryAgain) =
            libcapsule::syscalls::wait4(pid as i64, &mut status as *mut i32, 0)
        {
            libstd::thread::yield_now();
        }
    }
    libstd::io::print("ONLY TEST\n");

    // Stage 2: Spawn FileAgent (System VFS backend)
    let fa_res = Command::new("fileagent", "system/bin/fileagent").spawn();
    Logger::print_spawn_status("fileagent", fa_res);

    // Stage 3: Spawn ProcMgr (Process Manager)
    let procmgr_res = Command::new("procmgr", "system/bin/procmgr").spawn();
    Logger::print_spawn_status("procmgr", procmgr_res);

    // Stage 4: Polling Wait for File System Service registration
    let mut attempts = 0;
    const MAX_ATTEMPTS: usize = 200;
    while Channel::lookup("svc.vfs").is_err() {
        attempts += 1;
        if attempts >= MAX_ATTEMPTS {
            libstd::io::print("Loader: svc.vfs did not register, exec'ing osh anyway\n");
            break;
        }
        libstd::thread::yield_now();
    }

    // Stage 5: Wait for Process Manager registration
    let mut p_attempts = 0;
    while Channel::lookup("svc.procmgr").is_err() {
        p_attempts += 1;
        if p_attempts >= MAX_ATTEMPTS {
            libstd::io::print("Loader: svc.procmgr did not register, starting osh anyway\n");
            break;
        }
        libstd::thread::yield_now();
    }

    // Stage 6: Spawn user shell
    libstd::io::print("Loader: launching user shell osh\n");
    match Command::new("osh", "system/bin/osh").spawn() {
        Ok(pid) => {
            libstd::io::print("Loader: osh launched successfully, entering idle loop\n");
            let _ = pid;
        }
        Err(e) => {
            libstd::io::print("Loader: failed to spawn osh\n");
            let _ = e;
        }
    }

    loop {
        libstd::thread::yield_now();
    }
}
