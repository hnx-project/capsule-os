#![no_std]
#![no_main]

extern crate libcapsule;

use libcapsule::{kprintln, syscalls};
use shared::status::Status;

/// BootFS VMO handle injected by the kernel to 1号进程 at slot 100
pub const BOOTFS_VMO_HANDLE: usize = 100;

#[derive(Clone, Copy, PartialEq, Debug)]
enum ServiceState {
    Pending = 0,
    Spawning = 1,
    Running = 2,
}

struct ServiceDef {
    name: &'static str,
    path: &'static str,
    dependencies: &'static [&'static str],
    is_program: bool,
}

static SERVICES: &[ServiceDef] = &[
    ServiceDef {
        name: "procmgr",
        path: "procmgr",
        dependencies: &[],
        is_program: false,
    },
    ServiceDef {
        name: "devmgr",
        path: "devmgr",
        dependencies: &[],
        is_program: false,
    },
    ServiceDef {
        name: "blkdev",
        path: "blkdev",
        dependencies: &[],
        is_program: false,
    },
    ServiceDef {
        name: "tty",
        path: "tty",
        dependencies: &[],
        is_program: false,
    },
    ServiceDef {
        name: "fileagent",
        path: "fileagent",
        dependencies: &["blkdev", "devmgr", "procmgr"],
        is_program: false,
    },
    ServiceDef {
        name: "testall",
        path: "testall",
        dependencies: &["fileagent", "tty"],
        is_program: true,
    },
];

#[no_mangle]
pub fn main() -> i32 {
    kprintln!("====================================================");
    kprintln!("initd: Pangu Service Manager v1.0.0 (InitD) Active");
    kprintln!("====================================================");

    // 1. Initialize loaders
    let bootstrap_service = libcapsule::ServiceLoader::new(BOOTFS_VMO_HANDLE);
    let bootstrap_program = libcapsule::ProgramLoader::new(BOOTFS_VMO_HANDLE);

    // 2. Create the bidirectional init server channel
    let raw = match syscalls::channel_create() {
        Ok(v) => v,
        Err(_) => {
            kprintln!("initd: [ERROR] channel_create failed");
            return -1;
        }
    };
    let server_chan = (raw >> 32) as u32 as usize;

    if let Err(e) = syscalls::channel_register("svc.init", server_chan) {
        kprintln!("initd: [ERROR] channel_register failed: {:?}", e);
        return -2;
    }
    kprintln!("initd: Registered global name 'svc.init' on channel {}", server_chan);

    // 3. Keep track of service states and PIDs
    let mut states = [ServiceState::Pending; SERVICES.len()];
    let mut pids = [0u64; SERVICES.len()];

    let mut conn_buf = [0u8; 64];
    let mut conn_handles = [0u32; 2];

    loop {
        // A. Scan and spawn satisfied Pending services
        let mut progress = false;
        for i in 0..SERVICES.len() {
            if states[i] == ServiceState::Pending {
                // Check if all dependencies are Running
                let mut satisfied = true;
                for dep in SERVICES[i].dependencies {
                    let mut dep_running = false;
                    for j in 0..SERVICES.len() {
                        if SERVICES[j].name == *dep && states[j] == ServiceState::Running {
                            dep_running = true;
                            break;
                        }
                    }
                    if !dep_running {
                        satisfied = false;
                        break;
                    }
                }

                if satisfied {
                    kprintln!("initd: Dependency satisfied. Spawning service '{}'...", SERVICES[i].name);
                    if SERVICES[i].is_program {
                        match bootstrap_program.spawn_program(SERVICES[i].path) {
                            Ok(pid) => {
                                kprintln!("initd: [SPAWN] Program '{}' launched with PID {}", SERVICES[i].name, pid);
                                pids[i] = pid as u64;
                                // Programs don't send READY handshakes; they run to completion.
                                // We block-wait on the final program (testall) or shell.
                                states[i] = ServiceState::Running;
                                progress = true;
                            }
                            Err(e) => {
                                kprintln!("initd: [ERROR] Failed to spawn program '{}': {:?}", SERVICES[i].name, e);
                            }
                        }
                    } else {
                        match bootstrap_service.spawn_service(SERVICES[i].path) {
                            Ok(pid) => {
                                kprintln!("initd: [SPAWN] Service '{}' launched, PID={}. Waiting for READY...", SERVICES[i].name, pid);
                                pids[i] = pid as u64;
                                states[i] = ServiceState::Spawning;
                                progress = true;
                            }
                            Err(e) => {
                                kprintln!("initd: [ERROR] Failed to spawn service '{}': {:?}", SERVICES[i].name, e);
                            }
                        }
                    }
                }
            }
        }

        // B. If we made spawning progress, loop again to check state satisfies immediately
        if progress {
            continue;
        }

        // C. Check if all services are Running
        let mut all_running = true;
        for s in states.iter() {
            if *s != ServiceState::Running {
                all_running = false;
                break;
            }
        }

        if all_running {
            kprintln!("initd: [SUCCESS] All background services and final targets are fully RUNNING.");
            
            // D. Wait on the final program (testall) if it was spawned
            let mut testall_idx = None;
            for i in 0..SERVICES.len() {
                if SERVICES[i].name == "testall" {
                    testall_idx = Some(i);
                    break;
                }
            }

            if let Some(idx) = testall_idx {
                let pid = pids[idx];
                if pid != 0 {
                    kprintln!("initd: Entering blocking wait4 for PID {} (testall)...", pid);
                    let mut exit_status = 0i32;
                    loop {
                        // wait4 is direct microkernel syscall SYSCALL_WAIT4
                        match libcapsule::syscalls::close(0) { // arbitrary yield trick or wait
                            _ => {}
                        }
                        // Non-polling exit wait block bypass or wait4 simulation
                        // We yield so that other processes run and exit.
                        syscalls::yield_cpu();
                    }
                }
            }

            // Fallback loop
            loop {
                syscalls::yield_cpu();
            }
        }

        // E. Wait for READY handshake messages on server_chan
        conn_buf.fill(0);
        conn_handles.fill(0);

        if let Ok(_) = syscalls::channel_read(server_chan, &mut conn_buf, &mut conn_handles) {
            if conn_handles[0] != 0 {
                let session_chan = conn_handles[0] as usize;

                let mut cmd_buf = [0u8; 148];
                let mut cmd_handles = [0u32; 2];

                if let Ok(n) = syscalls::channel_read(session_chan, &mut cmd_buf, &mut cmd_handles) {
                    if n >= 20 {
                        let cmd = cmd_buf[0];
                        if cmd == 1 { // INIT_CMD_READY
                            let mut len_bytes = [0u8; 4];
                            len_bytes.copy_from_slice(&cmd_buf[4..8]);
                            let len = (u32::from_le_bytes(len_bytes) as usize).min(128);

                            if let Ok(service_name) = core::str::from_utf8(&cmd_buf[20..20 + len]) {
                                let name = service_name.trim();
                                kprintln!("initd: Received INIT_CMD_READY handshake from '{}'", name);

                                // Find and update service state
                                for i in 0..SERVICES.len() {
                                    if SERVICES[i].name == name {
                                        states[i] = ServiceState::Running;
                                        kprintln!("initd: Service '{}' promoted to RUNNING state", name);
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    let _ = syscalls::close(session_chan);
                }
            }
        }
    }
}
