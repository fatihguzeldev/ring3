#[path = "native/dynamic.rs"]
mod dynamic;
#[path = "native/image.rs"]
mod image;

use image::{Import, Symbol, image};
use ring3_engine::{
    loader::{
        ImportSymbol32, ModuleCatalog32, ModuleError32, ModuleSet32, ResolvedExport32,
        SystemExport32, SystemExportTarget32, SystemModule32,
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};

fn word(memory: &AddressSpace, address: u32) -> u32 {
    let mut bytes = [0; 4];
    memory.read(GuestAddress(address), &mut bytes).unwrap();
    u32::from_le_bytes(bytes)
}

#[test]
fn named_ordinal_data_and_forwarded_imports_bind_across_a_dependency_cycle() {
    let mut catalog = ModuleCatalog32::new();
    catalog
        .add_guest(
            "game.exe",
            &image(
                false,
                0x400000,
                &[Import {
                    module: "Alpha.DLL",
                    symbols: vec![
                        Symbol::Name("Run"),
                        Symbol::Ordinal(12),
                        Symbol::Name("Data"),
                    ],
                }],
                &[],
            ),
        )
        .unwrap();
    catalog
        .add_guest(
            "alpha.dll",
            &image(
                true,
                0x10000000,
                &[Import {
                    module: "beta",
                    symbols: vec![Symbol::Name("Return")],
                }],
                &[
                    (10, Some("Run"), "beta.#20"),
                    (12, None, "@1000"),
                    (13, Some("Data"), "@2f00"),
                ],
            ),
        )
        .unwrap();
    catalog
        .add_guest(
            "BETA.dll",
            &image(
                true,
                0x10000000,
                &[Import {
                    module: "alpha",
                    symbols: vec![Symbol::Ordinal(12)],
                }],
                &[(20, Some("Return"), "@1000")],
            ),
        )
        .unwrap();
    let mut memory = AddressSpace::new(20).unwrap();
    let mut set = ModuleSet32::new();
    let plan = catalog
        .plan_load(&set, &memory, "GAME.exe", Some(0x400000))
        .unwrap();
    assert_eq!(plan.additions().len(), 3);
    assert_eq!(memory.mapped_pages(), 0);
    assert_eq!(set.iter().len(), 0);
    let loaded = plan.install(&mut memory, &mut set).unwrap();
    let alpha = set.find("ALPHA").unwrap();
    let beta = set.find("beta.dll").unwrap();
    assert_eq!(alpha.base(), 0x10000000);
    assert_eq!(beta.base(), 0x10010000);
    assert_eq!(word(&memory, 0x402400), beta.base() + 0x1000);
    assert_eq!(word(&memory, 0x402404), alpha.base() + 0x1000);
    assert_eq!(word(&memory, 0x402408), alpha.base() + 0x2f00);
    assert_eq!(word(&memory, beta.base() + 0x1010), beta.base() + 0x2f00);
    assert_eq!(word(&memory, alpha.base() + 0x2400), beta.base() + 0x1000);
    assert_eq!(word(&memory, beta.base() + 0x2400), alpha.base() + 0x1000);
    assert_eq!(loaded.initialization_order.last(), Some(&loaded.root));
    assert!(alpha.dependencies().contains(&beta.id()));
    assert!(beta.dependencies().contains(&alpha.id()));
    assert_eq!(
        set.resolve_export(alpha.id(), &ImportSymbol32::Name("run".into())),
        Err(ModuleError32::MissingSymbol)
    );
    assert!(
        matches!(set.resolve_export(alpha.id(), &ImportSymbol32::Name("Data".into())).unwrap(), ResolvedExport32::GuestAddress { address: GuestAddress(address), .. } if address == alpha.base() + 0x2f00)
    );
}

#[test]
fn forwarder_cycles_leave_memory_and_membership_unchanged() {
    let mut catalog = ModuleCatalog32::new();
    catalog
        .add_guest(
            "game.exe",
            &image(
                false,
                0x400000,
                &[Import {
                    module: "alpha",
                    symbols: vec![Symbol::Name("Loop")],
                }],
                &[],
            ),
        )
        .unwrap();
    catalog
        .add_guest(
            "alpha",
            &image(true, 0x10000000, &[], &[(1, Some("Loop"), "beta.Loop")]),
        )
        .unwrap();
    catalog
        .add_guest(
            "beta",
            &image(true, 0x20000000, &[], &[(1, Some("Loop"), "alpha.Loop")]),
        )
        .unwrap();
    let memory = AddressSpace::new(20).unwrap();
    let set = ModuleSet32::new();
    assert_eq!(
        catalog
            .plan_load(&set, &memory, "game.exe", None)
            .unwrap_err(),
        ModuleError32::ForwarderCycle
    );
    assert_eq!(memory.mapped_pages(), 0);
    assert_eq!(set.iter().len(), 0);
}

#[test]
fn more_than_eight_imports_use_only_explicit_system_exports() {
    let mut catalog = ModuleCatalog32::new();
    let symbols: Vec<_> = (1..=12).map(Symbol::Ordinal).collect();
    catalog
        .add_guest(
            "game.exe",
            &image(
                false,
                0x400000,
                &[Import {
                    module: "system.dll",
                    symbols,
                }],
                &[],
            ),
        )
        .unwrap();
    catalog
        .add_system(SystemModule32 {
            name: "SYSTEM.dll".into(),
            preferred_base: 0x70000000,
            exports: (1..=12)
                .map(|ordinal| SystemExport32 {
                    name: Some(format!("Api{ordinal}")),
                    ordinal: Some(ordinal),
                    target: SystemExportTarget32::Gate(0x10000 + u32::from(ordinal)),
                })
                .collect(),
        })
        .unwrap();
    let mut memory = AddressSpace::new(8).unwrap();
    let mut set = ModuleSet32::new();
    catalog
        .plan_load(&set, &memory, "game.exe", None)
        .unwrap()
        .install(&mut memory, &mut set)
        .unwrap();
    let system = set.find("system").unwrap();
    for index in 0..12 {
        assert_eq!(word(&memory, 0x402400 + index * 4), 0x70000000 + index * 16);
    }
    assert_eq!(system.gates().len(), 12);
    assert!(matches!(
        set.resolve_export(system.id(), &ImportSymbol32::Ordinal(1))
            .unwrap(),
        ResolvedExport32::SystemGate { .. }
    ));
    assert_eq!(
        set.resolve_export(system.id(), &ImportSymbol32::Ordinal(13)),
        Err(ModuleError32::MissingSymbol)
    );
    assert!(memory.write(GuestAddress(0x70000000), &[0]).is_err());
}

#[test]
fn stale_plans_and_mapped_module_gaps_are_rejected_without_partial_publication() {
    let mut catalog = ModuleCatalog32::new();
    catalog
        .add_guest("a.dll", &image(true, 0x10000000, &[], &[]))
        .unwrap();
    catalog
        .add_guest("b.dll", &image(true, 0x10000000, &[], &[]))
        .unwrap();
    let mut memory = AddressSpace::new(12).unwrap();
    let mut set = ModuleSet32::new();
    let stale = catalog.plan_load(&set, &memory, "b", None).unwrap();
    catalog
        .plan_load(&set, &memory, "a", None)
        .unwrap()
        .install(&mut memory, &mut set)
        .unwrap();
    let count = memory.mapped_pages();
    assert_eq!(
        stale.install(&mut memory, &mut set).unwrap_err(),
        ModuleError32::StalePlan
    );
    assert_eq!(memory.mapped_pages(), count);
    assert_eq!(set.iter().len(), 1);
    assert_eq!(
        catalog
            .plan_load(&set, &memory, "b", Some(0x10000000))
            .unwrap_err(),
        ModuleError32::AddressConflict
    );
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x10010000), 1).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    let plan = catalog.plan_load(&set, &memory, "b", None).unwrap();
    assert_eq!(plan.additions()[0].base(), 0x10020000);
}
