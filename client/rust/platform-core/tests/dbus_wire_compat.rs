#![cfg(target_os = "linux")]

use enumflags2::{bitflags, BitFlags};
use zbus::{
    wire::{serialized::Context, to_bytes, LE},
    OwnedVariant, Type,
};

#[bitflags]
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq)]
enum PortalDevice {
    Keyboard = 1,
    Pointer = 2,
}

#[test]
fn portal_flags_keep_the_dbus_numeric_signature_and_wire_bytes() {
    let flags = PortalDevice::Keyboard | PortalDevice::Pointer;
    assert_eq!(
        <BitFlags<PortalDevice> as Type>::SIGNATURE,
        <u32 as Type>::SIGNATURE
    );
    let data = to_bytes(Context::new_dbus(LE, 0), &flags).unwrap();
    assert_eq!(data.bytes(), &[3, 0, 0, 0]);
    let (decoded, _): (BitFlags<PortalDevice>, _) = data.deserialize().unwrap();
    assert_eq!(decoded, flags);
}

#[test]
fn portal_variant_conversion_rejects_unknown_bits_and_wrong_wire_type() {
    assert_eq!(
        BitFlags::<PortalDevice>::try_from(OwnedVariant::from(3u32))
            .unwrap()
            .bits(),
        3
    );
    assert!(BitFlags::<PortalDevice>::try_from(OwnedVariant::from(4u32)).is_err());
    assert!(BitFlags::<PortalDevice>::try_from(OwnedVariant::from(3u64)).is_err());
}
