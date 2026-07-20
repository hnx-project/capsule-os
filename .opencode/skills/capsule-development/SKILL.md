---
name: capsule-development
description: Use ONLY when modifying code, writing microkernel systems, designing standard library shims, or verifying software integrity. Trigger on DEVELOPMENT.md.
---

# CapsuleOS Core Engineering & Testing Standards

Every code modification inside this workspace must comply with the 8 Core Development Standards.

## 🛡️ The 8 Core Standards
1.  **架构抽象 (Architecture Abstraction)**: Keep high-level kernel logic independent of raw physical assembly or AArch64 hardware instructions. Encapsulate HAL details strictly inside `kernel/src/arch/aarch64`.
2.  **内核逻辑 (Kernel Logic)**: Keep IPC message copying zero-allocation. Prevent deadlocks by dropping `HandleTable` locks before blocking or yielding. Wrapper channel objects in heap-stable `Box<Channel>` to avoid Use-After-Free (UAF).
3.  **系统调用 (Syscall Standard)**: Align raw arguments with registers `x0`-`x7`. Secure user buffers and string limits.
4.  **libcapsule标准 (libcapsule Standard)**: Standardize raw syscall returns into safe Rust `Status` codes.
5.  **libc翻译标准 (libc Translation Standard)**: Map standard POSIX filesystem symbols to inline VFS byte stream protocols, insulating standard EL0 applications from raw handles.
6.  **rust std标准 (Rust std Standard)**: Bridge standard allocations and formatting macros inside `libraries/libstd` down to `libc` C-ABI.
7.  **服务开发标准 (Service Development Standard)**: Background daemons (`devmgr`, `fileagent`) must run as zero-polling, synchronous blocking event loops.
8.  **程序开发标准 (Program Development Standard)**: Sandboxed utilities (`testall`, `osh`, `ls`) must be completely insulated from microkernel interfaces, building 100% against POSIX C-ABI.

## 🧪 Integration Verification
Run tests using QEMU integration:
```bash
xtask code run --arch aarch64
```
Ensure the `testall` binary finishes with standard output:
```text
===== All Test Suite =====
...
12/12 passed
```
