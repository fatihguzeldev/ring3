use super::*;
use crate::windows::com::{InterfaceBinding32, InterfaceId};

fn binding(address: u32) -> InterfaceBinding32 {
    InterfaceBinding32 {
        iid: InterfaceId([9; 16]),
        address: GuestAddress(address),
        vtable: GuestAddress(0x8000),
    }
}

#[test]
fn warm_two_object_hold_changes_use_inline_storage_and_preserve_counter_guards() {
    let mut registry = ComRegistry32::new(2, 2).unwrap();
    registry.register(1, &[binding(0x1000)]).unwrap();
    registry.register(2, &[binding(0x2000)]).unwrap();
    let initial = registry
        .prepare_owned_reference_changes(&[], &[], &[GuestAddress(0x2000)], &[])
        .unwrap();
    registry.commit_owned_reference_changes(initial);
    let prepared = registry
        .prepare_owned_reference_changes(&[], &[], &[GuestAddress(0x1000)], &[GuestAddress(0x2000)])
        .unwrap();
    assert!(matches!(
        prepared.plan,
        OwnedReferencePlan32::Inline { count: 2, .. }
    ));
    assert!(prepared.destroyed().is_empty());
    registry.add_ref(GuestAddress(0x1000)).unwrap();
    assert_eq!(
        registry.can_commit_owned_reference_changes(&prepared),
        Err(ComError32::InvalidPointer)
    );
    let prepared = registry
        .prepare_owned_reference_changes(&[], &[], &[GuestAddress(0x1000)], &[GuestAddress(0x2000)])
        .unwrap();
    registry
        .can_commit_owned_reference_changes(&prepared)
        .unwrap();
    registry.commit_owned_reference_changes(prepared);
    assert_eq!(registry.holds(GuestAddress(0x1000)), Ok(1));
    assert_eq!(registry.holds(GuestAddress(0x2000)), Ok(0));
}

#[test]
fn owned_reference_identity_cannot_be_replaced_and_alias_deltas_are_checked_after_merging() {
    let mut registry = ComRegistry32::new(1, 2).unwrap();
    registry.register(1, &[binding(0x1000)]).unwrap();
    let old = registry
        .prepare_owned_reference_changes(&[GuestAddress(0x1000)], &[], &[], &[])
        .unwrap();
    registry.release(GuestAddress(0x1000)).unwrap();
    registry.register(2, &[binding(0x2000)]).unwrap();
    assert_eq!(
        registry.can_commit_owned_reference_changes(&old),
        Err(ComError32::InvalidPointer)
    );
    registry.objects[0].as_mut().unwrap().references = u32::MAX;
    let merged = registry
        .prepare_owned_reference_changes(&[GuestAddress(0x2000)], &[GuestAddress(0x2000)], &[], &[])
        .unwrap();
    assert!(matches!(
        merged.plan,
        OwnedReferencePlan32::Inline { count: 1, .. }
    ));
    assert!(matches!(
        registry.prepare_owned_reference_changes(&[GuestAddress(0x2000)], &[], &[], &[]),
        Err(ComError32::Exhausted)
    ));
    registry.commit_owned_reference_changes(merged);
    assert_eq!(registry.references(GuestAddress(0x2000)), Ok(u32::MAX));
}
