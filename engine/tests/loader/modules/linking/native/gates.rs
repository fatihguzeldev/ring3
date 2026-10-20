use super::image::image;
use ring3_engine::{
    cpu::dbt::GateSpec,
    loader::{
        ImportSymbol32, ModuleCatalog32, ModuleError32, ModuleId32, ModuleSet32, SystemExport32,
        SystemExportTarget32, SystemModule32,
    },
    memory::{AddressSpace, GuestAddress},
};

fn exported(name: &str, ordinal: u16, id: u32) -> SystemExport32 {
    SystemExport32 {
        name: Some(name.into()),
        ordinal: Some(ordinal),
        target: SystemExportTarget32::Gate(id),
    }
}

fn load(
    catalog: &ModuleCatalog32,
    memory: &mut AddressSpace,
    set: &mut ModuleSet32,
    name: &str,
) -> ModuleId32 {
    catalog
        .plan_load(set, memory, name, None)
        .unwrap()
        .install(memory, set)
        .unwrap()
        .root
}

#[test]
fn unsorted_gate_ids_and_aliases_keep_exact_address_authority() {
    let mut catalog = ModuleCatalog32::new();
    catalog
        .add_system(SystemModule32 {
            name: "objects.dll".into(),
            preferred_base: 0x7000_0000,
            exports: vec![
                exported("Zeta", 9, 90),
                exported("Alpha", 3, 7),
                SystemExport32 {
                    name: None,
                    ordinal: None,
                    target: SystemExportTarget32::PrivateGate(55),
                },
                exported("Alias", 15, 90),
                SystemExport32 {
                    name: None,
                    ordinal: None,
                    target: SystemExportTarget32::PrivateGate(7),
                },
            ],
        })
        .unwrap();
    catalog
        .add_system(SystemModule32 {
            name: "higher.dll".into(),
            preferred_base: 0x7100_0000,
            exports: vec![exported("Other", 1, 99)],
        })
        .unwrap();
    catalog
        .add_guest("guest.dll", &image(true, 0x0040_0000, &[], &[]))
        .unwrap();
    let mut memory = AddressSpace::new(16).unwrap();
    let mut set = ModuleSet32::new();
    load(&catalog, &mut memory, &mut set, "higher");
    let objects = load(&catalog, &mut memory, &mut set, "objects");
    load(&catalog, &mut memory, &mut set, "guest");
    let expected = [
        GateSpec {
            entry: GuestAddress(0x7000_0000),
            id: 90,
        },
        GateSpec {
            entry: GuestAddress(0x7000_0010),
            id: 7,
        },
        GateSpec {
            entry: GuestAddress(0x7000_0020),
            id: 55,
        },
    ];
    assert_eq!(set.get(objects).unwrap().gates(), &expected);
    for gate in expected {
        assert_eq!(set.gate(gate.entry), Some(gate));
    }
    assert_eq!(
        set.gate(GuestAddress(0x7100_0000)),
        Some(GateSpec {
            entry: GuestAddress(0x7100_0000),
            id: 99
        })
    );
    for name in ["Zeta", "Alias"] {
        assert_eq!(
            set.resolve_export(objects, &ImportSymbol32::Name(name.into()))
                .unwrap()
                .address(),
            GuestAddress(0x7000_0000)
        );
    }
    assert_eq!(
        set.resolve_export(objects, &ImportSymbol32::Ordinal(55)),
        Err(ModuleError32::MissingSymbol)
    );
    for pc in [
        0,
        0x0040_0000,
        0x0040_1000,
        0x0040_2400,
        0x6fff_ffff,
        0x7000_0001,
        0x7000_000f,
        0x7000_0011,
        0x7000_0021,
        0x7000_0fff,
        0x7000_1000,
        0xffff_ffff,
    ] {
        assert_eq!(set.gate(GuestAddress(pc)), None, "pc {pc:#010x}");
    }
}

#[test]
fn unloading_and_reusing_an_image_base_replaces_gate_authority() {
    let mut catalog = ModuleCatalog32::new();
    for (name, exports) in [
        (
            "first.dll",
            vec![exported("First", 1, 13), exported("Second", 2, 7)],
        ),
        ("replacement.dll", vec![exported("Replacement", 1, 81)]),
    ] {
        catalog
            .add_system(SystemModule32 {
                name: name.into(),
                preferred_base: 0x7000_0000,
                exports,
            })
            .unwrap();
    }
    let mut memory = AddressSpace::new(4).unwrap();
    let mut set = ModuleSet32::new();
    let original = load(&catalog, &mut memory, &mut set, "first");
    let first = GateSpec {
        entry: GuestAddress(0x7000_0000),
        id: 13,
    };
    let second = GateSpec {
        entry: GuestAddress(0x7000_0010),
        id: 7,
    };
    assert_eq!(set.gate(first.entry), Some(first));
    assert_eq!(set.gate(second.entry), Some(second));
    set.plan_unload(&memory, &[original])
        .unwrap()
        .unmap(&mut memory, &mut set)
        .unwrap();
    assert_eq!(set.gate(first.entry), None);
    assert_eq!(set.gate(second.entry), None);
    let replacement = load(&catalog, &mut memory, &mut set, "replacement");
    assert_ne!(replacement, original);
    assert_eq!(set.get(replacement).unwrap().base(), first.entry.0);
    assert_eq!(
        set.gate(first.entry),
        Some(GateSpec {
            entry: first.entry,
            id: 81
        })
    );
    assert_eq!(set.gate(second.entry), None);
    set.plan_unload(&memory, &[replacement])
        .unwrap()
        .unmap(&mut memory, &mut set)
        .unwrap();
    let reloaded = load(&catalog, &mut memory, &mut set, "first");
    assert_ne!(reloaded, original);
    assert_ne!(reloaded, replacement);
    assert_eq!(set.get(reloaded).unwrap().base(), first.entry.0);
    assert_eq!(set.gate(first.entry), Some(first));
    assert_eq!(set.gate(second.entry), Some(second));
}
