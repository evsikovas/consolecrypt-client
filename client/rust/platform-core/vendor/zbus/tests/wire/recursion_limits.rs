use serde::{
    Serialize, Serializer,
    de::IgnoredAny,
    ser::{SerializeMap, SerializeTuple},
};
use zbus::{
    Error, MaxDepthExceeded, Variant,
    wire::{
        Array, Dict, LE, Signature, Structure,
        serialized::{Context, Data},
        to_bytes, to_bytes_for_signature,
    },
};

#[test]
fn recursion_limits() {
    let ctxt = Context::new(LE, 0);
    // Total container depth exceeds limit (64)
    let mut value = Variant::from(0u8);
    for _ in 0..64 {
        value = Variant::Variant(Box::new(value));
    }
    assert!(matches!(
        to_bytes(ctxt, &value),
        Err(Error::MaxDepthExceeded(MaxDepthExceeded::Container))
    ));

    // Array depth exceeds limit (32)
    let vec = vec![vec![vec![vec![vec![vec![vec![vec![vec![vec![vec![
        vec![vec![vec![vec![vec![vec![vec![vec![vec![vec![vec![
            vec![vec![vec![vec![vec![vec![vec![vec![vec![vec![vec![
                0u8,
            ]]]]]]]]]]],
        ]]]]]]]]]]],
    ]]]]]]]]]]];
    assert!(matches!(
        to_bytes(ctxt, &vec),
        Err(Error::MaxDepthExceeded(MaxDepthExceeded::Array))
    ));

    // Struct depth exceeds limit (32)
    let tuple = ((((((((((((((((((
        (((((((((((((((0u8,),),),),),),),),),),),),),),),
    ),),),),),),),),),),),),),),),),),);
    assert!(matches!(
        to_bytes(ctxt, &tuple),
        Err(Error::MaxDepthExceeded(MaxDepthExceeded::Structure))
    ));

    // total depth exceeds limit (64) with struct, array and variant.
    let mut value = Variant::from(0u8);
    for _ in 0..32 {
        value = Variant::Variant(Box::new(value));
    }
    let tuple_array =
        (
            ((((((((((((((((vec![vec![vec![vec![vec![vec![vec![vec![vec![
                vec![vec![vec![vec![vec![vec![vec![value]]]]]]],
            ]]]]]]]]],),),),),),),),),),),),),),),),),
        );
    assert!(matches!(
        to_bytes(ctxt, &tuple_array),
        Err(Error::MaxDepthExceeded(MaxDepthExceeded::Container))
    ));
}

// No more than 32 arrays, and no more than 32 structures, are allowed in a signature. A variant's
// value has a signature of its own, which starts the count of both anew.
#[test]
fn nesting_per_signature() {
    let ctxt = Context::new(LE, 0);

    // 16 arrays around a variant of 17 arrays around the byte 7.
    let array = in_arrays(in_variants(in_arrays(Variant::U8(7), 17), 1), 16);
    let array = Array::try_from(array).unwrap();
    assert_eq!(array.signature(), "aaaaaaaaaaaaaaaav");
    let encoded = to_bytes(ctxt, &array).unwrap();
    let (decoded, len) = encoded
        .deserialize_for_dynamic_signature::<_, Array<'_>>(array.signature())
        .unwrap();
    assert_eq!(len, encoded.len());
    assert_eq!(decoded, array);

    // 16 structures around a variant of 17 structures around the byte 7.
    let structure = in_structures(in_variants(in_structures(Variant::U8(7), 17), 1), 16);
    let structure = Structure::try_from(structure).unwrap();
    let signature = format!("{}v{}", "(".repeat(16), ")".repeat(16));
    assert_eq!(structure.signature(), signature.as_str());
    let encoded = to_bytes(ctxt, &structure).unwrap();
    let (decoded, len) = encoded
        .deserialize_for_dynamic_signature::<_, Structure<'_>>(structure.signature())
        .unwrap();
    assert_eq!(len, encoded.len());
    assert_eq!(decoded, structure);
}

