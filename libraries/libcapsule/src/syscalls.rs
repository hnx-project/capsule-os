//! # 📟 System Call Wrapper Interface
//!
//! This module provides safe, thin, type-safe Rust wrappers around CapsuleOS raw microkernel system traps.
//! It abstracts register manipulation (register constraints `x0`–`x7`) and handles conversion of return values
//! into the standard `Result<T, Status>` model.

use shared::status::{Result, Status};
#[allow(unused_imports)]
pub use shared::syscall_nums::*;

/// Raw system call invocation macro.
///
/// Dispatches the system call by loading arguments into architecture-specific parameters registers
/// (`x0`–`x5` on AArch64, `a0`–`a5` on RISC-V 64) and triggering the software trap (`svc #0` or `ecall`).
///
/// # Safety
/// System calls are fundamentally raw traps that communicate directly with the EL1 microkernel. Invalid pointers
/// or wrong handle parameters can lead to immediate process termination.
#[macro_export]
macro_rules! syscall {
    ($num:expr, $a0:expr, $a1:expr, $a2:expr, $a3:expr, $a4:expr, $a5:expr) => {{
        let r0_tmp = $a0 as usize;
        let r1 = $a1 as usize;
        let r2 = $a2 as usize;
        let r3 = $a3 as usize;
        let r4 = $a4 as usize;
        let r5 = $a5 as usize;
        let mut ret_val: usize;
        #[cfg(target_arch = "aarch64")]
        unsafe {
            core::arch::asm!(
                "mov x0, {tmp}",
                "svc #0",
                "mov {tmp2}, x0",
                tmp = in(reg) r0_tmp,
                tmp2 = out(reg) ret_val,
                in("x1") r1,
                in("x2") r2,
                in("x3") r3,
                in("x4") r4,
                in("x5") r5,
                in("x16") $num,
                out("x30") _,             // Tell the compiler that the Link Register (x30) is clobbered!
                options(nostack)          // Enforce that memory is completely clean
            );
        }
        #[cfg(target_arch = "riscv64")]
        unsafe {
            core::arch::asm!(
                "mv a0, {tmp}",
                "ecall",
                "mv {tmp2}, a0",
                tmp = in(reg) r0_tmp,
                tmp2 = out(reg) ret_val,
                in("a1") r1,
                in("a2") r2,
                in("a3") r3,
                in("a4") r4,
                in("a5") r5,
                in("a7") $num,
                out("ra") _,
                options(nostack)
            );
        }
        ret_val
    }};
}

/// Create a new point-to-point bidirectional IPC channel pair.
///
/// Returns a composite `usize` value representing both the client-end handle (lower 32-bits)
/// and the server-end handle (upper 32-bits).
///
/// # Errors
/// - `Status::NoMemory`: If the kernel lacks memory to create a new IPC `Channel` pair.
pub fn channel_create() -> Result<usize> {
    let ret = syscall!(
        shared::syscall_nums::SYSCALL_CHANNEL_CREATE,
        0,
        0,
        0,
        0,
        0,
        0
    );
    if ret == 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(ret)
    }
}

/// Read a message payload and any associated capability handles from an IPC channel.
///
/// This call is blocking by default. It returns the number of bytes read on success.
///
/// # Parameters
/// - `handle`: The raw `HandleValue` index of the channel.
/// - `buf`: User-space destination buffer for the incoming byte payload.
/// - `handles`: User-space destination buffer for any transferred capability handles.
///
/// # Errors
/// - `Status::InvalidArgs`: If the provided buffer pointers are invalid.
/// - `Status::PeerClosed`: If the remote end of the channel has been closed.
pub fn channel_read(handle: usize, buf: &mut [u8], handles: &mut [u32]) -> Result<usize> {
    let ret = syscall!(
        shared::syscall_nums::SYSCALL_CHANNEL_READ,
        handle,
        buf.as_mut_ptr() as usize,
        buf.len(),
        handles.as_mut_ptr() as usize,
        handles.len(),
        0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(ret)
    }
}

