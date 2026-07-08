#![feature(rustc_private)]

extern crate rustc_codegen_ssa;
extern crate rustc_middle;
extern crate rustc_driver;
extern crate rustc_metadata;
extern crate rustc_session;
extern crate rustc_span;
extern crate rustc_target;
extern crate rustc_errors;
extern crate rustc_data_structures;

pub mod backend;
pub mod object;

use rustc_codegen_ssa::traits::CodegenBackend;
use rustc_codegen_ssa::{CompiledModules, CrateInfo};
use rustc_session::Session;
use rustc_session::config::OutputFilenames;
use rustc_middle::ty::TyCtxt;
use rustc_metadata::EncodedMetadata;
use rustc_data_structures::unord::UnordMap;
use std::any::Any;

pub struct OhlinkCodegenBackend;

impl CodegenBackend for OhlinkCodegenBackend {
    fn name(&self) -> &'static str {
        "ohlink"
    }

    fn target_cpu(&self, _sess: &Session) -> String {
        "generic".to_string()
    }

    fn codegen_crate(
        &self,
        tcx: TyCtxt<'_>,
    ) -> Box<dyn Any> {
        Box::new(backend::codegen_crate(tcx))
    }

    fn join_codegen(
        &self,
        ongoing_codegen: Box<dyn Any>,
        _sess: &Session,
        _outputs: &OutputFilenames,
        _crate_info: &CrateInfo,
    ) -> (CompiledModules, UnordMap<rustc_middle::dep_graph::WorkProductId, rustc_middle::dep_graph::WorkProduct>) {
        *ongoing_codegen
            .downcast::<(CompiledModules, UnordMap<rustc_middle::dep_graph::WorkProductId, rustc_middle::dep_graph::WorkProduct>)>()
            .unwrap()
    }

    fn link(
        &self,
        sess: &Session,
        codegen_results: CompiledModules,
        crate_info: CrateInfo,
        metadata: EncodedMetadata,
        outputs: &OutputFilenames,
    ) {
        let _ = backend::link(sess, codegen_results, crate_info, metadata, outputs);
    }
}

/// Dynamic library entrypoint for rustc compiler to recognize and load our backend plugin
#[no_mangle]
pub fn __rustc_codegen_backend() -> Box<dyn CodegenBackend> {
    Box::new(OhlinkCodegenBackend)
}
