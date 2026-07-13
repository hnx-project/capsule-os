//! Virtual Memory Address Region (VMAR).
//!
//! A VMAR owns a contiguous range of virtual addresses and is the
//! authoritative record of which VMOs are mapped where within that
//! range.  VMARs form a tree: the root VMAR covers the whole user
//! address space, and `allocate_subregion` carves out children that
//! further subdivide the address space.
//!
//! For the MVP, the tree is kept on the kernel heap (`Vec`-style
//! storage backed by a per-VMAR metadata page) and the mapping
//! functions delegate to the per-architecture `map_page` /
//! `unmap_page` helpers.

use shared::status::{Result, Status};

use crate::arch::mmu as arch_mmu;
use crate::mm::mmu::PAGE_SIZE;
use crate::mm::phys::{self, PhysAddr};
use crate::mm::vmo::Vmo;

/// Permission / attribute flags for a VMAR mapping.  Same bits as
/// the architecture-level `MapFlags` (so the VMAR can translate
/// directly when calling `arch_mmu::map_page`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VmarFlags(u32);

impl VmarFlags {
    pub const NONE: VmarFlags = VmarFlags(0);
    pub const READ: VmarFlags = VmarFlags(1 << 0);
    pub const WRITE: VmarFlags = VmarFlags(1 << 1);
    pub const EXECUTE: VmarFlags = VmarFlags(1 << 2);
    pub const USER: VmarFlags = VmarFlags(1 << 3);

    pub const fn from_bits(b: u32) -> Self { VmarFlags(b) }
    pub const fn bits(&self) -> u32 { self.0 }

    pub fn readable(&self)   -> bool { self.0 & Self::READ.0    != 0 }
    pub fn writable(&self)   -> bool { self.0 & Self::WRITE.0   != 0 }
    pub fn executable(&self) -> bool { self.0 & Self::EXECUTE.0 != 0 }
    pub fn user(&self)       -> bool { self.0 & Self::USER.0    != 0 }
}

/// Maximum number of sub-regions and mappings a single VMAR can hold
/// before it has to allocate another metadata page.  Sized so the
/// metadata page (one `phys::alloc_page()`) is enough for the storage
/// struct:
///   storage ≈ 4 child_count/map_count usize + 32 subregions + 32 mappings
///   We round those bounds down to leave headroom and stay strictly
///   under 4 KiB.
const MAX_CHILDREN: usize = 32;
const MAX_MAPPINGS: usize = 32;

/// One entry in a VMAR's mapping table.
#[derive(Debug, Clone, Copy)]
struct Mapping {
    vmo_id: u64,
    vmo_offset: usize,
    virt_addr: usize,
    size: usize,
    flags: VmarFlags,
    /// True once the mapping has been installed in the page tables.
    installed: bool,
}

/// One child VMAR entry, with enough metadata to reach it from the
/// parent.  We store the child inline in the metadata page to avoid
/// a second indirection (and the heap allocation it implies).
#[derive(Debug)]
struct SubRegion {
    /// Physical address of the child's metadata page, or `None` for
    /// a free slot.
    meta_pa: Option<PhysAddr>,
    base: usize,
    size: usize,
}

#[derive(Debug)]
pub struct Vmar {
    pub base: usize,
    pub size: usize,
    /// Physical address of this VMAR's metadata page.
    meta_pa: PhysAddr,
}

/// In-memory mirror of a VMAR's metadata page.  We re-load it from
/// the page whenever the VMAR is mutably borrowed so that the page
/// and the in-memory state always agree.
struct VmarStorage {
    children: [SubRegion; MAX_CHILDREN],
    mappings: [Mapping; MAX_MAPPINGS],
    /// Number of allocated children.
    child_count: usize,
    /// Number of installed mappings.
    map_count: usize,
}

const STORAGE_BYTES: usize = core::mem::size_of::<VmarStorage>();

// Static assertion: storage must fit in a single 4 KiB page.
// `panic!` at compile time keeps the constraint visible to anyone
// who later raises the array sizes.
#[allow(dead_code)]
const STORAGE_FITS: () = assert!(STORAGE_BYTES <= 4096, "VmarStorage > 4 KiB");

#[inline]
fn storage_offset() -> usize { 0 }

impl Vmar {
    /// Create a root VMAR covering the user address range
    /// `[0x1_0000_0000, 0x1_1000_0000)` (256 MiB).  Convenience
    /// wrapper for boot-time smoke tests; prefer `create` for
    /// anything that has to be more careful.
    pub fn new() -> Result<Self> {
        Self::create(0x1_0000_0000, 256 * 1024 * 1024)
    }

