//! Independent device-reader authority. Never a Node AgentRequest cast.
mod encoding;
mod policy;
#[cfg(target_os = "linux")]
mod sqlite;
mod wire;
pub use encoding::{reader_base64url, reader_decode_base64url};
pub(crate) use policy::ReaderNodeTarget;
pub use policy::*;
#[cfg(target_os = "linux")]
pub use sqlite::*;
pub use wire::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReaderError {
    InvalidWire,
    InvalidSignature,
    WrongIssuer,
    WrongContext,
    WrongAssociation,
    WrongDevice,
    WrongTarget,
    WrongOperation,
    WrongHead,
    Expired,
    Replay,
    NotFound,
    Revoked,
    CapabilityDenied,
    InvalidTime,
    ClockUnavailable,
    Quota,
    Conflict,
    Deadline,
    LostHandle,
    Storage,
    UncertainCommit,
    Entropy,
    Protocol,
}
pub type ReaderResult<T> = std::result::Result<T, ReaderError>;
impl std::fmt::Display for ReaderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for ReaderError {}
impl From<crate::Error> for ReaderError {
    fn from(value: crate::Error) -> Self {
        match value {
            crate::Error::InvalidTime => Self::InvalidTime,
            crate::Error::Expired => Self::Expired,
            crate::Error::Entropy => Self::Entropy,
            crate::Error::Storage => Self::Storage,
            crate::Error::UncertainCommit => Self::UncertainCommit,
            crate::Error::Conflict => Self::Conflict,
            crate::Error::Quota => Self::Quota,
            crate::Error::Revoked => Self::Revoked,
            crate::Error::WrongContext => Self::WrongContext,
            crate::Error::InvalidSignature => Self::InvalidSignature,
            crate::Error::NotFound => Self::NotFound,
            _ => Self::WrongTarget,
        }
    }
}
#[cfg(test)]
mod tests;
