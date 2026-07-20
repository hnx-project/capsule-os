---
name: capsule-architecture
description: Use ONLY when designing, documenting, or analyzing CapsuleOS system structures, capability models, or memory layouts. Trigger on ARCHITECTURE.md.
---

# CapsuleOS Architecture Specifications

Use this skill to navigate the high-level design of the CapsuleOS ecosystem.

## ⚙️ Key Structural Models
*   **Layered Topology**: L0 (Bootloader), L1 (HNX Microkernel), L2 (libc/libstd support), L3 (System Services), and L4 (User programs).
*   **Capability Protection**: Resources are exposed local-only via process-specific `HandleValue` mappings, keeping raw pointers off userspace.
*   **Synchronous Rendezvous**: Direct stack-to-stack copies for high performance zero-allocation communication.
*   **OHLINK Standard**: Header check parsing, alignment, and 6 relocation mechanics.
