use super::*;
use crate::{
    memory::{Access, AddressSpace, FaultReason, MemoryFault, PAGE_SIZE, Permissions},
    windows::com::{IID_IUNKNOWN, ReleaseOutcome32},
};

const IA: InterfaceId = InterfaceId([1; 16]);
const IB: InterfaceId = InterfaceId([2; 16]);
const METHODS: [GuestAddress; 3] = [
    GuestAddress(0x4000),
    GuestAddress(0x4010),
    GuestAddress(0x4020),
];
const FLOOR: u32 = 0x1000_0000;

fn owner(objects: usize, tables: usize) -> GuestComObjects32 {
    GuestComObjects32::new(
        PageRange::new(GuestAddress(FLOOR), 32).unwrap(),
        objects,
        objects * 4,
        tables,
    )
    .unwrap()
}

fn layout(iid: InterfaceId) -> InterfaceLayout32<'static> {
    InterfaceLayout32 {
        iid,
        methods: &METHODS,
    }
}

fn read_word(memory: &AddressSpace, address: u32) -> u32 {
    let mut bytes = [0; 4];
    memory.read(GuestAddress(address), &mut bytes).unwrap();
    u32::from_le_bytes(bytes)
}

#[test]
fn writable_objects_share_readonly_vtables_and_reclaim_object_backing() {
    let mut memory = AddressSpace::new(6).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(FLOOR), 1).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    memory.write(GuestAddress(FLOOR), &[0xa5; 32]).unwrap();
    let mut objects = owner(3, 2);
    let first = objects
        .create(&mut memory, 0x1234_5678_9abc_def0, &[layout(IA)])
        .unwrap();
    assert_eq!(first.identity_pointer.0, FLOOR + 2 * PAGE_SIZE);
    assert_eq!(
        read_word(&memory, first.identity_pointer.0),
        FLOOR + PAGE_SIZE
    );
    assert_eq!(read_word(&memory, FLOOR + PAGE_SIZE), 0x4000);
    assert_eq!(read_word(&memory, FLOOR + PAGE_SIZE + 4), 0x4010);
    assert_eq!(read_word(&memory, FLOOR + PAGE_SIZE + 8), 0x4020);
    assert_eq!(read_word(&memory, FLOOR), 0xa5a5_a5a5);
    assert_eq!(objects.retained_pages(), 2);
    assert_eq!(
        memory.permissions_at(first.identity_pointer),
        Ok(Permissions::READ_WRITE)
    );
    assert_eq!(
        memory.permissions_at(GuestAddress(FLOOR + PAGE_SIZE)),
        Ok(Permissions::READ)
    );
    memory.write(first.identity_pointer, &[0; 4]).unwrap();
    assert_eq!(
        objects.preflight(&memory, first.identity_pointer),
        Ok(first)
    );
    let second = objects.create(&mut memory, 9, &[layout(IA)]).unwrap();
    assert_eq!(
        read_word(&memory, second.identity_pointer.0),
        FLOOR + PAGE_SIZE
    );
    assert_eq!(objects.retained_pages(), 3);
    assert_eq!(objects.add_ref(&memory, first.identity_pointer), Ok(2));
    assert_eq!(
        objects
            .release(&mut memory, first.identity_pointer)
            .unwrap(),
        ReleaseOutcome32 {
            remaining: 1,
            destroyed: None
        }
    );
    assert_eq!(
        objects
            .release(&mut memory, first.identity_pointer)
            .unwrap(),
        ReleaseOutcome32 {
            remaining: 0,
            destroyed: Some(first)
        }
    );
    assert_eq!(objects.retained_pages(), 2);
    assert_eq!(memory.mapped_pages(), 3);
    assert!(memory.read(first.identity_pointer, &mut [0; 4]).is_err());
    assert_eq!(
        objects.preflight(&memory, second.identity_pointer),
        Ok(second)
    );
    let third = objects.create(&mut memory, 10, &[layout(IA)]).unwrap();
    assert!(third.identity_pointer.0 > second.identity_pointer.0);
    assert_eq!(
        objects.lookup(first.identity_pointer),
        Err(GuestComError32::Com(ComError32::InvalidPointer))
    );
}

