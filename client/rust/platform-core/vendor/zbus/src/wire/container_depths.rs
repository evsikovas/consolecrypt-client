use crate::{Error, MaxDepthExceeded, Result, wire::Signature};

/// The containers around a value that is being (de)serialized, whose nesting the D-Bus
/// specification limits.
///
/// The signature of the body of a message, and that of each variant, holds no more than 32 arrays
/// and 32 structures nested in each other, which are counted for each signature on its own, and a
/// value is in no more than 64 containers in all, across variants, dict entries included. Those are
/// counted as GLib counts them, which rejects all that libdbus rejects: the elements of an array
/// are a level deeper than it even if there are none, if they're numbers or file descriptors, and
/// a variant counts as holding any value of its type, even in an array that's empty.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct ContainerDepths {
    /// The structures around the value, in the signature of the variant that the value is in, or
    /// in that of the value that the (de)serializer started with, if it's in no variant.
    structure: u8,
    /// The arrays around the value, in the same signature.
    array: u8,
    /// All the containers around the value, across variants.
    total: u8,
}

impl ContainerDepths {
    /// The depths of the fields of a structure at `self`.
    pub fn inc_structure(mut self) -> Result<Self> {
        self.structure += 1;
        if self.structure > MAX_STRUCT_DEPTH {
            return Err(Error::MaxDepthExceeded(MaxDepthExceeded::Structure));
        }
        self.total += 1;

        self.check()
    }

    /// The depths of the elements of an array of `element`s at `self`, a level deeper than it.
    ///
    /// Numbers and file descriptors count toward the 64 containers even if there are none, as GLib
    /// has them. Other elements count only if there are any, which is for the caller to
    /// [`check`](Self::check) once it knows.
    pub fn inc_array(self, element: &Signature) -> Result<Self> {
        let depths = self.enter_array()?;
        match element {
            Signature::U8
            | Signature::I16
            | Signature::U16
            | Signature::I32
            | Signature::U32
            | Signature::I64
            | Signature::U64
            | Signature::F64 => depths.check(),
            #[cfg(unix)]
            Signature::Fd => depths.check(),
            _ => Ok(depths),
        }
    }

    /// The depths of the keys and values of a dict at `self`, which are in its dict entries, in its
    /// array: 2 levels deeper than the dict.
    ///
    /// They count toward the 64 containers only if there are any, which is for the caller to
    /// [`check`](Self::check) once it knows.
    pub fn inc_dict(self) -> Result<Self> {
        let mut depths = self.enter_array()?;
        depths.total += 1;

        Ok(depths)
    }

    /// The depths of the value of a variant at `self`, whose signature is `signature`, which starts
    /// the count of arrays and structures anew.
    ///
    /// The variant counts as holding any value of its type, even in an array that's empty, as GLib
    /// has it.
    #[inline]
    pub fn inc_variant(self, signature: &Signature) -> Result<Self> {
        // The value is a level deeper than the variant, and the deepest value that its type could
        // hold is as many levels deeper than the variant as the type goes, which is 1 for most.
        let type_depth = match signature {
            Signature::Array(_) | Signature::Dict { .. } | Signature::Structure(_) => {
                type_depth(signature)
            }
            _ => 1,
        };
        if usize::from(self.total) + type_depth > usize::from(MAX_TOTAL_DEPTH) {
            return Err(Error::MaxDepthExceeded(MaxDepthExceeded::Container));
        }

        Ok(Self {
            structure: 0,
            array: 0,
            total: self.total + 1,
        })
    }

    /// `self`, unless a value at `self` is in more containers than a value can be in.
    pub fn check(self) -> Result<Self> {
        if self.total > MAX_TOTAL_DEPTH {
            return Err(Error::MaxDepthExceeded(MaxDepthExceeded::Container));
        }

        Ok(self)
    }

    /// `self` a level deeper, in an array, unless the signature holds more arrays than it can.
    fn enter_array(mut self) -> Result<Self> {
        self.array += 1;
        if self.array > MAX_ARRAY_DEPTH {
            return Err(Error::MaxDepthExceeded(MaxDepthExceeded::Array));
        }
        self.total += 1;

        Ok(self)
    }
}

/// How many levels deep the values of `signature` go: 1 for a basic type or a variant, and 1 more
/// than the deepest of its children for a container, which a dict entry is.
///
/// `signature` is one that was parsed, as that of a variant is, so it's no deeper than a signature
/// can be.
fn type_depth(signature: &Signature) -> usize {
    match signature {
        Signature::Array(child) => 1 + type_depth(child.signature()),
        // Its array and its dict entries, whose keys are of a basic type.
        Signature::Dict { value, .. } => 2 + type_depth(value.signature()),
        Signature::Structure(fields) => 1 + fields.iter().map(type_depth).max().unwrap_or(0),
        _ => 1,
    }
}

// The limits come from the D-Bus specification.
const MAX_STRUCT_DEPTH: u8 = 32;
const MAX_ARRAY_DEPTH: u8 = 32;
const MAX_TOTAL_DEPTH: u8 = 64;
