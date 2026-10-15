use super::*;
use crate::{loader::SystemExportTarget32, memory::GuestAddress, windows::CallingConvention32};

#[test]
fn user32_catalog_resolves_explicit_stdcall_gates_without_id_or_symbol_aliases() {
    let module = user32_module(0x70000000);
    assert_eq!(module.exports.len(), 89);
    for api in User32Api32::catalog() {
        assert_eq!(User32Api32::from_id(api.id()), Some(api));
        assert_eq!(User32Api32::resolve("USER32.DLL", api.name()), Some(api));
        assert_eq!(api.convention(), CallingConvention32::Stdcall);
        assert_eq!(api.id() & 0xffff0000, 0x00020000);
        assert!(
            module
                .exports
                .iter()
                .any(|export| export.name.as_deref() == Some(api.name())
                    && export.target == SystemExportTarget32::Gate(api.id()))
        );
    }
    assert_eq!(
        User32Api32::resolve("kernel32.dll", "CreateWindowExA"),
        None
    );
    assert_eq!(User32Api32::resolve("user32.dll", "createwindowexa"), None);
    assert_eq!(User32Api32::from_id(0x2005a), None);
    assert_eq!(User32Api32::CreateWindowExW.stack_words(), 12);
}

#[test]
fn class_identity_is_case_insensitive_instance_scoped_and_never_reuses_retired_atoms() {
    let mut state = User32State32::new(User32Limits32::default()).unwrap();
    let class = |instance| WindowClass32 {
        atom: 0,
        name: "ÄRing3".encode_utf16().collect(),
        instance,
        procedure: GuestAddress(0x400000),
        unicode: true,
        style: 0,
        extra_bytes: 0,
    };
    let atom = state.register_class(class(1)).unwrap();
    assert_eq!(
        state.register_class(class(1)),
        Err(User32Error32::Win32(1410))
    );
    assert_eq!(
        state
            .class(1, &"äring3".encode_utf16().collect::<Vec<_>>(), None)
            .unwrap()
            .atom,
        atom
    );
    assert!(state.class(2, &[], Some(atom)).is_err());
    let other = state.register_class(class(2)).unwrap();
    assert_ne!(atom, other);
    state.unregister_class(1, &[], Some(atom)).unwrap();
    let next = state.register_class(class(1)).unwrap();
    assert!(next > other);
}