#[test]
fn registry_capacity_and_failed_mapping_publish_neither_pages_nor_identities() {
    let mut memory = AddressSpace::new(4).unwrap();
    let mut objects = owner(1, 1);
    let first = objects.create(&mut memory, 1, &[layout(IA)]).unwrap();
    let version = memory.transaction_version();
    let high_water = objects.high_water_address();
    assert_eq!(
        objects.create(&mut memory, 2, &[layout(IA)]),
        Err(GuestComError32::Com(ComError32::Capacity))
    );
    assert_eq!(memory.transaction_version(), version);
    assert_eq!(memory.mapped_pages(), 2);
    assert_eq!(objects.high_water_address(), high_water);
    assert_eq!(objects.live_objects(), 1);
    objects
        .release(&mut memory, first.identity_pointer)
        .unwrap();
    let second = objects.create(&mut memory, 2, &[layout(IA)]).unwrap();
    assert_eq!(second.id.get(), first.id.get() + 1);
    let mut empty_memory = AddressSpace::new(1).unwrap();
    let mut fresh = owner(1, 1);
    assert_eq!(
        fresh.create(&mut empty_memory, 1, &[layout(IA)]),
        Err(GuestComError32::Memory(MemoryError::Capacity))
    );
    assert_eq!(empty_memory.mapped_pages(), 0);
    assert_eq!(empty_memory.transaction_version(), 0);
    assert_eq!(fresh.live_objects(), 0);
    let mut exhausted = AddressSpace::new(3).unwrap();
    exhausted.exhaust_versions_for_test();
    assert_eq!(
        fresh.create(&mut exhausted, 1, &[layout(IA)]),
        Err(GuestComError32::Memory(MemoryError::VersionExhausted))
    );
    assert_eq!(exhausted.mapped_pages(), 0);
    assert_eq!(fresh.retained_pages(), 0);
}

#[test]
fn replacement_mappings_and_foreign_address_spaces_cannot_be_released() {
    let mut memory = AddressSpace::new(4).unwrap();
    let mut objects = owner(1, 1);
    let identity = objects.create(&mut memory, 11, &[layout(IA)]).unwrap();
    let range = PageRange::new(identity.identity_pointer, 1).unwrap();
    memory.unmap(range).unwrap();
    memory.map_zeroed(range, Permissions::READ).unwrap();
    let version = memory.transaction_version();
    assert_eq!(
        objects.release(&mut memory, identity.identity_pointer),
        Err(GuestComError32::StaleMapping)
    );
    assert_eq!(memory.transaction_version(), version);
    assert_eq!(memory.mapped_pages(), 2);
    assert_eq!(read_word(&memory, identity.identity_pointer.0), 0);
    assert_eq!(objects.references(identity.identity_pointer), Ok(1));
    let mut other = AddressSpace::new(4).unwrap();
    other.map_zeroed(range, Permissions::READ).unwrap();
    assert_eq!(
        objects.release(&mut other, identity.identity_pointer),
        Err(GuestComError32::StaleMapping)
    );
    assert_eq!(other.mapped_pages(), 1);
}

#[test]
fn object_and_interface_output_publish_in_one_transaction_or_remain_neutral() {
    let mut memory = AddressSpace::new(5).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x8000), 1).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    memory.write(GuestAddress(0x8ffc), &[0xa5; 4]).unwrap();
    let mut objects = owner(2, 2);
    let before = memory.transaction_version();
    assert_eq!(
        objects.create_with_output(&mut memory, 5, &[layout(IA)], GuestAddress(0x8ffe), IA),
        Err(GuestComError32::Memory(MemoryError::Fault(MemoryFault {
            address: GuestAddress(0x9000),
            access: Access::Write,
            reason: FaultReason::Unmapped
        })))
    );
    assert_eq!(read_word(&memory, 0x8ffc), 0xa5a5_a5a5);
    assert_eq!(memory.transaction_version(), before);
    assert_eq!(memory.mapped_pages(), 1);
    assert_eq!(objects.live_objects(), 0);
    let identity = objects
        .create_with_output(
            &mut memory,
            5,
            &[layout(IA), layout(IB)],
            GuestAddress(0x8ffc),
            IB,
        )
        .unwrap();
    assert_eq!(memory.transaction_version(), before + 1);
    let selected = objects
        .query_target(&memory, identity.identity_pointer, IB)
        .unwrap();
    assert_eq!(read_word(&memory, 0x8ffc), selected.0);
    assert_eq!(
        objects.canonical_pointer(selected),
        Ok(identity.identity_pointer)
    );
    assert_eq!(
        objects.query_target(&memory, selected, IID_IUNKNOWN),
        Ok(identity.identity_pointer)
    );
}

