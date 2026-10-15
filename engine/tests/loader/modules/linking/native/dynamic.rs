use super::{
    image::{Import, Symbol, image},
    word,
};
use ring3_engine::{
    loader::{
        ImportSymbol32, LoadError, ModuleCatalog32, ModuleError32, ModuleLimits32, ModuleSet32,
        ParserLimits32,
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions, WordWrite32},
};

fn set_word(memory: &mut AddressSpace, address: u32, value: u32) {
    memory
        .write(GuestAddress(address), &value.to_le_bytes())
        .unwrap();
}

#[test]
fn live_mutated_strings_and_table_counts_use_the_configured_parser_budget() {
    let limits = ModuleLimits32 {
        parser: ParserLimits32 {
            metadata_entries: 32,
            string_bytes: 64,
        },
        ..ModuleLimits32::default()
    };
    let mut catalog = ModuleCatalog32::with_limits(limits).unwrap();
    catalog
        .add_guest(
            "alpha",
            &image(true, 0x10000000, &[], &[(1, Some("Run"), "beta.#20")]),
        )
        .unwrap();
    let mut set = ModuleSet32::new();
    let mut memory = AddressSpace::new(8).unwrap();
    catalog
        .plan_load(&set, &memory, "alpha", None)
        .unwrap()
        .install(&mut memory, &mut set)
        .unwrap();
    let alpha = set.find("alpha").unwrap().id();
    let mut long = vec![b'r'; 70];
    long.push(0);
    memory.write(GuestAddress(0x10002900), &long).unwrap();
    let pages = memory.mapped_pages();
    assert_eq!(
        set.resolve_export_live(&memory, alpha, &ImportSymbol32::Ordinal(1)),
        Err(ModuleError32::Capacity)
    );
    assert_eq!(
        catalog
            .plan_export_live(&set, &memory, alpha, &ImportSymbol32::Ordinal(1))
            .unwrap_err(),
        ModuleError32::Capacity
    );
    memory.write(GuestAddress(0x10002e00), &long).unwrap();
    set_word(&mut memory, 0x10002880, 0x2e00);
    assert_eq!(
        set.resolve_export_live(&memory, alpha, &ImportSymbol32::Name("r".repeat(70))),
        Err(ModuleError32::Capacity)
    );
    set_word(&mut memory, 0x10002814, 65);
    assert_eq!(
        set.resolve_export_live(&memory, alpha, &ImportSymbol32::Ordinal(1)),
        Err(ModuleError32::Capacity)
    );
    assert_eq!(memory.mapped_pages(), pages);
    assert_eq!(set.iter().len(), 1);
    assert!(set.get(alpha).unwrap().dependencies().is_empty());
}

#[test]
fn plans_are_bound_to_one_module_set_even_at_equal_revisions() {
    let mut catalog = ModuleCatalog32::new();
    catalog
        .add_guest("alpha", &image(true, 0x10000000, &[], &[]))
        .unwrap();
    let original = ModuleSet32::new();
    let mut different = ModuleSet32::new();
    let mut memory = AddressSpace::new(8).unwrap();
    let plan = catalog
        .plan_load(&original, &memory, "alpha", None)
        .unwrap();
    assert_eq!(
        plan.install(&mut memory, &mut different).unwrap_err(),
        ModuleError32::StalePlan
    );
    assert_eq!(memory.mapped_pages(), 0);
    assert_eq!(original.iter().len(), 0);
    assert_eq!(different.iter().len(), 0);
}

#[test]
fn forwarder_edge_journal_allows_exact_failure_rollback_and_retained_target_cleanup() {
    let mut catalog = ModuleCatalog32::new();
    catalog
        .add_guest(
            "alpha",
            &image(true, 0x10000000, &[], &[(1, Some("Run"), "beta.#20")]),
        )
        .unwrap();
    catalog
        .add_guest(
            "beta",
            &image(true, 0x20000000, &[], &[(20, None, "@1000")]),
        )
        .unwrap();
    let mut set = ModuleSet32::new();
    let mut memory = AddressSpace::new(12).unwrap();
    catalog
        .plan_load(&set, &memory, "alpha", None)
        .unwrap()
        .install(&mut memory, &mut set)
        .unwrap();
    let alpha = set.find("alpha").unwrap().id();
    let name = ImportSymbol32::Name("Run".into());
    let (plan, _) = catalog
        .plan_export_live(&set, &memory, alpha, &name)
        .unwrap();
    let load = plan.install(&mut memory, &mut set).unwrap();
    let beta = set.find("beta").unwrap().id();
    assert_eq!(load.introduced_edges, vec![(alpha, beta)]);
    assert_eq!(
        set.plan_unload(&memory, &[beta]).unwrap_err(),
        ModuleError32::ModuleInUse
    );
    set.plan_unload_rollback(&memory, &[beta], &load.introduced_edges)
        .unwrap()
        .unmap(&mut memory, &mut set)
        .unwrap();
    assert!(set.find("beta").is_none());
    assert!(set.get(alpha).unwrap().dependencies().is_empty());
    let (plan, _) = catalog
        .plan_export_live(&set, &memory, alpha, &name)
        .unwrap();
    let reload = plan.install(&mut memory, &mut set).unwrap();
    let replacement = set.find("beta").unwrap().id();
    assert_ne!(replacement, beta);
    set.plan_unload_rollback(&memory, &[], &reload.introduced_edges)
        .unwrap()
        .unmap(&mut memory, &mut set)
        .unwrap();
    assert!(set.get(alpha).unwrap().dependencies().is_empty());
    assert!(set.get(replacement).is_some());
    set.plan_unload(&memory, &[replacement])
        .unwrap()
        .unmap(&mut memory, &mut set)
        .unwrap();
    assert!(set.find("beta").is_none());
}

