# 🛠️ CapsuleOS Development & Testing Standards

This document establishes the strict engineering principles, architectural boundaries, and testing protocols for contributors and AI collaborators working on **CapsuleOS (Current Version: Pangu 1.0.0.beta)**.

---

## 🧭 The 8 Core Development Standards

To keep the codebase modular, robust, and safe, any code modification must adhere to the corresponding domain standard below:

### 1. 架构抽象标准 (Architecture Abstraction)
*   **Decoupled HAL (硬件抽象隔离)**: All CPU-specific and hardware-specific code (registers, vectors, MMU translations, interrupt routing) must be isolated inside `kernel/src/arch/aarch64`. The core microkernel (`kernel/src`) must never directly invoke assembly instructions or manipulate architecture-specific registers (e.g., `TTBR0_EL1`, `FAR_EL1`) outside of the HAL.
*   **Abstract Interfaces**: Leverage strong-typed Rust traits and types to wrap HAL operations. If an architecture-specific operation is required, expose it via a clean helper in the HAL module, ensuring high-level kernel logic remains target-agnostic.

### 2. 内核逻辑标准 (Kernel Logic)
*   **Zero-Allocation IPC**: Channels and ports must operate with zero dynamic heap allocation during runtime message passing.
*   **Decoupled Locking & Blocking (解锁与阻塞分离)**: To prevent deadlocks, a thread must **never** enter a sleeping, waiting, or yielding state while holding a lock on any shared data structures, especially the global `HandleTable` lock. Ensure locks are released before calling scheduler block/yield routines.
*   **Stable Pointer Guarantee (防止使用后释放 - UAF)**: When transferring resource handles across sandboxed processes, the underlying objects (e.g., `Channel`, `Port`) must be wrapped inside heap-allocated `Box` pointers (e.g., `KernelObject::Channel(Box<Channel>)`). This guarantees that their physical memory addresses remain stable and static in the heap, preventing the peer from holding a dangling pointer during channel transfers.

### 3. 系统调用标准 (Syscall Standard)
*   **Pristine Register Interface**: The syscall entry/exit interface must strictly map to AArch64 registers `x0`–`x7` for parameters and return values.
*   **Handle-Based Capability Delegation**: The kernel boundary is a strict security perimeter. Raw virtual or physical memory pointers must **never** be passed across the EL1/EL0 boundary for resource indexing. Instead, all resources are designated as `KernelObject` instances, addressed securely via process-local `HandleValue` indexes.
*   **Strict Security Auditing**: All userspace-provided memory buffers and strings must be explicitly validated for alignment, size limits, and mapping permissions (e.g., checking that the buffer does not overlap with kernel-space memory) to prevent unauthorized kernel probe attacks.

### 4. `libcapsule` 标准 (libcapsule Standard)
*   **Low-Level Capability Mapping**: `libcapsule` acts as the primary Rust runtime layer bridging raw system calls to safe, idiomatic Rust types (e.g., mapping a raw `Status` enum).
*   **Unified Error System**: It owns the definition of standard OS `Status` codes (e.g., `Status::Ok`, `Status::PeerClosed`, `Status::InvalidArgs`, `Status::AlreadyExists`). This keeps the error contract between sandboxes and the kernel uniform and predictable.

