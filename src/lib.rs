#![feature(register_tool)]
#![register_tool(sage)]
#![feature(rustc_private)]
#![allow(internal_features)]

extern crate rustc_driver;
extern crate rustc_interface;
extern crate rustc_middle;

pub mod driver;
pub mod inspector;
pub mod metadata;

pub use sage_rustc_bridge::{RustcTcxDb, SageServer, serve_tcx_requests};

// Source anchors for architectural modules and distributed representations.
mod sage_model {
    /// Architectural crate anchor.
    #[sage::item(crate)]
    mod crate_model {}
    /// Architectural module anchor.
    #[sage::item(crate::driver)]
    mod driver_model {}
}
