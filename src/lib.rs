#![feature(rustc_private)]
#![feature(proc_macro_internals)]

extern crate rustc_driver;
extern crate rustc_expand;
extern crate rustc_hir;
extern crate rustc_interface;
extern crate rustc_metadata;
extern crate rustc_middle;
extern crate rustc_proc_macro;
extern crate rustc_span;

pub mod driver;
pub mod metadata;
pub mod proc_macro_srv;
pub mod tcx_impl;