### 5. `libc` 翻译标准 (libc Translation Standard)
*   **Standard POSIX Compatibility**: Provide fully compliant C-ABI standard library symbols (`open`, `close`, `read`, `write`, `stat`, `readdir`, `mkdir`, `rmdir`, `unlink`, `pipe`, `getpid`, `gettimeofday`, `fcntl`, `ioctl`, ...).
*   **`fork()` is NOT a public libc API.**  CapsuleOS follows the Fuchsia / Zircon process model: new processes are created through **fresh-container spawn**, not the legacy `fork+exec` pattern.  `libc::fork()` returns -1 with `errno = ENOSYS`.  Programs that need to launch children should call `posix_spawn(3)` (see `libcapsule::posix_spawn`).  The underlying `SYSCALL_FORK` syscall is **deliberately kept wired** in the kernel because the bash compatibility layer and `procmgr`'s internal service-spawn path both depend on it; raw callers reach it through `libcapsule::syscalls::fork()`.  See `libraries/libcapsule/include/capsule_deprecation.h` for the full notice.
*   **`posix_spawn(3)` is the recommended spawn API.**  See `libraries/libcapsule/src/posix_spawn/API.md`.  The 1.0 implementation honours `adddup2` and `addclose`, forwards structured `argv` AND `envp` (each ≤ 16 entries / 256 bytes) into the kernel as separate VMOs, materialises both onto the child user stack at spawn time, applies `POSIX_SPAWN_SETSID` and `POSIX_SPAWN_SETPGROUP` to the freshly-spawned `Process.sid` / `Process.pgroup`, and supports `POSIX_SPAWN_WAITPID`.  `addopen` is recorded but the kernel encodes it as a no-op until the path VMO machinery ships (tracked under S13).
*   **Inline Byte VFS Protocol**: `libc` translates classic POSIX file I/O operations into uniform VFS byte streams sent over synchronous IPC channels to `fileagent`. It hides low-level capability handle manipulations from standard programs, keeping POSIX code clean.
*   **Hybrid Microkernel Boundary (Pangu 1.0)**: CapsuleOS Pangu 1.0 implements a **hybrid** microkernel model. The kernel exposes the S1–S8 POSIX-compatible syscall surface (`SYSCALL_GETUID`, `SYSCALL_PIPE_RW`, `SYSCALL_FCNTL`, `SYSCALL_IOCTL`, `SYSCALL_TTY_*`, `SYSCALL_FORK`, ...) directly because every downstream user — bash, osh, GNU coreutils — expects them through the C-ABI. The boundary is enforced at a different level: **the kernel does not implement a POSIX VFS or filesystem**. File/directory traversal goes through `fileagent`'s IPC VFS byte stream (the *Inline Byte VFS Protocol* rule above); the POSIX syscalls the kernel handles itself only operate on the kernel's *capability objects* (FdEntry::Pipe, FdEntry::Tty, FdEntry::File, HandleTable, Channel, VMOs, PTYs) — never on POSIX pathnames, inodes, or mode bits. This preserves the **security perimeter** (handle-based capability delegation, no raw pointer crossing) without forcing a long-term architectural rewrite before bash can run.
*   **Future Pure-Microkernel Path**: When the user-mode service catalogue (`procmgr`, `ttyd`, `procserv`) matures enough to host identity / pipe / fork / TTY dispatch, the kernel-side POSIX handlers should be migrated one syscall class at a time. Until then, the hybrid rule above is the contract — agents reviewing diffs must judge each new syscall against the capability-object criterion ("does it operate on a kernel object handle, or on a POSIX path string?") rather than the absolute "no POSIX in the kernel" prohibition.

### 6. `rust std` 标准 (Rust std Standard)
*   **Custom `#![no_std]` Stdlib**: `libraries/libstd` provides standard collections (`Vec`, `String`, `Box`, `BTreeMap`, etc.) and formatting macros (`println!`) to user-space.
*   **Seamless ABI Redirects**: All system allocations, panics, and standard output macros inside `libstd` must be transparently redirected down to the underlying `libc` standard C-ABI functions.

### 7. 服务开发标准 (Service Development Standard)
*   **Zero-Polling Event Loops**: To maximize efficiency and prevent CPU starvation, background services (such as `devmgr` and `fileagent`) must utilize synchronous blocking reads (`channel_read`) or port waits. Busy-waiting or active polling loops are strictly prohibited.
*   **Sandboxed Isolation**: Services must run in strict EL0 userspace and can only access system resources granted via capability handles explicitly transferred during startup or IPC negotiation.

