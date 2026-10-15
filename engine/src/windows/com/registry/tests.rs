use super::*;
use crate::windows::com::IID_IUNKNOWN;

const IA: InterfaceId = InterfaceId([1; 16]);
const IB: InterfaceId = InterfaceId([2; 16]);
const IC: InterfaceId = InterfaceId([3; 16]);

fn binding(iid: InterfaceId, address: u32) -> InterfaceBinding32 {
    InterfaceBinding32 {
        iid,
        address: GuestAddress(address),
        vtable: GuestAddress(0x8000),
    }
}

#[test]
fn all_interfaces_share_identity_and_successful_queries_hold_references() {
    let mut registry = ComRegistry32::new(2, 8).unwrap();
    let id = registry
        .register(
            0x1234_5678_9abc_def0,
            &[binding(IA, 0x1000), binding(IB, 0x1004)],
        )
        .unwrap();
    let identity = ComIdentity32 {
        id,
        identity_pointer: GuestAddress(0x1000),
        payload: 0x1234_5678_9abc_def0,
    };
    assert_eq!(registry.lookup(GuestAddress(0x1004)), Ok(identity));
    for pointer in [0x1000, 0x1004] {
        let references = registry.references(GuestAddress(pointer)).unwrap();
        assert_eq!(
            registry.query_target(GuestAddress(pointer), IID_IUNKNOWN),
            Ok(GuestAddress(0x1000))
        );
        assert_eq!(registry.references(GuestAddress(pointer)), Ok(references));
        assert_eq!(
            registry.query_interface(GuestAddress(pointer), IA),
            Ok(GuestAddress(0x1000))
        );
        assert_eq!(
            registry.query_interface(GuestAddress(pointer), IB),
            Ok(GuestAddress(0x1004))
        );
    }
    assert_eq!(registry.references(GuestAddress(0x1000)), Ok(5));
    for remaining in (1..5).rev() {
        assert_eq!(
            registry.release(GuestAddress(0x1004)).unwrap(),
            ReleaseOutcome32 {
                remaining,
                destroyed: None
            }
        );
    }
    assert_eq!(
        registry.release(GuestAddress(0x1000)).unwrap(),
        ReleaseOutcome32 {
            remaining: 0,
            destroyed: Some(identity)
        }
    );
    for pointer in [0x1000, 0x1004] {
        assert_eq!(
            registry.lookup(GuestAddress(pointer)),
            Err(ComError32::InvalidPointer)
        );
        assert_eq!(
            registry.add_ref(GuestAddress(pointer)),
            Err(ComError32::InvalidPointer)
        );
        assert_eq!(
            registry.release(GuestAddress(pointer)),
            Err(ComError32::InvalidPointer)
        );
    }
}

#[test]
fn inherited_interface_aliases_may_share_storage_but_unknown_is_canonical() {
    let mut registry = ComRegistry32::new(1, 4).unwrap();
    registry
        .register(
            7,
            &[
                binding(IA, 0x1004),
                binding(IB, 0x1004),
                binding(IID_IUNKNOWN, 0x1000),
            ],
        )
        .unwrap();
    assert_eq!(
        registry.query_target(GuestAddress(0x1004), IID_IUNKNOWN),
        Ok(GuestAddress(0x1000))
    );
    for iid in [IA, IB] {
        assert_eq!(
            registry.query_target(GuestAddress(0x1000), iid),
            Ok(GuestAddress(0x1004))
        );
    }
    assert_eq!(
        registry.query_interface(GuestAddress(0x1004), IC),
        Err(ComError32::NoInterface)
    );
    assert_eq!(registry.references(GuestAddress(0x1004)), Ok(1));
}

#[test]
fn malformed_registration_and_exhaustion_publish_nothing() {
    let mut registry = ComRegistry32::new(2, 4).unwrap();
    for bindings in [
        vec![],
        vec![binding(IA, 0)],
        vec![binding(IA, 0x1001)],
        vec![binding(IA, 0x1000), binding(IA, 0x1004)],
        vec![
            binding(IA, 0x1000),
            InterfaceBinding32 {
                vtable: GuestAddress(0x8004),
                ..binding(IB, 0x1000)
            },
        ],
        vec![InterfaceBinding32 {
            vtable: GuestAddress(0),
            ..binding(IA, 0x1000)
        }],
    ] {
        assert_eq!(
            registry.register(9, &bindings),
            Err(ComError32::InvalidBinding)
        );
        assert_eq!(registry.live_objects(), 0);
    }
    let id = registry.register(1, &[binding(IA, 0x1000)]).unwrap();
    assert_eq!(id.get(), 1);
    registry
        .objects
        .iter_mut()
        .flatten()
        .next()
        .unwrap()
        .references = u32::MAX;
    assert_eq!(
        registry.add_ref(GuestAddress(0x1000)),
        Err(ComError32::Exhausted)
    );
    assert_eq!(
        registry.query_interface(GuestAddress(0x1000), IA),
        Err(ComError32::Exhausted)
    );
    assert_eq!(registry.references(GuestAddress(0x1000)), Ok(u32::MAX));
    registry.next_id = u64::MAX;
    assert_eq!(
        registry.register(2, &[binding(IA, 0x1004)]),
        Err(ComError32::Exhausted)
    );
    assert_eq!(registry.live_objects(), 1);
    assert_eq!(
        registry.lookup(GuestAddress(0x1004)),
        Err(ComError32::InvalidPointer)
    );
}

#[test]
fn released_slots_are_reusable_without_reviving_old_guest_addresses_or_ids() {
    let mut registry = ComRegistry32::new(1, 2).unwrap();
    let first = registry.register(3, &[binding(IA, 0x1000)]).unwrap();
    assert_eq!(
        registry.register(4, &[binding(IA, 0x1004)]),
        Err(ComError32::Capacity)
    );
    registry.release(GuestAddress(0x1000)).unwrap();
    assert_eq!(
        registry.register(4, &[binding(IA, 0x1000)]),
        Err(ComError32::PointerReuse)
    );
    let second = registry.register(4, &[binding(IA, 0x1004)]).unwrap();
    assert_ne!(first, second);
    assert_eq!(
        registry.lookup(GuestAddress(0x1000)),
        Err(ComError32::InvalidPointer)
    );
    assert_eq!(registry.lookup(GuestAddress(0x1004)).unwrap().payload, 4);
}

#[test]
fn aggregate_alias_deltas_check_final_counts_without_intermediate_overflow() {
    let mut registry = ComRegistry32::new(1, 2).unwrap();
    let id = registry
        .register(4, &[binding(IA, 0x1000), binding(IB, 0x1004)])
        .unwrap();
    registry
        .objects
        .iter_mut()
        .flatten()
        .next()
        .unwrap()
        .references = u32::MAX;
    let prepared = registry
        .prepare_reference_changes(&[GuestAddress(0x1000)], &[GuestAddress(0x1004)], &[], &[])
        .unwrap();
    assert!(prepared.publish().is_empty());
    assert_eq!(registry.references(GuestAddress(0x1000)), Ok(u32::MAX));
    assert_eq!(registry.lookup(GuestAddress(0x1004)).unwrap().id, id);
    assert!(matches!(
        registry.prepare_reference_changes(&[GuestAddress(0x1000)], &[], &[], &[]),
        Err(ComError32::Exhausted)
    ));
    assert_eq!(registry.references(GuestAddress(0x1004)), Ok(u32::MAX));
}