// A dict puts its keys and values 2 levels deeper than itself, in an entry in its array, but only
// if it has any entries.
#[test]
fn dict_entries() {
    let ctxt = Context::new(LE, 0);

    // Dicts of a single entry each, whose value is a variant of the next dict, the last of which
    // has a variant of the byte 7. Each dict puts the byte 3 levels deeper: in its array, in its
    // entry and in the variant. With 21 dicts, the byte is in 63 containers, and with 22, the keys
    // and values of the last dict are in 65.
    for (dicts, fits) in [(21, true), (22, false)] {
        let mut value = Variant::U8(7);
        for _ in 0..dicts {
            value = in_dicts(in_variants(value, 1), 1);
        }
        let dict = Dict::try_from(value).unwrap();
        assert_eq!(dict.signature(), "a{sv}");
        let result = dict_to_bytes(ctxt, &dict);
        let case = format!("{dicts} dicts");
        if fits {
            assert_deserializes_back(result, dict.signature(), &case);
        } else {
            assert_too_deep(result, &case);
        }
    }
}

// A variant counts as holding any value of its type, even in an array that's empty.
#[test]
fn variants_hold_any_value_of_their_type() {
    let ctxt = Context::new(LE, 0);

    // The array, and the most variants that it can be in. The innermost of `n` variants is in
    // `n - 1` containers, and the type of its value goes 2 levels deeper for `ay` and 3 for `aas`,
    // whether there are any elements or not.
    let cases = [
        ("ay", Variant::from(vec![7u8]), 63),
        ("empty as", Variant::from(Vec::<&str>::new()), 63),
        ("as", Variant::from(vec!["7"]), 63),
        ("ab", Variant::from(vec![true]), 63),
        ("empty ab", Variant::from(Vec::<bool>::new()), 63),
        ("empty aas", Variant::from(Vec::<Vec<&str>>::new()), 62),
        ("empty a(yy)", Variant::from(Vec::<(u8, u8)>::new()), 62),
    ];

    for (name, array, max) in cases {
        // The variants are those around the array, and the one that `to_bytes` writes around them.
        let variant = in_variants(array.clone(), max - 1);
        let case = format!("{name} in {max} variants");
        let encoded = to_bytes(ctxt, &variant).unwrap_or_else(|e| panic!("{case}: {e}"));
        let (decoded, len) = encoded.deserialize::<Variant<'_>>().unwrap();
        assert_eq!(len, encoded.len(), "{case}");
        assert_eq!(decoded, variant, "{case}");

        let variant = in_variants(array, max);
        let case = format!("{name} in {} variants", max + 1);
        assert_too_deep(to_bytes(ctxt, &variant), &case);
    }
}

// The elements of an array are a level deeper than it. Those of numbers count even if there are
// none, but those of any other type only if there are any. Nothing here is in a variant, so the
// array is as deep as the containers around it.
#[test]
fn arrays_outside_variants() {
    let ctxt = Context::new(LE, 0);

    // The array, and whether its elements count.
    let cases = [
        ("ay", Variant::from(vec![7u8]), true),
        ("empty ay", Variant::from(Vec::<u8>::new()), true),
        ("ab", Variant::from(vec![true]), true),
        ("empty ab", Variant::from(Vec::<bool>::new()), false),
        ("empty as", Variant::from(Vec::<&str>::new()), false),
        ("as", Variant::from(vec!["7"]), true),
    ];

    for (name, array, elements_count) in cases {
        for structures in [1, 2] {
            // The array is in 31 dicts, each of which puts it 2 levels deeper, and in the
            // structures. That's 63 containers with 1 structure and 64 with 2, which puts its
            // elements in 64 and 65.
            let dict = in_dicts(in_structures(array.clone(), structures), 31);
            let dict = Dict::try_from(dict).unwrap();
            let result = dict_to_bytes(ctxt, &dict);
            let case = format!("{name} in {structures} structures");
            if structures == 2 && elements_count {
                assert_too_deep(result, &case);
            } else {
                assert_deserializes_back(result, dict.signature(), &case);
            }
        }
    }
}

