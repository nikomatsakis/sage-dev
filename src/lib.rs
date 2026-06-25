#![feature(rustc_private)]
#![allow(internal_features)]

extern crate rustc_driver;
extern crate rustc_interface;
extern crate rustc_middle;

pub mod driver;
pub mod metadata;

pub use sage_rustc_bridge::{RustcTcxDb, SageServer, serve_tcx_requests};
