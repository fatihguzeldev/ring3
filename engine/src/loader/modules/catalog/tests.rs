use super::*;
#[path = "../../../../tests/loader/modules/linking/native/image.rs"]
mod pe;
#[test]
fn system_group_failure_keeps_the_existing_catalog_and_does_not_admit_a_prefix() {
    let mut catalog = ModuleCatalog32::with_limits(ModuleLimits32 {
        modules: 2,
        ..Default::default()
    })
    .unwrap();
    let module = |name: &str| SystemModule32 {
        name: name.into(),
        preferred_base: 0x70000000,
        exports: Vec::new(),
    };
    catalog.add_system(module("existing.dll")).unwrap();
    assert_eq!(
        catalog.add_system_group(vec![module("kernel32.dll"), module("ntdll.dll")]),
        Err(ModuleError32::Capacity)
    );
    assert!(catalog.contains("existing.dll"));
    assert!(!catalog.contains("kernel32.dll"));
    assert!(!catalog.contains("ntdll.dll"));
}

#[test]
fn qualified_identity_uses_canonical_windows_names_without_collapsing_directories() {
    assert_eq!(
        normalize_module_identity(r"C:\one\..\two\Ä.dll").unwrap(),
        r"C:\TWO\Ä.DLL"
    );
    assert_ne!(
        normalize_module_identity(r"C:\one\plugin.dll").unwrap(),
        normalize_module_identity(r"C:\two\plugin.dll").unwrap()
    );
    for name in [
        r"relative\plugin.dll",
        "C:\\folder\\",
        r"C:\NUL.dll",
        r"C:\asset:stream",
    ] {
        assert_eq!(
            normalize_module_identity(name),
            Err(ModuleError32::InvalidName)
        );
    }
}

#[test]
fn qualified_catalog_sources_install_separate_images_and_reject_only_exact_aliases() {
    use crate::{loader::ModuleSet32, memory::AddressSpace};
    let mut catalog = ModuleCatalog32::new();
    for (path, base) in [
        (r"C:\one\Plugin.dll", 0x600000u32),
        (r"C:\two\Plugin.dll", 0x800000),
    ] {
        let bytes = pe::image(true, base, &[], &[]);
        catalog.add_guest(path, &bytes).unwrap();
        assert_eq!(
            catalog.add_guest(&path.to_ascii_uppercase(), &bytes),
            Err(ModuleError32::DuplicateModule)
        );
    }
    let mut memory = AddressSpace::new(16).unwrap();
    let mut modules = ModuleSet32::new();
    for path in [r"C:\one\Plugin.dll", r"C:\two\Plugin.dll"] {
        catalog
            .plan_load(&modules, &memory, path, None)
            .unwrap()
            .install(&mut memory, &mut modules)
            .unwrap();
    }
    let first = modules.find(r"C:\one\Plugin.dll").unwrap();
    let second = modules.find(r"C:\two\Plugin.dll").unwrap();
    assert_ne!(first.id(), second.id());
    assert_eq!((first.base(), second.base()), (0x600000, 0x800000));
    assert_eq!(first.file_path(), Some(r"C:\one\Plugin.dll"));
    assert_eq!(second.file_path(), Some(r"C:\two\Plugin.dll"));
    assert_eq!(first.base_name(), "PLUGIN.DLL");
    assert!(modules.find(r"C:\third\Plugin.dll").is_none());
}

#[test]
fn header_plan_exposes_root_before_missing_imports_without_installing_partial_memory() {
    use crate::{loader::ModuleSet32, memory::AddressSpace};
    let mut catalog = ModuleCatalog32::new();
    let bytes = pe::image(
        false,
        0x400000,
        &[pe::Import {
            module: "redirected.dll",
            symbols: vec![pe::Symbol::Name("Run"), pe::Symbol::Ordinal(12)],
        }],
        &[],
    );
    catalog.add_guest("main.exe", &bytes).unwrap();
    let memory = AddressSpace::new(16).unwrap();
    let modules = ModuleSet32::new();
    let version = memory.transaction_version();
    assert!(matches!(
        catalog.plan_load(&modules, &memory, "main.exe", None),
        Err(ModuleError32::MissingModule)
    ));
    let headers = catalog
        .plan_load_headers(&modules, &memory, "main.exe", None, &[], &[])
        .unwrap();
    assert_eq!(headers.additions().len(), 1);
    assert_eq!(headers.additions()[0].identity(), "main.exe");
    let mut signature = [0; 2];
    headers
        .read_staged(GuestAddress(0x400000), &mut signature)
        .unwrap();
    assert_eq!(signature, *b"MZ");
    assert_eq!(memory.mapped_pages(), 0);
    assert_eq!(memory.transaction_version(), version);
}