/// Write a message payload and any associated capability handles to an IPC channel.
///
/// # Parameters
/// - `handle`: The raw `HandleValue` index of the channel.
/// - `buf`: The byte payload to transmit.
/// - `handles`: An array of capability handles to transfer atomically.
///
/// # Errors
/// - `Status::PeerClosed`: If the remote end has been disconnected.
/// - `Status::NotAllowed`: If the caller lacks `Rights::WRITE` permissions on the channel handle.
pub fn channel_write(handle: usize, buf: &[u8], handles: &[u32]) -> Result<usize> {
    let ret = syscall!(
        shared::syscall_nums::SYSCALL_CHANNEL_WRITE,
        handle,
        buf.as_ptr() as usize,
        buf.len(),
        handles.as_ptr() as usize,
        handles.len(),
        0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(ret)
    }
}

/// Register an IPC channel handle under a global public namespace (e.g. `"svc.vfs"`).
///
/// Only privileged bootloaders or launchers can register services.
///
/// # Parameters
/// - `name`: The namespace name string.
/// - `handle`: The channel server-end handle to bind.
pub fn channel_register(name: &str, handle: usize) -> Result<()> {
    let ret = syscall!(
        shared::syscall_nums::SYSCALL_CHANNEL_REGISTER,
        name.as_ptr() as usize,
        name.len(),
        handle,
        0,
        0,
        0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(())
    }
}

/// Resolve a registered service name and obtain a direct IPC channel connection to it.
///
/// # Parameters
/// - `name`: The global namespace string to query (e.g., `"svc.dev"`).
///
/// # Errors
/// - `Status::NotFound`: If the requested service has not been registered.
pub fn channel_lookup(name: &str) -> Result<usize> {
    let ret = syscall!(
        shared::syscall_nums::SYSCALL_CHANNEL_LOOKUP,
        name.as_ptr() as usize,
        name.len(),
        0,
        0,
        0,
        0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(ret)
    }
}

/// Close a capability handle, terminating its connection and cleaning up kernel resources.
///
/// This is automatically invoked during the `Drop` phase of wrapper handles (RAII).
pub fn close(handle: usize) -> Result<()> {
    let ret = syscall!(shared::syscall_nums::SYSCALL_CLOSE, handle, 0, 0, 0, 0, 0);
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(())
    }
}

/// Yield the current thread's remaining timeslice back to the scheduler.
pub fn yield_cpu() -> isize {
    syscall!(shared::syscall_nums::SYSCALL_YIELD, 0, 0, 0, 0, 0, 0) as isize
}

/// Query hardware device topology information from the kernel.
///
/// # Parameters
/// - `buf`: The destination buffer to write serialized device descriptors.
pub fn device_info(buf: &mut [u8]) -> shared::status::Result<usize> {
    let ret = syscall!(
        shared::syscall_nums::SYSCALL_DEVICE_INFO,
        buf.as_mut_ptr() as usize,
        buf.len(),
        0,
        0,
        0,
        0
    );
    if (ret as isize) < 0 {
        Err(shared::status::Status::from_raw(ret as i32))
    } else {
        Ok(ret)
    }
}

/// Perform a volatile MMIO register read on behalf of the calling process.
///
/// Validated against authorized base addresses associated with registered hardware devices.
pub fn mmio_read(base: usize, offset: usize) -> Result<u32> {
    let ret = syscall!(
        shared::syscall_nums::SYSCALL_MMIO_READ,
        base,
        offset,
        0,
        0,
        0,
        0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(ret as u32)
    }
}

/// Perform a volatile MMIO register write on behalf of the calling process.
///
/// Validated against authorized base addresses associated with registered hardware devices.
pub fn mmio_write(base: usize, offset: usize, value: u32) -> Result<()> {
    let ret = syscall!(
        shared::syscall_nums::SYSCALL_MMIO_WRITE,
        base,
        offset,
        value as usize,
        0,
        0,
        0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(())
    }
}

