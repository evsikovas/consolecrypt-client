//! Native Kubernetes access. A context's policy wraps the transport so UI,
//! direct API calls and the local CLI proxy share the same enforcement.

pub mod ai_tools;
pub mod auth;
pub mod diagnostics;
pub mod gitops;
pub mod helm;
pub mod import;
pub mod import_files;
pub mod local_proxy;
pub mod policy;
pub mod resources;
pub mod session;
mod stream_scope;
pub mod vault_import;
