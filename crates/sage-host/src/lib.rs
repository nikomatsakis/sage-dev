#![feature(register_tool)]
#![register_tool(sage)]
#![feature(rustc_private)]
#![allow(internal_features)]
extern crate rustc_driver;
extern crate rustc_interface;
extern crate rustc_middle;
pub mod driver;
pub mod metadata;
pub mod trace;
#[sage::item(crate)]
mod crate_model {}
#[sage::item(crate::driver)]
mod driver_model {}
