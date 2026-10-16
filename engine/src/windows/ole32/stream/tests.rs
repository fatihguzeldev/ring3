use super::*;
fn store() -> StreamStore32 {
    StreamStore32::new(StreamLimits32 {
        buffers: 8,
        streams: 4,
        buffer_bytes: 32,
        total_bytes: 64,
    })
    .unwrap()
}
fn create(store: &mut StreamStore32, maximum: usize, bytes: &[u8]) -> StreamId32 {
    let prepared = store.prepare_stream(maximum, bytes).unwrap();
    store.commit_stream(prepared)
}
fn seek(store: &mut StreamStore32, id: StreamId32, position: u64) {
    let prepared = store.prepare_seek(id, position, 0).unwrap();
    store.commit_seek(prepared);
}
fn read(store: &mut StreamStore32, id: StreamId32, count: usize) -> Vec<u8> {
    let prepared = store.prepare_read(id, count).unwrap();
    let bytes = store.read_bytes(&prepared).unwrap().to_vec();
    store.commit_read(prepared);
    bytes
}
#[test]
fn cloned_streams_share_bytes_but_keep_independent_64_bit_cursors_and_buffer_lifetime() {
    let mut store = store();
    let first = create(&mut store, 32, b"abcd");
    seek(&mut store, first, 1);
    let prepared = store.prepare_clone(first).unwrap();
    let second = store.commit_clone(prepared);
    assert_eq!(store.position(second), Ok(1));
    assert_eq!(store.reserved_bytes(), 32);
    let prepared = store.prepare_write(second, b"XY").unwrap();
    store.commit_write(prepared);
    assert_eq!(store.position(first), Ok(1));
    assert_eq!(read(&mut store, first, 9), b"XYd");
    let prepared = store.prepare_release_stream(first).unwrap();
    store.commit_release_stream(prepared);
    assert_eq!(store.position(first), Err(StreamError32::InvalidHandle));
    assert_eq!(store.reserved_bytes(), 32);
    let prepared = store.prepare_release_stream(second).unwrap();
    store.commit_release_stream(prepared);
    assert_eq!(store.reserved_bytes(), 0);
}
#[test]
fn seek_set_is_unsigned_while_relative_seek_rejects_underflow_without_allocating() {
    let mut store = store();
    let stream = create(&mut store, 16, b"x");
    seek(&mut store, stream, u64::MAX);
    assert_eq!(store.position(stream), Ok(u64::MAX));
    assert!(read(&mut store, stream, 9).is_empty());
    assert_eq!(store.position(stream), Ok(u64::MAX));
    let prepared = store.prepare_write(stream, b"").unwrap();
    store.commit_write(prepared);
    assert_eq!(store.size(stream), Ok(1));
    assert!(matches!(
        store.prepare_write(stream, b"a"),
        Err(StreamError32::MediumFull)
    ));
    seek(&mut store, stream, 0);
    assert!(matches!(
        store.prepare_seek(stream, u64::MAX, 1),
        Err(StreamError32::InvalidFunction)
    ));
    assert_eq!(store.position(stream), Ok(0));
    assert_eq!(store.reserved_bytes(), 16);
}
#[test]
fn bounded_growth_zero_fills_gaps_and_never_reallocates_the_declared_packet_capacity() {
    let mut store = store();
    let stream = create(&mut store, 16, b"ab");
    let buffer = store.stream(stream).unwrap().1.buffer;
    let original = store.owned(buffer).0.as_ptr();
    seek(&mut store, stream, 5);
    let prepared = store.prepare_write(stream, b"z").unwrap();
    store.commit_write(prepared);
    let prepared = store.prepare_resize(stream, 12).unwrap();
    store.commit_resize(prepared);
    assert_eq!(store.position(stream), Ok(6));
    seek(&mut store, stream, 0);
    assert_eq!(
        read(&mut store, stream, 99),
        [b'a', b'b', 0, 0, 0, b'z', 0, 0, 0, 0, 0, 0]
    );
    assert_eq!(store.owned(buffer).0.as_ptr(), original);
    assert!(matches!(
        store.prepare_resize(stream, 17),
        Err(StreamError32::MediumFull)
    ));
    assert_eq!(store.size(stream), Ok(12));
    assert_eq!(store.reserved_bytes(), 16);
}
#[test]
fn all_stream_and_external_rpc_leases_share_one_capacity_budget_and_stale_receipts_are_neutral() {
    let mut store = store();
    create(&mut store, 32, b"abc");
    let pending = store.prepare_stream(32, b"x").unwrap();
    let external = store.prepare_external_buffer(32).unwrap();
    let lease = store.commit_external_buffer(external);
    assert_eq!(store.can_commit_stream(&pending), Err(StreamError32::Stale));
    assert_eq!(store.reserved_bytes(), 64);
    assert!(matches!(
        store.prepare_external_buffer(1),
        Err(StreamError32::OutOfMemory)
    ));
    let prepared = store.prepare_release_buffer(lease).unwrap();
    store.commit_release_buffer(prepared);
    assert_eq!(store.reserved_bytes(), 32);
    assert!(store.prepare_release_buffer(lease).is_err());
}
#[test]
fn overlapping_copy_between_clones_is_memmove_and_publishes_both_cursors_once() {
    let mut store = store();
    let source = create(&mut store, 16, b"abcdef");
    let prepared = store.prepare_clone(source).unwrap();
    let target = store.commit_clone(prepared);
    seek(&mut store, target, 2);
    let prepared = store.prepare_copy(source, target, 4).unwrap();
    assert_eq!(prepared.length(), 4);
    store.commit_copy(prepared);
    assert_eq!(store.position(source), Ok(4));
    assert_eq!(store.position(target), Ok(6));
    seek(&mut store, source, 0);
    assert_eq!(read(&mut store, source, 99), b"ababcd");
}
#[test]
fn stale_read_and_write_authorities_cannot_consume_data_or_modify_bytes() {
    let mut store = store();
    let stream = create(&mut store, 16, b"abc");
    let pending = store.prepare_read(stream, 2).unwrap();
    let write = store.prepare_write(stream, b"Z").unwrap();
    let resize = store.prepare_resize(stream, 4).unwrap();
    store.commit_resize(resize);
    assert_eq!(store.read_bytes(&pending), Err(StreamError32::Stale));
    assert_eq!(store.can_commit_write(&write), Err(StreamError32::Stale));
    assert_eq!(store.position(stream), Ok(0));
    assert_eq!(read(&mut store, stream, 4), b"abc\0");
}
#[test]
fn self_copy_reads_then_writes_at_the_one_shared_seek_pointer() {
    let mut store = store();
    let prepared = store.prepare_stream(8, b"abcd").unwrap();
    let stream = store.commit_stream(prepared);
    let copy = store.prepare_copy(stream, stream, 4).unwrap();
    assert_eq!(copy.length(), 4);
    store.commit_copy(copy);
    assert_eq!(store.position(stream), Ok(8));
    assert_eq!(store.size(stream), Ok(8));
    let seek = store.prepare_seek(stream, 0, 0).unwrap();
    store.commit_seek(seek);
    let read = store.prepare_read(stream, 8).unwrap();
    assert_eq!(store.read_bytes(&read).unwrap(), b"abcdabcd");
}
