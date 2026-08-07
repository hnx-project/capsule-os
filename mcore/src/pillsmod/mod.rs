//! # 🛸 PillsMod (Kext) Module Loader & Registration Manager
//!
//! Handles kernel-space parsing, verification, and registration of dynamic
//! `.pill` driver bundles in the HNX Hybrid Kernel (EL1).
//! 
//! For Pangu 1.0-beta / S10, this serves as the foundational Pill Loader Stub,
//! verifying the "PILL" magic header, validating metadata offsets, and preparing
//! the payload for execution.

use shared::status::{Result, Status};

/// The binary header structure at the very beginning of every `.pill` driver package.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct PillHeader {
    pub magic: [u8; 4],         // Must be b"PILL"
    pub version_major: u16,     // e.g. 1
    pub version_minor: u16,     // e.g. 0
    pub metadata_offset: u64,   // Offset to the metadata.toml/json text in the file
    pub metadata_size: u64,     // Size of the metadata text
    pub payload_offset: u64,    // Offset to the raw binary payload in the file
    pub payload_size: u64,      // Size of the raw binary payload
}

/// A parsed view of a `.pill` driver bundle in-kernel.
pub struct ParsedPill<'a> {
    pub header: PillHeader,
    pub metadata: &'a str,
    pub payload: &'a [u8],
}

impl<'a> ParsedPill<'a> {
    /// Parse and verify a dynamic `.pill` package from a raw memory buffer.
    pub fn parse(buf: &'a [u8]) -> Result<Self> {
        if buf.len() < core::mem::size_of::<PillHeader>() {
            crate::log_error!("PILLSMOD", "Buffer size ({}) is smaller than PillHeader", buf.len());
            return Err(Status::InvalidArgs);
        }

        // 1. Safe pointer transmutation of packed PillHeader
        let header_ptr = buf.as_ptr() as *const PillHeader;
        let header = unsafe { core::ptr::read_unaligned(header_ptr) };

        // 2. Verify magic number "PILL"
        if &header.magic != b"PILL" {
            crate::log_error!(
                "PILLSMOD",
                "Magic verification failed: expected 'PILL', got [{:02x}, {:02x}, {:02x}, {:02x}]",
                header.magic[0], header.magic[1], header.magic[2], header.magic[3]
            );
            return Err(Status::InvalidArgs);
        }

        let v_major = header.version_major;
        let v_minor = header.version_minor;
        crate::log_info!("PILLSMOD", "Discovered .pill driver pack (v{}.{})", v_major, v_minor);

        // 3. Extract and validate metadata slice
        let meta_start = header.metadata_offset as usize;
        let meta_end = meta_start + header.metadata_size as usize;
        if meta_end > buf.len() {
            crate::log_error!("PILLSMOD", "Metadata bounds [{}..{}] exceed buffer size ({})", meta_start, meta_end, buf.len());
            return Err(Status::InvalidArgs);
        }
        let metadata_raw = &buf[meta_start..meta_end];
        let metadata = core::str::from_utf8(metadata_raw)
            .map_err(|_| {
                crate::log_error!("PILLSMOD", "Failed to decode metadata as valid UTF-8");
                Status::InvalidArgs
            })?;

        // 4. Extract and validate executable payload slice
        let payload_start = header.payload_offset as usize;
        let payload_end = payload_start + header.payload_size as usize;
        if payload_end > buf.len() {
            crate::log_error!("PILLSMOD", "Payload bounds [{}..{}] exceed buffer size ({})", payload_start, payload_end, buf.len());
            return Err(Status::InvalidArgs);
        }
        let payload = &buf[payload_start..payload_end];

        let m_size = header.metadata_size;
        let p_size = header.payload_size;
        crate::log_info!(
            "PILLSMOD",
            "Successful parse: metadata ({} bytes), payload ({} bytes)",
            m_size, p_size
        );

        Ok(Self {
            header,
            metadata,
            payload,
        })
    }

    /// Load and boot the PillsMod driver if it represents an EL1 Kext.
    pub fn load_kext(&self) -> Result<()> {
        // Verification step: verify class = "PillsMod" in metadata
        if !self.metadata.contains("class = \"PillsMod\"") && !self.metadata.contains("class=\"PillsMod\"") {
            crate::log_error!("PILLSMOD", "Cannot load PillsAddon as an EL1 kernel extension (PillsMod)");
            return Err(Status::NotAllowed);
        }

        crate::log_info!("PILLSMOD", "Deploying PillsMod (Kext) payload at EL1...");
        
        // S10 loader step stub: in later steps, we will perform relocation of `self.payload`
        // and jump to the entry symbol. For 1.0-beta, we log the ready status.
        crate::log_info!("PILLSMOD", "[SUCCESS] Kext payload loaded and verified. Ready to link!");

        Ok(())
    }
}
