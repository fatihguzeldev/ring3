use super::*;
use crate::{
    memory::{PAGE_SIZE, Permissions},
    process::windows::objects::InterfaceLayout32,
    windows::com::InterfaceId,
};

const IID: InterfaceId = InterfaceId([7; 16]);
const METHODS: [GuestAddress; 3] = [
    GuestAddress(0x8000),
    GuestAddress(0x8010),
    GuestAddress(0x8020),
];

fn fixture() -> (GuestComObjects32, AddressSpace, GuestAddress) {
    let mut owner = GuestComObjects32::new(
        PageRange::new(GuestAddress(0x1000_0000), 16).unwrap(),
        4,
        4,
        2,
    )
    .unwrap();
    let mut memory = AddressSpace::new(16).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x2000), 1).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    memory.write(GuestAddress(0x2000), &[0xa5; 16]).unwrap();
    let pointer = owner
        .create(
            &mut memory,
            7,
            &[InterfaceLayout32 {
                iid: IID,
                methods: &METHODS,
            }],
        )
        .unwrap()
        .identity_pointer;
    (owner, memory, pointer)
}

#[test]
fn prepared_retains_survive_disjoint_scratch_mapping_and_refuse_counter_changes() {
    let (mut owner, mut memory, pointer) = fixture();
    let prepared = owner.prepare_retain_changes(&memory, &[pointer]).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x4000), 1).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    owner.can_commit_retain_changes(&prepared).unwrap();
    let version = memory.transaction_version();
    owner.commit_retain_changes(prepared);
    assert_eq!(owner.references(pointer), Ok(2));
    assert_eq!(memory.transaction_version(), version);
    let stale = owner.prepare_retain_changes(&memory, &[pointer]).unwrap();
    owner.add_ref(&memory, pointer).unwrap();
    assert!(owner.can_commit_retain_changes(&stale).is_err());
    assert_eq!(owner.references(pointer), Ok(3));
}

#[test]
fn owned_receipt_rejects_counter_changes_before_any_ram_or_identity_publication() {
    let (mut owner, mut memory, pointer) = fixture();
    let prepared = owner
        .prepare_lifetime_changes(
            &memory,
            &[],
            &[pointer],
            &[],
            &[],
            &[WordWrite32 {
                address: GuestAddress(0x2000),
                value: 3,
            }],
        )
        .unwrap();
    let version = memory.transaction_version();
    owner.add_ref(&memory, pointer).unwrap();
    assert_eq!(memory.transaction_version(), version);
    assert_eq!(
        owner.commit_lifetime_changes(&mut memory, prepared),
        Err(GuestComError32::Com(
            crate::windows::com::ComError32::InvalidPointer
        ))
    );
    assert_eq!(owner.references(pointer), Ok(2));
    assert!(owner.preflight(&memory, pointer).is_ok());
    let mut bytes = [0; 4];
    memory.read(GuestAddress(0x2000), &mut bytes).unwrap();
    assert_eq!(bytes, [0xa5; 4]);
}

#[test]
fn owned_receipt_rejects_memory_change_and_reclaimed_identity_without_side_effects() {
    let (mut owner, mut memory, pointer) = fixture();
    let prepared = owner
        .prepare_lifetime_changes(
            &memory,
            &[pointer],
            &[],
            &[],
            &[],
            &[WordWrite32 {
                address: GuestAddress(0x2000),
                value: 3,
            }],
        )
        .unwrap();
    memory.write(GuestAddress(0x2008), &[1]).unwrap();
    assert_eq!(
        owner.commit_lifetime_changes(&mut memory, prepared),
        Err(GuestComError32::StaleMapping)
    );
    assert_eq!(owner.references(pointer), Ok(1));
    let prepared = owner
        .prepare_lifetime_changes(&memory, &[pointer], &[], &[], &[], &[])
        .unwrap();
    owner.release(&mut memory, pointer).unwrap();
    let replacement = owner
        .create(
            &mut memory,
            8,
            &[InterfaceLayout32 {
                iid: IID,
                methods: &METHODS,
            }],
        )
        .unwrap()
        .identity_pointer;
    assert_ne!(replacement, pointer);
    assert_eq!(
        owner.commit_lifetime_changes(&mut memory, prepared),
        Err(GuestComError32::StaleMapping)
    );
    assert_eq!(owner.references(replacement), Ok(1));
}

