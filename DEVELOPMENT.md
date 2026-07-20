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
*   **Standard POSIX Compatibility**: Provide fully compliant C-ABI standard library symbols (`open`, `close`, `read`, `write`, `stat`, `readdir`, `mkdir`, `rmdir`, `unlink`).
*   **Inline Byte VFS Protocol**: `libc` translates classic POSIX operations into uniform VFS byte streams sent over synchronous IPC channels to `fileagent`. It hides low-level capability handle manipulations from standard programs, keeping POSIX code clean.

### 6. `rust std` 标准 (Rust std Standard)
*   **Custom `#![no_std]` Stdlib**: `libraries/libstd` provides standard collections (`Vec`, `String`, `Box`, `BTreeMap`, etc.) and formatting macros (`println!`) to user-space.
*   **Seamless ABI Redirects**: All system allocations, panics, and standard output macros inside `libstd` must be transparently redirected down to the underlying `libc` standard C-ABI functions.

### 7. 服务开发标准 (Service Development Standard)
*   **Zero-Polling Event Loops**: To maximize efficiency and prevent CPU starvation, background services (such as `devmgr` and `fileagent`) must utilize synchronous blocking reads (`channel_read`) or port waits. Busy-waiting or active polling loops are strictly prohibited.
*   **Sandboxed Isolation**: Services must run in strict EL0 userspace and can only access system resources granted via capability handles explicitly transferred during startup or IPC negotiation.

### 8. 程序开发标准 (Program Development Standard)
*   **Absolute Separation of Concerns**: Sandboxed EL0 applications (e.g., `testall`, `osh`, `ls`, `rm`) are completely insulated from kernel-specific objects, handles, and syscalls. They must build exclusively against standard POSIX C-ABI APIs.
*   **Portability & Cleanliness**: Applications should match standard POSIX shell utilities, facilitating high code reuse and robust integration testing.

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
