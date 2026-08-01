# TOUCHD — Virtio-Input Touch Driver

## Status
- **Stage**: Implementation
- **Owner**: Orb 0.5 Graphical Stack
- **Tier**: EL0 user-space service
- **Service name**: `svc.touch`

## Overview
`touchd` is the multi-touch driver for OrbisOS / CapsuleOS. It probes the
Virtio-Input MMIO block for a device exposing `EV_ABS` (absent in the
mouse-only path used by `inputd`), allocates an event ring, and forwards
the parsed multi-touch state machine over a registered `svc.touch` IPC
channel to subscribing clients (typically the Spectrum compositor).

`touchd` is intentionally a **separate EL0 service** from `inputd`:
- distinct virtio-input slot scan path,
- distinct VA mapping range (`0x8000000`) to avoid MMIO collisions,
- distinct IPC name (`svc.touch`) so compositor can subscribe to both.

## Protocol: MT Protocol B
Touchscreen events are decoded in the Linux `input_event` subset:
- `EV_SYN / SYN_REPORT` (0, 0) — frame boundary, triggers dispatch
- `EV_ABS / ABS_MT_SLOT` (3, 47) — select slot 0..9
- `EV_ABS / ABS_MT_POSITION_X` (3, 53)
- `EV_ABS / ABS_MT_POSITION_Y` (3, 54)
- `EV_ABS / ABS_MT_TRACKING_ID` (3, 57) — 0xffffffff releases slot
- `EV_ABS / ABS_MT_PRESSURE` (3, 58)

## Public Symbols
- `TouchSlot` (16 bytes): `{ tracking_id: i32, x: i32, y: i32, pressure: u32 }`
- `TouchStatePacket` (168 bytes): `{ slots: [TouchSlot; 10], active_count: u8, reserved: [u8; 7] }`
- `MAX_TOUCH_SLOTS = 10`

## Dependencies
- `libcapsule` (syscalls + notify_init)
- `shared` (Status / Result types)

## Configuration
- `configs/auto.toml`: `name = "touchd"`, depends on `devmgr`, `procmgr`.

## Build Registration
- `Cargo.toml` workspace member
- `xtask.build.toml::userspace-apps.crates` row `hnx-touchd → touchd`