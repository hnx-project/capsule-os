//! # 🌌 libcapsule - CapsuleOS Native Capability SDK
//!
//! `libcapsule` is the official, native Capability SDK and System Runtime Library for **CapsuleOS**.
//! It bridges raw microkernel system calls to safe, ergonomic, and type-safe Rust abstractions.
//!
//! ## 🧭 Architecture Overview
//!
//! - **Capability Handle Model**: Handles system resources safely via `HandleValue` wrappers.
//! - **IPC Messaging**: High-performance, zero-polling point-to-point and port-queued IPC.
//! - **Executable Loading**: Decoupled `ProgramLoader` and `ServiceLoader` to resolve, clone,
//!   and launch binaries from boot memory (`BootFS VMO`) into sandboxed containers.
//!
//! For a complete guide, please refer to the main repository `README.md` and `DEVELOPMENT.md`.

#![no_std]

pub mod channel;
pub mod log;
pub mod service;
pub mod program;
pub mod syscalls;

pub use channel::Channel;
pub use service::ServiceLoader;
pub use program::ProgramLoader;
pub use shared::status::Status;
pub use shared::syscall_nums::*;
