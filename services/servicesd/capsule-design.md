# 🌌 Init & Service Manager (`svc.init`) - Detailed Engineering Design

This document outlines the internal state machine, dependency graph architecture, and operational event loop of `initd` (the 1号进程) inside **CapsuleOS**.

---

## 1. ⚙️ Dependency Definition (静态声明式 DAG)

To satisfy `#![no_std]` constraints and maintain zero dynamic memory allocations during booting, `initd` maintains a static dependency tree mapping L3 Core Services:

```rust
pub struct ServiceDef {
    pub name: &'static str,
    pub path: &'static str,
    pub dependencies: &'static [&'static str],
}

pub static SERVICES: &[ServiceDef] = &[
    ServiceDef {
        name: "procmgr",
        path: "procmgr",
        dependencies: &[],
    },
    ServiceDef {
        name: "devmgr",
        path: "devmgr",
        dependencies: &[],
    },
    ServiceDef {
        name: "blkdev",
        path: "blkdev",
        dependencies: &[],
    },
    ServiceDef {
        name: "tty",
        path: "tty",
        dependencies: &[],
    },
    ServiceDef {
        name: "fileagent",
        path: "fileagent",
        dependencies: &["blkdev", "devmgr", "procmgr"],
    },
    ServiceDef {
        name: "testall",
        path: "testall",
        dependencies: &["fileagent", "tty"],
    },
];
```

---

## 2. 🌀 State Machine Transitions

Every service transitions through four logical states:

```text
       ┌───────────┐
       │  Pending  │ (Waiting for dependencies to reach Running)
       └─────┬─────┘
             │ All Dependencies are Running
             ▼
       ┌───────────┐
       │ Spawning  │ (initd has called spawn_service, waiting for READY)
       └─────┬─────┘
             │ Received INIT_CMD_READY handshake
             ▼
       ┌───────────┐
       │  Running  │ (Service is fully operational)
       └─────┬─────┘
             │ Child Process exits (wait4 returns exit_code)
             ▼
       ┌───────────┐
       │  Crashed  │ (Trigger re-spawning and dependency recovery)
       └───────────┘
```

---

## 3. 📡 Non-Blocking Ready Handshake Flow

To avoid deadlock and prevent race conditions:
1.  **Direct Execution Spawn**: `initd` calls `libcapsule::ServiceLoader::spawn_service("procmgr")`.
2.  **Child Process Initialization**:
    *   `procmgr` completes its internal setup (creates `server_chan`, binds to `"svc.procmgr"`).
    *   `procmgr` connects to `"svc.init"` and sends `INIT_CMD_READY` with payload `"procmgr"`.
3.  **Status Promotion**: `initd` receives the packet, matches `"procmgr"`, and promotes its state to `Running`.
4.  **DAG Tick**: `initd` scans all `Pending` services to find ones whose dependencies are now fully satisfied (i.e. all dependency names are in the `Running` state) and spawns them.

---

## 4. 🚀 Process Reclamation & Health Check (`wait4` Watcher)

To keep the service monitor non-blocking, `initd` integrates a periodic non-blocking child reaper:

1.  **Non-Blocking Reap**: On each tick of the event loop, `initd` calls `libc::syscalls::wait4(-1, &mut status, WNOHANG)`.
2.  **Event Match**: If a child PID is returned:
    *   `initd` finds the matching service structure corresponding to that PID.
    *   Logs the crash/exit status: `initd: [CRASH] Service 'xxx' exited with code {}.`
    *   Transition the service back to `Crashed` or `Pending` and trigger a **Re-spawn Sequence**.