#[test]
fn live_forwarders_demand_load_and_eat_mutations_invalidate_plans() {
    let mut catalog = ModuleCatalog32::new();
    catalog
        .add_guest(
            "alpha",
            &image(
                true,
                0x10000000,
                &[],
                &[(1, Some("Run"), "beta.dll.#00020")],
            ),
        )
        .unwrap();
    catalog
        .add_guest(
            "beta",
            &image(true, 0x20000000, &[], &[(20, None, "@1000")]),
        )
        .unwrap();
    catalog
        .add_guest(
            "gamma",
            &image(true, 0x30000000, &[], &[(40, None, "@2f00")]),
        )
        .unwrap();
    let mut set = ModuleSet32::new();
    let mut memory = AddressSpace::new(20).unwrap();
    catalog
        .plan_load(&set, &memory, "alpha", None)
        .unwrap()
        .install(&mut memory, &mut set)
        .unwrap();
    let alpha = set.find("alpha").unwrap().id();
    let name = ImportSymbol32::Name("Run".into());
    assert_eq!(set.iter().len(), 1);
    let (plan, target) = catalog
        .plan_export_live(&set, &memory, alpha, &name)
        .unwrap();
    assert_eq!(plan.additions().len(), 1);
    assert_eq!(target.address(), GuestAddress(0x20001000));
    assert_eq!(set.iter().len(), 1);
    let loaded = plan.install(&mut memory, &mut set).unwrap();
    assert_eq!(loaded.initialization_order, loaded.new_ids);
    assert_eq!(
        set.resolve_export_live(&memory, alpha, &name)
            .unwrap()
            .address(),
        GuestAddress(0x20001000)
    );

    memory
        .write(GuestAddress(0x10002900), b"gamma.#40\0")
        .unwrap();
    let (stale, _) = catalog
        .plan_export_live(&set, &memory, alpha, &name)
        .unwrap();
    let mapped = memory.mapped_pages();
    set_word(&mut memory, 0x10002840, 0x2f00);
    assert_eq!(
        stale.install(&mut memory, &mut set).unwrap_err(),
        ModuleError32::StalePlan
    );
    assert_eq!(memory.mapped_pages(), mapped);
    assert!(set.find("gamma").is_none());
    assert_eq!(
        set.resolve_export_live(&memory, alpha, &name)
            .unwrap()
            .address(),
        GuestAddress(0x10002f00)
    );
    set_word(&mut memory, 0x10002840, 0x2900);
    let (fresh, target) = catalog
        .plan_export_live(&set, &memory, alpha, &name)
        .unwrap();
    assert_eq!(target.address(), GuestAddress(0x30002f00));
    assert_eq!(fresh.additions().len(), 1);
    fresh.install(&mut memory, &mut set).unwrap();
    assert_eq!(
        set.resolve_export_live(&memory, alpha, &name)
            .unwrap()
            .address(),
        GuestAddress(0x30002f00)
    );
    assert_eq!(set.iter().len(), 3);

    set_word(&mut memory, 0x10002840, 0xfffff000);
    assert_eq!(
        set.resolve_export_live(&memory, alpha, &name),
        Err(ModuleError32::Pe(LoadError::Malformed))
    );
    assert_eq!(set.iter().len(), 3);
    assert_eq!(memory.mapped_pages(), mapped + 4);
}

