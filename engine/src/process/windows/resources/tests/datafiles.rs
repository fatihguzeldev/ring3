use super::*;
use crate::{
    loader::{
        DataFileLayout32 as Layout, ModuleCatalog32, ModuleError32, ModuleLimits32, ModuleSet32,
        ParserLimits32, PeResources32, ResourceIdentifier32 as Id,
    },
    memory::{Access, AddressSpace, Permissions},
};

#[test]
fn raw_file_and_image_mappings_share_ids_and_resolve_different_physical_layouts() {
    let mut catalog = ModuleCatalog32::with_limits(ModuleLimits32 {
        modules: 3,
        ..Default::default()
    })
    .unwrap();
    catalog
        .add_guest("resource.dll", &image(0x500000, true, false))
        .unwrap();
    catalog
        .add_guest("ordinary.dll", &image(0x600000, true, false))
        .unwrap();
    let mut set = ModuleSet32::new();
    let mut memory = AddressSpace::new(16).unwrap();
    let file = catalog
        .plan_datafile(&set, &memory, "resource.dll", Layout::File, &[])
        .unwrap()
        .install(&mut memory, &mut set)
        .unwrap();
    let mapped = catalog
        .plan_datafile(
            &set,
            &memory,
            "resource.dll",
            Layout::Image,
            &[file.reservation()],
        )
        .unwrap()
        .install(&mut memory, &mut set)
        .unwrap();
    assert_eq!(file.id().get(), 1);
    assert_eq!(mapped.id().get(), 2);
    assert_eq!(file.handle() & 3, 1);
    assert_eq!(mapped.handle() & 3, 2);
    assert_ne!(file.base(), mapped.base());
    assert_eq!(set.iter().len(), 0);
    for (module, offset) in [(&file, 0x580), (&mapped, 0x2180)] {
        let mut reader =
            PeResources32::from_datafile(&memory, module, ParserLimits32::default()).unwrap();
        let location = reader.find_exact(&Id::Id(10), &Id::Id(7), 0x409).unwrap();
        let address = reader.address(location.data_rva, location.size).unwrap();
        assert_eq!(address.0, module.base() + offset);
        let mut bytes = [0; 4];
        memory.read(address, &mut bytes).unwrap();
        assert_eq!(bytes, [0x11, 0x22, 0x33, 0x44]);
        assert!(memory.check_span(address, 1, Access::Write).is_err());
        assert!(memory.check_span(address, 1, Access::Execute).is_err());
    }
    catalog
        .plan_load(&set, &memory, "ordinary.dll", None)
        .unwrap()
        .install(&mut memory, &mut set)
        .unwrap();
    assert_eq!(set.iter().next().unwrap().id().get(), 3);
    assert!(matches!(
        catalog.plan_datafile(&set, &memory, "resource.dll", Layout::File, &[]),
        Err(ModuleError32::Capacity)
    ));
    mapped.unload(&mut memory, &mut set).unwrap();
    assert!(
        catalog
            .plan_datafile(&set, &memory, "resource.dll", Layout::File, &[])
            .is_ok()
    );
}

#[test]
fn stale_install_and_replaced_mapping_never_publish_or_reclaim_another_owner() {
    let mut catalog = ModuleCatalog32::new();
    catalog
        .add_guest("resource.dll", &image(0x500000, true, false))
        .unwrap();
    let mut set = ModuleSet32::new();
    let mut memory = AddressSpace::new(16).unwrap();
    let pending = catalog
        .plan_datafile(&set, &memory, "resource.dll", Layout::File, &[])
        .unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x20000), 1).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    assert!(matches!(
        pending.install(&mut memory, &mut set),
        Err(ModuleError32::StalePlan)
    ));
    assert_eq!(memory.mapped_pages(), 1);
    let file = catalog
        .plan_datafile(&set, &memory, "resource.dll", Layout::File, &[])
        .unwrap()
        .install(&mut memory, &mut set)
        .unwrap();
    assert_eq!(file.id().get(), 1);
    memory.unmap(file.reservation()).unwrap();
    memory
        .map_zeroed(file.reservation(), Permissions::READ_WRITE)
        .unwrap();
    memory
        .write(GuestAddress(file.base()), &0xfeedbeefu32.to_le_bytes())
        .unwrap();
    assert!(matches!(
        file.unload(&mut memory, &mut set),
        Err(ModuleError32::StalePlan)
    ));
    let mut bytes = [0; 4];
    memory.read(GuestAddress(file.base()), &mut bytes).unwrap();
    assert_eq!(u32::from_le_bytes(bytes), 0xfeedbeef);
}

