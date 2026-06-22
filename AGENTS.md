# Capsule OS - Agent Instructions

## Build Commands

```bash
# Build workspace (libs only - userspace binaries have linking issues)
cargo build --workspace

# Build kernel for bare metal (AArch64) - ✅ Works
cargo build --target aarch64-unknown-none -p kernel

# Build kernel release
cargo build --target aarch64-unknown-none -p kernel --release

# Check kernel compiles
cargo check --target aarch64-unknown-none -p kernel
```

## Target Triple

- Kernel: `aarch64-unknown-none` (bare metal, no_std)
- Userspace: native host target for development, `aarch64-unknown-none` for final

## Workspace Structure

```
capsule-os/
├── hal/                    # Hardware Abstraction Layer (no_std)
│   └── src/               # Traits only, no implementations
├── shared/                 # Shared types (no_std)
│   └── src/
│       ├── status.rs       # Status/Result types
│       ├── types.rs        # HandleValue, ObjectType
│       ├── ipc.rs          # Message types
│       └── boot.rs         # BootInfo
├── kernel/                 # Microkernel (no_std)
│   └── src/
│       ├── arch/           # Arch implementations (aarch64/, x86_64/)
│       ├── task/           # Scheduler, Thread, Process
│       ├── mm/             # VMO, VMAR, physical memory
│       ├── ipc/            # Channel, Port
│       ├── object/          # Handle, HandleTable
│       ├── syscall/         # Syscall dispatch
│       └── kcore/           # Kernel core utilities
├── userspace/              # User space programs
│   ├── libc/               # Syscall wrappers
│   ├── services/           # init, vfs, loader (need linker scripts)
│   └── programs/          # shell
└── gui/                    # Desktop environment
    ├── compositor/         # Window compositor (stub)
    ├── renderer/           # 2D rendering (stub)
    └── client/            # GUI client library (stub)
```

## Key Conventions

### HAL Design
- Traits defined in `hal/`, implementations in `kernel/src/arch/<arch>/`
- Never implement traits in `hal/` crate itself

### No_std Crates
- `hal/`, `shared/`, `kernel/` are all `#![no_std]`
- Userspace binaries are also `#![no_std]` with `_start` entry point
- No `std`, no `alloc` (except where explicitly needed)

### Kernel Entry Point
- `_start()` in `kernel/src/lib.rs`
- Panic handler required: `#[panic_handler] fn panic(info: &PanicInfo) -> !`

### Status/Error Handling
- `shared::status::Status` enum with error codes
- `shared::status::Result<T> = core::result::Result<T, Status>`

## Development Status

### Phase 0 (Current) - Complete ✅
- Project skeleton created
- HAL traits defined
- Kernel skeleton compiles
- Userspace skeleton compiles (linking pending linker scripts)

### Phase 1 - Pending
- AArch64 boot code
- UART console
- QEMU testing

## QEMU Testing (Requires QEMU installation)

```bash
# Install QEMU first
brew install qemu

# Build kernel
cargo build --target aarch64-unknown-none -p kernel --release

# Link kernel (requires linker script)
ld.lld -T kernel/kernel.ld build/target/aarch64-unknown-none/release/libkernel.a -o kernel.elf

# Run in QEMU
qemu-system-aarch64 -machine virt -cpu cortex-a57 -nographic \
  -kernel kernel.elf
```

## Known Issues

- Userspace binaries need linker scripts for bare metal
- QEMU not installed on development machine
- GUI components are stubs

## Verification Commands

```bash
# Check kernel compiles (no linking needed)
cargo check --target aarch64-unknown-none -p kernel

# Build kernel (produces libkernel.a)
cargo build --target aarch64-unknown-none -p kernel --release

# Full workspace check
cargo check --workspace
```