#[test]
fn extra_staging_unmap_and_com_reclamation_share_one_atomic_memory_version() {
    let (mut owner, mut memory, pointer) = fixture();
    let extra = PageRange::new(GuestAddress(0x4000), 1).unwrap();
    memory.map_zeroed(extra, Permissions::READ_WRITE).unwrap();
    let prepared = owner
        .prepare_lifetime_changes_with_unmaps(
            &memory,
            &[],
            &[pointer],
            &[],
            &[],
            &[WordWrite32 {
                address: GuestAddress(0x2000),
                value: 0x1234,
            }],
            &[extra, extra],
        )
        .unwrap();
    let before = memory.transaction_version();
    assert_eq!(
        owner
            .commit_lifetime_changes(&mut memory, prepared)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(memory.transaction_version(), before + 1);
    assert!(memory.read(pointer, &mut [0]).is_err());
    assert!(memory.read(GuestAddress(0x4000), &mut [0]).is_err());
    let mut word = [0; 4];
    memory.read(GuestAddress(0x2000), &mut word).unwrap();
    assert_eq!(u32::from_le_bytes(word), 0x1234);
    assert_eq!(owner.retained_pages(), 1);
}

#[test]
fn extra_unmap_cannot_destroy_a_live_other_com_object_and_version_exhaustion_is_neutral() {
    let (mut owner, mut memory, pointer) = fixture();
    let other = owner
        .create(
            &mut memory,
            9,
            &[InterfaceLayout32 {
                iid: IID,
                methods: &METHODS,
            }],
        )
        .unwrap()
        .identity_pointer;
    let range = PageRange::new(GuestAddress(other.0 / PAGE_SIZE * PAGE_SIZE), 1).unwrap();
    assert!(matches!(
        owner.prepare_lifetime_changes_with_unmaps(
            &memory,
            &[],
            &[pointer],
            &[],
            &[],
            &[],
            &[range]
        ),
        Err(GuestComError32::Memory(MemoryError::InvalidRange))
    ));
    assert_eq!(owner.references(pointer), Ok(1));
    assert_eq!(owner.references(other), Ok(1));
    let vtable = PageRange::new(owner.vtables[0].address, 1).unwrap();
    assert!(matches!(
        owner.prepare_lifetime_changes_with_unmaps(
            &memory,
            &[],
            &[pointer],
            &[],
            &[],
            &[],
            &[vtable]
        ),
        Err(GuestComError32::Memory(MemoryError::InvalidRange))
    ));
    memory.exhaust_versions_for_test();
    assert!(matches!(
        owner.prepare_lifetime_changes(&memory, &[], &[pointer], &[], &[], &[]),
        Err(GuestComError32::Memory(MemoryError::VersionExhausted))
    ));
    let retain = owner
        .prepare_lifetime_changes(&memory, &[pointer], &[], &[], &[], &[])
        .unwrap();
    owner.commit_lifetime_changes(&mut memory, retain).unwrap();
    assert_eq!(owner.references(pointer), Ok(2));
    assert!(owner.preflight(&memory, pointer).is_ok());
}

#[test]
fn aggregate_unmap_retires_com_metadata_after_one_shared_ram_and_word_transaction() {
    let (mut owner, mut memory, pointer) = fixture();
    let external = PageRange::new(GuestAddress(0x4000), 1).unwrap();
    memory
        .map_zeroed(external, Permissions::READ_WRITE)
        .unwrap();
    let receipt = owner
        .prepare_lifetime_changes(&memory, &[], &[pointer], &[], &[], &[])
        .unwrap();
    owner
        .can_commit_lifetime_changes(&memory, &receipt)
        .unwrap();
    let mut ranges = receipt.ranges().to_vec();
    ranges.push(external);
    ranges.sort_unstable_by_key(|range| range.first);
    let version = memory.transaction_version();
    memory
        .unmap_ranges_with_word_writes(
            &ranges,
            &[WordWrite32 {
                address: GuestAddress(0x2000),
                value: 0x11223344,
            }],
        )
        .unwrap();
    assert_eq!(memory.transaction_version(), version + 1);
    let destroyed = owner.commit_lifetime_after_aggregate_unmap(&memory, receipt, true);
    assert_eq!(destroyed.len(), 1);
    assert!(owner.lookup(pointer).is_err());
    assert_eq!(memory.transaction_version(), version + 1);
    let mut value = [0; 4];
    memory.read(GuestAddress(0x2000), &mut value).unwrap();
    assert_eq!(u32::from_le_bytes(value), 0x11223344);
    assert!(memory.mapping_version_at(GuestAddress(0x4000)).is_err());
}

#[test]
fn aggregate_reference_only_retirement_keeps_the_surviving_object_without_a_ram_transaction() {
    let (mut owner, memory, pointer) = fixture();
    owner.add_ref(&memory, pointer).unwrap();
    let receipt = owner
        .prepare_lifetime_changes(&memory, &[], &[pointer], &[], &[], &[])
        .unwrap();
    assert!(receipt.ranges().is_empty());
    owner
        .can_commit_lifetime_changes(&memory, &receipt)
        .unwrap();
    let version = memory.transaction_version();
    let destroyed = owner.commit_lifetime_after_aggregate_unmap(&memory, receipt, false);
    assert!(destroyed.is_empty());
    assert_eq!(owner.references(pointer), Ok(1));
    owner.preflight(&memory, pointer).unwrap();
    assert_eq!(memory.transaction_version(), version);
}
