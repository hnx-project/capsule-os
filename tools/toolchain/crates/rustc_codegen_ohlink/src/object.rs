use cranelift_codegen::isa;
use cranelift_codegen::settings::{self, Configurable};
use cranelift_object::{ObjectBuilder, ObjectModule};
use target_lexicon::Triple;
use std::str::FromStr;

pub struct OhlinkCraneliftContext {
    pub module: ObjectModule,
}

impl OhlinkCraneliftContext {
    pub fn new() -> Self {
        let mut flag_builder = settings::builder();
        // Set standard bare-metal flags
        flag_builder.set("is_pic", "false").unwrap();
        flag_builder.set("use_colocated_libcalls", "false").unwrap();

        // Standard solution: Explicitly map custom AArch64 baremetal target to use generic ELF containers
        let mut triple = Triple::from_str("aarch64-unknown-none").unwrap();
        triple.binary_format = target_lexicon::BinaryFormat::Elf;

        let isa_builder = isa::lookup(triple).unwrap();
        let isa = isa_builder.finish(settings::Flags::new(flag_builder)).unwrap();

        let builder = ObjectBuilder::new(
            isa,
            "ohlink_module",
            cranelift_module::default_libcall_names(),
        ).unwrap();

        let module = ObjectModule::new(builder);

        Self { module }
    }

    /// Emits actual AArch64 machine-instructions representing a simple UART print loop
    pub fn emit_simple_uart_print_code(&mut self) -> Vec<u8> {
        let mut machine_code = Vec::new();
        
        // 1. movz x0, #0x0000 (0xD2800000)
        machine_code.extend_from_slice(&[0x00, 0x00, 0x80, 0xD2]);
        // 2. movz x0, #0x0900, lsl #16 (X variant movz imm=0x900, hw=1 => 0xD2A12000)
        machine_code.extend_from_slice(&[0x00, 0x20, 0xA1, 0xD2]);

        let message = b"Hello HNX\n";
        for &byte in message {
            // movz x1, #byte (0xD2800000 | (byte << 5) | 1)
            let inst_movz_val = 0xD2800001u32 | ((byte as u32) << 5);
            machine_code.extend_from_slice(&inst_movz_val.to_le_bytes());
            // strb w1, [x0] (0x39000001)
            machine_code.extend_from_slice(&[0x01, 0x00, 0x00, 0x39]);
        }

        // Infinite branch loop: b . (0x17FFFFFF)
        machine_code.extend_from_slice(&[0xFF, 0xFF, 0xFF, 0x17]);

        machine_code
    }
}
