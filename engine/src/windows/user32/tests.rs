use super::*;
use crate::{loader::SystemExportTarget32, memory::GuestAddress, windows::CallingConvention32};

#[test]
fn user32_catalog_resolves_explicit_stdcall_gates_without_id_or_symbol_aliases() {
    let module = user32_module(0x70000000);
    let named =
        User32Api32::catalog().count() + 10 + crate::windows::gdi::user32_gdi_exports().count();
    assert_eq!(module.exports.len(), named + 2);
    let mut names = std::collections::BTreeSet::new();
    let mut gates = std::collections::BTreeSet::new();
    for export in &module.exports {
        match &export.target {
            SystemExportTarget32::Gate(gate) => {
                assert!(names.insert(export.name.as_deref().unwrap()));
                assert!(gates.insert(*gate));
            }
            SystemExportTarget32::PrivateGate(gate) => {
                assert!(export.name.is_none() && export.ordinal.is_none());
                assert!(matches!(
                    *gate,
                    crate::windows::controls::BUILTIN_PROC_A
                        | crate::windows::controls::BUILTIN_PROC_W
                ));
                assert!(gates.insert(*gate));
            }
            _ => panic!("user32 exports must be explicit gates"),
        }
    }
    let extensions = [
        ("RegisterRawInputDevices", 0x0014_0001),
        ("GetRegisteredRawInputDevices", 0x0014_0002),
        ("GetRawInputData", 0x0014_0003),
        ("GetRawInputBuffer", 0x0014_0004),
        ("GetRawInputDeviceList", 0x0014_0005),
        ("GetRawInputDeviceInfoA", 0x0014_0006),
        ("GetRawInputDeviceInfoW", 0x0014_0007),
        ("DefRawInputProc", 0x0014_0008),
        ("LoadStringA", 0x0013_0015),
        ("LoadStringW", 0x0013_0016),
    ];
    assert_eq!(User32Api32::catalog().count() + extensions.len(), 162);
    for (name, gate) in extensions {
        assert!(module.exports.iter().any(|export| {
            export.name.as_deref() == Some(name)
                && export.target == SystemExportTarget32::Gate(gate)
        }));
    }
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
    assert_eq!(User32Api32::from_id(0x2ffff), None);
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
        ..Default::default()
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
