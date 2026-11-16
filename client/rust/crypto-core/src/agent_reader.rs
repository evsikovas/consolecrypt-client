//! Purpose-specific Agent reader wire and cryptography.
//!
//! This module supplies signatures, not Native approval, issuer provisioning,
//! original-session provenance, one-action custody, transport or Agent permits.
mod issuer;
mod signer;
mod validation;
mod wire;

pub use issuer::{ReaderIssuerSecretV1, advance_reader_head_v1};
pub use signer::AgentReaderPreparedProofV1;
pub use validation::{
    AgentReaderExpectedV1, AgentReaderSigningPacketV1, AgentReaderValidatedInputsV1,
};
pub use wire::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AgentReaderError {
    #[error("invalid reader wire")]
    InvalidWire,
    #[error("invalid reader signature")]
    InvalidSignature,
    #[error("reader issuer mismatch")]
    WrongIssuer,
    #[error("reader context mismatch")]
    WrongContext,
    #[error("reader association mismatch")]
    WrongAssociation,
    #[error("reader device mismatch")]
    WrongDevice,
    #[error("reader target mismatch")]
    WrongTarget,
    #[error("reader operation mismatch")]
    WrongOperation,
    #[error("reader authority head mismatch")]
    WrongHead,
    #[error("reader input expired")]
    Expired,
    #[error("invalid reader time")]
    InvalidTime,
    #[error("reader byte budget exceeded")]
    Quota,
    #[error("reader OS entropy unavailable")]
    Entropy,
}
pub type AgentReaderResult<T> = Result<T, AgentReaderError>;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod personal_account_tests;
