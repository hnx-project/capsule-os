# Capsule OS - Agent Instructions

## Build Commands

```bash
# Build kernel (dev)
make build

# Build kernel (release) + link ELF
make kernel-release

# Run in QEMU
make run

# Check compilation
make check

# Clean build artifacts
make clean

# Manual build
cargo build --target aarch64-unknown-none -p kernel
rust-lld -flavor gnu -T kernel/kernel.ld build/target/aarch64-unknown-none/release/libkernel.a -o kernel.elf
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
│   ├── kernel.ld          # Linker script
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
│   ├── services/           # init, vfs, loader
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

### Phase 0 - Complete ✅
- Project skeleton created
- HAL traits defined
- Kernel compiles to libkernel.a
- Linker script created
- Makefile with build commands

### Phase 1 - In Progress 🔄
- AArch64 boot code (partial)
- UART console working
- QEMU testing available

## QEMU Testing

```bash
# Install QEMU (if needed)
brew install qemu

# Build and run
make kernel-release
make run

# QEMU command (manual)
qemu-system-aarch64 -machine virt -cpu cortex-a57 -nographic -kernel kernel.elf
```

## Known Issues

- Userspace binaries need linker scripts for bare metal
- GUI components are stubs
- QEMU serial output needs verification

## Verification Commands

```bash
# Check kernel compiles (no linking needed)
cargo check --target aarch64-unknown-none -p kernel

# Build kernel (produces libkernel.a)
cargo build --target aarch64-unknown-none -p kernel --release

# Full workspace check
cargo check --workspace
```