/// Create a new Virtual Memory Object (VMO) representing a size-aligned set of pages.
pub fn vmo_create(size: usize) -> Result<usize> {
    let ret = syscall!(
        shared::syscall_nums::SYSCALL_VMO_CREATE,
        size,
        0,
        0,
        0,
        0,
        0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(ret)
    }
}

/// Clone a read-only child segment from an existing VMO.
pub fn vmo_create_child(parent_handle: usize, offset: usize, size: usize) -> Result<usize> {
    let ret = syscall!(
        shared::syscall_nums::SYSCALL_VMO_CREATE_CHILD,
        parent_handle,
        offset,
        size,
        0,
        0,
        0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(ret)
    }
}

/// Create a VMO representing a physical memory segment.
pub fn vmo_create_physical(phys_addr: usize, size: usize) -> Result<usize> {
    let ret = syscall!(
        shared::syscall_nums::SYSCALL_VMO_CREATE_PHYSICAL,
        phys_addr,
        size,
        0,
        0,
        0,
        0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(ret)
    }
}

/// Query the physical address of a mapped page within a VMO.
pub fn vmo_get_phys(vmo_handle: usize, offset: usize) -> Result<usize> {
    let ret = syscall!(
        shared::syscall_nums::SYSCALL_VMO_GET_PHYS,
        vmo_handle,
        offset,
        0,
        0,
        0,
        0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(ret)
    }
}

/// Synchronously read bytes from a VMO into user memory.
pub fn vmo_read(handle: usize, offset: usize, buf: &mut [u8]) -> Result<usize> {
    let ret = syscall!(
        shared::syscall_nums::SYSCALL_VMO_READ,
        handle,
        offset,
        buf.as_mut_ptr() as usize,
        buf.len(),
        0,
        0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(ret)
    }
}

/// Synchronously write bytes from user memory into a VMO.
pub fn vmo_write(handle: usize, offset: usize, buf: &[u8]) -> Result<usize> {
    let ret = syscall!(
        shared::syscall_nums::SYSCALL_VMO_WRITE,
        handle,
        offset,
        buf.as_ptr() as usize,
        buf.len(),
        0,
        0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(ret)
    }
}

/// Duplicate a handle and assign specific permission rights to the new copy.
pub fn handle_duplicate(handle: usize, rights: u32) -> Result<usize> {
    let ret = syscall!(
        shared::syscall_nums::SYSCALL_HANDLE_DUPLICATE,
        handle,
        rights as usize,
        0,
        0,
        0,
        0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(ret)
    }
}

/// Map a VMO's physical frames into the process's own virtual address space (VMAR).
pub fn vmar_map_self(
    vmo_handle: usize,
    vaddr_offset: usize,
    size: usize,
    flags: u32,
) -> Result<usize> {
    let ret = syscall!(
        shared::syscall_nums::SYSCALL_VMAR_MAP_SELF,
        vmo_handle,
        vaddr_offset,
        size,
        flags as usize,
        0,
        0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(ret)
    }
}

/// Unmap page translations from the process's local virtual address range (VMAR).
pub fn vmar_unmap(vaddr_offset: usize, size: usize) -> Result<()> {
    let ret = syscall!(
        shared::syscall_nums::SYSCALL_VMAR_UNMAP,
        vaddr_offset,
        size,
        0,
        0,
        0,
        0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(())
    }
}

/// Create an empty process container PCB with the specified debugging name.
pub fn process_create(name: &str) -> Result<usize> {
    let ret = syscall!(
        shared::syscall_nums::SYSCALL_PROCESS_CREATE,
        name.as_ptr() as usize,
        name.len(),
        0,
        0,
        0,
        0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(ret)
    }
}

/// Load an OHLINK executable binary from a VMO into a newly initialized sandboxed process.
pub fn load_binary(vmo_handle: usize, name: &str, offset: usize) -> Result<u64> {
    let ret = syscall!(
        shared::syscall_nums::SYSCALL_LOAD_BINARY,
        vmo_handle,
        name.as_ptr() as usize,
        name.len(),
        offset,
        0,
        0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(ret as u64)
    }
}

