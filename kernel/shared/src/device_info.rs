pub const DEVICE_TYPE_UART: u8 = 0;
pub const DEVICE_TYPE_GICD: u8 = 1;
pub const DEVICE_TYPE_GICC: u8 = 2;
pub const DEVICE_TYPE_TIMER: u8 = 3;
pub const DEVICE_TYPE_RTC: u8 = 4;
pub const DEVICE_TYPE_MEMORY: u8 = 5;
pub const DEVICE_TYPE_DISPLAY: u8 = 6;
pub const DEVICE_TYPE_MOUSE: u8 = 7;

pub const DEVICE_RECORD_SIZE: usize = 40;

/// Maximum number of device records we ever serialise.
pub const MAX_DEVICE_RECORDS: usize = 8;

/// Serialised device info record (40 bytes, packed).
///
/// Layout:
///   [0]      type: u8
///   [1-16]   name: [u8; 16]   (NUL-padded)
///   [17-24]  base: u64
///   [25-32]  size: u64
///   [33-36]  irq: u32         (0xFFFFFFFF = none)
///   [37-39]  _pad: [u8; 3]
#[repr(C, packed)]
pub struct DeviceInfoRecord {
    pub dev_type: u8,
    pub name: [u8; 16],
    pub base: u64,
    pub size: u64,
    pub irq: u32,
    _pad: [u8; 3],
}

impl DeviceInfoRecord {
    pub fn new(dev_type: u8, name: &str, base: u64, size: u64, irq: u32) -> Self {
        let mut name_buf = [0u8; 16];
        let len = name.len().min(15);
        name_buf[..len].copy_from_slice(&name.as_bytes()[..len]);
        DeviceInfoRecord {
            dev_type,
            name: name_buf,
            base,
            size,
            irq,
            _pad: [0u8; 3],
        }
    }
}

pub const DEVICE_BUFFER_SIZE: usize = 4 + MAX_DEVICE_RECORDS * DEVICE_RECORD_SIZE;
