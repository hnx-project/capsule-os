use crate::symbol_resolve::SymbolTable;
use crate::relocation::apply_relocation;
use ohlink_format::parser::OHLK_Parser;
use ohlink_format::builder::OHLK_Builder;
use ohlink_format::{OHLK_Entry, OHLK_Symbol, OHLK_Reloc, SegmentType};
use std::fs;
use std::path::Path;

pub struct Linker {
    symtab: SymbolTable,
    merged_text: Vec<u8>,
    merged_data: Vec<u8>,
    merged_rodata: Vec<u8>,
    merged_relocs: Vec<OHLK_Reloc>,
    base_address: u64,
}

impl Linker {
    pub fn new(base_address: u64) -> Self {
        Self {
            symtab: SymbolTable::new(),
            merged_text: Vec::new(),
            merged_data: Vec::new(),
            merged_rodata: Vec::new(),
            merged_relocs: Vec::new(),
            base_address,
        }
    }

    /// Reads an OHLINK target file, parses its sections, and registers symbols/relocations
    pub fn load_object_file<P: AsRef<Path>>(&mut self, path: P) -> Result<(), String> {
        let binary_data = fs::read(path).map_err(|e| format!("Failed to read object: {:?}", e))?;
        let parser = OHLK_Parser::new(&binary_data).map_err(|e| format!("Failed to parse: {:?}", e))?;

        // Local cache of symbols and entry mapping offsets inside the parser
        let mut object_symbols = Vec::new();
        let mut string_table = Vec::new();

        // Pass 1: Parse Meta segments (String Table, Symbol Table) to build local context
        for entry in parser.entries() {
            if entry.ty == SegmentType::Strtab.to_u32() {
                string_table = parser.get_segment_data(&entry).map_err(|e| format!("{:?}", e))?.to_vec();
            }
        }

        for entry in parser.entries() {
            if entry.ty == SegmentType::Symtab.to_u32() {
                let sym_data = parser.get_segment_data(&entry).map_err(|e| format!("{:?}", e))?;
                let count = sym_data.len() / OHLK_Symbol::SIZE;
                for i in 0..count {
                    let start = i * OHLK_Symbol::SIZE;
                    let symbol = OHLK_Symbol::from_bytes(&sym_data[start..(start + OHLK_Symbol::SIZE)])
                        .map_err(|e| format!("{:?}", e))?;
                    object_symbols.push(symbol);
                }
            }
        }

        // Register symbols to the global Symbol Table
        for sym in &object_symbols {
            let name = if sym.name_offset < string_table.len() as u64 {
                let sub = &string_table[sym.name_offset as usize..];
                let end = sub.iter().position(|&b| byte_is_null(b)).unwrap_or(sub.len());
                std::str::from_utf8(&sub[..end]).unwrap_or("").to_string()
            } else {
                format!("sym_offset_{}", sym.name_offset)
            };

            let is_defined = sym.section_idx != 0xFFFF;
            // Map relative section offsets to global merged offsets
            let global_value = if is_defined {
                let entry = parser.get_entry(sym.section_idx).map_err(|e| format!("{:?}", e))?;
                if entry.ty == SegmentType::Text.to_u32() {
                    self.base_address + self.merged_text.len() as u64 + sym.value
                } else if entry.ty == SegmentType::Data.to_u32() {
                    self.base_address + 0x100000 + self.merged_data.len() as u64 + sym.value
                } else {
                    sym.value
                }
            } else {
                0
            };

            self.symtab.add_symbol(name, sym.ty, sym.binding, global_value, sym.size, is_defined);
        }

        // Merge physical content segments
        for entry in parser.entries() {
            let seg_data = parser.get_segment_data(&entry).map_err(|e| format!("{:?}", e))?;
            if entry.ty == SegmentType::Text.to_u32() {
                self.merged_text.extend_from_slice(seg_data);
            } else if entry.ty == SegmentType::Data.to_u32() {
                self.merged_data.extend_from_slice(seg_data);
            } else if entry.ty == SegmentType::Rodata.to_u32() {
                self.merged_rodata.extend_from_slice(seg_data);
            } else if entry.ty == SegmentType::Reloc.to_u32() {
                let count = seg_data.len() / OHLK_Reloc::SIZE;
                for i in 0..count {
                    let start = i * OHLK_Reloc::SIZE;
                    let reloc = OHLK_Reloc::from_bytes(&seg_data[start..(start + OHLK_Reloc::SIZE)])
                        .map_err(|e| format!("{:?}", e))?;
                    self.merged_relocs.push(reloc);
                }
            }
        }

        Ok(())
    }

    /// Links all parsed modules together, applying P0 relocations and packing into a final executable .ohlk image.
    pub fn link<P: AsRef<Path>>(&mut self, out_path: P) -> Result<(), String> {
        // Map relocations and patch instruction stream
        // For standard relocations, we intercept instructions in merged_text
        for reloc in &self.merged_relocs {
            // Find target absolute address of referenced symbol
            // For MVP simplicity, we lookup by indices or map to symtab
            // Let's patch instructions in merged_text directly
            if reloc.offset + 4 <= self.merged_text.len() as u64 {
                let patch_offset = reloc.offset as usize;
                let original_instruction = u32::from_le_bytes([
                    self.merged_text[patch_offset],
                    self.merged_text[patch_offset + 1],
                    self.merged_text[patch_offset + 2],
                    self.merged_text[patch_offset + 3],
                ]);

                // Calculate relocation target (S) and location (P)
                let p = self.base_address + reloc.offset;
                let s = self.base_address; // Lookup from symbol table in fully-implemented resolver

                let patched_instruction = apply_relocation(
                    reloc.ty,
                    s,
                    reloc.addend,
                    p,
                    original_instruction,
                )?;

                self.merged_text[patch_offset..(patch_offset + 4)]
                    .copy_from_slice(&patched_instruction.to_le_bytes());
            }
        }

        // Build the final packed binary
        let mut builder = OHLK_Builder::new(1, 0); // ARM64, flags=0
        builder.add_segment(
            SegmentType::Text.to_u32(),
            OHLK_Entry::FLAG_R | OHLK_Entry::FLAG_X,
            &self.merged_text,
            self.merged_text.len() as u64,
        );

        if !self.merged_data.is_empty() {
            builder.add_segment(
                SegmentType::Data.to_u32(),
                OHLK_Entry::FLAG_R | OHLK_Entry::FLAG_W,
                &self.merged_data,
                self.merged_data.len() as u64,
            );
        }

        let binary_data = builder.build().map_err(|e| format!("Builder failed: {:?}", e))?;
        fs::write(out_path, binary_data).map_err(|e| format!("Failed to save output: {:?}", e))?;

        Ok(())
    }
}

fn byte_is_null(b: u8) -> bool {
    b == 0
}
