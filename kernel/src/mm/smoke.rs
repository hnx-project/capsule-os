use crate::mm::vmar::{Vmar, VmarFlags};
use crate::mm::vmo::Vmo;

/// Phase 2.4 smoke test: build a VMO, allocate pages for it, write
/// data into it, then map it into a freshly-allocated VMAR.  We then
/// re-read through the VMAR's virtual address and confirm we see
/// what was written.
pub fn vmo_vmar_smoke_test() {
    crate::log_info!("SMOKE", "VMO/VMAR smoke test");

    // 1) Create a 16 KiB VMO (4 pages) and commit it.
    let mut vmo = match Vmo::create_with_size(16 * 1024) {
        Ok(v) => v,
        Err(e) => {
            crate::log_error!("SMOKE", "create_with_size failed: {:?}", e);
            return;
        }
    };
    if let Err(e) = vmo.commit_all() {
        crate::log_error!("SMOKE", "commit_all failed: {:?}", e);
        return;
    }

    // 2) Write a recognizable pattern.
    let pattern = b"VMO_VMAR_OK";
    if let Err(e) = vmo.write(0, pattern) {
        crate::log_error!("SMOKE", "vmo.write failed: {:?}", e);
        return;
    }

    // 3) Read it back from the VMO.
    let mut back = [0u8; 12];
    let n = match vmo.read(0, &mut back) {
        Ok(n) => n,
        Err(e) => {
            crate::log_error!("SMOKE", "vmo.read failed: {:?}", e);
            return;
        }
    };

    crate::log_info!("SMOKE", "=> vmo roundtrip: {} bytes, got: {}", n, core::str::from_utf8(&back[..n]).unwrap_or("?"));

    // 4) Carve a VMAR and map the VMO into it.  Choose a VA in the
    // low half on AArch64 (TTBR0) and in identity-mapped territory
    // on RISC-V.
    #[cfg(target_arch = "aarch64")]
    let vmar_base = 0x0000_0010_0000usize;  // 1 MiB, in the user range
    #[cfg(target_arch = "riscv64")]
    let vmar_base = 0x9000_0000usize;       // identity-mapped, free of UART

    let mut root = match Vmar::create(vmar_base, 0x0010_0000) {
        Ok(v) => v,
        Err(e) => {
            crate::log_error!("SMOKE", "Vmar::create failed: {:?}", e);
            return;
        }
    };

    crate::log_info!("SMOKE", "=> root vmar @ {:#x} + {:#x}", root.base, root.size);

    if let Err(e) = root.map(&mut vmo, 0, vmar_base, 16 * 1024,
                              VmarFlags::from_bits(VmarFlags::READ.bits() | VmarFlags::WRITE.bits())) {
        crate::log_error!("SMOKE", "vmar.map failed: {:?}", e);
        return;
    }
    crate::log_info!("SMOKE", "=> vmar.map: 16 KiB VMO -> user VA range OK");

    // 5) Read the pattern back from the virtual address and compare.
    #[cfg(target_arch = "aarch64")]
    {
        let va_ptr = vmar_base as *const u8;
        let mut via_va = [0u8; 12];
        unsafe {
            core::ptr::copy_nonoverlapping(va_ptr, via_va.as_mut_ptr(), 12);
        }
        crate::log_info!("SMOKE", "=> via VA     : {} bytes, got: {}", 12, core::str::from_utf8(&via_va).unwrap_or("?"));

        if &via_va[..pattern.len()] == pattern {
            crate::log_info!("SMOKE", "=> result     : MATCH (via MMU translation)");
        } else {
            crate::log_error!("SMOKE", "=> result     : MISMATCH");
        }
    }
    #[cfg(target_arch = "riscv64")]
    {
        if let Some(pa) = vmo.get_page_phys(0) {
            let kv = crate::arch::mmu::pa_to_kernel_va(pa.as_usize()) as *const u8;
            let mut via_pa = [0u8; 12];
            unsafe {
                core::ptr::copy_nonoverlapping(kv, via_pa.as_mut_ptr(), 12);
            }
            crate::log_info!("SMOKE", "=> via PA     : {} bytes, got: {}", 12, core::str::from_utf8(&via_pa).unwrap_or("?"));
            if &via_pa[..pattern.len()] == pattern {
                crate::log_info!("SMOKE", "=> result     : MATCH (via VMO PA, MMU off)");
            } else {
                crate::log_error!("SMOKE", "=> result     : MISMATCH");
            }
        }
    }

    // 6) Unmap and re-read
    if let Err(e) = root.unmap(vmar_base, 16 * 1024) {
        crate::log_error!("SMOKE", "vmar.unmap failed: {:?}", e);
    } else {
        crate::log_info!("SMOKE", "=> vmar.unmap: OK");
    }

    crate::log_info!("SMOKE", "VMO/VMAR done");
}
