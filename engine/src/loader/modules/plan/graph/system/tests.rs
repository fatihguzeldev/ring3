use super::*;
use crate::loader::{ImportSymbol32, ModuleCatalog32, ModuleSet32, SystemExport32};
use crate::memory::AddressSpace;

fn source(private: SystemExport32) -> SystemModule32 {
    SystemModule32 {
        name: "objects.dll".into(),
        preferred_base: 0x5000_0000,
        exports: vec![
            SystemExport32 {
                name: Some("Create".into()),
                ordinal: Some(1),
                target: SystemExportTarget32::Gate(7),
            },
            private,
        ],
    }
}

#[test]
fn private_method_gates_are_executable_authority_without_public_exports() {
    let mut catalog = ModuleCatalog32::new();
    catalog
        .add_system(source(SystemExport32 {
            name: None,
            ordinal: None,
            target: SystemExportTarget32::PrivateGate(8),
        }))
        .unwrap();
    let mut memory = AddressSpace::new(2).unwrap();
    let mut set = ModuleSet32::new();
    let module = catalog
        .plan_load(&set, &memory, "objects", None)
        .unwrap()
        .install(&mut memory, &mut set)
        .unwrap()
        .root;
    let loaded = set.get(module).unwrap();
    assert_eq!(
        loaded.gates(),
        &[
            GateSpec {
                entry: GuestAddress(0x5000_0000),
                id: 7
            },
            GateSpec {
                entry: GuestAddress(0x5000_0010),
                id: 8
            },
        ]
    );
    assert!(
        set.resolve_export(module, &ImportSymbol32::Name("Create".into()))
            .is_ok()
    );
    assert_eq!(
        set.resolve_export(module, &ImportSymbol32::Ordinal(2)),
        Err(ModuleError32::MissingSymbol)
    );
    let mut bytes = [0; 2];
    memory.read(GuestAddress(0x5000_0010), &mut bytes).unwrap();
    assert_eq!(bytes, [0x0f, 0x0b]);
    assert_eq!(
        memory.permissions_at(GuestAddress(0x5000_0010)),
        Ok(Permissions::READ_EXECUTE)
    );
}

#[test]
fn private_gate_metadata_has_one_unambiguous_shape() {
    for private in [
        SystemExport32 {
            name: Some("Hidden".into()),
            ordinal: None,
            target: SystemExportTarget32::PrivateGate(8),
        },
        SystemExport32 {
            name: None,
            ordinal: Some(2),
            target: SystemExportTarget32::PrivateGate(8),
        },
        SystemExport32 {
            name: None,
            ordinal: None,
            target: SystemExportTarget32::PrivateGate(0),
        },
        SystemExport32 {
            name: None,
            ordinal: None,
            target: SystemExportTarget32::Gate(8),
        },
    ] {
        assert_eq!(
            ModuleCatalog32::new().add_system(source(private)),
            Err(ModuleError32::InvalidSystemExport)
        );
    }
}
