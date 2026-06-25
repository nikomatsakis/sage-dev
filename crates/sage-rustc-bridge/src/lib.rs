#![feature(rustc_private)]
#![feature(proc_macro_internals)]
#![allow(internal_features)]

extern crate rustc_driver;
extern crate rustc_expand;
extern crate rustc_hir;
extern crate rustc_interface;
extern crate rustc_metadata;
extern crate rustc_middle;
extern crate rustc_proc_macro;
extern crate rustc_span;

mod proc_macro_srv;
mod tcx_impl;

pub use proc_macro_srv::SageServer;
pub use tcx_impl::RustcTcxDb;

use std::sync::mpsc;

use rustc_middle::ty::TyCtxt;
use sage_ir::tcx::TcxRequest;

/// Locate the sysroot for the current rustc toolchain.
pub fn find_sysroot() -> String {
    let output = std::process::Command::new("rustc")
        .arg("--print=sysroot")
        .output()
        .expect("cannot run `rustc --print=sysroot`");
    assert!(output.status.success(), "rustc --print=sysroot failed");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// Serve `TcxRequest`s from a channel using a live `TyCtxt`.
///
/// Blocks until the sender is dropped (the sage side is done).
pub fn serve_tcx_requests(tcx: TyCtxt<'_>, req_rx: mpsc::Receiver<TcxRequest>) {
    let tcx_db = RustcTcxDb::new(tcx);
    for req in req_rx {
        match req {
            TcxRequest::ExternCrate { name, reply } => {
                let _ = reply.send(tcx_db.extern_crate(&name));
            }
            TcxRequest::ModuleChildren {
                crate_num,
                def_index,
                reply,
            } => {
                let _ = reply.send(tcx_db.module_children(crate_num, def_index));
            }
            TcxRequest::IsBuiltinDerive {
                crate_num,
                def_index,
                reply,
            } => {
                let _ = reply.send(tcx_db.is_builtin_derive(crate_num, def_index));
            }
            TcxRequest::ItemName {
                crate_num,
                def_index,
                reply,
            } => {
                let _ = reply.send(tcx_db.item_name(crate_num, def_index));
            }
            TcxRequest::IsModule {
                crate_num,
                def_index,
                reply,
            } => {
                let _ = reply.send(tcx_db.is_module(crate_num, def_index));
            }
            TcxRequest::DefPath {
                crate_num,
                def_index,
                reply,
            } => {
                let _ = reply.send(tcx_db.def_path(crate_num, def_index));
            }
            TcxRequest::StructuredDefPath {
                crate_num,
                def_index,
                reply,
            } => {
                let _ = reply.send(tcx_db.structured_def_path(crate_num, def_index));
            }
            TcxRequest::ExpandDerive {
                crate_num,
                def_index,
                item_source,
                reply,
            } => {
                let _ = reply.send(tcx_db.expand_proc_macro_derive(
                    crate_num,
                    def_index,
                    &item_source,
                ));
            }
            TcxRequest::ExpandBang {
                crate_num,
                def_index,
                input_tokens,
                reply,
            } => {
                let _ = reply.send(tcx_db.expand_proc_macro_bang(
                    crate_num,
                    def_index,
                    &input_tokens,
                ));
            }
            TcxRequest::ExpandAttr {
                crate_num,
                def_index,
                attr_args,
                item_source,
                reply,
            } => {
                let _ = reply.send(tcx_db.expand_proc_macro_attr(
                    crate_num,
                    def_index,
                    &attr_args,
                    &item_source,
                ));
            }
        }
    }
}
