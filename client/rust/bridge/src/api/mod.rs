//! The FRB surface: every `pub fn` / `pub struct` / `pub enum` below is
//! exported to Dart (`client/flutter/lib/src/rust/api/**`). Helpers are
//! `pub(crate)` or private so the codegen ignores them.
//!
//! Functions returning `String` return JSON of the named app-core DTO
//! (ADR-0107 wire format); `*_json` parameters take JSON of that DTO.

pub mod account;
pub mod ai;
pub mod app;
pub mod agent_reader;
pub mod agent_node_association;
pub mod agent_tunnel_route;
pub mod agent_collector;
pub mod agent_exporter;
pub mod agent_collector_service;
pub mod automation;
pub mod automation_service;
pub mod automation_awx;
pub mod automation_portable;
pub mod automation_portable_import;
pub mod backup;
pub mod credentials;
pub mod database;
pub mod database_designer;
pub mod docker;
pub mod error;
pub mod inventory;
pub mod kubernetes;
pub mod monitoring;
pub mod monitoring_consent;
pub mod password_vault;
pub mod profiles;
pub mod rdp;
pub mod rdp_hosts;
pub mod sftp;
pub mod sharing;
pub mod ssh;
pub mod sync;
pub mod teams;

pub mod enrollment;
pub mod agent_installer;