#[test]
fn replacement_is_atomic_and_waits_for_every_loaded_or_prepared_source_owner() {
    use crate::{loader::ModuleSet32, memory::AddressSpace};
    let old = pe::image(true, 0x600000, &[], &[]);
    let new = pe::image(true, 0x800000, &[], &[]);
    let mut catalog = ModuleCatalog32::with_limits(ModuleLimits32 {
        source_bytes: old.len(),
        ..Default::default()
    })
    .unwrap();
    catalog.add_guest(r"C:\game\Plugin.dll", &old).unwrap();
    let mut memory = AddressSpace::new(16).unwrap();
    let mut modules = ModuleSet32::new();
    let prepared = catalog
        .plan_load(&modules, &memory, r"C:\game\Plugin.dll", None)
        .unwrap();
    assert_eq!(
        catalog.replace_guest(r"C:\game\plugin.dll", &new),
        Err(ModuleError32::ModuleInUse)
    );
    assert_eq!(
        catalog.remove_guest(r"C:\game\plugin.dll"),
        Err(ModuleError32::ModuleInUse)
    );
    drop(prepared);
    assert!(matches!(
        catalog.replace_guest(r"C:\game\plugin.dll", &[0]),
        Err(ModuleError32::Pe(_))
    ));
    assert_eq!(catalog.remaining_source_bytes(), 0);
    let mut oversized = new.clone();
    oversized.push(0);
    assert_eq!(
        catalog.replace_guest(r"C:\game\plugin.dll", &oversized),
        Err(ModuleError32::Capacity)
    );
    catalog.replace_guest(r"C:\game\plugin.dll", &new).unwrap();
    let installed = catalog
        .plan_load(&modules, &memory, r"C:\game\plugin.dll", None)
        .unwrap()
        .install(&mut memory, &mut modules)
        .unwrap();
    let module = modules.get(installed.root).unwrap();
    assert_eq!(module.base(), 0x800000);
    assert_eq!(module.file_path(), Some(r"C:\game\Plugin.dll"));
    assert_eq!(
        catalog.replace_guest(r"C:\game\plugin.dll", &old),
        Err(ModuleError32::ModuleInUse)
    );
    modules
        .plan_unload(&memory, &[installed.root])
        .unwrap()
        .unmap(&mut memory, &mut modules)
        .unwrap();
    catalog.replace_guest(r"C:\game\plugin.dll", &old).unwrap();
    catalog.remove_guest(r"C:\game\plugin.dll").unwrap();
    assert!(catalog.is_empty());
    assert_eq!(catalog.remaining_source_bytes(), old.len());
}

#[test]
fn refreshed_resource_snapshots_keep_their_source_budget_until_last_owner_releases() {
    use crate::{
        loader::{DataFileLayout32, ModuleSet32},
        memory::AddressSpace,
    };
    let mut old = pe::image(true, 0x600000, &[], &[]);
    old[0x201] = 0x11;
    let mut new = old.clone();
    new[0x201] = 0x22;
    let mut catalog = ModuleCatalog32::with_limits(ModuleLimits32 {
        source_bytes: old.len() * 2,
        ..Default::default()
    })
    .unwrap();
    let mut memory = AddressSpace::new(16).unwrap();
    let mut modules = ModuleSet32::new();
    catalog.add_guest("resource.dll", &old).unwrap();
    let retained = catalog
        .plan_datafile(
            &modules,
            &memory,
            "resource.dll",
            DataFileLayout32::File,
            &[],
        )
        .unwrap()
        .install(&mut memory, &mut modules)
        .unwrap();
    catalog.retire_guest_snapshot("resource.dll").unwrap();
    assert_eq!(catalog.remaining_source_bytes(), old.len());
    assert_eq!(
        catalog.add_system_group(vec![SystemModule32 {
            name: "invalid.dll".into(),
            preferred_base: 0,
            exports: Vec::new(),
        }]),
        Err(ModuleError32::InvalidSystemExport)
    );
    assert_eq!(catalog.remaining_source_bytes(), old.len());
    catalog
        .add_system_group(vec![SystemModule32 {
            name: "provider.dll".into(),
            preferred_base: 0x70000000,
            exports: Vec::new(),
        }])
        .unwrap();
    assert_eq!(catalog.remaining_source_bytes(), old.len());
    retained.check(&memory).unwrap();
    catalog.add_guest("resource.dll", &new).unwrap();
    let current = catalog
        .plan_datafile(
            &modules,
            &memory,
            "resource.dll",
            DataFileLayout32::File,
            &[retained.reservation()],
        )
        .unwrap()
        .install(&mut memory, &mut modules)
        .unwrap();
    assert_eq!(catalog.remaining_source_bytes(), 0);
    let version = memory.transaction_version();
    assert_eq!(
        catalog.add_guest("extra.dll", &old),
        Err(ModuleError32::Capacity)
    );
    assert_eq!(
        catalog.remove_guest("resource.dll"),
        Err(ModuleError32::ModuleInUse)
    );
    assert_eq!(memory.transaction_version(), version);
    let mut byte = [0];
    for (module, expected) in [(&retained, 0x11), (&current, 0x22)] {
        memory
            .read(GuestAddress(module.base() + 0x201), &mut byte)
            .unwrap();
        assert_eq!(byte, [expected]);
        module.check(&memory).unwrap();
    }
    retained.unload(&mut memory, &mut modules).unwrap();
    drop(retained);
    assert_eq!(catalog.remaining_source_bytes(), old.len());
    catalog.add_guest("extra.dll", &old).unwrap();
    assert_eq!(catalog.remaining_source_bytes(), 0);
    current.check(&memory).unwrap();
}
