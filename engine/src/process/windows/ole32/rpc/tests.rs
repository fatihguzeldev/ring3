use super::*;
use crate::{
    memory::{AddressSpace, GuestAddress, MemoryError, PageRange, Permissions},
    process::{
        HostError,
        windows::ole32::{Ole32Config32, Ole32Runtime32},
    },
    windows::ole32::{
        rpc::RpcMessage32,
        stream::{StreamLimits32, StreamStore32},
    },
};
fn fixture() -> (Ole32Runtime32, AddressSpace) {
    let mut runtime = Ole32Runtime32::new(Ole32Config32 {
        objects: PageRange::new(GuestAddress(0x30000000), 16).unwrap(),
        max_objects: 8,
        max_threads: 8,
    })
    .unwrap();
    runtime.streams.store = StreamStore32::new(StreamLimits32 {
        buffers: 8,
        streams: 4,
        buffer_bytes: 8192,
        total_bytes: 16384,
    })
    .unwrap();
    let mut memory = AddressSpace::new(16).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x10000), 1).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    (runtime, memory)
}
fn message(memory: &mut AddressSpace, address: u32, length: u32) -> RpcMessage32 {
    let message = RpcMessage32 {
        reserved: 0x12345678,
        representation: 0x10,
        buffer: 0xdeadbeef,
        length,
        method: 7,
        reserved2: [1, 2, 3, 4, 5],
        flags: 0xa5000000,
    };
    memory
        .write(GuestAddress(address), &message.encode())
        .unwrap();
    message
}
#[test]
fn rpc_buffers_are_zeroed_and_preserve_every_opaque_message_field() {
    let (mut runtime, mut memory) = fixture();
    let original = message(&mut memory, 0x10000, 5000);
    runtime
        .allocate_rpc_buffer(&mut memory, 0, 0x10000)
        .unwrap();
    let actual = buffers::read_message(&memory, 0x10000).unwrap();
    assert_eq!(
        actual,
        RpcMessage32 {
            buffer: 0x67000000,
            ..original
        }
    );
    assert_eq!(runtime.streams.store.reserved_bytes(), 8192);
    let mut bytes = [1u8; 8192];
    memory
        .read(GuestAddress(actual.buffer), &mut bytes)
        .unwrap();
    assert_eq!(bytes, [0u8; 8192]);
    runtime.free_rpc_buffer(&mut memory, 0, 0x10000).unwrap();
    assert_eq!(runtime.streams.store.reserved_bytes(), 0);
    assert_eq!(
        buffers::read_message(&memory, 0x10000).unwrap(),
        RpcMessage32 {
            buffer: 0,
            length: 0,
            ..original
        }
    );
    assert!(runtime.free_rpc_buffer(&mut memory, 0, 0x10000).is_err());
}
#[test]
fn stream_and_rpc_pages_share_one_budget_and_failed_allocation_is_neutral() {
    let (mut runtime, mut memory) = fixture();
    let stream = runtime
        .streams
        .store
        .prepare_stream(8192, b"packet")
        .unwrap();
    runtime.streams.store.commit_stream(stream);
    message(&mut memory, 0x10000, 5000);
    runtime
        .allocate_rpc_buffer(&mut memory, 0, 0x10000)
        .unwrap();
    let expected = message(&mut memory, 0x10040, 1);
    let before = memory.transaction_version();
    let pages = memory.mapped_pages();
    assert!(
        runtime
            .allocate_rpc_buffer(&mut memory, 0, 0x10040)
            .is_err()
    );
    assert_eq!(memory.transaction_version(), before);
    assert_eq!(memory.mapped_pages(), pages);
    assert_eq!(buffers::read_message(&memory, 0x10040).unwrap(), expected);
    assert_eq!(runtime.streams.store.reserved_bytes(), 16384);
    runtime.free_rpc_buffer(&mut memory, 0, 0x10000).unwrap();
    assert_eq!(runtime.streams.store.reserved_bytes(), 8192);
    runtime
        .allocate_rpc_buffer(&mut memory, 0, 0x10040)
        .unwrap();
}
#[test]
fn invalid_output_or_wrong_channel_never_publishes_or_consumes_a_buffer() {
    let (mut runtime, mut memory) = fixture();
    message(&mut memory, 0x10000, 1);
    memory
        .protect(
            PageRange::new(GuestAddress(0x10000), 1).unwrap(),
            Permissions::READ,
        )
        .unwrap();
    let before = memory.transaction_version();
    assert!(
        runtime
            .allocate_rpc_buffer(&mut memory, 0, 0x10000)
            .is_err()
    );
    assert_eq!(memory.transaction_version(), before);
    assert_eq!(runtime.streams.store.reserved_bytes(), 0);
    memory
        .protect(
            PageRange::new(GuestAddress(0x10000), 1).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    runtime
        .allocate_rpc_buffer(&mut memory, 0, 0x10000)
        .unwrap();
    let before = memory.transaction_version();
    assert_eq!(
        runtime.free_rpc_buffer(&mut memory, 1, 0x10000),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(memory.transaction_version(), before);
    assert_eq!(runtime.streams.store.reserved_bytes(), 4096);
}
#[test]
fn stale_rpc_mapping_cannot_retire_a_replacement_or_release_its_budget() {
    let (mut runtime, mut memory) = fixture();
    message(&mut memory, 0x10000, 5000);
    runtime
        .allocate_rpc_buffer(&mut memory, 0, 0x10000)
        .unwrap();
    let range = PageRange::new(GuestAddress(0x67001000), 1).unwrap();
    memory.unmap_ranges(&[range]).unwrap();
    memory.map_zeroed(range, Permissions::READ_WRITE).unwrap();
    memory
        .write(GuestAddress(0x67001000), b"replacement")
        .unwrap();
    let before = memory.transaction_version();
    assert!(matches!(
        runtime.free_rpc_buffer(&mut memory, 0, 0x10000),
        Err(HostError::Memory(MemoryError::InvalidRange))
    ));
    assert_eq!(memory.transaction_version(), before);
    assert_eq!(runtime.streams.store.reserved_bytes(), 8192);
    let mut bytes = [0; 11];
    memory.read(GuestAddress(0x67001000), &mut bytes).unwrap();
    assert_eq!(&bytes, b"replacement");
}
#[test]
fn oversized_rpc_request_keeps_message_and_physical_capacity_unchanged() {
    let (mut runtime, mut memory) = fixture();
    let expected = message(&mut memory, 0x10000, u32::MAX);
    let before = memory.transaction_version();
    assert_eq!(
        runtime.allocate_rpc_buffer(&mut memory, 0, 0x10000),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(memory.transaction_version(), before);
    assert_eq!(memory.mapped_pages(), 1);
    assert_eq!(buffers::read_message(&memory, 0x10000).unwrap(), expected);
}

mod captured;
mod lifetime;
