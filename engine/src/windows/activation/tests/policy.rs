use super::*;
use crate::windows::activation::{ActivationError32 as E, policy::BindingPolicy32};
fn limits() -> ActivationConfig32 {
    ActivationConfig32::default()
}

#[test]
fn application_policy_redirects_only_matching_identity_and_bounds_private_paths() {
    let xml = br#"<configuration><windows><publisherPolicy apply='no'/><assemblyBinding xmlns='urn:schemas-microsoft-com:asm.v1'><assemblyIdentity name='Ring3.App' type='win32' version='1.0.0.0'/><probing privatePath='bin;..\shared;..\..\common'/><dependency><dependentAssembly><assemblyIdentity name='Ring3.Component' type='win32' processorArchitecture='x86'/><bindingRedirect oldVersion='1.0.0.0-1.0.9.9' newVersion='1.0.10.0'/></dependentAssembly></dependency></assemblyBinding></windows></configuration>"#;
    let policy = BindingPolicy32::application(xml, limits()).unwrap();
    assert!(!policy.publisher);
    assert_eq!(policy.private_paths.len(), 3);
    let manifest = Manifest32::parse(br#"<assembly xmlns='urn:schemas-microsoft-com:asm.v1' manifestVersion='1.0'><assemblyIdentity name='Ring3.Component' type='win32' version='1.0.2.3'/></assembly>"#, limits()).unwrap();
    assert_eq!(
        policy.redirected(&manifest.identity).unwrap(),
        Some([1, 0, 10, 0])
    );
    let mut identity = manifest.identity;
    identity.public_key_token = Some([1; 8]);
    assert_eq!(policy.redirected(&identity).unwrap(), None);
    for paths in [
        r"..\..\..\escape",
        r"C:\host",
        r"...\bad",
        "1;2;3;4;5;6;7;8;9;10",
    ] {
        let xml = String::from_utf8(xml.to_vec())
            .unwrap()
            .replace(r"bin;..\shared;..\..\common", paths);
        assert!(matches!(
            BindingPolicy32::application(xml.as_bytes(), limits()),
            Err(E::ManifestFormat)
        ));
    }
}

#[test]
fn publisher_policy_uses_policy_identity_and_rejects_cross_major_and_external_entities() {
    let xml = br#"<assembly xmlns='urn:schemas-microsoft-com:asm.v1' manifestVersion='1.0'><assemblyIdentity name='policy.1.0.Ring3.Component' type='win32-policy' version='2.0.0.0' publicKeyToken='0102030405060708'/><dependency><dependentAssembly><assemblyIdentity name='Ring3.Component' type='win32' publicKeyToken='0102030405060708'/><bindingRedirect oldVersion='1.0.0.0' newVersion='1.0.7.0'/></dependentAssembly></dependency></assembly>"#;
    let policy = BindingPolicy32::publisher(xml, limits()).unwrap();
    assert_eq!(policy.identity.unwrap().name, "policy.1.0.Ring3.Component");
    assert_eq!(policy.redirects[0].target, [1, 0, 7, 0]);
    let bad = String::from_utf8(xml.to_vec())
        .unwrap()
        .replace("newVersion='1.0.7.0'", "newVersion='2.0.7.0'");
    assert!(matches!(
        BindingPolicy32::publisher(bad.as_bytes(), limits()),
        Err(E::ManifestFormat)
    ));
    assert!(matches!(
        BindingPolicy32::application(
            b"<!DOCTYPE configuration SYSTEM 'file:///host'><configuration/>",
            limits()
        ),
        Err(E::ManifestParse)
    ));
}