### 8. 程序开发标准 (Program Development Standard)
*   **Absolute Separation of Concerns**: Sandboxed EL0 applications (e.g., `testall`, `osh`, `ls`, `rm`) are completely insulated from kernel-specific objects, handles, and syscalls. They must build exclusively against standard POSIX C-ABI APIs.
*   **Spawn children via `posix_spawn(3)`, not `fork()`**: New processes are created through `posix_spawn(3)` (Fuchsia / Zircon model).  `libc::fork()` is `ENOSYS` at the libc boundary; see §5 for the rationale.  See `libraries/libcapsule/src/posix_spawn/API.md` for the 1.0 surface (file actions `adddup2`/`addclose`; structured `argv` + `envp` VMO forwarding; kernel materialises both onto the child user stack at spawn time; `POSIX_SPAWN_SETSID` / `POSIX_SPAWN_SETPGROUP` apply on entry).
*   **Portability & Cleanliness**: Applications should match standard POSIX shell utilities, facilitating high code reuse and robust integration testing.

### 9. 模块开发文档标准 (Module Documentation - API.md Standard)
*   **Mandatory Document Review & Update**: When creating or modifying a program, library, or background service, the developer or AI Agent must:
    1. First read the component's `API.md` file (if existing) to understand specifications, limits, and associations.
    2. Update the `API.md` document upon completion if the code changes touch public methods, structures, constant values, or communication protocols.
*   **Required Template Content**: The `API.md` file must strictly incorporate:
    - **Status (状态)**: `[Active (使用中) | Deprecated (已废弃)]`
    - **Name (组件名称)**
    - **Dependencies & Related Components (依赖/关联组件说明)**: Enumerate dependencies (e.g. `libcapsule`) and other microkernel services (e.g. `devmgr`) it collaborates with.
    - **Core Definition (核心职责与定义)**
    - **Exposed Interfaces (暴露接口与公共约定)**: Standard public methods, protocols, constants, or IPC packets.

### 10. 严禁重复实现与强制重构规范 (Zero-Duplication & Mandatory Refactoring Standard)
*   **Strict Anti-Duplication Rule**: It is strictly forbidden to duplicate existing methods, macros, functions, or static variables.
*   **Pre-Implementation Inspection**: Prior to adding new functionality, you must thoroughly scan the codebase using standard tools (such as Grep) to see if similar operations are already implemented.
*   **Refactor First**: If an existing utility, helper, or core implementation is suboptimal, insufficient, or poorly designed for your needs, you must refactor and extend the existing code directly rather than introducing redundant functions, workarounds, or duplicate helper wrappers.

---

## 🧪 Testing & Verification Standards

To guarantee a stable, production-ready release cycle, every contribution must undergo and pass the verification grid:

### 1. Zero-Regression Policy
Any changes made to the microkernel, standard libraries, or background services must be verified locally before being staged or committed.

### 2. Standard Test Suite (`testall`)
The fundamental POSIX filesystem APIs and thread/channel runtime integrations are verified by running the **`testall`** CLI utility inside the AArch64 QEMU environment. The suite covers:
*   `connect`: Channel connection establishing between application and file server.
*   `create_file` & `read_file`: Synchronous file write/read verification.
*   `mkdir` & `mkdir_dup`: Multi-level directory creation and duplicate mapping rejection.
*   `readdir` & `stat`: Directory listing and file metadata querying.
*   `unlink`: Active file unlinking.
*   `rmdir` & `rmdir_nonempty`: Empty directory removal and non-empty deletion prevention.
*   `stress_vfs`: High-throughput, multi-cycle POSIX operational stress testing.
*   `peer_close`: Channel destruction and graceful teardown.

A successful verification must print:
```text
===== All Test Suite =====
[PASS] connect
[PASS] create_file
...
12/12 passed
```

### 3. Warning Policy (警告级别规范)
While local compilation allows warning tolerance under quick iterations, compiling clean without warnings is prioritized as a high-value quality target. Unused imports, unused variables, and unreachable code should be minimized prior to upstream merging.

---
