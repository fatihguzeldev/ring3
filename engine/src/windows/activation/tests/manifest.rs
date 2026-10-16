use super::*;

#[test]
fn native_registration_manifest_preserves_real_guids_and_interface_metadata() {
    let bytes = manifest(
        "Ring3.Native",
        "<file name='native.dll'>\
         <comClass clsid='{12345678-9abc-def0-0102-030405060708}' threadingModel='Apartment' \
         tlbid='{11111111-2222-3333-4455-66778899aabb}' progid='Ring3.Native.1'>\
         <progid>Ring3.Native</progid></comClass>\
         <typelib tlbid='{11111111-2222-3333-4455-66778899aabb}' version='2.3' helpdir='' flags='HIDDEN,CONTROL'/>\
         <comInterfaceProxyStub iid='{aaaaaaaa-bbbb-cccc-ddee-ff0011223344}' numMethods='7' \
         proxyStubClsid32='{12345678-9abc-def0-0102-030405060708}'/>\
         <windowClass versioned='no'>Ring3Window</windowClass></file>",
    );
    let parsed = Manifest32::parse(&bytes, ActivationConfig32::default()).unwrap();
    let file = &parsed.files[0];
    assert_eq!(file.classes[0].clsid, CLSID);
    assert_eq!(
        file.classes[0].threading,
        crate::windows::com::ServerThreadingModel32::Apartment
    );
    assert_eq!(file.classes[0].progids, ["Ring3.Native.1", "Ring3.Native"]);
    assert_eq!(file.interfaces[0].methods, Some(7));
    assert_eq!(file.type_libraries[0].version, [2, 3]);
    assert_eq!(file.type_libraries[0].flags, 6);
    assert!(!file.window_classes[0].versioned);
}

#[test]
fn xml_limits_encoding_namespaces_and_duplicate_class_fail_without_partial_graph() {
    let bytes = manifest("Ring3.Encoding", "<file name='日本.dll'/>");
    let text = core::str::from_utf8(&bytes).unwrap();
    let utf16 = [0xff, 0xfe]
        .into_iter()
        .chain(text.encode_utf16().flat_map(u16::to_le_bytes))
        .collect::<Vec<_>>();
    assert_eq!(
        Manifest32::parse(&utf16, ActivationConfig32::default())
            .unwrap()
            .files[0]
            .name,
        "日本.dll"
    );
    let dtd = b"<!DOCTYPE assembly [<!ENTITY payload 'x'>]><assembly/>";
    assert!(matches!(
        Manifest32::parse(dtd, ActivationConfig32::default()),
        Err(ActivationError32::ManifestParse)
    ));
    let limits = ActivationConfig32 {
        nodes: 2,
        ..Default::default()
    };
    assert!(matches!(
        Manifest32::parse(&bytes, limits),
        Err(ActivationError32::ManifestParse)
    ));
    let wrong_namespace = bytes
        .iter()
        .copied()
        .map(|byte| if byte == b':' { b'_' } else { byte })
        .collect::<Vec<_>>();
    assert!(matches!(
        Manifest32::parse(&wrong_namespace, ActivationConfig32::default()),
        Err(ActivationError32::ManifestFormat)
    ));
    let duplicate = manifest(
        "Ring3.Duplicate",
        "<file name='a.dll'><comClass clsid='{12345678-9abc-def0-0102-030405060708}'/></file><file name='b.dll'><comClass clsid='{12345678-9abc-def0-0102-030405060708}'/></file>",
    );
    assert!(matches!(
        Manifest32::parse(&duplicate, ActivationConfig32::default()),
        Err(ActivationError32::DuplicateClass)
    ));
}

#[test]
fn dependency_graph_requires_exact_identity_and_applies_declared_version_redirect() {
    let root = manifest(
        "Ring3.Root",
        "<dependency><dependentAssembly><assemblyIdentity name='Ring3.Dependency' version='1.0.0.0' type='win32' processorArchitecture='*'/><bindingRedirect oldVersion='1.0.0.0-1.1.0.0' newVersion='1.2.3.4'/></dependentAssembly></dependency>",
    );
    let root = ActivationAssembly32 {
        manifest: Manifest32::parse(&root, ActivationConfig32::default()).unwrap(),
        manifest_path: "C:\\app\\root.manifest".encode_utf16().collect(),
        directory: "C:\\app".encode_utf16().collect(),
        modified: 0,
        resource_name: None,
    };
    assert!(matches!(
        ActivationGraph32::new(vec![root.clone()], ActivationConfig32::default()),
        Err(ActivationError32::AssemblyNotFound)
    ));
    let dependency = graph("Ring3.Dependency").assemblies()[0].clone();
    let result =
        ActivationGraph32::new(vec![root, dependency], ActivationConfig32::default()).unwrap();
    assert_eq!(result.assemblies().len(), 2);
}