/// Wait on a child process's termination (similar to standard wait4/waitpid).
pub fn wait4(pid: i64, status_ptr: *mut i32, options: i32) -> Result<u64> {
    let ret = syscall!(
        shared::syscall_nums::SYSCALL_WAIT4,
        pid as usize,
        status_ptr as usize,
        options as usize,
        0,
        0,
        0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(ret as u64)
    }
}

/// POSIX `fork(2)` — duplicate the calling process.  Returns the
/// child's pid in the parent, 0 in the child.  See DEVELOPMENT.md
/// §3 / kernel::syscall::handlers::process::sys_fork for the
/// kernel-side semantics (S11 kstack fix lives there).
pub fn fork() -> Result<u64> {
    let ret = syscall!(
        shared::syscall_nums::SYSCALL_FORK,
        0, 0, 0, 0, 0, 0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(ret as u64)
    }
}

/// Atomically spawn a background service in kernel context.
pub fn service_spawn(desc: &shared::launcher::ServiceDescriptor) -> Result<u64> {
    // Stage 1: print info before entering syscall
    let desc_ptr = desc as *const shared::launcher::ServiceDescriptor as usize;

    // We cannot use normal formatting here easily without allocator/format macros,
    // so we write a raw print using a simple block.
    // Let's write the syscall with a simple wrapper to see if we ever get back.
    let ret = syscall!(
        shared::syscall_nums::SYSCALL_SERVICE_SPAWN,
        desc_ptr,
        0,
        0,
        0,
        0,
        0
    );

    // If we reach here, we're extremely lucky! Let's print the return value as raw bytes
    // or through some syscall.
    let mut temp = [0u8; 32];
    temp[0..13].copy_from_slice(b"SPAWN RETURN ");
    let mut val = ret;
    for i in 0..8 {
        let b = (val & 0xF) as u8;
        temp[13 + 7 - i] = if b < 10 { b'0' + b } else { b'A' + (b - 10) };
        val >>= 4;
    }
    temp[21] = b'\n';
    let _ = syscall!(
        shared::syscall_nums::SYSCALL_WRITE,
        1,
        temp.as_ptr() as usize,
        22,
        0,
        0,
        0
    );

    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(ret as u64)
    }
}

/// Spawn a process from a given binary VMO and command-line arguments VMO.
pub fn spawn(binary_vmo: usize, argv_vmo: usize) -> Result<u64> {
    let ret = syscall!(
        shared::syscall_nums::SYSCALL_SPAWN,
        binary_vmo,
        argv_vmo,
        0, // envp_vmo: 0 = no envp forwarding
        0, // file_actions_vmo: 0 = skip
        0, // attr_vmo: 0 = no attr
        0  // reserved / future
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(ret as u64)
    }
}

/// Replace the current process with a new program specified by its binary VMO and command-line arguments VMO.
pub fn execve(binary_vmo: usize, argv_vmo: usize) -> Result<()> {
    let ret = syscall!(
        shared::syscall_nums::SYSCALL_EXECVE,
        binary_vmo,
        argv_vmo,
        0,
        0,
        0,
        0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(())
    }
}

/// Suspend execution of the calling thread for the specified number of ticks.
pub fn thread_sleep(ticks: u64) -> Result<()> {
    let ret = syscall!(
        shared::syscall_nums::SYSCALL_THREAD_SLEEP,
        ticks as usize,
        0,
        0,
        0,
        0,
        0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(())
    }
}

/// Invoke a control command on process management subsystems.
pub fn proc_mgmt(cmd: u32, arg1: usize, arg2: usize, arg3: usize) -> Result<usize> {
    let ret = syscall!(
        shared::syscall_nums::SYSCALL_PROC_MGMT,
        cmd as usize,
        arg1,
        arg2,
        arg3,
        0,
        0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(ret)
    }
}

