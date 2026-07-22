#![no_std]
#![no_main]

extern crate libcapsule;

use libcapsule::{kprintln, syscalls};
use shared::status::Status;

/// Block service protocol command codes.
const BLK_CMD_READ: u8 = 0;
const BLK_CMD_WRITE: u8 = 1;
const BLK_CMD_SIZE: u8 = 2;

#[no_mangle]
pub fn main() -> i32 {
    kprintln!("blkdev: init");

    // 1. Create a bidirectional server channel
    let raw = match syscalls::channel_create() {
        Ok(v) => v,
        Err(_) => {
            kprintln!("blkdev: channel_create failed");
            return -1;
        }
    };
    let server_chan = (raw >> 32) as u32 as usize;
    kprintln!("blkdev: channel={}", server_chan);

    // 2. Register global name "svc.blk"
    if let Err(e) = syscalls::channel_register("svc.blk", server_chan) {
        kprintln!("blkdev: channel_register failed: {:?}", e);
        return -2;
    }
    kprintln!("blkdev: registered svc.blk");

    let mut conn_buf = [0u8; 64];
    let mut conn_handles = [0u32; 2];

    // 3. Service main event loop
    loop {
        // Read incoming connections
        if let Ok(_) = syscalls::channel_read(server_chan, &mut conn_buf, &mut conn_handles) {
            if conn_handles[0] != 0 {
                let session_chan = conn_handles[0] as usize;

                // Handle commands inside this session
                loop {
                    let mut cmd_buf = [0u8; 528]; // Max size of write request (16 byte header + 512 byte payload)
                    let mut cmd_handles = [0u32; 2];
                    match syscalls::channel_read(session_chan, &mut cmd_buf, &mut cmd_handles) {
                        Ok(n) if n >= 16 => {
                            let command = cmd_buf[0];
                            let mut sector_bytes = [0u8; 8];
                            sector_bytes.copy_from_slice(&cmd_buf[8..16]);
                            let sector = u64::from_le_bytes(sector_bytes);

                            match command {
                                BLK_CMD_READ => {
                                    let mut sector_buf = [0u8; 512];
                                    let mut resp_buf = [0u8; 520]; // 8 byte status + 512 byte data

                                     let status = match syscalls::block_read(sector, sector_buf.as_mut_ptr() as usize) {
                                         Ok(()) => {
                                             resp_buf[8..520].copy_from_slice(&sector_buf);
                                             0i64
                                         }
                                         Err(e) => e.to_raw() as i64,
                                     };

                                     resp_buf[0..8].copy_from_slice(&status.to_le_bytes());
                                    let _ = syscalls::channel_write(session_chan, &resp_buf, &[]);
                                }

                                BLK_CMD_WRITE => {
                                    if n < 528 {
                                        let resp = (-3i64).to_le_bytes(); // InvalidArgs
                                        let _ = syscalls::channel_write(session_chan, &resp, &[]);
                                        continue;
                                    }
                                    let mut sector_buf = [0u8; 512];
                                    sector_buf.copy_from_slice(&cmd_buf[16..528]);

                                    let status = match syscalls::block_write(sector, sector_buf.as_ptr() as usize) {
                                        Ok(()) => 0i64,
                                        Err(e) => e.to_raw() as i64,
                                    };

                                    let resp = status.to_le_bytes();
                                    let _ = syscalls::channel_write(session_chan, &resp, &[]);
                                }

                                BLK_CMD_SIZE => {
                                    let mut resp_buf = [0u8; 16]; // 8 byte status + 8 byte size
                                    let status = match syscalls::block_size() {
                                        Ok(size) => {
                                            resp_buf[8..16].copy_from_slice(&size.to_le_bytes());
                                            0i64
                                        }
                                        Err(e) => e.to_raw() as i64,
                                    };

                                    resp_buf[0..8].copy_from_slice(&status.to_le_bytes());
                                    let _ = syscalls::channel_write(session_chan, &resp_buf, &[]);
                                }

                                _ => {
                                    let resp = (-3i64).to_le_bytes(); // InvalidArgs
                                    let _ = syscalls::channel_write(session_chan, &resp, &[]);
                                }
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
