use super::*;
use crate::windows::process::ProcessError32;

#[test]
fn loader_mapping_and_peb_writes_fail_together_before_any_metadata_publication() {
    let (catalog, mut set, mut memory, mut startup) = fixture();
    let range = PageRange::new(GuestAddress(PEB), 1).unwrap();
    memory.protect(range, Permissions::READ).unwrap();
    let version = memory.transaction_version();
    let bump = startup.bump;
    let plan = catalog.plan_load(&set, &memory, "first.dll", None).unwrap();
    let prepared = startup
        .prepare_loader_change(plan.additions(), plan.initialization_order(), &[], &[])
        .unwrap();
    assert!(
        plan.install_with_writes(&mut memory, &mut set, prepared.words())
            .is_err()
    );
    assert_eq!(memory.transaction_version(), version);
    assert_eq!(startup.bump, bump);
    assert_eq!(startup.loader_entries.len(), 1);
    assert_eq!(set.iter().len(), 1);
    assert_eq!(list(&memory, 0x20c).len(), 1);
    memory.protect(range, Permissions::READ_WRITE).unwrap();
    let id = load(&catalog, &mut set, &mut memory, &mut startup, "first.dll");
    memory.protect(range, Permissions::READ).unwrap();
    let plan = set.plan_unload(&memory, &[id]).unwrap();
    let prepared = startup.prepare_loader_change(&[], &[], &[id], &[]).unwrap();
    assert!(
        plan.unmap_with_writes(&mut memory, &mut set, prepared.words())
            .is_err()
    );
    assert!(set.get(id).is_some());
    assert_eq!(list(&memory, 0x20c).len(), 2);
    assert_eq!(startup.loader_entries.len(), 2);
}

#[test]
fn current_directory_replacement_is_atomic_and_reuses_the_shared_arena() {
    let (_, _, mut memory, mut startup) = fixture();
    let original = startup.current_directory;
    let revision = startup.allocation_revision;
    let version = memory.transaction_version();
    memory.restore_version_for_test(u64::MAX);
    assert_eq!(
        startup.replace_current_directory(&mut memory, &[67, 58, 92, 0xd800]),
        Err(ProcessError32::NoAccess)
    );
    assert_eq!(startup.current_directory, original);
    assert_eq!(startup.allocation_revision, revision);
    memory.restore_version_for_test(version);
    startup
        .replace_current_directory(&mut memory, &[67, 58, 92, 0xd800])
        .unwrap();
    assert_eq!(name(&memory, PEB + 0x124), vec![67, 58, 92, 0xd800]);
    let bump = startup.bump;
    for _ in 0..64 {
        startup
            .replace_current_directory(&mut memory, &[67, 58, 92, 65])
            .unwrap();
        assert_eq!(name(&memory, PEB + 0x124), vec![67, 58, 92, 65]);
    }
    assert_eq!(startup.bump, bump);
}