/// Create a new execution thread within the specified process container.
pub fn thread_create(
    process_handle: u32,
    entry_pc: usize,
    arg0: usize,
    arg1: usize,
    stack_size: usize,
) -> Result<usize> {
    let ret = syscall!(
        shared::syscall_nums::SYSCALL_THREAD_CREATE,
        process_handle as usize,
        entry_pc,
        arg0,
        arg1,
        stack_size,
        0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(ret)
    }
}

/// Wake up and enqueue a thread into the scheduler's ready queue.
pub fn thread_start(thread_handle: u32) -> Result<()> {
    let ret = syscall!(
        shared::syscall_nums::SYSCALL_THREAD_START,
        thread_handle as usize,
        0,
        0,
        0,
        0,
        0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(())
    }
}

/// Commit and flush our local GPU frame buffer VMO to the host screen display window.
pub fn display_flush(vmo_handle: usize) -> Result<()> {
    let ret = syscall!(
        shared::syscall_nums::SYSCALL_DISPLAY_FLUSH,
        vmo_handle,
        0,
        0,
        0,
        0,
        0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(())
    }
}

/// Read a single 512-byte sector from the block device.
pub fn block_read(sector: u64, dst_va: usize) -> Result<()> {
    let ret = syscall!(
        shared::syscall_nums::SYSCALL_BLOCK_READ,
        sector as usize,
        dst_va,
        0,
        0,
        0,
        0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(())
    }
}

/// Write a single 512-byte sector to the block device.
pub fn block_write(sector: u64, src_va: usize) -> Result<()> {
    let ret = syscall!(
        shared::syscall_nums::SYSCALL_BLOCK_WRITE,
        sector as usize,
        src_va,
        0,
        0,
        0,
        0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(())
    }
}

/// Get the total size of the block device in 512-byte sectors.
pub fn block_size() -> Result<u64> {
    let ret = syscall!(
        shared::syscall_nums::SYSCALL_BLOCK_SIZE,
        0,
        0,
        0,
        0,
        0,
        0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(ret as u64)
    }
}

/// Get the current working directory of the process.
pub fn getcwd(buf: &mut [u8]) -> Result<usize> {
    let ret = syscall!(
        shared::syscall_nums::SYSCALL_GETCWD,
        buf.as_mut_ptr() as usize,
        buf.len(),
        0,
        0,
        0,
        0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(ret)
    }
}

/// Send a raw network packet from user space.
pub fn net_send(buf: &[u8]) -> Result<()> {
    let ret = syscall!(
        shared::syscall_nums::SYSCALL_NET_SEND,
        buf.as_ptr() as usize,
        buf.len(),
        0,
        0,
        0,
        0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(())
    }
}

/// Receive a raw network packet into user space.
pub fn net_recv(buf: &mut [u8]) -> Result<usize> {
    let ret = syscall!(
        shared::syscall_nums::SYSCALL_NET_RECV,
        buf.as_mut_ptr() as usize,
        buf.len(),
        0,
        0,
        0,
        0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(ret as usize)
    }
}

// -------------------------------------------------------------------------
// virtio-mmio bus primitives
//
// EL0 device drivers (`blkdev`, `netd`, ...) call into these to discover
// virtio-mmio devices and allocate queue memory.  Once they have the
// queue VMOs mapped, they drive the device's MMIO registers themselves
// via the existing `mmio_read` / `mmio_write` syscalls.
// -------------------------------------------------------------------------

/// On-wire layout of `VirtioDeviceInfo`.  Kept in sync with the kernel.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VirtioDeviceInfo {
    pub slot: u32,
    pub mmio_base: u64,
    pub device_id: u32,
    pub version: u32,
    pub irq: u32,
}

/// On-wire layout of `VirtioQueueHandles`.  40 bytes.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VirtioQueueHandles {
    pub desc_vmo: u64,
    pub avail_vmo: u64,
    pub used_vmo: u64,
    pub desc_bytes: u64,
    pub avail_bytes: u64,
    pub used_bytes: u64,
    pub qsize: u32,
    pub _pad: u32,
}

