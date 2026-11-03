//! Compatibility for existing XDG portal flag enums. The on-wire value is
//! the flag enum's numeric representation, not a change to the D-Bus schema.
use crate::{Error, OwnedVariant, Signature, Type};

impl<T: enumflags2::BitFlag> Type for enumflags2::BitFlags<T>
where
    T::Numeric: Type,
{
    const SIGNATURE: &'static Signature = <T::Numeric as Type>::SIGNATURE;
}

impl<T: enumflags2::BitFlag> TryFrom<OwnedVariant> for enumflags2::BitFlags<T>
where
    T::Numeric: TryFrom<OwnedVariant>,
    Error: From<<T::Numeric as TryFrom<OwnedVariant>>::Error>,
{
    type Error = Error;

    fn try_from(value: OwnedVariant) -> Result<Self, Self::Error> {
        let bits = T::Numeric::try_from(value).map_err(Error::from)?;
        Self::from_bits(bits).map_err(|_| Error::IncorrectType)
    }
}
