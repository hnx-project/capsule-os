# 🌌 TTY & Console Service (`svc.tty`) - Detailed Engineering Design

This document details the internal architecture, state machine, and data structures of the `svc.tty` system service in **CapsuleOS**. It bridges the raw physical console devices with standard user-space shell processes like `osh`.

---

## 1. ⚙️ Architectural Overview

```text
┌──────────────────────────────────────────────────────────────────┐
│               L4 Client Application (e.g. osh)                   │
│                                                                  │
│  Stdin (Fd 0)                Stdout (Fd 1)       Stderr (Fd 2)   │
└───────┬────────────────────────────▲───────────────────┬──────────┘
        │ Read (TTY_CMD_READ)        │ Write (TTY_WRITE) │
        ▼                            │                   ▼
┌────────────────────────────────────┴─────────────────────────────┐
│                 TTY & Console Service (svc.tty)                  │
│                                                                  │
│  ┌───────────────────┐    ┌─────────────────┐    ┌────────────┐  │
│  │  Line Buffer      │    │ Termios State   │    │  Session   │  │
│  │  [u8; 1024]       │    │ (ICANON, ECHO)  │    │  Manager   │  │
│  └─────────┬─────────┘    └────────┬────────┘    └─────┬──────┘  │
│            │                       │                   │         │
└────────────┼───────────────────────┼───────────────────┼─────────┘
             │ Read Key              │ Write Char        │ Send Signal (Ctrl+C)
┌────────────▼───────────────────────▼─────────────┐     │
│       Device Manager (devmgr / "svc.dev")        │     │
│   (Talks to physical PL011 UART / Keyboard)      │     │
└──────────────────────────────────────────────────┘     ▼
┌──────────────────────────────────────────────────────────────────┐
│               Process Manager (procmgr / "svc.proc")             │
└──────────────────────────────────────────────────────────────────┘
```

---

## 2. 🌀 State Machine & Line Discipline (Cooked vs. Raw)

The terminal behavior is driven by two main operational modes:

### A. Cooked Mode (Canonical / `ICANON` enabled)
In this mode, input is processed line-by-line.
*   **Key Filtering**:
    *   **Backspace (`0x08` or `0x7f`)**: If the line buffer is not empty, pop the last byte. Send `\x08 \x08` (backspace, space, backspace) to the physical write channel to erase the character from the user's screen.
    *   **Carriage Return (`\r` or `\n`)**: Commit the line. Append `\n` to the line buffer, echo `\r\n` to the screen, and wake up any pending `TTY_CMD_READ` client blocked on this session.
    *   **Ctrl+C (`0x03`)**: Do not append to buffer. Call `procmgr` to send a `SIGINT` to the foreground process group.
    *   **Ctrl+D (`0x04`)**: Commit the line immediately without a trailing newline. Treat as `EOF`.
*   **Echoing**: Every standard printable ASCII character is immediately echoed back to the output channel.

### B. Raw Mode (`ICANON` disabled)
In this mode, characters are passed immediately to the client as they arrive, without any filtering, buffering, or echoing.
*   **Latency**: Instant (zero buffering).
*   **Use Cases**: Interactive applications, terminal games, screen editors (like `nano` / `vim`).

---

## 3. 📡 Protocol & IPC Details

All communications are synchronized via 148-byte IPC packets.

### A. Handling `TTY_CMD_READ`
1.  **Buffered Availability Check**: TTY checks if there is any committed line in its internal buffer.
    *   If **yes**, TTY returns the requested number of bytes to the client and clears the buffer.
    *   If **no**, TTY parks the client's session channel (retaining a reference to it) and yields. The client thread naturally blocks on its synchronous `channel_read` until TTY writes back.
2.  **Physical Key Input Processing**:
    *   When the physical UART signals that a new character is available, TTY processes it through the active line discipline.
    *   Once a newline is entered (or buffer is filled), TTY retrieves the waiting client's session, writes the data back, and clears the pending reader slot.

### B. Handling `TTY_CMD_WRITE`
1.  **Carriage Return Expansion**: If output-translation (`ONLCR`) is enabled, TTY scans the incoming string payload. For every `\n`, it outputs `\r\n` to the raw console.
2.  **Physical Dispatch**: Sends the finalized raw byte stream to `devmgr` (or the underlying physical MMIO buffer).

---

## 4. 🚀 Foreground Process Group & Signals (Job Control)

To support Ctrl+C (`SIGINT`), `svc.tty` must track which process group currently controls the terminal:
1.  **Registering Foreground PGID**: When `osh` spawns a program or pipeline, it sends a `TTY_CMD_BIND_PGID` command to `svc.tty` containing the new process group ID (or child PID).
2.  **Signal Delivery**: When `0x03` is typed:
    *   TTY identifies the registered `PGID`.
    *   TTY sends a specialized IPC packet to `procmgr` (`svc.proc`) instructing it to raise signal 2 (`SIGINT`) for that specific `PGID`.
    *   `procmgr` delivers the signal, safely terminating the foreground application while letting `osh` remain running to print the next prompt.
