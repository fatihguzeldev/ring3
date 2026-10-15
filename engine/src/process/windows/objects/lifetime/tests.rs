use super::*;
use crate::{
    memory::{MemoryError, PageRange, Permissions},
    process::windows::objects::InterfaceLayout32,
    windows::com::{ComError32, InterfaceId},
};

const IA: InterfaceId = InterfaceId([1; 16]);
const IB: InterfaceId = InterfaceId([2; 16]);
const METHODS: [GuestAddress; 3] = [
    GuestAddress(0x4000),
    GuestAddress(0x4010),
    GuestAddress(0x4020),
];

fn owner() -> GuestComObjects32 {
    GuestComObjects32::new(
        PageRange::new(GuestAddress(0x1000_0000), 32).unwrap(),
        4,
        16,
        4,
    )
    .unwrap()
}

fn layout(iid: InterfaceId) -> InterfaceLayout32<'static> {
    InterfaceLayout32 {
        iid,
        methods: &METHODS,
    }
}

#[test]
fn internal_holds_preserve_zero_external_identities_until_last_hold_is_removed() {
    let mut memory = AddressSpace::new(6).unwrap();
    let mut objects = owner();
    let identity = objects
        .create(&mut memory, 7, &[layout(IA), layout(IB)])
        .unwrap();
    let alias = objects
        .query_target(&memory, identity.identity_pointer, IB)
        .unwrap();
    let before = memory.transaction_version();
    let changes = objects
        .preview_reference_changes(&memory, &[], &[alias], &[identity.identity_pointer], &[])
        .unwrap();
    assert_eq!(
        changes,
        vec![ReferenceTransition32 {
            identity,
            before: ReferenceState32 {
                references: 1,
                holds: 0
            },
            after: ReferenceState32 {
                references: 0,
                holds: 1
            }
        }]
    );
    assert_eq!(
        objects.state(alias),
        Ok(ReferenceState32 {
            references: 1,
            holds: 0
        })
    );
    assert!(
        objects
            .apply_lifetime_changes(
                &mut memory,
                &[],
                &[alias],
                &[identity.identity_pointer],
                &[]
            )
            .unwrap()
            .is_empty()
    );
    assert_eq!(memory.transaction_version(), before);
    assert_eq!(
        objects.state(alias),
        Ok(ReferenceState32 {
            references: 0,
            holds: 1
        })
    );
    assert_eq!(objects.preflight(&memory, alias), Ok(identity));
    assert_eq!(objects.add_ref(&memory, alias), Ok(1));
    assert_eq!(objects.release(&mut memory, alias).unwrap().remaining, 0);
    assert_eq!(objects.release(&mut memory, alias).unwrap().remaining, 0);
    let destroyed = objects
        .apply_lifetime_changes(&mut memory, &[], &[], &[], &[alias])
        .unwrap();
    assert_eq!(destroyed, vec![identity]);
    assert_eq!(memory.transaction_version(), before + 1);
    assert_eq!(
        objects.lookup(alias),
        Err(GuestComError32::Com(ComError32::InvalidPointer))
    );
}

#[test]
fn complete_device_resource_cascade_releases_both_pages_in_one_memory_version() {
    let mut memory = AddressSpace::new(6).unwrap();
    let mut objects = owner();
    let device = objects.create(&mut memory, 1, &[layout(IA)]).unwrap();
    let resource = objects.create(&mut memory, 2, &[layout(IA)]).unwrap();
    objects.add_ref(&memory, device.identity_pointer).unwrap();
    objects
        .apply_lifetime_changes(&mut memory, &[], &[], &[resource.identity_pointer], &[])
        .unwrap();
    objects
        .apply_reference_changes(
            &mut memory,
            &[],
            &[resource.identity_pointer, device.identity_pointer],
        )
        .unwrap();
    assert_eq!(
        objects.state(device.identity_pointer),
        Ok(ReferenceState32 {
            references: 1,
            holds: 0
        })
    );
    assert_eq!(
        objects.state(resource.identity_pointer),
        Ok(ReferenceState32 {
            references: 0,
            holds: 1
        })
    );
    let before = memory.transaction_version();
    let destroyed = objects
        .apply_lifetime_changes(
            &mut memory,
            &[],
            &[device.identity_pointer],
            &[],
            &[resource.identity_pointer],
        )
        .unwrap();
    assert_eq!(destroyed, vec![device, resource]);
    assert_eq!(memory.transaction_version(), before + 1);
    assert_eq!(memory.mapped_pages(), 1);
    assert_eq!(objects.live_objects(), 0);
}

#[test]
fn reference_underflow_and_exhausted_mapping_versions_preserve_complete_cascade() {
    let mut memory = AddressSpace::new(6).unwrap();
    let mut objects = owner();
    let first = objects
        .create(&mut memory, 1, &[layout(IA), layout(IB)])
        .unwrap();
    let alias = objects
        .query_target(&memory, first.identity_pointer, IB)
        .unwrap();
    let second = objects.create(&mut memory, 2, &[layout(IA)]).unwrap();
    let before = memory.transaction_version();
    assert_eq!(
        objects.apply_reference_changes(&mut memory, &[], &[first.identity_pointer, alias]),
        Err(GuestComError32::Com(ComError32::ReferenceUnderflow))
    );
    assert_eq!(memory.transaction_version(), before);
    assert_eq!(objects.references(first.identity_pointer), Ok(1));
    memory.exhaust_versions_for_test();
    assert_eq!(
        objects.apply_reference_changes(
            &mut memory,
            &[],
            &[first.identity_pointer, second.identity_pointer]
        ),
        Err(GuestComError32::Memory(MemoryError::VersionExhausted))
    );
    for identity in [first, second] {
        assert_eq!(objects.references(identity.identity_pointer), Ok(1));
        assert_eq!(
            objects.preflight(&memory, identity.identity_pointer),
            Ok(identity)
        );
    }
    assert_eq!(memory.mapped_pages(), 4);
    assert_eq!(objects.live_objects(), 2);
}

