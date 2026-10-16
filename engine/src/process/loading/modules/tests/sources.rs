#[path = "../../../../../tests/loader/modules/linking/native/image.rs"]
mod pe;

use crate::{
    files::{ATTR_DIRECTORY, ATTR_NORMAL, AssetRef32, FileTimes32},
    memory::GuestAddress,
    process::{EngineInstance, FileConfig32, HostError, ModuleOperation32},
};

mod datafiles;
mod paths;
mod security;
mod unresolved;

fn image(dll: bool, base: u32, import: Option<&str>, manifest: Option<&[u8]>) -> Vec<u8> {
    let imports = import.map(|module| pe::Import {
        module,
        symbols: vec![pe::Symbol::Name("Run"), pe::Symbol::Ordinal(1)],
    });
    let mut bytes = pe::image(dll, base, imports.as_slice(), &[(1, Some("Run"), "@1000")]);
    if let Some(manifest) = manifest {
        assert!(manifest.len() <= 0x300);
        let resource = &mut bytes[0x1000..0x1400];
        resource.fill(0);
        for (offset, value) in [(14, 1u16), (0x20 + 14, 1), (0x40 + 14, 1)] {
            resource[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        }
        for (offset, value) in [
            (16, 24u32),
            (20, 0x80000020),
            (0x30, if dll { 2 } else { 1 }),
            (0x34, 0x80000040),
            (0x50, 0x409),
            (0x54, 0x60),
            (0x60, 0x2d00),
            (0x64, manifest.len() as u32),
        ] {
            resource[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        resource[0x100..0x100 + manifest.len()].copy_from_slice(manifest);
        bytes[0x108..0x10c].copy_from_slice(&0x2c00u32.to_le_bytes());
        bytes[0x10c..0x110].copy_from_slice(&0x400u32.to_le_bytes());
    }
    bytes
}

fn filesystem(assets: &[(&str, &[u8])]) -> EngineInstance {
    let mut engine = EngineInstance::new(512, 7).unwrap();
    engine.configure_files(FileConfig32::default()).unwrap();
    for directory in [r"C:\app", r"C:\app\RootLib", r"C:\app\InnerLib"] {
        engine
            .mount_directory(
                &directory.encode_utf16().collect::<Vec<_>>(),
                ATTR_DIRECTORY,
                FileTimes32::default(),
            )
            .unwrap();
    }
    for (index, &(path, bytes)) in assets.iter().enumerate() {
        engine
            .mount_file(
                &path.encode_utf16().collect::<Vec<_>>(),
                AssetRef32 {
                    id: index as u64 + 1,
                    length: bytes.len() as u64,
                },
                ATTR_NORMAL,
                FileTimes32::default(),
            )
            .unwrap();
    }
    engine
}

fn supply(
    engine: &mut EngineInstance,
    request: crate::files::FileReadRequest32,
    assets: &[(&str, &[u8])],
) {
    let bytes = assets[request.asset as usize - 1].1;
    let start = request.offset as usize;
    for (chunk, bytes) in bytes[start..start + request.length as usize]
        .chunks(4096)
        .enumerate()
    {
        engine
            .supply_file_bytes(request, (chunk * 4096) as u32, bytes)
            .unwrap();
    }
}

fn finish(
    engine: &mut EngineInstance,
    mut operation: ModuleOperation32,
    assets: &[(&str, &[u8])],
) -> (u32, usize) {
    let mut waits = 0;
    loop {
        match operation {
            ModuleOperation32::Loaded { handle } => return (handle, waits),
            ModuleOperation32::NeedBytes { request } => {
                let pending = engine.pending_module_source().unwrap();
                assert_eq!(pending.request, Some(request));
                assert!(!pending.ready);
                assert_eq!(engine.loader_initialization_owner(), Some(pending.thread));
                assert!(
                    engine
                        .resume_module_source(pending.epoch, pending.token + 1)
                        .is_err()
                );
                assert_eq!(engine.pending_module_source().unwrap(), pending);
                supply(engine, request, assets);
                assert!(engine.pending_module_source().unwrap().ready);
                operation = engine
                    .resume_module_source(pending.epoch, pending.token)
                    .unwrap();
                waits += 1;
                assert!(waits <= 16);
            }
            other => panic!("unexpected module operation {other:?}"),
        }
    }
}

#[test]
fn automatic_manifest_uses_the_configured_ui_language_before_neutral_or_english() {
    let english = br#"<assembly xmlns='urn:schemas-microsoft-com:asm.v1' manifestVersion='1.0'><assemblyIdentity name='English' type='win32' version='1.0.0.0'/></assembly>"#;
    let japanese = br#"<assembly xmlns='urn:schemas-microsoft-com:asm.v1' manifestVersion='1.0'><assemblyIdentity name='Japanese' type='win32' version='1.0.0.0'/></assembly>"#;
    let neutral = br#"<assembly xmlns='urn:schemas-microsoft-com:asm.v1' manifestVersion='1.0'><assemblyIdentity name='Neutral' type='win32' version='1.0.0.0'/></assembly>"#;
    let mut bytes = image(false, 0x400000, None, Some(english));
    let resource = &mut bytes[0x1000..0x1400];
    resource[0x4e..0x50].copy_from_slice(&3u16.to_le_bytes());
    for (offset, value) in [
        (0x50, 0u32),
        (0x54, 0x90),
        (0x58, 0x409),
        (0x5c, 0x70),
        (0x60, 0x411),
        (0x64, 0x80),
        (0x70, 0x2d00),
        (0x74, english.len() as u32),
        (0x80, 0x2e00),
        (0x84, japanese.len() as u32),
        (0x90, 0x2f00),
        (0x94, neutral.len() as u32),
    ] {
        resource[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    resource[0x200..0x200 + japanese.len()].copy_from_slice(japanese);
    resource[0x300..0x300 + neutral.len()].copy_from_slice(neutral);
    let mut engine = filesystem(&[]);
    engine
        .configure_nls(crate::windows::nls::NlsProfile32 {
            locale: 0x411,
            ..Default::default()
        })
        .unwrap();
    engine
        .register_guest_module(r"C:\app\localized.exe", &bytes)
        .unwrap();
    assert!(matches!(
        engine
            .start_module_process(r"C:\app\localized.exe", None, Default::default())
            .unwrap(),
        ModuleOperation32::Loaded { handle: 0x400000 }
    ));
    let graph = engine
        .capture_module_process_default_graph()
        .unwrap()
        .unwrap();
    assert_eq!(graph.assemblies()[0].manifest.identity.name, "Japanese");
    assert_eq!(
        graph.assemblies()[0].resource_name,
        Some(crate::loader::ResourceIdentifier32::Id(1))
    );
}

#[test]
fn root_and_dll_manifests_bind_before_each_importer_dependency_search() {
    let root = br#"<assembly xmlns='urn:schemas-microsoft-com:asm.v1' manifestVersion='1.0'><assemblyIdentity name='App' type='win32' version='1.0.0.0'/><dependency><dependentAssembly><assemblyIdentity name='RootLib' type='win32' version='1.0.0.0'/></dependentAssembly></dependency></assembly>"#;
    let outer = br#"<assembly xmlns='urn:schemas-microsoft-com:asm.v1' manifestVersion='1.0'><assemblyIdentity name='Outer' type='win32' version='1.0.0.0'/><dependency><dependentAssembly><assemblyIdentity name='InnerLib' type='win32' version='1.0.0.0'/></dependentAssembly></dependency></assembly>"#;
    let root_library = br#"<assembly xmlns='urn:schemas-microsoft-com:asm.v1' manifestVersion='1.0'><assemblyIdentity name='RootLib' type='win32' version='1.0.0.0'/><file name='outer.dll'/></assembly>"#;
    let inner_library = br#"<assembly xmlns='urn:schemas-microsoft-com:asm.v1' manifestVersion='1.0'><assemblyIdentity name='InnerLib' type='win32' version='1.0.0.0'/><file name='inner.dll'/></assembly>"#;
    let assets: &[(&str, &[u8])] = &[
        (r"C:\app\RootLib\RootLib.manifest", root_library),
        (r"C:\app\InnerLib\InnerLib.manifest", inner_library),
    ];
    let mut engine = filesystem(assets);
    engine
        .register_guest_module(
            r"C:\app\main.exe",
            &image(false, 0x400000, Some("outer.dll"), Some(root)),
        )
        .unwrap();
    engine
        .register_guest_module(
            r"C:\app\RootLib\outer.dll",
            &image(true, 0x600000, Some("inner.dll"), Some(outer)),
        )
        .unwrap();
    engine
        .register_guest_module(
            r"C:\app\InnerLib\inner.dll",
            &image(true, 0x800000, None, None),
        )
        .unwrap();
    let before = engine.memory().unwrap().transaction_version();
    let first = engine
        .start_module_process(r"C:\app\main.exe", None, Default::default())
        .unwrap();
    assert!(matches!(first, ModuleOperation32::NeedBytes { .. }));
    assert_eq!(engine.memory().unwrap().mapped_pages(), 0);
    assert_eq!(engine.memory().unwrap().transaction_version(), before);
    let (handle, waits) = finish(&mut engine, first, assets);
    assert_eq!(handle, 0x400000);
    assert_eq!(waits, 2);
    assert_eq!(
        engine
            .module_handle(Some(r"C:\app\RootLib\outer.dll"))
            .unwrap(),
        Some(0x600000)
    );
    assert_eq!(
        engine
            .module_handle(Some(r"C:\app\InnerLib\inner.dll"))
            .unwrap(),
        Some(0x800000)
    );
    let mut iat = [0; 8];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x402400), &mut iat)
        .unwrap();
    assert_eq!(
        iat,
        [0x601000u32.to_le_bytes(), 0x601000u32.to_le_bytes()]
            .concat()
            .as_slice()
    );
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x602400), &mut iat)
        .unwrap();
    assert_eq!(
        iat,
        [0x801000u32.to_le_bytes(), 0x801000u32.to_le_bytes()]
            .concat()
            .as_slice()
    );
    assert!(engine.pending_module_source().is_none());
    assert!(engine.loader_initialization_owner().is_none());
    assert!(engine.pending_file_request().unwrap().is_none());
}

#[test]
fn lazy_vfs_dll_keeps_loaded_identity_then_refreshes_exact_content_version_after_unload() {
    let old = image(true, 0x600000, None, None);
    let new = image(true, 0x800000, None, None);
    let assets: &[(&str, &[u8])] = &[(r"C:\app\plugin.dll", &old)];
    let mut engine = filesystem(assets);
    engine
        .register_guest_module(r"C:\app\main.exe", &image(false, 0x400000, None, None))
        .unwrap();
    assert_eq!(
        engine
            .start_module_process(r"C:\app\main.exe", None, Default::default())
            .unwrap(),
        ModuleOperation32::Loaded { handle: 0x400000 }
    );
    let operation = engine
        .load_library_module(r"C:\app\plugin.dll", None)
        .unwrap();
    let (handle, waits) = finish(&mut engine, operation, assets);
    assert_eq!((handle, waits), (0x600000, 1));
    let path = r"C:\app\plugin.dll".encode_utf16().collect::<Vec<_>>();
    let version = engine.provider_path_version(&path).unwrap();
    engine.replace_provider_file(&path, &new).unwrap();
    assert_ne!(engine.provider_path_version(&path).unwrap(), version);
    assert_eq!(
        engine
            .load_library_module(r"C:\app\plugin.dll", None)
            .unwrap(),
        ModuleOperation32::Loaded { handle: 0x600000 }
    );
    engine.free_library_module(0x600000).unwrap();
    engine.free_library_module(0x600000).unwrap();
    assert_eq!(
        engine
            .load_library_module(r"C:\app\plugin.dll", None)
            .unwrap(),
        ModuleOperation32::Loaded { handle: 0x800000 }
    );
    assert!(engine.pending_module_source().is_none());
    let writable = engine.open_provider_file(&path, true).unwrap();
    engine.close_provider_file(writable).unwrap();
}

#[test]
fn malformed_lazy_pe_closes_source_and_keeps_ram_and_loader_unpublished() {
    let assets: &[(&str, &[u8])] = &[(r"C:\app\main.exe", b"not a PE image")];
    let mut engine = filesystem(assets);
    let before = engine.memory().unwrap().transaction_version();
    let ModuleOperation32::NeedBytes { request } = engine
        .start_module_process(r"C:\app\main.exe", None, Default::default())
        .unwrap()
    else {
        panic!("lazy source")
    };
    let pending = engine.pending_module_source().unwrap();
    supply(&mut engine, request, assets);
    assert!(matches!(
        engine.resume_module_source(pending.epoch, pending.token),
        Err(HostError::Module(crate::loader::ModuleError32::Pe(_)))
    ));
    assert!(engine.pending_module_source().is_none());
    assert!(engine.loader_initialization_owner().is_none());
    assert_eq!(engine.memory().unwrap().transaction_version(), before);
    assert_eq!(engine.memory().unwrap().mapped_pages(), 0);
    assert_eq!(engine.loaded_windows_modules().len(), 0);
    let writable = engine
        .open_provider_file(&r"C:\app\main.exe".encode_utf16().collect::<Vec<_>>(), true)
        .unwrap();
    engine.close_provider_file(writable).unwrap();
}
