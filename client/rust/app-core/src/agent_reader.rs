//! Native-internal Reader prototype. No Bridge, UI or automatic activation.
mod authentication;
mod authority;
mod public_api;
pub use public_api::ReaderActionHandle;
mod node_owner;
mod facade;
mod scope;
mod transport;

pub(crate) use facade::NativeReaderRuntime;

pub(crate) use authentication::{OriginalSessionAuth,OriginalSessionProvenance};