#[test]
fn resource_only_frontend_preserves_tagged_identity_and_normal_loaded_module_reuse() {
    let mut engine = engine(false, 8);
    register(&mut engine, "resource.dll", &image(0x500000, true, false));
    let ModuleOperation32::Loaded { handle: file } =
        engine.load_resource_only_module("resource.dll", 2).unwrap()
    else {
        panic!("resource load cannot initialize")
    };
    let ModuleOperation32::Loaded { handle: mapped } = engine
        .load_resource_only_module("resource.dll", 0x20)
        .unwrap()
    else {
        panic!("image resource load cannot initialize")
    };
    assert_eq!(file, 0x500001);
    assert_eq!(file & 3, 1);
    assert_eq!(mapped & 3, 2);
    assert_ne!(file & !3, mapped & !3);
    assert_eq!(engine.module_handle(Some("resource.dll")).unwrap(), None);
    let resource = value(&mut engine, Api::FindResourceW, &[file, 7, 10]).unwrap();
    assert_eq!(
        value(&mut engine, Api::LoadResource, &[file, resource]),
        Ok((file & !3) + 0x580)
    );
    assert_eq!(
        engine.free_resource_only_module(file),
        Ok(ModuleOperation32::Released)
    );
    assert_eq!(
        value(&mut engine, Api::SizeofResource, &[mapped, resource]),
        Err(ResourceError32::InvalidHandle)
    );
    engine.free_resource_only_module(mapped).unwrap();
    let normal = engine.load_library_module("resource.dll", None).unwrap();
    assert_eq!(normal, ModuleOperation32::Loaded { handle: 0x500000 });
    assert_eq!(
        engine.load_resource_only_module("resource.dll", 0x60),
        Ok(normal)
    );
    assert_eq!(
        engine.module_handle(Some("resource.dll")).unwrap(),
        Some(0x500000)
    );
}

#[test]
fn sparse_image_reserves_virtual_extent_without_allocating_gaps_and_protection_keeps_witness() {
    let mut source = image(0x500000, true, false);
    for (at, value) in [
        (0x98 + 56, 0x11000u32),
        (0x98 + 112, 0x10000),
        (0x178 + 40 + 12, 0x10000),
        (0x400 + 0xb0, 0x10100),
        (0x400 + 0xc0, 0x10180),
    ] {
        source[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    let mut engine = engine(false, 8);
    register(&mut engine, "sparse.dll", &source);
    let before = engine.memory.as_ref().unwrap().mapped_pages();
    let ModuleOperation32::Loaded { handle } = engine
        .load_resource_only_module("sparse.dll", 0x20)
        .unwrap()
    else {
        panic!("no initializer")
    };
    assert_eq!(engine.memory.as_ref().unwrap().mapped_pages() - before, 3);
    let base = handle & !3;
    assert!(
        engine
            .memory
            .as_ref()
            .unwrap()
            .permissions_at(GuestAddress(base + 0x8000))
            .is_err()
    );
    assert_eq!(
        engine.resource_mapping_info(base + 0x8000).unwrap(),
        Some((base, 0x11000, Layout::Image))
    );
    let page = PageRange::new(GuestAddress(base + 0x10000), 1).unwrap();
    assert_eq!(
        engine.validate_resource_mapping_protection(page, Permissions::READ_WRITE),
        Err(ModuleError32::InvalidRequest)
    );
    assert_eq!(
        engine.validate_resource_mapping_protection(page, Permissions::NONE),
        Ok(true)
    );
    engine
        .memory
        .as_mut()
        .unwrap()
        .protect(page, Permissions::NONE)
        .unwrap();
    engine.commit_resource_mapping_protection(page);
    assert_eq!(
        engine.free_resource_only_module(handle),
        Ok(ModuleOperation32::Released)
    );
}
