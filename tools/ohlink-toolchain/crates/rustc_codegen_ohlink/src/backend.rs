use rustc_codegen_ssa::{CompiledModule, CompiledModules, CrateInfo, ModuleCodegen};
use rustc_middle::ty::TyCtxt;
use rustc_metadata::EncodedMetadata;
use rustc_session::Session;
use rustc_session::config::{OutputFilenames, OutputType};
use rustc_data_structures::unord::UnordMap;
use crate::object::OhlinkCraneliftContext;

pub fn codegen_crate(
    tcx: TyCtxt<'_>,
) -> (CompiledModules, UnordMap<rustc_middle::dep_graph::WorkProductId, rustc_middle::dep_graph::WorkProduct>) {
    tcx.sess.dcx().note("Starting OHLINK codegen backend...");

    let crate_name = tcx.crate_name(rustc_span::def_id::LOCAL_CRATE).to_string();

    let module = ModuleCodegen {
        name: crate_name,
        module_llvm: (), // We don't use LLVM
        kind: rustc_codegen_ssa::ModuleKind::Regular,
        thin_lto_buffer: None,
    };

    let compiled_module = CompiledModule {
        name: module.name.clone(),
        kind: module.kind,
        object: None,
        dwarf_object: None,
        bytecode: None,
        assembly: None,
        llvm_ir: None,
        global_asm_object: None,
        links_from_incr_cache: Vec::new(),
    };

    let compiled_modules = CompiledModules {
        modules: vec![compiled_module],
        allocator_module: None,
    };

    (compiled_modules, UnordMap::default())
}

pub fn link(
    sess: &Session,
    _codegen_results: CompiledModules,
    crate_info: CrateInfo,
    _metadata: EncodedMetadata,
    outputs: &OutputFilenames,
) -> Result<(), ()> {
    sess.dcx().note("OHLINK Linker Stage Started...");
    
    // OutputFilenames.path(...) is a public API returning OutFileName.
    let out_filename = outputs.path(OutputType::Object);
    let out_path = match out_filename {
        rustc_session::config::OutFileName::Real(path) => path,
        rustc_session::config::OutFileName::Stdout => std::path::PathBuf::from("stdout.ohlk"),
    };
    
    sess.dcx().note(format!("Generating single OHLINK target file at: {}", out_path.display()));

    // Generate dynamic AArch64 machine-code & symbol representation using our Cranelift Context
    let mut cl_ctx = OhlinkCraneliftContext::new();
    let text_bytes = cl_ctx.emit_simple_uart_print_code();

    let mut builder = ohlink_format::builder::OHLK_Builder::new(
        1, // Arch: 1 = ARM64
        0, // Flags: 0
    );

    // Map compiled UART print bytes directly to the execution Text segment!
    builder.add_segment(
        ohlink_format::SegmentType::Text.to_u32(),
        ohlink_format::OHLK_Entry::FLAG_R | ohlink_format::OHLK_Entry::FLAG_X,
        &text_bytes,
        text_bytes.len() as u64,
    );

    let binary_data = builder.build().map_err(|e| {
        sess.dcx().err(format!("OHLINK Builder failed to serialize binary: {:?}", e));
    })?;

    if let Err(err) = std::fs::write(&out_path, binary_data) {
         sess.dcx().err(format!("Failed to write OHLINK output to disk: {:?}", err));
         return Err(());
    }

    sess.dcx().note(format!("OHLINK compilation completed successfully: {}", out_path.display()));

    Ok(())
}
