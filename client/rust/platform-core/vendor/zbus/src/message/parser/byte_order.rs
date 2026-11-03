//! The byte orders that a message can be in, which the parser is compiled for once each.

use winnow::{Parser, token::take};

use super::{Input, PResult};
use crate::message::EndianSig;

/// A byte order that a message can be in.
pub(super) trait ByteOrder {
    /// The endian signature of the byte order.
    const ENDIAN_SIG: EndianSig;

    /// A `u32` from its bytes in this byte order.
    fn u32_from_bytes(bytes: [u8; 4]) -> u32;

    /// A `u32`, in this byte order.
    #[inline(always)]
    fn u32(input: &mut Input<'_>) -> PResult<u32> {
        take(4usize)
            .map(|bytes: &[u8]| {
                Self::u32_from_bytes(bytes.try_into().expect("`take` returns 4 bytes"))
            })
            .parse_next(input)
    }
}

/// The little-endian byte order.
pub(super) enum LittleEndian {}

impl ByteOrder for LittleEndian {
    const ENDIAN_SIG: EndianSig = EndianSig::Little;

    #[inline(always)]
    fn u32_from_bytes(bytes: [u8; 4]) -> u32 {
        u32::from_le_bytes(bytes)
    }
}

/// The big-endian byte order.
pub(super) enum BigEndian {}

impl ByteOrder for BigEndian {
    const ENDIAN_SIG: EndianSig = EndianSig::Big;

    #[inline(always)]
    fn u32_from_bytes(bytes: [u8; 4]) -> u32 {
        u32::from_be_bytes(bytes)
    }
}
