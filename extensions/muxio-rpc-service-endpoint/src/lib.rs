#[cfg(doctest)]
doc_comment::doctest!("../README.md");

mod endpoint;
pub use endpoint::*;

mod endpoint_interface;
pub use endpoint_interface::*;

pub mod error;

mod with_handlers_trait;
pub use with_handlers_trait::*;
