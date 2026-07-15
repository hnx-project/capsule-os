#![no_std]
#![no_main]

extern crate hnxlibc;
extern crate hnxstd;

mod bootfs;
mod log;

use bootfs::BootFsLoader;
use hnxlibc::syscalls;
use hnxstd::Vmo;
use log::Logger;

#[no_mangle]
pub fn main() -> i32 {
    Logger::write(
        "Loader: bringing up EL0 services (devmgr, fileagent, procmgr) via Userboot VMO slices!\n",
    );

    // Strictly resolve raw handle on main stack to guarantee handle 100 lives until main exits!
    let root_vmo = unsafe { Vmo::from_raw_handle(100) };
    let loader = BootFsLoader::new(&root_vmo);

    // Stage 1: Spawn DevMgr (Device Manager)
    let devmgr_res = loader.load_and_spawn("system/bin/devmgr");
    Logger::print_spawn_status("devmgr", devmgr_res);

    // Stage 2: Spawn FileAgent (System VFS backend)
    let fa_res = loader.load_and_spawn("system/bin/fileagent");
    Logger::print_spawn_status("fileagent", fa_res);

    // Stage 3: Spawn ProcMgr (Process Manager)
    let procmgr_res = loader.load_and_spawn("system/bin/procmgr");
    Logger::print_spawn_status("procmgr", procmgr_res);

    // Stage 4: Polling Wait for File System Service registration
    let mut attempts = 0;
    const MAX_ATTEMPTS: usize = 200;
    while syscalls::channel_lookup("svc.vfs").is_err() {
        attempts += 1;
        if attempts >= MAX_ATTEMPTS {
            Logger::write("Loader: svc.vfs did not register, exec'ing osh anyway\n");
            break;
        }
        let _ = syscalls::yield_cpu();
    }

    // Stage 5: Wait for Process Manager registration
    let mut p_attempts = 0;
    while syscalls::channel_lookup("svc.procmgr").is_err() {
        p_attempts += 1;
        if p_attempts >= MAX_ATTEMPTS {
            Logger::write("Loader: svc.procmgr did not register, starting osh anyway\n");
            break;
        }
        let _ = syscalls::yield_cpu();
    }

    // Stage 6: Spawn user shell
    Logger::write("Loader: launching user shell osh\n");
    match loader.load_and_spawn("system/bin/osh") {
        Ok(pid) => {
            Logger::write("Loader: osh launched successfully, entering idle loop\n");
            let _ = pid;
        }
        Err(e) => {
            Logger::write("Loader: failed to spawn osh\n");
            let _ = e;
        }
    }

    loop {
        let _ = syscalls::yield_cpu();
    }
}
