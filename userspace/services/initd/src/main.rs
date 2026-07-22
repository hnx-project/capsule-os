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
        // A. Sweep the DAG to spawn any satisfied Pending services
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
                    kprintln!("initd: Dependency satisfied. Spawning '{}'...", SERVICES[i].name);
                    if SERVICES[i].is_program {
                        match bootstrap_program.spawn_program(SERVICES[i].path) {
                            Ok(pid) => {
                                kprintln!("initd: [SPAWN] Program '{}' launched with PID {}", SERVICES[i].name, pid);
                                pids[i] = pid as u64;
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

        // B. If we spawned a new service, loop immediately to re-check satisfied dependencies
        if progress {
            continue;
        }

        // C. Check if any service is in the Spawning state
        let mut anyone_spawning = false;
        for s in states.iter() {
            if *s == ServiceState::Spawning {
                anyone_spawning = true;
                break;
            }
        }

        if anyone_spawning {
            // We MUST block-read on server_chan to process incoming READY handshakes
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

                                    // Match name and promote state
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
            continue;
        }

        // D. Steady-State: All background services are Running.
        // We poll wait4 non-blocking to check if any child has exited or crashed.
        let mut exit_status = 0i32;
        let ret = libcapsule::syscall!(
            shared::syscall_nums::SYSCALL_WAIT4,
            !0usize, // -1: Wait for any child of the caller
            &mut exit_status as *mut i32 as usize,
            0,
            0,
            0,
            0
        ) as isize;

        if ret > 0 {
            let reaped_pid = ret as u64;
            // Find which service/program had this PID
            let mut found_idx = None;
            for i in 0..SERVICES.len() {
                if pids[i] == reaped_pid {
                    found_idx = Some(i);
                    break;
                }
            }

            if let Some(idx) = found_idx {
                let s_name = SERVICES[idx].name;
                if SERVICES[idx].is_program {
                    kprintln!("initd: [INFO] Program '{}' (PID {}) has completed. Exit code: {}.", s_name, reaped_pid, exit_status);
                } else {
                    kprintln!("initd: [1;31m[CRASH] Service '{}' (PID {}) has terminated with exit code {}![0m", s_name, reaped_pid, exit_status);
                    kprintln!("initd: [1;32m[AUTO-HEALING] Resetting dependency graph to restart '{}'...[0m", s_name);
                    
                    // Reset service state and PID to trigger automatic DAG-based re-spawning
                    states[idx] = ServiceState::Pending;
                    pids[idx] = 0;
                    
                    // If fileagent crashed, we must also reset its downstream program (testall)
                    // so that the test suite is safely re-executed once the FS is rebuilt.
                    for j in 0..SERVICES.len() {
                        if SERVICES[j].is_program {
                            states[j] = ServiceState::Pending;
                            pids[j] = 0;
                        }
                    }
                }
            }
        } else {
            // No child exited on this tick. Sleep for 10 ticks (100ms) to ensure 0% CPU consumption!
            let _ = syscalls::thread_sleep(10);
        }
    }
}
