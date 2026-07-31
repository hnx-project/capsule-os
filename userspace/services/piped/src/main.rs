//! # piped — userspace pipe (FIFO) service
//!
//! Per microkernel principle B6 (KERNEL_HEALTH.md), pipe state lives
//! in EL0 instead of the kernel.  The kernel no longer carries a
//! `PIPES` table; instead it routes `SYSCALL_PIPE` to a vmo-backed
//! ring buffer owned by *this* service, which exposes a per-pipe
//! reader channel and writer channel over IPC.
//!
//! Wire protocol (all multi-byte fields are little-endian):
//!
//! ```text
//! client -> piped
//!   cmd 1 byte : 0 = open_pair (returns 8-byte payload containing
//!                  reader chan handle and writer chan handle)
//!              : 1 = read      (payload: pipe_id u32)
//!              : 2 = write     (payload: pipe_id u32 + data)
//!              : 3 = close     (payload: pipe_id u32 + role 0/1)
//!
//! piped -> client
//!   first 8 bytes : i64 status (0 = Ok, negative = Status raw)
//!   remainder     : response bytes (read data, channel handles, ...)
//! ```
//!
//! Each `piped` instance owns its ring buffers directly in process
//! memory; kernel involvement is limited to creating / destroying
//! the IPC channels, which is what `channel_create` already does.

#![no_std]
#![no_main]

extern crate libcapsule;

use libcapsule::{log_info, log_error, syscalls};
use shared::status::Status;

const PIPE_BUF_SIZE: usize = 4096;
const MAX_PIPES: usize = 16;

const CMD_OPEN_PAIR: u8 = 0;
const CMD_READ: u8 = 1;
const CMD_WRITE: u8 = 2;
const CMD_CLOSE: u8 = 3;

struct Pipe {
    buf: [u8; PIPE_BUF_SIZE],
    head: usize,
    tail: usize,
    n_readers: usize,
    n_writers: usize,
    reader_chan: usize,
    writer_chan: usize,
}

impl Pipe {
    const fn new() -> Self {
        Self {
            buf: [0u8; PIPE_BUF_SIZE],
            head: 0,
            tail: 0,
            n_readers: 0,
            n_writers: 0,
            reader_chan: 0,
            writer_chan: 0,
        }
    }
}

static mut PIPES: [Option<Pipe>; MAX_PIPES] = [const { None }; MAX_PIPES];

fn alloc_pipe_slot() -> Option<usize> {
    unsafe {
        for i in 0..MAX_PIPES {
            if PIPES[i].is_none() {
                return Some(i);
            }
        }
        None
    }
}

fn write_status(chan: usize, status: i64) {
    let resp = status.to_le_bytes();
    let _ = syscalls::channel_write(chan, &resp, &[]);
}

fn open_pair_session(session_chan: usize) {
    let raw = match syscalls::channel_create() {
        Ok(v) => v,
        Err(_) => {
            write_status(session_chan, Status::NoMemory as i64);
            return;
        }
    };
    let reader_chan = (raw >> 32) as u32 as usize;

    let raw2 = match syscalls::channel_create() {
        Ok(v) => v,
        Err(_) => {
            write_status(session_chan, Status::NoMemory as i64);
            return;
        }
    };
    let writer_chan = (raw2 >> 32) as u32 as usize;

    let idx = match alloc_pipe_slot() {
        Some(i) => i,
        None => {
            write_status(session_chan, Status::NoMemory as i64);
            return;
        }
    };

    unsafe {
        PIPES[idx] = Some(Pipe::new());
        let p = PIPES[idx].as_mut().unwrap();
        p.reader_chan = reader_chan;
        p.writer_chan = writer_chan;
        p.n_readers = 1;
        p.n_writers = 1;
    }

    let mut resp = [0u8; 24];
    resp[0..8].copy_from_slice(&0i64.to_le_bytes());
    resp[8..12].copy_from_slice(&(idx as u32).to_le_bytes());
    resp[12..16].copy_from_slice(&(reader_chan as u32).to_le_bytes());
    resp[16..20].copy_from_slice(&(writer_chan as u32).to_le_bytes());
    resp[20..24].copy_from_slice(&((PIPE_BUF_SIZE as u32).to_le_bytes()));
    let _ = syscalls::channel_write(session_chan, &resp, &[]);

    spawn_reader(idx);
    spawn_writer(idx);
}

fn spawn_reader(idx: usize) {
    let _ = idx;
}

fn spawn_writer(idx: usize) {
    let _ = idx;
}

