#![no_std]
#![no_main]

extern crate libcapsule;

mod config;

use config::{ActiveService, parse_auto_toml, MAX_SERVICES, MAX_DEPS, CONFIG_BUFFERS};
use libcapsule::{log_info, log_warn, log_error, syscalls};
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
        name: "netd",
        path: "netd",
        dependencies: &["devmgr", "procmgr"],
        is_program: false,
    },
    ServiceDef {
        name: "testall",
        path: "testall",
        dependencies: &["fileagent", "tty", "netd"],
        is_program: true,
    },
];

#[no_mangle]
pub fn main() -> i32 {
    log_info!("SERVICESD", "Service Manager v1.0.0 (ServicesD) Active");

    // 1. Initialize loaders
    let bootstrap_service = libcapsule::ServiceLoader::new(BOOTFS_VMO_HANDLE);
    let bootstrap_program = libcapsule::ProgramLoader::new(BOOTFS_VMO_HANDLE);

    // 2. Create the bidirectional servicesd server channel
    let raw = match syscalls::channel_create() {
        Ok(v) => v,
        Err(_) => {
            log_error!("SERVICESD", "[ERROR] channel_create failed");
            return -1;
        }
    };
    let server_chan = (raw >> 32) as u32 as usize;

    if let Err(e) = syscalls::channel_register("svc.servicesd", server_chan) {
        log_error!("SERVICESD", "[ERROR] channel_register failed: {:?}", e);
        return -2;
    }
    log_info!(
        "SERVICESD",
        "Registered global name 'svc.servicesd' on channel {}",
        server_chan
    );

    // Dynamic configuration discovery
    let mut services = [const {
        ActiveService {
            name: "",
            path: "",
            dependencies: [""; MAX_DEPS],
            dep_count: 0,
            is_program: false,
        }
    }; MAX_SERVICES];
    let mut service_count = 0;

    // Scan BootFS directory for auto.toml configs
    let mut superblock = [0u8; 16];
    if let Ok(_) = syscalls::vmo_read(BOOTFS_VMO_HANDLE, 0, &mut superblock) {
        if &superblock[0..8] == b"HNXF_VFS" {
            let count = u64::from_le_bytes(superblock[8..16].try_into().unwrap()) as usize;
            let mut entry_bytes = [0u8; 144];

            for i in 0..count {
                let offset = 16 + (i * 144);
                if let Ok(_) = syscalls::vmo_read(BOOTFS_VMO_HANDLE, offset, &mut entry_bytes) {
                    let path_len = entry_bytes[0..128]
                        .iter()
                        .position(|&b| b == 0)
                        .unwrap_or(128);
                    if let Ok(path_str) = core::str::from_utf8(&entry_bytes[0..path_len]) {
                        if path_str.starts_with("system/share/configs/")
                            && path_str.ends_with("/auto.toml")
                            && service_count < MAX_SERVICES
                        {
                            let file_offset =
                                u64::from_le_bytes(entry_bytes[128..136].try_into().unwrap())
                                    as usize;
                            let file_size =
                                u64::from_le_bytes(entry_bytes[136..144].try_into().unwrap())
                                    as usize;

                            unsafe {
                                let read_size = file_size.min(512);
                                if let Ok(_) = syscalls::vmo_read(
                                    BOOTFS_VMO_HANDLE,
                                    file_offset,
                                    &mut CONFIG_BUFFERS[service_count][..read_size],
                                ) {
                                    if let Ok(content_str) = core::str::from_utf8(
                                        &CONFIG_BUFFERS[service_count][..read_size],
                                    ) {
                                        if let Some(service) = parse_auto_toml(content_str) {
                                            log_info!("SERVICESD", "Dynamically discovered service '{}' from BootFS", service.name);
                                            services[service_count] = service;
                                            service_count += 1;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    if service_count == 0 {
        log_info!(
            "SERVICESD",
            "No dynamic configurations found. Falling back to static hardcoded SERVICES."
        );
        for (i, s) in SERVICES.iter().enumerate() {
            if i >= MAX_SERVICES {
                break;
            }
            let mut deps = [""; MAX_DEPS];
            let dep_count = s.dependencies.len().min(MAX_DEPS);
            for d in 0..dep_count {
                deps[d] = s.dependencies[d];
            }
            services[i] = ActiveService {
                name: s.name,
                path: s.path,
                dependencies: deps,
                dep_count,
                is_program: s.is_program,
            };
            service_count += 1;
        }
    }

    // 3. Keep track of service states and PIDs
    let mut states = [ServiceState::Pending; MAX_SERVICES];
    let mut pids = [0u64; MAX_SERVICES];

    let mut conn_buf = [0u8; 64];
    let mut conn_handles = [0u32; 2];

    loop {
        // A. Sweep the DAG to spawn any satisfied Pending services
        let mut progress = false;
        for i in 0..service_count {
            if states[i] == ServiceState::Pending {
                // Check if all dependencies are Running
                let mut satisfied = true;
                for d in 0..services[i].dep_count {
                    let dep = services[i].dependencies[d];
                    let mut dep_running = false;
                    for j in 0..service_count {
                        if services[j].name == dep && states[j] == ServiceState::Running {
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
                    log_info!(
                        "SERVICESD",
                        "Dependency satisfied. Spawning '{}'...",
                        services[i].name
                    );
                    if services[i].is_program {
                        match bootstrap_program.spawn_program(services[i].path) {
                            Ok(pid) => {
                                log_info!(
                                    "SERVICESD",
                                    "[SPAWN] Program '{}' launched with PID {}",
                                    services[i].name,
                                    pid
                                );
                                pids[i] = pid as u64;
                                states[i] = ServiceState::Running;
                                progress = true;
                            }
                            Err(e) => {
                                log_error!(
                                    "SERVICESD",
                                    "[ERROR] Failed to spawn program '{}': {:?}",
                                    services[i].name,
                                    e
                                );
                            }
                        }
                    } else {
                        match bootstrap_service.spawn_service(services[i].path) {
                            Ok(pid) => {
                                log_info!("SERVICESD", "[SPAWN] Service '{}' launched, PID={}. Waiting for READY...", services[i].name, pid);
                                pids[i] = pid as u64;
                                states[i] = ServiceState::Spawning;
                                progress = true;
                            }
                            Err(e) => {
                                log_error!(
                                    "SERVICESD",
                                    "[ERROR] Failed to spawn service '{}': {:?}",
                                    services[i].name,
                                    e
                                );
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
        for i in 0..service_count {
            if states[i] == ServiceState::Spawning {
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

                    if let Ok(n) =
                        syscalls::channel_read(session_chan, &mut cmd_buf, &mut cmd_handles)
                    {
                        if n >= 20 {
                            let cmd = cmd_buf[0];
                            if cmd == 1 {
                                // INIT_CMD_READY
                                let mut len_bytes = [0u8; 4];
                                len_bytes.copy_from_slice(&cmd_buf[4..8]);
                                let len = (u32::from_le_bytes(len_bytes) as usize).min(128);

                                if let Ok(service_name) =
                                    core::str::from_utf8(&cmd_buf[20..20 + len])
                                {
                                    let name = service_name.trim();
                                    log_info!(
                                        "SERVICESD",
                                        "Received INIT_CMD_READY handshake from '{}'",
                                        name
                                    );

                                    // Match name and promote state
                                    for i in 0..service_count {
                                        if services[i].name == name {
                                            states[i] = ServiceState::Running;
                                            log_info!(
                                                "SERVICESD",
                                                "Service '{}' promoted to RUNNING state",
                                                name
                                            );
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
            for i in 0..service_count {
                if pids[i] == reaped_pid {
                    found_idx = Some(i);
                    break;
                }
            }

            if let Some(idx) = found_idx {
                let s_name = services[idx].name;
                if services[idx].is_program {
                    log_info!(
                        "SERVICESD",
                        "[INFO] Program '{}' (PID {}) has completed. Exit code: {}.",
                        s_name,
                        reaped_pid,
                        exit_status
                    );
                } else {
                    log_error!("SERVICESD", "[CRASH] Service '{}' (PID {}) has terminated with exit code {}!", s_name, reaped_pid, exit_status);
                    log_warn!("SERVICESD", "[AUTO-HEALING] Resetting dependency graph to restart '{}'...", s_name);

                    // Reset service state and PID to trigger automatic DAG-based re-spawning
                    states[idx] = ServiceState::Pending;
                    pids[idx] = 0;

                    // If a service crashed, we must also reset its downstream programs
                    for j in 0..service_count {
                        if services[j].is_program {
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
