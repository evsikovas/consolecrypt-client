//! The primary header, the part of the header that every message has, of a fixed size.

use std::num::NonZeroU32;

use winnow::{Parser, token::any};

use super::{ByteOrder, Input, PResult, ParseError};
use crate::message::{Flags, PrimaryHeader, Type};

/// The primary header of a message in byte order `O`.
///
/// `O` is the byte order that the endian signature, the first byte, gave, so that byte isn't
/// checked again.
pub(super) fn primary_header<O: ByteOrder>(input: &mut Input<'_>) -> PResult<PrimaryHeader> {
    (
        any,
        any.try_map(message_type),
        // Flags that the specification doesn't define are kept, as it asks for them to be ignored.
        any.map(Flags::from_bits_retain),
        any,
        O::u32,
        O::u32.try_map(serial_num),
    )
        .map(
            |(_, msg_type, flags, protocol_version, body_len, serial_num)| {
                PrimaryHeader::from_parts(
                    O::ENDIAN_SIG,
                    msg_type,
                    flags,
                    protocol_version,
                    body_len,
                    serial_num,
                )
            },
        )
        .parse_next(input)
}

/// The type of a message, from its code.
fn message_type(code: u8) -> Result<Type, ParseError> {
    match code {
        1 => Ok(Type::MethodCall),
        2 => Ok(Type::MethodReturn),
        3 => Ok(Type::Error),
        4 => Ok(Type::Signal),
        _ => Err(ParseError::MessageType),
    }
}

/// The serial number of a message, which can't be 0.
fn serial_num(serial_num: u32) -> Result<NonZeroU32, ParseError> {
    NonZeroU32::new(serial_num).ok_or(ParseError::Serial)
}
