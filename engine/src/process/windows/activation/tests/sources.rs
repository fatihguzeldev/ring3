use super::*;
use crate::{
    files::{ATTR_NORMAL, AssetRef32, FileIo32, FileTimes32},
    process::{FileConfig32, HostError},
};

fn source_engine(assets: &[(&str, &[u8])]) -> EngineInstance {
    let (mut engine, _) = engine(2);
    engine.configure_files(FileConfig32::default()).unwrap();
    for directory in [
        r"C:\game\plugins",
        r"C:\game\fr",
        r"C:\Windows\WinSxS\Manifests",
    ] {
        engine
            .mount_directory(
                &directory.encode_utf16().collect::<Vec<_>>(),
                0x10,
                FileTimes32::default(),
            )
            .unwrap();
    }
    for (index, (path, bytes)) in assets.iter().enumerate() {
        engine
            .mount_file(
                &path.encode_utf16().collect::<Vec<_>>(),
                AssetRef32 {
                    id: index as u64 + 1,
                    length: bytes.len() as u64,
                },
                ATTR_NORMAL,
                FileTimes32 {
                    written: 100 + index as u64,
                    ..Default::default()
                },
            )
            .unwrap();
    }
    let path = r"C:\root.manifest"
        .encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    write(&mut engine, INPUT + 0x100, &path);
    write(
        &mut engine,
        INPUT,
        &[32u32, 0, INPUT + 0x100, 0, 0, 0, 0, 0]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>(),
    );
    engine
}
fn resolve(
    engine: &mut EngineInstance,
    assets: &[(&str, &[u8])],
) -> (source_provider::CreateWork32, usize) {
    try_resolve(engine, assets).unwrap()
}
fn try_resolve(
    engine: &mut EngineInstance,
    assets: &[(&str, &[u8])],
) -> Result<(source_provider::CreateWork32, usize), HostError> {
    let source = source_provider::CreateSource32::read(
        engine.memory().unwrap(),
        INPUT,
        true,
        engine.windows_ansi_code_page(),
        0x409,
    )
    .unwrap();
    let mut work = source_provider::CreateWork32::new(source);
    let config = engine.activation.as_ref().unwrap().config;
    let version = engine.memory().unwrap().transaction_version();
    let mut waits = 0;
    loop {
        match work.poll(engine, config)? {
            FileIo32::Ready(()) => break,
            FileIo32::NeedBytes(request) => {
                assert_eq!(engine.memory().unwrap().transaction_version(), version);
                assert!(work.graph.is_none());
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
                waits += 1;
                assert!(waits <= 16);
            }
        }
    }
    Ok((work, waits))
}

#[test]
fn localized_dependency_binds_only_the_actual_probed_fallback_identity() {
    let root = String::from_utf8(ROOT.to_vec()).unwrap().replace(
        "name='Component' type='win32'",
        "name='Component' language='fr-BE' type='win32'",
    );
    let component = br#"<assembly xmlns='urn:schemas-microsoft-com:asm.v1' manifestVersion='1.0'><assemblyIdentity name='Component' type='win32' language='fr' version='1.0.0.0' publicKeyToken='0102030405060708'/><file name='component.dll'/></assembly>"#;
    let assets: &[(&str, &[u8])] = &[
        (r"C:\root.manifest", root.as_bytes()),
        (r"C:\game\fr\Component.manifest", component),
    ];
    let mut engine = source_engine(assets);
    let (work, waits) = resolve(&mut engine, assets);
    assert_eq!(waits, 2);
    let graph = work.graph.as_ref().unwrap();
    assert_eq!(graph.assemblies()[1].manifest.identity.language, "fr");
    assert_eq!(
        graph.assemblies()[0].manifest.dependencies[0]
            .identity
            .language,
        "fr"
    );
    assert_eq!(
        graph.assemblies()[1].manifest_path,
        r"C:\game\fr\Component.manifest"
            .encode_utf16()
            .collect::<Vec<_>>()
    );
    engine.publish_activation_context(&work).unwrap();

    for (old, new) in [
        ("language='fr'", "language='de'"),
        ("version='1.0.0.0'", "version='1.0.0.1'"),
        ("0102030405060708", "0102030405060709"),
    ] {
        let wrong = String::from_utf8(component.to_vec())
            .unwrap()
            .replace(old, new);
        let assets: &[(&str, &[u8])] = &[
            (r"C:\root.manifest", root.as_bytes()),
            (r"C:\game\fr\Component.manifest", wrong.as_bytes()),
        ];
        let mut engine = source_engine(assets);
        let version = engine.memory().unwrap().transaction_version();
        assert!(matches!(
            try_resolve(&mut engine, assets),
            Err(HostError::Activation(
                crate::windows::activation::ActivationError32::IdentityMismatch
            ))
        ));
        assert_eq!(engine.memory().unwrap().transaction_version(), version);
        assert!(engine.activation.as_ref().unwrap().storage.is_empty());
    }
}
const ROOT: &[u8] = br#"<assembly xmlns='urn:schemas-microsoft-com:asm.v1' manifestVersion='1.0'><assemblyIdentity name='Ring3.App' type='win32' version='1.0.0.0'/><dependency><dependentAssembly><assemblyIdentity name='Component' type='win32' version='1.0.0.0' publicKeyToken='0102030405060708'/></dependentAssembly></dependency></assembly>"#;

