use ohlink_format::RelocType;

/// Calculates AArch64 standard P0 relocation values.
/// Formulas correspond with OHLINK-SPEC.md definitions.
///
/// * `s` - Absolute physical/virtual address of the target symbol.
/// * `a` - Addend.
/// * `p` - Absolute physical/virtual address of the patch point itself.

pub fn apply_relocation(
    reloc_type: u32,
    s: u64,
    a: i64,
    p: u64,
    original_instruction: u32,
) -> Result<u32, &'static str> {
    let r_ty = RelocType::from_u32(reloc_type);
    match r_ty {
        RelocType::Abs64 => {
            // S + A -> 64-bit absolute value. Handled separately if writing full 64-bit address,
            // but if patched directly inside 32-bit registers we return its low word.
            let val = (s as i64 + a) as u64;
            Ok((val & 0xFFFFFFFF) as u32)
        }
        RelocType::Call26 => {
            // (S + A - P) >> 2
            let offset = (s as i64 + a - p as i64) >> 2;
            if offset < -33554432 || offset > 33554431 {
                return Err("Relocation overflow: R_AARCH64_CALL26 out of range (+/-128MB)");
            }
            // AArch64 BL instruction top 6-bits are 100101 (0x25). Lower 26-bits are the branch offset.
            let mask = 0xFC000000;
            let patched = (original_instruction & mask) | ((offset as u32) & 0x3FFFFFF);
            Ok(patched)
        }
        RelocType::AdrPrelPgHi21 => {
            // (Page(S + A) - Page(P)) >> 12
            let page_s = (s as i64 + a) & !0xFFF;
            let page_p = (p as i64) & !0xFFF;
            let offset = (page_s - page_p) >> 12;
            if offset < -1048576 || offset > 1048575 {
                return Err("Relocation overflow: R_AARCH64_ADR_PREL_PG_HI21 out of range (+/-4GB)");
            }
            // ADRP instruction offset encoding is non-contiguous:
            // bits 29..30: lowest 2 bits of offset
            // bits 5..23: highest 19 bits of offset
            let immlo = (offset & 3) as u32;
            let immhi = ((offset >> 2) & 0x7FFFF) as u32;
            let clear_mask = !((3 << 29) | (0x7FFFF << 5));
            let patched = (original_instruction & clear_mask) | (immlo << 29) | (immhi << 5);
            Ok(patched)
        }
        RelocType::AddAbsLo12Nc => {
            // (S + A) & 0xFFF
            let val = (s as i64 + a) & 0xFFF;
            // ADD immediate instruction offset encoding: bits 10..21
            let clear_mask = !(0xFFF << 10);
            let patched = (original_instruction & clear_mask) | ((val as u32) << 10);
            Ok(patched)
        }
        RelocType::Ldst64AbsLo12Nc => {
            // ((S + A) & 0xFFF) >> 3
            let val = ((s as i64 + a) & 0xFFF) >> 3;
            // LDR/STR offset encoding: bits 10..21
            let clear_mask = !(0xFFF << 10);
            let patched = (original_instruction & clear_mask) | ((val as u32) << 10);
            Ok(patched)
        }
        RelocType::Ldst32AbsLo12Nc => {
            // ((S + A) & 0xFFF) >> 2
            let val = ((s as i64 + a) & 0xFFF) >> 2;
            // LDR/STR offset encoding: bits 10..21
            let clear_mask = !(0xFFF << 10);
            let patched = (original_instruction & clear_mask) | ((val as u32) << 10);
            Ok(patched)
        }
        RelocType::Custom(_) => Err("Unsupported or unhandled custom relocation type"),
    }
}