/// Probe the virtio-mmio bus.  Writes a u32 device count followed by
/// `count` x 24-byte `VirtioDeviceInfo` records into `buf`.
pub fn virtio_probe(buf: &mut [u8]) -> Result<usize> {
    let ret = syscall!(
        shared::syscall_nums::SYSCALL_VIRTIO_PROBE,
        buf.as_mut_ptr() as usize,
        buf.len(),
        0,
        0,
        0,
        0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(ret as usize)
    }
}

/// Allocate three VMOs backing a virtqueue.  Returns the `VirtioQueueHandles`
/// structure containing the VMO handles and per-ring byte counts.
pub fn virtio_setup_queue(slot: u32, qsel: u16, qsize: u16) -> Result<VirtioQueueHandles> {
    let mut handle = VirtioQueueHandles {
        desc_vmo: 0,
        avail_vmo: 0,
        used_vmo: 0,
        desc_bytes: 0,
        avail_bytes: 0,
        used_bytes: 0,
        qsize: 0,
        _pad: 0,
    };
    let ret = syscall!(
        shared::syscall_nums::SYSCALL_VIRTIO_SETUP_QUEUE,
        (&mut handle as *mut _) as usize,
        slot as usize,
        qsel as usize,
        qsize as usize,
        0,
        0
    );
    if (ret as isize) < 0 || ret == 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(handle)
    }
}

/// Write QueueNotify (offset 0x050) for `(slot, qsel)`.
pub fn virtio_kick(slot: u32, qsel: u16) -> Result<()> {
    let ret = syscall!(
        shared::syscall_nums::SYSCALL_VIRTIO_KICK,
        slot as usize,
        qsel as usize,
        0,
        0,
        0,
        0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(())
    }
}

/// Read ISR (offset 0x060) for `slot`.
pub fn virtio_read_isr(slot: u32) -> Result<u32> {
    let ret = syscall!(
        shared::syscall_nums::SYSCALL_VIRTIO_READ_ISR,
        slot as usize,
        0,
        0,
        0,
        0,
        0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(ret as u32)
    }
}

// -------------------------------------------------------------------------
// S1: helper for libc::getpid.
//
// libcapsule used to call straight into `SCHEDULER` via a kernel
// trap, but libc is `no_std` and was happy to share the same
// library as libcapsule.  Here we provide a thinner, panic-free
// accessor that libcapsule's `users.rs::getpid` (and libc's own
// shim) can call without dragging in the syscall macro.
// -------------------------------------------------------------------------

#[repr(C)]
pub struct UserThreadView {
    /// `id` is the kernel `Thread.id`, useful for diagnostics.
    pub id: usize,
    /// `process_id` is the user's perception of `getpid() == task.pid`.
    pub process_id: u64,
    /// Reserved for a future `ppid`/`pgid` field; set to 0 for now.
    pub reserved: u64,
}

/// Fetch the calling thread's per-process identity triple.
///
/// Internally does a single `PROC_MGMT_GET_IDENTITY` syscall that
/// returns `(tid << 32) | pid` packed in one u64; this avoids the
/// race where the thread might be migrated between separate
/// `gettid` / `getpid` calls.  Returns a `UserThreadView { 0, 0, 0 }`
/// if there is no current thread (e.g. before `_start`).
#[inline]
pub fn get_current_thread_checked() -> UserThreadView {
    let r = syscall!(
        shared::syscall_nums::SYSCALL_PROC_MGMT,
        5, // PROC_MGMT_GET_IDENTITY
        0, 0, 0, 0, 0
    );
    if r == 0 || (r as isize) < 0 {
        UserThreadView { id: 0, process_id: 0, reserved: 0 }
    } else {
        let v = r as u64;
        let tid = (v >> 32) as usize;
        let process_id = (v & 0xffff_ffff) as u64;
        UserThreadView { id: tid, process_id, reserved: 0 }
    }
}