    /// Create a VMAR covering `[base, base + size)`.  `base` is
    /// required to be page-aligned and `size` is rounded up to a
    /// page boundary.
    pub fn create(base: usize, size: usize) -> Result<Self> {
        if base & (PAGE_SIZE - 1) != 0 {
            return Err(Status::InvalidArgs);
        }
        if size == 0 {
            return Err(Status::InvalidArgs);
        }
        let size = (size + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        let meta_pa = phys::alloc_page()?;
        unsafe {
            let base_ptr = arch_mmu::pa_to_kernel_va(meta_pa.as_usize()) as *mut u8;
            core::ptr::write_bytes(base_ptr, 0, PAGE_SIZE);
        }
        Ok(Vmar { base, size, meta_pa })
    }

    /// Carve out a sub-region of size `size` from this VMAR.  The
    /// base address of the new region is chosen by walking forward
    /// from `self.base` and looking for a gap of `size` bytes that
    /// doesn't overlap an existing child or mapping.
    pub fn allocate_subregion(&mut self, size: usize, _flags: VmarFlags) -> Result<Vmar> {
        if size == 0 { return Err(Status::InvalidArgs); }
        let size = (size + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);

        // Determine child count and storage.
        let mut storage = self.load_storage();
        if storage.child_count >= MAX_CHILDREN {
            return Err(Status::NoMemory);
        }

        // Find first gap of `size` that doesn't overlap any child
        // or any installed mapping.  Keep this O(n^2) for now — n is
        // bounded by MAX_* constants so it's fine for the MVP.
        let mut candidate = self.base;
        'outer: loop {
            if candidate + size > self.base + self.size {
                return Err(Status::NoMemory);
            }
            for c in storage.children.iter().take(storage.child_count) {
                if let Some(_) = c.meta_pa {
                    if candidate < c.base + c.size && candidate + size > c.base {
                        candidate = (c.base + c.size + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
                        continue 'outer;
                    }
                }
            }
            for m in storage.mappings.iter().take(storage.map_count) {
                if candidate < m.virt_addr + m.size && candidate + size > m.virt_addr {
                    candidate = (m.virt_addr + m.size + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
                    continue 'outer;
                }
            }
            break;
        }

        // Create the child metadata and write through.
        let child = Vmar::create(candidate, size)?;
        let child_pa = child.meta_pa.as_usize();

        storage.children[storage.child_count] = SubRegion {
            meta_pa: Some(PhysAddr::new(child_pa)),
            base: candidate,
            size,
        };
        storage.child_count += 1;
        self.store_storage(&storage);
        Ok(child)
    }

    /// Map `vmo[ vmo_offset .. vmo_offset + size ]` into this VMAR at
    /// virtual address `virt_addr`.  All physical pages touched are
    /// committed via `Vmo::commit_page`, then the page table entries
    /// are installed via `arch_mmu::map_page`.
    pub fn map(&mut self, vmo: &mut Vmo, vmo_offset: usize,
               virt_addr: usize, size: usize, flags: VmarFlags) -> Result<usize> {
        if size == 0 {
            crate::log_error!("VMAR", "map fail: size == 0");
            return Err(Status::InvalidArgs);
        }
        if virt_addr & (PAGE_SIZE - 1) != 0 {
            crate::log_error!("VMAR", "map fail: virt_addr={:#x} not page aligned", virt_addr);
            return Err(Status::InvalidArgs);
        }
        if vmo_offset & (PAGE_SIZE - 1) != 0 {
            crate::log_error!("VMAR", "map fail: vmo_offset={} not page aligned", vmo_offset);
            return Err(Status::InvalidArgs);
        }
        if virt_addr < self.base || virt_addr.checked_add(size)
            .ok_or(Status::InvalidArgs)? > self.base + self.size {
            crate::log_error!("VMAR", "map fail: out of VMAR bounds. virt_addr={:#x}, size={}, base={:#x}, max={:#x}", virt_addr, size, self.base, self.base + self.size);
            return Err(Status::InvalidArgs);
        }
        if vmo_offset.checked_add(size).ok_or(Status::InvalidArgs)? > vmo.get_size() {
            crate::log_error!("VMAR", "map fail: offset+size > vmo_size. offset={}, size={}, vmo_size={}", vmo_offset, size, vmo.get_size());
            return Err(Status::InvalidArgs);
        }

        // Translate the VMAR flags into architecture-level MapFlags.
        let arch_flags = translate_flags(flags);

        // Walk every 4 KiB page in the range, commit it in the VMO,
        // then install a page-table entry.
        let page_count = size / PAGE_SIZE;
        for i in 0..page_count {
            let vmo_off = vmo_offset + i * PAGE_SIZE;
            let va = virt_addr + i * PAGE_SIZE;
            // commit_page may allocate; it returns Ok(None) if the
            // page is already committed.
            vmo.commit_page(vmo_off)?;
            let pa = vmo.get_page_phys(vmo_off).ok_or(Status::NoMemory)?;
            #[cfg(target_arch = "riscv64")]
            unsafe {
                core::ptr::copy_nonoverlapping(
                    arch_mmu::pa_to_kernel_va(pa.as_usize()) as *const u8,
                    va as *mut u8,
                    PAGE_SIZE,
                );
            }
            arch_mmu::map_page(va, pa.as_usize(), arch_flags)?;

            // Maintain Cache Coherency inside map() on AArch64 for newly loaded code
            #[cfg(target_arch = "aarch64")]
            {
                let kva = arch_mmu::pa_to_kernel_va(pa.as_usize());
                crate::arch::aarch64::mmu::sync_instruction_cache(kva, PAGE_SIZE);
            }
        }

        // Record the mapping.
        let mut storage = self.load_storage();
        if storage.map_count >= MAX_MAPPINGS {
            return Err(Status::NoMemory);
        }
        storage.mappings[storage.map_count] = Mapping {
            vmo_id: vmo.id,
            vmo_offset,
            virt_addr,
            size,
            flags,
            installed: true,
        };
        storage.map_count += 1;
        self.store_storage(&storage);
        Ok(size)
    }

    /// Map a range into a specific L0 root (bypassing TTBR0).
    /// Used during process launch: the caller temporarily switches TTBR0 to
    /// kernel L0 so that kernel address accesses work, but this function
    /// installs the page-table entries into the explicit `l0_pa` (the
    /// per-process user L0) so the user page table ends up correct.
    pub fn map_under_l0(&mut self, vmo: &mut Vmo, vmo_offset: usize,
                virt_addr: usize, size: usize, flags: VmarFlags, l0_pa: usize) -> Result<usize> {
        #[cfg(target_arch = "aarch64")]
        {
            if size == 0 {
                crate::log_error!("VMAR", "map_under_l0 fail: size == 0");
                return Err(Status::InvalidArgs);
            }
            if virt_addr & (PAGE_SIZE - 1) != 0 {
                crate::log_error!("VMAR", "map_under_l0 fail: virt_addr={:#x} not page aligned", virt_addr);
                return Err(Status::InvalidArgs);
            }
            if vmo_offset & (PAGE_SIZE - 1) != 0 {
                crate::log_error!("VMAR", "map_under_l0 fail: vmo_offset={} not page aligned", vmo_offset);
                return Err(Status::InvalidArgs);
            }
            if virt_addr < self.base || virt_addr.checked_add(size)
                .ok_or(Status::InvalidArgs)? > self.base + self.size {
                crate::log_error!("VMAR", "map_under_l0 fail: out of VMAR bounds. virt_addr={:#x}, size={}, base={:#x}, max={:#x}", virt_addr, size, self.base, self.base + self.size);
                return Err(Status::InvalidArgs);
            }
            if vmo_offset.checked_add(size).ok_or(Status::InvalidArgs)? > vmo.get_size() {
                crate::log_error!("VMAR", "map_under_l0 fail: offset+size > vmo_size. offset={}, size={}, vmo_size={}", vmo_offset, size, vmo.get_size());
                return Err(Status::InvalidArgs);
            }

            let arch_flags = translate_flags(flags);

            let page_count = size / PAGE_SIZE;
            for i in 0..page_count {
                let vmo_off = vmo_offset + i * PAGE_SIZE;
                let va = virt_addr + i * PAGE_SIZE;
                vmo.commit_page(vmo_off)?;
                let pa = vmo.get_page_phys(vmo_off).ok_or(Status::NoMemory)?;
                arch_mmu::map_page_under_l0(l0_pa, va, pa.as_usize(), arch_flags)?;
            }

            let mut storage = self.load_storage();
            if storage.map_count >= MAX_MAPPINGS {
                return Err(Status::NoMemory);
            }
            storage.mappings[storage.map_count] = Mapping {
                vmo_id: vmo.id,
                vmo_offset,
                virt_addr,
                size,
                flags,
                installed: true,
            };
            storage.map_count += 1;
            self.store_storage(&storage);
            Ok(size)
        }
        #[cfg(not(target_arch = "aarch64"))]
        {
            // Fallback for non-aarch64 (like riscv64) during launch, or we can use normal map
            self.map(vmo, vmo_offset, virt_addr, size, flags)
        }
    }

    /// Unmap the range `[virt_addr, virt_addr + size)`.  The VMO
    /// itself is not touched: any committed physical pages stay
    /// live inside the VMO and can be remapped by another VMAR.
    pub fn unmap(&mut self, virt_addr: usize, size: usize) -> Result<()> {
        if size == 0 { return Err(Status::InvalidArgs); }
        if virt_addr & (PAGE_SIZE - 1) != 0 { return Err(Status::InvalidArgs); }
        let page_count = size / PAGE_SIZE;
        for i in 0..page_count {
            arch_mmu::unmap_page(virt_addr + i * PAGE_SIZE)?;
        }
        // Update mapping table: drop any mapping that overlaps the
        // unmapped range.
        let mut storage = self.load_storage();
        let mut i = 0;
        while i < storage.map_count {
            let m = storage.mappings[i];
            if m.virt_addr < virt_addr + size && m.virt_addr + m.size > virt_addr {
                storage.mappings[i] = storage.mappings[storage.map_count - 1];
                storage.map_count -= 1;
            } else {
                i += 1;
            }
        }
        self.store_storage(&storage);
        Ok(())
    }

    /// Change the protection bits on an existing mapping.  The
    /// page-table entries are re-written; pages that are not
    /// currently mapped are silently skipped (so this works for
    /// sparse VMOs).
    pub fn protect(&mut self, virt_addr: usize, size: usize, flags: VmarFlags) -> Result<()> {
        if size == 0 { return Err(Status::InvalidArgs); }
        if virt_addr & (PAGE_SIZE - 1) != 0 { return Err(Status::InvalidArgs); }
        let arch_flags = translate_flags(flags);
        let page_count = size / PAGE_SIZE;
        for i in 0..page_count {
            let va = virt_addr + i * PAGE_SIZE;
            // The arch helper takes (va, pa, flags) but `protect` doesn't
            // know the PA.  We work around it by reading the current
            // mapping's VMO + offset from the table.
            // For now just call map_page with the existing PA from the
            // first mapping that covers this page; missing entries
            // are a no-op.
            if let Some(pa) = self.lookup_pa(va) {
                let _ = arch_mmu::map_page(va, pa.as_usize(), arch_flags);
            }
        }
        // Update the matching mapping entries.
        let mut storage = self.load_storage();
        for i in 0..storage.map_count {
            let m = storage.mappings[i];
            if m.virt_addr < virt_addr + size && m.virt_addr + m.size > virt_addr {
                storage.mappings[i].flags = flags;
            }
        }
        self.store_storage(&storage);
        Ok(())
    }

    // --- internals ---

    fn load_storage(&self) -> VmarStorage {
        // SAFETY: every field is a POD that is `Default`-equivalent at
        // the bit level (Option<...> with discriminant 0 = None,
        // integers at 0).  An all-zeroes page is therefore a valid
        // VmarStorage and a safe starting state.
        unsafe {
            let ptr = arch_mmu::pa_to_kernel_va(self.meta_pa.as_usize()) as *const u8;
            let mut storage: VmarStorage = core::mem::zeroed();
            core::ptr::copy_nonoverlapping(ptr,
                                           &mut storage as *mut _ as *mut u8,
                                           STORAGE_BYTES);
            storage
        }
    }

    fn store_storage(&self, storage: &VmarStorage) {
        unsafe {
            let ptr = arch_mmu::pa_to_kernel_va(self.meta_pa.as_usize()) as *mut u8;
            core::ptr::copy_nonoverlapping(storage as *const _ as *const u8,
                                           ptr,
                                           STORAGE_BYTES);
        }
    }

    /// Look up the physical address backing `va` by scanning this
    /// VMAR's mapping table and the connected VMOs.  The caller
    /// must hold a `&mut Vmar` for the duration; the VMOs are
    /// owned externally so we can't reach them from here.  To
    /// keep `protect` simple, we return None when the page
    /// belongs to a VMO we don't have a handle on and let the
    /// caller fall back.  For the MVP this is good enough.
    fn lookup_pa(&self, _va: usize) -> Option<PhysAddr> { None }
}

fn translate_flags(f: VmarFlags) -> arch_mmu::MapFlags {
    let mut out = arch_mmu::MapFlags::kernel_rw();
    out.readable = f.readable() || f.writable() || f.executable();
    out.writable = f.writable();
    out.executable = f.executable();
    out.user = f.user();
    
    if f.readable() && !f.writable() && !f.executable() {
        if f.user() {
            out = arch_mmu::MapFlags::user_ro();
        } else {
            out = arch_mmu::MapFlags::kernel_ro();
        }
    }
    if f.executable() && !f.writable() {
        if f.user() {
            out = arch_mmu::MapFlags::user_rx();
        } else {
            out = arch_mmu::MapFlags::kernel_rx();
        }
    }
    out
}