// The same as `arrays_outside_variants`, with the bytes of an array written by `serialize_bytes`,
// rather than as a sequence.
#[test]
fn bytes() {
    let ctxt = Context::new(LE, 0);

    for structures in [1, 2] {
        let signature = format!(
            "{}{}ay{}{}",
            "a{s".repeat(31),
            "(".repeat(structures),
            ")".repeat(structures),
            "}".repeat(31),
        );
        let signature = Signature::try_from(signature.as_str()).unwrap();
        let value = EmptyBytes {
            dicts: 31,
            structures,
        };
        let result = to_bytes_for_signature(ctxt, &signature, &value);
        let case = format!("bytes in {structures} structures");
        // The elements of bytes count even though there are none, as they are numbers.
        if structures == 2 {
            assert_too_deep(result, &case);
        } else {
            assert_deserializes_back(result, &signature, &case);
        }
    }
}

/// `value` in `n` arrays, each of which has the one inside it as its only element.
fn in_arrays(value: Variant<'static>, n: usize) -> Variant<'static> {
    (0..n).fold(value, |element, _| {
        let mut array = Array::new(element.value_signature());
        array.append(element).unwrap();

        Variant::Array(array)
    })
}

/// `value` in `n` structures, each of which has the one inside it as its only field.
fn in_structures(value: Variant<'static>, n: usize) -> Variant<'static> {
    (0..n).fold(value, |field, _| {
        let structure = Structure::builder().append_field(field).build().unwrap();

        Variant::Structure(structure)
    })
}

/// `value` in `n` dicts, each of which has the one inside it as the value of its only entry, whose
/// key is `k`.
fn in_dicts(value: Variant<'static>, n: usize) -> Variant<'static> {
    (0..n).fold(value, |value, _| {
        let mut dict = Dict::new(&Signature::Str, value.value_signature());
        dict.append(Variant::from("k"), value).unwrap();

        Variant::Dict(dict)
    })
}

/// `value` in `n` variants, each of which holds the one inside it.
fn in_variants(value: Variant<'static>, n: usize) -> Variant<'static> {
    (0..n).fold(value, |value, _| Variant::Variant(Box::new(value)))
}

/// `dict` serialized, with its own signature. `to_bytes` can't do that, as `Dict` is not
/// `DynamicType`.
fn dict_to_bytes(ctxt: Context, dict: &Dict<'_, '_>) -> zbus::Result<Data<'static, 'static>> {
    to_bytes_for_signature(ctxt, dict.signature(), dict)
}

/// Assert that `result` is the serialization of a value of `signature`, and that deserializing a
/// value of `signature` from it takes all of its bytes. `case` names the case in a failure.
fn assert_deserializes_back(
    result: zbus::Result<Data<'static, 'static>>,
    signature: &Signature,
    case: &str,
) {
    let encoded = result.unwrap_or_else(|e| panic!("{case}: {e}"));
    let (_, len) = encoded
        .deserialize_for_signature::<_, IgnoredAny>(signature)
        .unwrap_or_else(|e| panic!("{case}: {e}"));
    assert_eq!(len, encoded.len(), "{case}");
}

/// Assert that `result` is the error of a value in more than 64 containers. `case` names the case
/// in a failure.
fn assert_too_deep(result: zbus::Result<Data<'static, 'static>>, case: &str) {
    // Not the bytes of a success, which would fill the message of the failure.
    let result = result.map(|encoded| encoded.len());
    assert!(
        matches!(
            result,
            Err(Error::MaxDepthExceeded(MaxDepthExceeded::Container))
        ),
        "{case}: {result:?}"
    );
}

/// Empty bytes, in `structures` single-field tuples, in `dicts` single-entry maps of the key `k`,
/// each of which has the next one as its value.
struct EmptyBytes {
    dicts: usize,
    structures: usize,
}

impl Serialize for EmptyBytes {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match (self.dicts, self.structures) {
            (0, 0) => serializer.serialize_bytes(&[]),
            (0, structures) => {
                let mut tuple = serializer.serialize_tuple(1)?;
                tuple.serialize_element(&Self {
                    dicts: 0,
                    structures: structures - 1,
                })?;
                tuple.end()
            }
            (dicts, structures) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry(
                    "k",
                    &Self {
                        dicts: dicts - 1,
                        structures,
                    },
                )?;
                map.end()
            }
        }
    }
}