#[test]
fn unload_respects_inbound_dependencies_and_clears_runtime_words_atomically() {
    let mut catalog = ModuleCatalog32::new();
    catalog
        .add_guest(
            "alpha",
            &image(
                true,
                0x10000000,
                &[Import {
                    module: "beta",
                    symbols: vec![Symbol::Ordinal(20)],
                }],
                &[],
            ),
        )
        .unwrap();
    catalog
        .add_guest(
            "beta",
            &image(true, 0x20000000, &[], &[(20, None, "@1000")]),
        )
        .unwrap();
    let mut set = ModuleSet32::new();
    let mut memory = AddressSpace::new(12).unwrap();
    catalog
        .plan_load(&set, &memory, "alpha", None)
        .unwrap()
        .install(&mut memory, &mut set)
        .unwrap();
    let alpha = set.find("alpha").unwrap().id();
    let beta = set.find("beta").unwrap().id();
    assert_eq!(
        set.plan_unload(&memory, &[beta]).unwrap_err(),
        ModuleError32::ModuleInUse
    );
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x70000000), 1).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    set_word(&mut memory, 0x70000000, 0x11223344);
    let code = memory.snapshot_code(GuestAddress(0x10001000), 1).unwrap();
    let stale = set.plan_unload(&memory, &[alpha, beta]).unwrap();
    set_word(&mut memory, 0x70000000, 0x55667788);
    assert_eq!(
        stale.unmap(&mut memory, &mut set),
        Err(ModuleError32::StalePlan)
    );
    assert_eq!(set.iter().len(), 2);
    assert!(memory.is_code_current(&code));
    let plan = set.plan_unload(&memory, &[alpha, beta]).unwrap();
    plan.unmap_with_writes(
        &mut memory,
        &mut set,
        &[WordWrite32 {
            address: GuestAddress(0x70000000),
            value: 0,
        }],
    )
    .unwrap();
    assert_eq!(word(&memory, 0x70000000), 0);
    assert_eq!(set.iter().len(), 0);
    assert_eq!(memory.mapped_pages(), 1);
    assert!(!memory.is_code_current(&code));
}

#[test]
fn staged_span_errors_and_failed_existing_word_publication_are_neutral() {
    let mut catalog = ModuleCatalog32::new();
    catalog
        .add_guest("alpha", &image(true, 0x10000000, &[], &[]))
        .unwrap();
    let mut set = ModuleSet32::new();
    let mut memory = AddressSpace::new(8).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x70000000), 1).unwrap(),
            Permissions::READ,
        )
        .unwrap();
    let mut plan = catalog.plan_load(&set, &memory, "alpha", None).unwrap();
    let mut before = [0xa5; 8];
    assert_eq!(
        plan.read_staged(GuestAddress(0x10002ffc), &mut before),
        Err(ModuleError32::AddressConflict)
    );
    assert_eq!(before, [0xa5; 8]);
    let mut tail = [0; 4];
    plan.read_staged(GuestAddress(0x10002ffc), &mut tail)
        .unwrap();
    assert_eq!(
        plan.stage_write(GuestAddress(0x10002ffc), &[0x5a; 8]),
        Err(ModuleError32::AddressConflict)
    );
    let mut after = [0; 4];
    plan.read_staged(GuestAddress(0x10002ffc), &mut after)
        .unwrap();
    assert_eq!(after, tail);
    assert_eq!(
        plan.add_page(GuestAddress(0x10003000), Permissions::READ_WRITE, &[]),
        Err(ModuleError32::AddressConflict)
    );
    assert!(matches!(
        plan.install_with_writes(
            &mut memory,
            &mut set,
            &[WordWrite32 {
                address: GuestAddress(0x70000000),
                value: 1,
            }]
        ),
        Err(ModuleError32::Memory(_))
    ));
    assert_eq!(memory.mapped_pages(), 1);
    assert_eq!(word(&memory, 0x70000000), 0);
    assert_eq!(set.iter().len(), 0);
}

#[test]
fn explicit_catalog_budgets_do_not_change_identity_or_insert_partial_sources() {
    let limits = ModuleLimits32 {
        modules: 1,
        source_bytes: 0x1600,
        forwarder_depth: 2,
        parser: Default::default(),
    };
    let mut catalog = ModuleCatalog32::with_limits(limits).unwrap();
    let source = image(true, 0x10000000, &[], &[]);
    catalog.add_guest("Game Engine.DLL", &source).unwrap();
    assert!(catalog.contains("game engine.dll"));
    assert_eq!(
        catalog.add_guest("GAME ENGINE.dll", &source),
        Err(ModuleError32::DuplicateModule)
    );
    assert_eq!(
        catalog.add_guest("other.dll", &source),
        Err(ModuleError32::Capacity)
    );
    assert!(!catalog.contains("other.dll"));
    assert_eq!(catalog.limits(), limits);
}