#[test]
fn any_replaced_mapping_rejects_the_whole_reference_batch_without_touching_other_objects() {
    let mut memory = AddressSpace::new(6).unwrap();
    let mut objects = owner();
    let first = objects.create(&mut memory, 1, &[layout(IA)]).unwrap();
    let second = objects.create(&mut memory, 2, &[layout(IA)]).unwrap();
    let range = PageRange::new(second.identity_pointer, 1).unwrap();
    memory.unmap(range).unwrap();
    memory.map_zeroed(range, Permissions::READ_WRITE).unwrap();
    let before = memory.transaction_version();
    assert_eq!(
        objects.apply_reference_changes(
            &mut memory,
            &[],
            &[first.identity_pointer, second.identity_pointer]
        ),
        Err(GuestComError32::StaleMapping)
    );
    assert_eq!(memory.transaction_version(), before);
    assert_eq!(
        objects.preflight(&memory, first.identity_pointer),
        Ok(first)
    );
    assert_eq!(objects.references(first.identity_pointer), Ok(1));
    assert_eq!(objects.references(second.identity_pointer), Ok(1));
    assert_eq!(memory.mapped_pages(), 3);
}

#[test]
fn output_words_and_reference_edges_publish_together_or_remain_neutral() {
    let mut memory = AddressSpace::new(6).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x8000), 1).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    let mut objects = owner();
    let identity = objects.create(&mut memory, 4, &[layout(IA)]).unwrap();
    let before = memory.transaction_version();
    let bad = [crate::memory::WordWrite32 {
        address: GuestAddress(0x9000),
        value: identity.identity_pointer.0,
    }];
    assert!(
        objects
            .apply_lifetime_changes_with_writes(
                &mut memory,
                &[identity.identity_pointer],
                &[],
                &[],
                &[],
                &bad
            )
            .is_err()
    );
    assert_eq!(objects.references(identity.identity_pointer), Ok(1));
    assert_eq!(memory.transaction_version(), before);
    let good = [crate::memory::WordWrite32 {
        address: GuestAddress(0x8000),
        value: identity.identity_pointer.0,
    }];
    assert!(
        objects
            .apply_lifetime_changes_with_writes(
                &mut memory,
                &[identity.identity_pointer],
                &[],
                &[],
                &[],
                &good
            )
            .unwrap()
            .is_empty()
    );
    assert_eq!(objects.references(identity.identity_pointer), Ok(2));
    assert_eq!(memory.transaction_version(), before + 1);
    let mut output = [0; 4];
    memory.read(GuestAddress(0x8000), &mut output).unwrap();
    assert_eq!(u32::from_le_bytes(output), identity.identity_pointer.0);
    let dispose = [crate::memory::WordWrite32 {
        address: GuestAddress(0x8000),
        value: 0,
    }];
    let destroyed = objects
        .apply_lifetime_changes_with_writes(
            &mut memory,
            &[],
            &[identity.identity_pointer, identity.identity_pointer],
            &[],
            &[],
            &dispose,
        )
        .unwrap();
    assert_eq!(destroyed, vec![identity]);
    assert_eq!(memory.transaction_version(), before + 2);
    memory.read(GuestAddress(0x8000), &mut output).unwrap();
    assert_eq!(output, [0; 4]);
}

#[test]
fn initial_holds_and_parent_references_publish_atomically_with_new_identity() {
    let mut memory = AddressSpace::new(6).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x8000), 1).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    let mut objects = owner();
    let parent = objects.create(&mut memory, 1, &[layout(IA)]).unwrap();
    let initial = ReferenceState32 {
        references: 1,
        holds: 1,
    };
    let before = memory.transaction_version();
    let publication = super::super::GuestComCreation32 {
        output: Some((GuestAddress(0x9000), IA)),
        initial,
        retain: &[parent.identity_pointer],
        writes: &[],
    };
    assert!(
        objects
            .create_with_lifetime(&mut memory, 2, &[layout(IA)], publication)
            .is_err()
    );
    assert_eq!(objects.references(parent.identity_pointer), Ok(1));
    assert_eq!(objects.live_objects(), 1);
    assert_eq!(memory.transaction_version(), before);
    let publication = super::super::GuestComCreation32 {
        output: Some((GuestAddress(0x8000), IA)),
        initial,
        retain: &[parent.identity_pointer],
        writes: &[],
    };
    let child = objects
        .create_with_lifetime(&mut memory, 2, &[layout(IA)], publication)
        .unwrap();
    assert_eq!(objects.references(parent.identity_pointer), Ok(2));
    assert_eq!(objects.state(child.identity_pointer), Ok(initial));
    assert_eq!(memory.transaction_version(), before + 1);
    assert_eq!(
        objects
            .release(&mut memory, child.identity_pointer)
            .unwrap()
            .remaining,
        0
    );
    assert_eq!(
        objects.state(child.identity_pointer),
        Ok(ReferenceState32 {
            references: 0,
            holds: 1
        })
    );
}
