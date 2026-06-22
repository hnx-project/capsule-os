# Capsule OS - Agent Instructions

## Build Commands

```bash
# Build workspace (uses host target, for libs only)
cargo build --workspace

# Build kernel for bare metal (AArch64)
cargo build --target aarch64-unknown-none -p kernel

# Build kernel for bare metal (X86_64)
cargo build --target x86_64-unknown-none -p kernel

# Build userspace services
cargo build --workspace

# Full release build
cargo build --workspace --release
```

## Target Triple

- Kernel: `aarch64-unknown-none` (bare metal, no_std)
- Userspace: native host target for development, `aarch64-unknown-none` for final

## Workspace Structure

```
capsule-os/
├── hal/                    # Hardware Abstraction Layer (no_std)
│   └── src/                # Traits only, no implementations
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
│       ├── object/         # Handle, HandleTable
│       ├── syscall/        # Syscall dispatch
│       └── kcore/          # Kernel core utilities
├── userspace/              # User space programs
│   ├── libc/               # Syscall wrappers
│   ├── services/           # init, vfs, loader
│   └── programs/           # shell
└── gui/                    # Desktop environment
    ├── compositor/         # Window compositor
    ├── renderer/           # 2D rendering
    └── client/             # GUI client library
```

## Key Conventions

### HAL Design
- Traits defined in `hal/`, implementations in `kernel/src/arch/<arch>/`
- Never implement traits in `hal/` crate itself

### No_std Crates
- `hal/`, `shared/`, `kernel/` are all `#![no_std]`
- No `std`, no `alloc` (except where explicitly needed)
- `#[global_allocator]` exists in `kernel/src/kcore/alloc.rs`

### Kernel Entry Point
- `_start()` in `kernel/src/lib.rs`
- Panic handler required: `#[panic_handler] fn panic(info: &PanicInfo) -> !`

### Status/Error Handling
- `shared::status::Status` enum with error codes
- `shared::status::Result<T> = core::result::Result<T, Status>`

## Development Workflow

1. **Phase 0**: Build workspace libs first, verify they compile
2. **Phase 1**: Get kernel running in QEMU (bare metal target)
3. **Phase 2-5**: Incremental feature development

## QEMU Testing (Planned)

```bash
# Expected run command (not yet implemented)
qemu-system-aarch64 -machine virt -cpu cortex-a57 -nographic \
  -kernel build/target/aarch64-unknown-none/release/kernel
```

## Important Files

| File | Purpose |
|------|---------|
| `Cargo.toml` | Workspace root, all members listed |
| `.cargo/config.toml` | Target linker configuration |
| `config/machine/aarch64/default.conf` | Memory layout, toolchain settings |
| `TODO.md` | Development phases and progress |

## Known Issues

- `kernel/` has compilation errors being fixed in Phase 0
- Userspace `no_std` binaries need linker scripts (not yet created)
- GUI components are stubs, need full implementation

## Verification Commands

```bash
# Check kernel compiles
cargo check --target aarch64-unknown-none -p kernel

# Check workspace compiles (host target)
cargo check --workspace

# Run clippy
cargo clippy --workspace -D warnings 2>/dev/null || true
```
