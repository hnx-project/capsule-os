# libtty — EL0 line discipline

`libtty` is the **userspace** counterpart to the kernel's raw
`sys_read(fd = 0)` primitive.  It implements the cooked-mode line
discipline that Linux's `n_tty.c` provides in the kernel — but split
across the EL1/EL0 boundary the way a microkernel ought to:

| Linux | CapsuleOS |
|---|---|
| `kernel/tty/n_tty.c` (line discipline) | `libtty::TtyLine` |
| `kernel/tty/tty_io.c` (raw read) | `sys_read(fd = 0, ...)` (raw byte loop) |
| `include/linux/tty.h` (constants) | `libtty::{BS, DEL, CTRL_C, ...}` |

## Public API

```rust
use libtty::TtyLine;

let mut tty = TtyLine::new();

// Shell emits the prompt *before* calling read_line; libtty only
// echoes the characters the user types.
print!("osh$ ");
let line = tty.read_line()?;          // blocks until '\n' or Ctrl-C

// Use `tty.line()` to inspect the captured bytes.
```

## Discipline semantics

| Key | Action |
|---|---|
| `CR` / `LF` | terminate line, echo `\r\n` |
| `DEL` (0x7f) / `BS` (0x08) | erase previous char, echo `\b \b` |
| `Ctrl-C` (0x03) | echo `^C\r\n`, return 0 |
| `Ctrl-D` (0x04) | flush, return 0 on empty line |
| `Ctrl-U` (0x15) | kill entire line, echo `\r\n` |
| `Ctrl-W` (0x17) | erase previous word |
| `ESC [A` / `ESC [B` | history up / down |
| `0x20..=0x7e` | append + echo |

## Boot-time contract

The kernel is **raw** by default: `sys_read(fd = 0, ...)` returns one
byte per syscall without any editing.  Every EL0 process that holds
the canonical stdin handle (i.e. everyone except `gpud` / `inputd`
which drive the UARTs directly through MMIO) is expected to use
`libtty::TtyLine::read_line` for cooked input.

This matches Linux's behaviour: the kernel exposes raw bytes, and
the line discipline sits in userspace (`n_tty.c` in Linux,
`libtty::TtyLine` here).  Other EL0 programs that need raw
semantics can call `TtyLine::read_byte_raw` instead.