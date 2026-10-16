use super::*;
#[test]
fn system_group_failure_keeps_the_existing_catalog_and_does_not_admit_a_prefix() {
    let mut catalog=ModuleCatalog32::with_limits(ModuleLimits32 {modules:2,..Default::default()}).unwrap();
    let module=|name:&str|SystemModule32 {name:name.into(),preferred_base:0x70000000,exports:Vec::new()};
    catalog.add_system(module("existing.dll")).unwrap();
    assert_eq!(catalog.add_system_group(vec![module("kernel32.dll"),module("ntdll.dll")]),Err(ModuleError32::Capacity));
    assert!(catalog.contains("existing.dll"));
    assert!(!catalog.contains("kernel32.dll"));
    assert!(!catalog.contains("ntdll.dll"));
}
