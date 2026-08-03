use crate::mmu::Mmu;

/// Simple, robust virtual CPU executing core standard AArch64 registers and instruction set.
/// 31 General Purpose Registers (X0-X30), 1 Zero Register / Stack Pointer (SP), PC, and Flags.

pub struct Cpu {
    pub regs: [u64; 31],
    pub sp: u64,
    pub pc: u64,
    // Flags: Negative, Zero, Carry, Overflow
    pub n: bool,
    pub z: bool,
    pub c: bool,
    pub v: bool,
}

impl Cpu {
    pub fn new(entry_point: u64, stack_pointer: u64) -> Self {
        Self {
            regs: [0u64; 31],
            sp: stack_pointer,
            pc: entry_point,
            n: false,
            z: false,
            c: false,
            v: false,
        }
    }

    /// Read value from a register (X0-X30). Index 31 maps to SP or XZR depending on context.
    pub fn get_reg(&self, idx: u8) -> u64 {
        if idx < 31 {
            self.regs[idx as usize]
        } else {
            0 // XZR (Zero Register) in most general contexts
        }
    }

    /// Write value to a register (X0-X30). Index 31 acts as XZR (ignores writes).
    pub fn set_reg(&mut self, idx: u8, val: u64) {
        if idx < 31 {
            self.regs[idx as usize] = val;
        }
    }

    /// Step and execute a single 32-bit AArch64 instruction from memory.
    /// Returns true if execution should continue, false if a stop or infinite self-loop is hit.
    pub fn step(&mut self, mmu: &mut Mmu) -> bool {
        let instr = mmu.read32(self.pc);
        if instr == 0 {
            // Null instruction (undefined/crash or empty)
            return false;
        }

        let curr_pc = self.pc;
        self.pc += 4; // Advance PC by default

        // 1. Check for infinite branch loop: `b .`
        // AArch64 unconditional branch offset encoding:
        // Opcode binary: 0001 01xx xxxx xxxx xxxx xxxx xxxx xxxx
        // Top 6 bits: 000101 (0x5) => unconditional branch
        if (instr >> 26) == 0x5 {
            let offset_raw = instr & 0x3FFFFFF;
            // Sign extend 26-bit offset
            let offset = if (offset_raw & 0x2000000) != 0 {
                (offset_raw as i32 | -67108864) as i64
            } else {
                offset_raw as i64
            };
            let branch_target = (curr_pc as i64 + (offset << 2)) as u64;
            if branch_target == curr_pc {
                // Infinite branch loop detected! (e.g. `b .`)
                // Terminate runner gracefully as execution completed.
                return false;
            }
            self.pc = branch_target;
            return true;
        }

        // 2. Decode standard AArch64 ARM64 instructions used by our compiler:
        // LDR (register/immediate), STR (register/immediate), ADD, SUB, MOV, CMP, SVC, etc.

        // MOVZ (Move wide with zero)
        // 1101 0010 1xxv vvvv vvvv vvvv vvvv dddd (X variant)
        if (instr & 0xFF800000) == 0xD2800000 {
            let rd = (instr & 0x1F) as u8;
            let imm = ((instr >> 5) & 0xFFFF) as u64;
            let hw = ((instr >> 21) & 0x3) as u8;
            self.set_reg(rd, imm << (hw * 16));
            return true;
        }

        // ADD (immediate)
        // 1001 0001 00xx xxxx xxxx xxxx xxxx xxxx (X variant)
        if (instr & 0xFF000000) == 0x91000000 {
            let rd = (instr & 0x1F) as u8;
            let rn = ((instr >> 5) & 0x1F) as u8;
            let imm = ((instr >> 10) & 0xFFF) as u64;
            let sh = ((instr >> 22) & 1) as u8;
            let val_rn = if rn == 31 { self.sp } else { self.get_reg(rn) };
            let added_val = imm << (sh * 12);
            let res = val_rn.wrapping_add(added_val);
            if rd == 31 { self.sp = res; } else { self.set_reg(rd, res); }
            return true;
        }

        // STR (immediate, unsigned offset) - 64-bit variant
        // 1111 1001 00xx xxxx xxxx xxxx xxxx xxxx
        if (instr & 0xFFC00000) == 0xF9000000 {
            let rt = (instr & 0x1F) as u8;
            let rn = ((instr >> 5) & 0x1F) as u8;
            let imm = ((instr >> 10) & 0xFFF) as u64;
            let base_addr = if rn == 31 { self.sp } else { self.get_reg(rn) };
            let val_rt = if rt == 31 { self.sp } else { self.get_reg(rt) };
            // Scale offset by 8 bytes for 64-bit
            let offset = imm * 8;
            mmu.write64(base_addr.wrapping_add(offset), val_rt);
            return true;
        }

        // STR (immediate, unsigned offset) - 8-bit variant
        // 0011 1001 00xx xxxx xxxx xxxx xxxx xxxx
        if (instr & 0xFFC00000) == 0x39000000 {
            let rt = (instr & 0x1F) as u8;
            let rn = ((instr >> 5) & 0x1F) as u8;
            let imm = ((instr >> 10) & 0xFFF) as u64;
            let base_addr = if rn == 31 { self.sp } else { self.get_reg(rn) };
            let val_rt = (self.get_reg(rt) & 0xFF) as u8;
            mmu.write8(base_addr.wrapping_add(imm), val_rt);
            return true;
        }

        // LDR (immediate, unsigned offset) - 64-bit variant
        // 1111 1001 01xx xxxx xxxx xxxx xxxx xxxx
        if (instr & 0xFFC00000) == 0xF9400000 {
            let rt = (instr & 0x1F) as u8;
            let rn = ((instr >> 5) & 0x1F) as u8;
            let imm = ((instr >> 10) & 0xFFF) as u64;
            let base_addr = if rn == 31 { self.sp } else { self.get_reg(rn) };
            // Scale offset by 8 bytes for 64-bit
            let offset = imm * 8;
            let loaded = mmu.read64(base_addr.wrapping_add(offset));
            if rt == 31 { self.sp = loaded; } else { self.set_reg(rt, loaded); }
            return true;
        }

        // LDR (immediate, unsigned offset) - 8-bit variant
        // 0011 1001 01xx xxxx xxxx xxxx xxxx xxxx (LDRB)
        if (instr & 0xFFC00000) == 0x39400000 {
            let rt = (instr & 0x1F) as u8;
            let rn = ((instr >> 5) & 0x1F) as u8;
            let imm = ((instr >> 10) & 0xFFF) as u64;
            let base_addr = if rn == 31 { self.sp } else { self.get_reg(rn) };
            let loaded = mmu.read8(base_addr.wrapping_add(imm)) as u64;
            if rt == 31 { self.sp = loaded; } else { self.set_reg(rt, loaded); }
            return true;
        }

        // RET (Register return)
        // 1101 0110 0101 1111 0000 00xx xxxx xxxx
        // Ret usually targets register X30 (LR)
        if (instr & 0xFFFFFC1F) == 0xD65F03C0 {
            let rn = ((instr >> 5) & 0x1F) as u8;
            self.pc = self.get_reg(rn);
            return true;
        }

        // Default: If an unsupported instruction is met, we print it as a warning and continue
        println!("[Runner Warning] Executed unhandled instruction 0x{:08X} at PC 0x{:08X}", instr, curr_pc);
        true
    }
}
