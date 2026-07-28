use shared::status::Result;
use shared::launcher::ServiceDescriptor;
use crate::task::Process;
use crate::object::handle_table::KernelObject;
use crate::object::rights::Rights;
use crate::memory::vmo::Vmo;

/// Default slot index in the spawned process's handle table where the
/// kernel injects the BootFS VMO handle.  Mirrors the
/// `BOOTFS_VMO_HANDLE = 100` constant used by every EL0 service.
pub const DEFAULT_BOOTFS_HANDLE_SLOT: u32 = 100;

/// Unified Service Launcher for early kernel-space boot stage.
pub struct ServiceLauncher;

impl ServiceLauncher {
    pub fn launch(desc: &ServiceDescriptor<'static>) -> Result<u64> {
        let bytes = crate::rootfs::get_file(desc.path)
            .ok_or(shared::status::Status::NotFound)?;

        // Launch the user process through the standard process loader
        let pid = Process::launch_user_program(desc.name, bytes)?;

        // Inject the BootFS VMO into the spawned process's handle table
        // so it can resolve further EL0 services via
        // `libcapsule::ServiceLoader::new(100)`.  We honour
        // `desc.bootstrap_vmo_handle_index` when set, falling back to
        // the canonical slot 100 used by every EL0 service.  Without
        // this injection the spawned service could not bootstrap any
        // further descendants using the user-space ServiceLoader,
        // since the kernel-level `ServiceLauncher` reads BootFS
        // directly and never propagates a BootFS capability to the
        // child.
        let handle_idx = desc
            .bootstrap_vmo_handle_index
            .unwrap_or(DEFAULT_BOOTFS_HANDLE_SLOT);
        if let Some(proc) = crate::task::process::find_process_mut(pid) {
            let rights = Rights::READ.bits() | Rights::WRITE.bits();
            if desc.name == "loader" {
                // Dual injection: 100 for LoaderPhase (loader.img) and 101 for ServicesPhase (rootfs.img)
                let loader_vmo = unsafe {
                    Vmo::create_physical(crate::BOOTFS_PHYS_ADDR, crate::BOOTFS_PHYS_SIZE)?
                };
                let services_vmo = unsafe {
                    Vmo::create_physical(crate::SERVICES_PHYS_ADDR, crate::SERVICES_PHYS_SIZE)?
                };
                let _ = proc.handle_table.add_raw_handle(100, KernelObject::Vmo(loader_vmo), rights);
                let _ = proc.handle_table.add_raw_handle(101, KernelObject::Vmo(services_vmo), rights);
                crate::log_info!("BOOT", "Dual VMO injection completed for loader (100 -> loader.img, 101 -> rootfs.img)");
            } else {
                // Normal early service injection (Slot 100 -> services image)
                let rootfs_vmo = unsafe {
                    Vmo::create_physical(crate::SERVICES_PHYS_ADDR, crate::SERVICES_PHYS_SIZE)?
                };
                let _ = proc.handle_table.add_raw_handle(handle_idx, KernelObject::Vmo(rootfs_vmo), rights);
            }
        }

        // CANARY: check PID 1's L3 page is still intact after launch.
        #[cfg(target_arch = "aarch64")]
        unsafe {
            if let Some(proc) = crate::task::process::find_process_mut(1) {
                let pt = &proc.page_table;
                if pt.has_root() {
                    let check_va = 0x92000000;
                    let l0_idx = (check_va >> 39) & 0x1FF;
                    let l0e = crate::arch::aarch64::page_table::read_pte(pt.l0_pa(), l0_idx);
                    if l0e & 1 != 0 {
                        let l1_pa = (l0e & 0x0000_FFFF_FFFF_F000) as usize;
                        let l1_idx = (check_va >> 30) & 0x1FF;
                        let l1e = crate::arch::aarch64::page_table::read_pte(l1_pa, l1_idx);
                        if l1e & 1 != 0 {
                            let l2_pa = (l1e & 0x0000_FFFF_FFFF_F000) as usize;
                            let l2_idx = (check_va >> 21) & 0x1FF;
                            let l2e = crate::arch::aarch64::page_table::read_pte(l2_pa, l2_idx);
                            if l2e & 1 != 0 {
                                let l3_pa = (l2e & 0x0000_FFFF_FFFF_F000) as usize;
                                use crate::arch::mmu_facade::pa_to_kernel_va;
                                let l3_kva = pa_to_kernel_va(l3_pa) as *const u64;
                                for ci in 0..4 {
                                    let val = core::ptr::read_volatile(l3_kva.add(ci));
                                    if val & 3 != 3 {
                                        crate::log_error!("CANARY", "PID 1 L3[{}]={:#x} (expected valid page desc) pa={:#x} after launching PID {}", ci, val, l3_pa, pid);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        crate::log_info!("LAUNCHER", "Service '{}' (PID {}) launched successfully via ServiceLauncher!", desc.name, pid);
        Ok(pid)
    }
}

pub fn launch_loader() -> Result<()> {
    let loader_desc = ServiceDescriptor {
        name: "loader",
        path: "system/bin/loader",
        bootstrap_vmo_handle_index: Some(100),
    };

    ServiceLauncher::launch(&loader_desc)?;
    Ok(())
}
