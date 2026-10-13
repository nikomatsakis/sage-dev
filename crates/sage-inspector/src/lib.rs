#![feature(register_tool)]
#![register_tool(sage)]
pub mod actor;
pub mod protocol;
pub mod server;

pub use actor::{FileUpdate, InspectionClient, InspectionProvider, ProviderDemand, serve_actor};
pub use protocol::*;
pub use server::{ServerOptions, bind, run_server};

// Source anchors for architectural modules and distributed representations.
mod sage_model {
    /// Architectural crate anchor.
    #[sage::item(crate)]
    mod crate_model {}
    /// Architectural module anchor.
    #[sage::item(crate::inspection)]
    mod inspection_model {}
    /// Groups protocol Response<T>, errors, and event payloads.
    #[sage::item(crate::inspection::Response)]
    mod inspection_response_model {}
    /// Groups actor and HTTP server operations; no single service type.
    #[sage::item(crate::inspection::InspectionService)]
    mod inspection_inspection_service_model {}
}

mod not_applicable {
    // {anchor}`crates.sage_inspector.inspection.generic_browser`
    // Implemented in web/src's TypeScript protocol renderer, not Rust code.

    // {anchor}`crates.sage_inspector.inspection.durable_navigation`
    // Implemented by the TypeScript browser's URL/navigation state.
}