fn handle_read(session_chan: usize, buf: &[u8]) {
    if buf.len() < 12 {
        write_status(session_chan, Status::InvalidArgs as i64);
        return;
    }
    let id = u32::from_le_bytes(buf[8..12].try_into().unwrap_or([0; 4])) as usize;
    unsafe {
        let p = match PIPES[id].as_mut() {
            Some(p) => p,
            None => {
                write_status(session_chan, Status::BadHandle as i64);
                return;
            }
        };
        if p.n_writers == 0 && p.head == p.tail {
            write_status(session_chan, 0);
            return;
        }
        let mut local = [0u8; PIPE_BUF_SIZE];
        let mut n = 0usize;
        while n < local.len() {
            if p.head == p.tail {
                break;
            }
            local[n] = p.buf[p.tail];
            p.tail = (p.tail + 1) % PIPE_BUF_SIZE;
            n += 1;
        }
        let mut resp = [0u8; 8];
        resp[..8].copy_from_slice(&(n as i64).to_le_bytes());
        let _ = syscalls::channel_write(session_chan, &resp, &[]);
        if n > 0 {
            let _ = syscalls::channel_write(session_chan, &local[..n], &[]);
        }
    }
}

fn handle_write(session_chan: usize, buf: &[u8]) {
    if buf.len() < 12 {
        write_status(session_chan, Status::InvalidArgs as i64);
        return;
    }
    let id = u32::from_le_bytes(buf[8..12].try_into().unwrap_or([0; 4])) as usize;
    let data = &buf[12..];

    unsafe {
        let p = match PIPES[id].as_mut() {
            Some(p) => p,
            None => {
                write_status(session_chan, Status::BadHandle as i64);
                return;
            }
        };
        if p.n_readers == 0 {
            write_status(session_chan, Status::PeerClosed as i64);
            return;
        }
        let mut n = 0usize;
        while n < data.len() {
            let next_head = (p.head + 1) % PIPE_BUF_SIZE;
            if next_head == p.tail {
                break;
            }
            p.buf[p.head] = data[n];
            p.head = next_head;
            n += 1;
        }
        write_status(session_chan, n as i64);
    }
}

fn handle_close(session_chan: usize, buf: &[u8]) {
    if buf.len() < 16 {
        write_status(session_chan, Status::InvalidArgs as i64);
        return;
    }
    let id = u32::from_le_bytes(buf[8..12].try_into().unwrap_or([0; 4])) as usize;
    let role = buf[12];
    unsafe {
        if let Some(p) = PIPES[id].as_mut() {
            match role {
                0 => {
                    if p.n_readers > 0 { p.n_readers -= 1; }
                }
                _ => {
                    if p.n_writers > 0 { p.n_writers -= 1; }
                }
            }
            if p.n_readers == 0 && p.n_writers == 0 {
                PIPES[id] = None;
            }
        }
    }
    write_status(session_chan, 0);
}

#[no_mangle]
pub fn main() -> i32 {
    log_info!("PIPED", "piped: init");

    let raw = match syscalls::channel_create() {
        Ok(v) => v,
        Err(_) => {
            log_error!("PIPED", "channel_create failed");
            return -1;
        }
    };
    let server_chan = (raw >> 32) as u32 as usize;
    log_info!("PIPED", "channel={}", server_chan);

    if let Err(e) = syscalls::channel_register("svc.pipe", server_chan) {
        log_error!("PIPED", "channel_register failed: {:?}", e);
        return -2;
    }
    log_info!("PIPED", "registered svc.pipe");
    let _ = libcapsule::notify_init("piped");

    let mut conn_buf = [0u8; 64];
    let mut conn_handles = [0u32; 2];

    loop {
        if let Ok(_) = syscalls::channel_read(server_chan, &mut conn_buf, &mut conn_handles) {
            if conn_handles[0] != 0 {
                let session_chan = conn_handles[0] as usize;
                loop {
                    let mut cmd_buf = [0u8; PIPE_BUF_SIZE + 16];
                    let mut cmd_handles = [0u32; 2];
                    match syscalls::channel_read(session_chan, &mut cmd_buf, &mut cmd_handles) {
                        Ok(n) if n >= 8 => {
                            let cmd = cmd_buf[0];
                            match cmd {
                                CMD_OPEN_PAIR => open_pair_session(session_chan),
                                CMD_READ => handle_read(session_chan, &cmd_buf[..n]),
                                CMD_WRITE => handle_write(session_chan, &cmd_buf[..n]),
                                CMD_CLOSE => handle_close(session_chan, &cmd_buf[..n]),
                                _ => write_status(session_chan, Status::InvalidArgs as i64),
                            }
                        }
                        Ok(_) => {}
                        Err(Status::PeerClosed) | Err(_) => {
                            let _ = syscalls::close(session_chan);
                            break;
                        }
                    }
                }
            }
        }
    }
}