fn pe_source(engine: &mut EngineInstance, resource: Option<u32>) {
    write(
        engine,
        INPUT + 0x100,
        &r"C:\root.dll"
            .encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    );
    write(
        engine,
        INPUT,
        &[
            32,
            if resource.is_some() { 8 } else { 0 },
            INPUT + 0x100,
            0,
            0,
            resource.unwrap_or(0),
            0,
            0,
        ]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>(),
    );
}

#[test]
fn pe_source_without_an_explicit_name_selects_first_manifest_then_associated_file() {
    let mut pe = image();
    pe[0x430..0x434].copy_from_slice(&7u32.to_le_bytes());
    let assets: &[(&str, &[u8])] = &[(r"C:\root.dll", &pe)];
    let mut engine = source_engine(assets);
    pe_source(&mut engine, None);
    let (work, _) = resolve(&mut engine, assets);
    let root = &work.graph.as_ref().unwrap().assemblies()[0];
    assert_eq!(
        root.resource_name,
        Some(crate::loader::ResourceIdentifier32::Id(7))
    );
    assert_eq!(root.manifest.identity.name, "Ring3.Activation");
    engine.publish_activation_context(&work).unwrap();

    pe[0x108..0x110].fill(0);
    let manifest = br#"<assembly xmlns='urn:schemas-microsoft-com:asm.v1' manifestVersion='1.0'><assemblyIdentity name='Associated' type='win32' version='1.0.0.0'/></assembly>"#;
    let assets: &[(&str, &[u8])] = &[(r"C:\root.dll", &pe), (r"C:\root.dll.manifest", manifest)];
    let mut engine = source_engine(assets);
    pe_source(&mut engine, None);
    let (work, _) = resolve(&mut engine, assets);
    let root = &work.graph.as_ref().unwrap().assemblies()[0];
    assert_eq!(root.resource_name, None);
    assert_eq!(root.manifest.identity.name, "Associated");
    assert_eq!(
        root.manifest_path,
        r"C:\root.dll.manifest".encode_utf16().collect::<Vec<_>>()
    );
    assert_eq!(work.leases.len(), 2);
    engine.publish_activation_context(&work).unwrap();
    assert!(engine.pending_file_request().unwrap().is_none());
}

#[test]
fn absent_implicit_resource_and_missing_explicit_name_keep_the_resource_error() {
    let mut pe = image();
    pe[0x108..0x110].fill(0);
    let assets: &[(&str, &[u8])] = &[(r"C:\root.dll", &pe)];
    let mut engine = source_engine(assets);
    pe_source(&mut engine, None);
    assert!(matches!(
        try_resolve(&mut engine, assets),
        Err(HostError::Activation(
            crate::windows::activation::ActivationError32::Win32(1813)
        ))
    ));
    assert!(engine.activation.as_ref().unwrap().storage.is_empty());

    let pe = image();
    let assets: &[(&str, &[u8])] = &[(r"C:\root.dll", &pe), (r"C:\root.dll.manifest", ROOT)];
    let mut engine = source_engine(assets);
    pe_source(&mut engine, Some(7));
    assert!(matches!(
        try_resolve(&mut engine, assets),
        Err(HostError::Activation(
            crate::windows::activation::ActivationError32::Win32(1814)
        ))
    ));
    assert!(engine.activation.as_ref().unwrap().storage.is_empty());
}

