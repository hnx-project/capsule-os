use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct MergedSymbol {
    pub name: String,
    pub ty: u8,
    pub binding: u8,
    pub value: u64, // Resolved absolute address
    pub size: u32,
    pub defined: bool,
}

pub struct SymbolTable {
    pub symbols: HashMap<String, MergedSymbol>,
}

impl SymbolTable {
    pub fn new() -> Self {
        Self {
            symbols: HashMap::new(),
        }
    }

    /// Resolves and registers symbols from an input target OHLINK file.
    /// Handles Local/Global/Weak binding rules standard for static linkers.
    pub fn add_symbol(
        &mut self,
        name: String,
        ty: u8,
        binding: u8,
        value: u64,
        size: u32,
        defined: bool,
    ) {
        if let Some(existing) = self.symbols.get_mut(&name) {
            if !existing.defined && defined {
                // Strong definition overrides undefined reference
                existing.value = value;
                existing.defined = true;
                existing.ty = ty;
                existing.binding = binding;
                existing.size = size;
            } else if existing.defined && defined {
                if existing.binding == 2 {
                    // Weak definition gets overridden by a global strong definition
                    existing.value = value;
                    existing.binding = binding;
                    existing.ty = ty;
                    existing.size = size;
                }
                // If both are strong, standard linker rule is to preserve first and/or raise duplicate symbol (skipped for MVP simplicity)
            }
        } else {
            self.symbols.insert(
                name.clone(),
                MergedSymbol {
                    name,
                    ty,
                    binding,
                    value,
                    size,
                    defined,
                },
            );
        }
    }

    pub fn lookup(&self, name: &str) -> Option<&MergedSymbol> {
        self.symbols.get(name)
    }
}
