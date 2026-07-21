# 🌌 libcapsule - CapsuleOS Native Capability SDK

`libcapsule` is the official, native Capability SDK and System Runtime Library for **CapsuleOS** (Codename: **Pangu**). It bridges raw microkernel system calls to safe, ergonomic, and type-safe Rust abstractions for user-space sandboxed services and applications.

---

## 🧭 Architecture & Philosophy

Unlike standard monolithic operating systems, CapsuleOS operates on a strict **Capability-Based Security Model**. User-space programs (running in EL0) do not have direct access to physical memory, hardware devices, or system objects. Instead:
- All kernel objects (Processes, Threads, Channels, Ports, VMOs, VMARs) are managed securely inside the kernel.
- User-space programs refer to these objects via **process-local integer indices** called `HandleValue`.
- `libcapsule` is the official library that encapsulates these raw `HandleValue` indices into safe, idiomatically structured Rust types (such as `Channel` and `Port`), automatically managing their lifecycles, resource cleanups, and capability transfers via RAII.

```text
 ┌─────────────────────────────────────────────────────────┐
 │               EL0 Sandbox Applications                  │
 ├────────────────────────────┬────────────────────────────┤
 │      POSIX Standard        │     Capsule Native         │
 │     C-ABI APIs (libc)      │  Capability SDK (libcapsule)│
 └─────────────┬──────────────┴─────────────┬──────────────┘
               │                            │
               │ (POSIX-to-Native Translation)
               ▼                            │
 ┌──────────────────────────────────────────▼──────────────┐
 │                    libcapsule Core                      │
 └────────────────────────────┬────────────────────────────┘
                              │ (Raw Syscalls x0-x7)
                              ▼
 ====================== EL1/EL0 Boundary ===================
                              ▼
 ┌─────────────────────────────────────────────────────────┐
 │               HNX Microkernel Core (EL1)                │
 └─────────────────────────────────────────────────────────┘
```

---

## 🛠️ Main Modules & Core Subsystems

`libcapsule` is composed of the following key subsystems:

### 1. 📦 Loader Subsystem (`ProgramLoader` & `ServiceLoader`)
The loader subsystem is responsible for bootstrapping and spawning EL0 executable binaries directly from the boot-time read-only memory archive (`BootFS`).

- **`ServiceLoader`**: Specifically designed for spawning **permanent, privileged background services** (such as `fileagent` and `devmgr`). Services typically run with higher priority (`Priority::High`), are registered in the global service namespace, and are expected to run indefinitely.
- **`ProgramLoader`**: Specifically designed for spawning **sandboxed ordinary applications** (such as `testall` or custom shell commands). Programs typically run with standard priority (`Priority::Normal`) and communicate solely through client-side IPC channels.

#### 💡 Design Choice: Intent-Based Separation & Future Integration
Currently, both loader wrappers are implemented separately to preserve **architectural intent-based separation**. This allows the kernel and runtime to apply distinct scheduling policies, security audits, and service registrations during bootstrap. 
In future versions, their shared boilerplate (parsing the `HNXF_VFS` archive, cloning child VMO segments, and initiating `sys_load_binary`) will be consolidated under a single, unified, high-cohesion `BootfsLoader` utility, while keeping `ServiceLoader` and `ProgramLoader` as distinct high-level intent-based interfaces.

### 2. 🔌 IPC Subsystem (`Channel` & `Port`)
CapsuleOS utilizes high-performance, zero-polling, synchronous and asynchronous message-passing IPC.
- **`Channel`**: A bidirectional, point-to-point communication pipe. Channels can transfer both **VFS byte payloads** and **capability handles** atomically in a single system call (`channel_call` or `channel_write`/`channel_read`).
- **`Port`**: A multi-source event-queuing object. Threads can bind channels or asynchronous events to a `Port` and block on `port_wait()`, enabling elegant, zero-polling event loops for background services.

### 3. 🧠 Memory Management Subsystem (`VMO` & `VMAR`)
- **`VMO` (Virtual Memory Object)**: Represents a physically backed or logically mapped contiguous segment of physical pages. Used for memory sharing, DMA buffers, and binary loading.
- **`VMAR` (Virtual Memory Address Range)**: Manages layout allocations and page translation mappings within the process's local virtual page tables.

### 4. 📟 System Call Wrapper Module (`syscalls`)
Provides thin, inlineable, type-safe wrappers around raw microkernel traps. Handles marshalling arguments into registers `x0`–`x7` and unmarshalling returning `Status` codes.

---

## 🚀 Usage & API Examples

### 1. Spawning a Program from BootFS
```rust
use libcapsule::ProgramLoader;

fn spawn_test() {
    // BOOTFS_VMO_HANDLE is injected into PID 1 by the kernel
    let bootfs_vmo = 100; 
    let loader = ProgramLoader::new(bootfs_vmo);

    match loader.spawn_program("testall") {
        Ok(handle) => crate::kprintln!("Program testall spawned with handle: {}", handle),
        Err(e) => crate::kprintln!("Failed to spawn: {:?}", e),
    }
}
```

### 2. Performing Synchronous Bidirectional Channel IPC
```rust
use libcapsule::syscalls;

fn query_vfs_service(vfs_channel: u32) {
    let mut cmd = [0u8; 148];
    cmd[0] = 1; // VFS_OPEN command code
    
    let mut resp = [0u8; 256];
    let mut resp_handles = [0u32; 2];

    // Write a request and block wait for the response atomically
    match syscalls::channel_call(vfs_channel, &cmd, &[], &mut resp, &mut resp_handles) {
        Ok(bytes_read) => {
            crate::kprintln!("Received response payload of {} bytes", bytes_read);
            if resp_handles[0] != 0 {
                crate::kprintln!("Received new session capability handle: {}", resp_handles[0]);
            }
        }
        Err(e) => crate::kprintln!("IPC Call failed: {:?}", e),
    }
}
```

---

## 🛡️ Core Development Standards (`DEVELOPMENT.md` Alignment)

When contributing to or utilizing `libcapsule`, you must strictly adhere to the following rules:
1. **Zero-Polling Loops**: Always use blocking `port_wait()` or synchronous IPC blocks (`channel_read` / `channel_call`). Active spinning or busy-polling loops are strictly prohibited to prevent CPU starvation.
2. **Stable Pointer Guarantee**: High-level resources transferred via channels must be securely encapsulated. Never pass raw virtual or physical pointers across user-space system calls. Use capability `HandleValue` mappings.
3. **Unlocked Blocking**: Never perform blocking system calls (`channel_read`, `nanosleep`, or `port_wait`) while holding any internal locks on process-shared structures.

---
*Developed as part of the CapsuleOS Operating System Ecosystem.*