#[test]
fn lazy_application_policy_overrides_versions_and_probes_configured_private_directory() {
    let configuration = br#"<configuration><windows><publisherPolicy apply='no'/><assemblyBinding xmlns='urn:schemas-microsoft-com:asm.v1'><assemblyIdentity name='Ring3.App' type='win32' version='1.0.0.0'/><probing privatePath='plugins'/><dependency><dependentAssembly><assemblyIdentity name='Component' type='win32' processorArchitecture='x86' publicKeyToken='0102030405060708'/><bindingRedirect oldVersion='1.0.0.0' newVersion='1.0.2.0'/></dependentAssembly></dependency></assemblyBinding></windows></configuration>"#;
    let component = br#"<assembly xmlns='urn:schemas-microsoft-com:asm.v1' manifestVersion='1.0'><assemblyIdentity name='Component' type='win32' version='1.0.2.0' publicKeyToken='0102030405060708'/><file name='component.dll'/></assembly>"#;
    let assets: &[(&str, &[u8])] = &[
        (r"C:\root.manifest", ROOT),
        (r"C:\root.config", configuration),
        (r"C:\game\plugins\Component.manifest", component),
    ];
    let mut engine = source_engine(assets);
    let (work, waits) = resolve(&mut engine, assets);
    assert!(waits >= 3);
    let graph = work.graph.as_ref().unwrap();
    assert_eq!(graph.assemblies().len(), 2);
    assert_eq!(
        graph.application_directory(),
        r"C:\game\".encode_utf16().collect::<Vec<_>>()
    );
    assert_eq!(
        graph.assemblies()[1].manifest.identity.version,
        [1, 0, 2, 0]
    );
    assert_eq!(
        graph.configuration_path().unwrap(),
        r"C:\root.config".encode_utf16().collect::<Vec<_>>()
    );
    let handle = engine.publish_activation_context(&work).unwrap();
    assert_eq!(
        engine
            .activation_call(1, Api::ActivateActCtx, &[handle, OUTPUT])
            .unwrap(),
        1
    );
    assert_eq!(
        engine
            .active_dll_redirection(&"component.dll".encode_utf16().collect::<Vec<_>>())
            .unwrap()
            .unwrap(),
        r"C:\game\plugins\component.dll"
            .encode_utf16()
            .collect::<Vec<_>>()
    );
    assert!(engine.pending_file_request().unwrap().is_none());
}

#[test]
fn publisher_policy_selects_effective_shared_version_and_preserves_winsxs_server_directory() {
    let policy = br#"<assembly xmlns='urn:schemas-microsoft-com:asm.v1' manifestVersion='1.0'><assemblyIdentity type='win32-policy' name='policy.1.0.Component' version='2.0.0.0' publicKeyToken='0102030405060708'/><dependency><dependentAssembly><assemblyIdentity type='win32' name='Component' publicKeyToken='0102030405060708'/><bindingRedirect oldVersion='1.0.0.0' newVersion='1.0.3.0'/></dependentAssembly></dependency></assembly>"#;
    let component = br#"<assembly xmlns='urn:schemas-microsoft-com:asm.v1' manifestVersion='1.0'><assemblyIdentity name='Component' type='win32' version='1.0.3.0' publicKeyToken='0102030405060708'/><file name='component.dll'/></assembly>"#;
    let assets: &[(&str, &[u8])] = &[
        (r"C:\root.manifest", ROOT),
        (
            r"C:\Windows\WinSxS\Manifests\x86_policy.1.0.Component_0102030405060708_2.0.0.0_none_1234.manifest",
            policy,
        ),
        (
            r"C:\Windows\WinSxS\Manifests\x86_Component_0102030405060708_1.0.3.0_none_abcd.manifest",
            component,
        ),
    ];
    let mut engine = source_engine(assets);
    let (work, waits) = resolve(&mut engine, assets);
    assert!(waits >= 3);
    let graph = work.graph.as_ref().unwrap();
    assert_eq!(
        graph.assemblies()[1].manifest.identity.version,
        [1, 0, 3, 0]
    );
    assert_eq!(
        graph.assemblies()[1].directory,
        r"C:\Windows\WinSxS\x86_Component_0102030405060708_1.0.3.0_none_abcd"
            .encode_utf16()
            .collect::<Vec<_>>()
    );
    assert_eq!(graph.assemblies()[1].modified, 102);
    let handle = engine.publish_activation_context(&work).unwrap();
    engine
        .activation_call(1, Api::ActivateActCtx, &[handle, OUTPUT])
        .unwrap();
    assert_eq!(
        engine
            .active_dll_redirection(&"component.dll".encode_utf16().collect::<Vec<_>>())
            .unwrap()
            .unwrap(),
        r"C:\Windows\WinSxS\x86_Component_0102030405060708_1.0.3.0_none_abcd\component.dll"
            .encode_utf16()
            .collect::<Vec<_>>()
    );
}
