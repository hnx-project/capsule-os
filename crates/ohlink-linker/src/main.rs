pub fn test_crc() {
    let data = b"123456789";
    let mut crc = 0xFFFFFFFFu32;
    for &byte in data {
        let lookup_idx = ((crc ^ byte as u32) & 0xFF) as usize;
        let tbl_val = ohlink_format::crc32::CRC32_TABLE[lookup_idx];
        let next_crc = (crc >> 8) ^ tbl_val;
        println!("byte: {}, lookup_idx: {}, tbl_val: 0x{:08X}, next_crc: 0x{:08X}", byte as char, lookup_idx, tbl_val, next_crc);
        crc = next_crc;
    }
    println!("Final CRC: 0x{:08X}", !crc);
}

fn main() {
    test_crc();
}
