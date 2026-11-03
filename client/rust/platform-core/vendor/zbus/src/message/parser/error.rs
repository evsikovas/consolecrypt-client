//! Why parsing a header fails.

use winnow::error::{FromExternalError, ParserError};

use super::Input;
use crate::{Error, wire};

/// Why parsing a header failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ParseError {
    /// The header ends past the end of the bytes.
    Truncated,
    /// The header is too large for where its fields lie to be kept.
    TooLarge,
    /// The first byte isn't an endian signature.
    Endian,
    /// The message type is unknown.
    MessageType,
    /// The serial number is 0.
    Serial,
    /// A padding byte isn't 0.
    PaddingNot0(u8),
    /// A field has an unknown code, a value of the wrong type, or a string or a signature that a 0
    /// doesn't end, comes more than once, or is a reply serial of 0.
    Field,
    /// A name is invalid, for the reason given.
    Name(&'static str),
    /// The path isn't a valid object path.
    ObjectPath,
    /// The signature of the body is invalid.
    Signature,
}

impl ParserError<Input<'_>> for ParseError {
    type Inner = Self;

    /// The error of winnow's parsers that the parser uses, which only fail when the input ends:
    /// every other way to fail goes through `try_map`, and so through [`FromExternalError`].
    fn from_input(_input: &Input<'_>) -> Self {
        Self::Truncated
    }

    fn into_inner(self) -> std::result::Result<Self::Inner, Self> {
        Ok(self)
    }
}

impl FromExternalError<Input<'_>, ParseError> for ParseError {
    fn from_external_error(_input: &Input<'_>, error: ParseError) -> Self {
        error
    }
}

impl From<ParseError> for Error {
    fn from(error: ParseError) -> Self {
        match error {
            ParseError::Truncated => Error::OutOfBounds,
            ParseError::TooLarge => Error::ExcessData,
            ParseError::Endian => Error::IncorrectEndian,
            ParseError::MessageType | ParseError::Field => Error::InvalidField,
            ParseError::Serial => Error::InvalidSerial,
            ParseError::PaddingNot0(byte) => Error::PaddingNot0(byte),
            ParseError::Name(reason) => Error::InvalidName(reason),
            ParseError::ObjectPath => Error::InvalidObjectPath,
            ParseError::Signature => {
                Error::SignatureParse(wire::signature::Error::InvalidSignature)
            }
        }
    }
}