#[test]
fn interface_queries_publish_null_or_pointer_without_leaking_failed_references() {
    let mut memory = AddressSpace::new(4).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x8000), 1).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    memory.write(GuestAddress(0x8ffc), &[0xa5; 4]).unwrap();
    let mut objects = owner(1, 1);
    let identity = objects.create(&mut memory, 7, &[layout(IA)]).unwrap();
    let version = memory.transaction_version();
    assert!(
        objects
            .query_interface(
                &mut memory,
                identity.identity_pointer,
                IA,
                GuestAddress(0x8ffe)
            )
            .is_err()
    );
    assert_eq!(read_word(&memory, 0x8ffc), 0xa5a5_a5a5);
    assert_eq!(objects.references(identity.identity_pointer), Ok(1));
    assert_eq!(memory.transaction_version(), version);
    assert_eq!(
        objects.query_interface(
            &mut memory,
            identity.identity_pointer,
            IA,
            GuestAddress(0x8ffc)
        ),
        Ok(Some(identity.identity_pointer))
    );
    assert_eq!(read_word(&memory, 0x8ffc), identity.identity_pointer.0);
    assert_eq!(objects.references(identity.identity_pointer), Ok(2));
    assert_eq!(
        objects.query_interface(
            &mut memory,
            identity.identity_pointer,
            IB,
            GuestAddress(0x8ffc)
        ),
        Ok(None)
    );
    assert_eq!(read_word(&memory, 0x8ffc), 0);
    assert_eq!(objects.references(identity.identity_pointer), Ok(2));
    memory.exhaust_versions_for_test();
    assert_eq!(
        objects.query_interface(
            &mut memory,
            identity.identity_pointer,
            IA,
            GuestAddress(0x8ffc)
        ),
        Err(GuestComError32::Memory(MemoryError::VersionExhausted))
    );
    assert_eq!(objects.references(identity.identity_pointer), Ok(2));
    assert_eq!(read_word(&memory, 0x8ffc), 0);
}

#[test]
fn interface_creation_and_normalized_words_commit_together() {
    let mut memory = AddressSpace::new(4).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x8000), 1).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    memory.write(GuestAddress(0x8080), &[0xa5; 12]).unwrap();
    let mut objects = owner(1, 1);
    let before = memory.transaction_version();
    let bad = [crate::memory::WordWrite32 {
        address: GuestAddress(0x9000),
        value: 640,
    }];
    assert!(
        objects
            .create_with_writes(
                &mut memory,
                1,
                &[layout(IA)],
                GuestAddress(0x8088),
                IA,
                &bad
            )
            .is_err()
    );
    assert_eq!(memory.mapped_pages(), 1);
    assert_eq!(memory.transaction_version(), before);
    assert_eq!(read_word(&memory, 0x8088), 0xa5a5_a5a5);
    assert_eq!(objects.live_objects(), 0);
    let normalized = [
        crate::memory::WordWrite32 {
            address: GuestAddress(0x8084),
            value: 480,
        },
        crate::memory::WordWrite32 {
            address: GuestAddress(0x8080),
            value: 640,
        },
    ];
    let identity = objects
        .create_with_writes(
            &mut memory,
            1,
            &[layout(IA)],
            GuestAddress(0x8088),
            IA,
            &normalized,
        )
        .unwrap();
    assert_eq!(memory.transaction_version(), before + 1);
    assert_eq!(
        [
            read_word(&memory, 0x8080),
            read_word(&memory, 0x8084),
            read_word(&memory, 0x8088)
        ],
        [640, 480, identity.identity_pointer.0]
    );
}
