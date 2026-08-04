pub mod expr;
pub mod infer;
pub mod infer_ctx;
pub(crate) mod method;
pub mod resolve;
pub mod sig;
pub mod solve;
pub(crate) mod trait_env;

pub use infer_ctx::{CheckError, ErrorContext, InferCtx, RecordErr, Scope, TypeError};
pub use sig::Check;
