//! Exercise the RUSTSEC-2024-0429 backport in optimized builds, against the
//! exact GLib used by the Linux clipboard/drag-and-drop plugin.
#[cfg(target_os = "linux")]
#[test]
fn variant_string_iterator_mutable_out_pointer_in_release() {
    use gtk::glib::variant::ToVariant;
    let value = ["first", "второй", "last"].to_variant();
    assert_eq!(value.array_iter_str().unwrap().collect::<Vec<_>>(), ["first", "второй", "last"]);
    assert_eq!(value.array_iter_str().unwrap().rev().collect::<Vec<_>>(), ["last", "второй", "first"]);
    assert_eq!(value.array_iter_str().unwrap().nth(1), Some("второй"));
    assert_eq!(value.array_iter_str().unwrap().nth_back(1), Some("второй"));
    assert_eq!(value.array_iter_str().unwrap().last(), Some("last"));
}
