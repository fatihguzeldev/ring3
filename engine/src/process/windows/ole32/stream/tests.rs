use crate::{
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
    process::windows::ole32::{Ole32Config32, Ole32Runtime32},
    windows::{
        com::IID_IUNKNOWN,
        ole32::{HResult32, IID_ISTREAM, StreamMethod32},
    },
};
fn runtime() -> (Ole32Runtime32, AddressSpace, u32, usize) {
    let mut runtime = Ole32Runtime32::new(Ole32Config32 {
        objects: PageRange::new(GuestAddress(0x1000_0000), 32).unwrap(),
        max_objects: 8,
        max_threads: 4,
    })
    .unwrap();
    runtime.streams.methods = core::array::from_fn(|slot| GuestAddress(0x9000 + slot as u32 * 4));
    let mut memory = AddressSpace::new(64).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x2000), 2).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    let pointer = runtime.create_stream(&mut memory, 32, b"abcdef").unwrap().0;
    let index = runtime.stream_index(&memory, pointer).unwrap();
    (runtime, memory, pointer, index)
}
fn word(memory: &AddressSpace, address: u32) -> u32 {
    let mut bytes = [0; 4];
    memory.read(GuestAddress(address), &mut bytes).unwrap();
    u32::from_le_bytes(bytes)
}
#[test]
fn invalid_guest_read_output_preserves_cursor_and_both_output_spans() {
    let (mut runtime, mut memory, pointer, index) = runtime();
    memory
        .write(GuestAddress(0x2000), &0xaabbccddu32.to_le_bytes())
        .unwrap();
    memory
        .protect(
            PageRange::new(GuestAddress(0x3000), 1).unwrap(),
            Permissions::READ,
        )
        .unwrap();
    let before = memory.transaction_version();
    assert_eq!(
        runtime.stream_call(
            &mut memory,
            index,
            StreamMethod32(3),
            &[pointer, 0x3000, 3, 0x2000]
        ),
        Err(HResult32(0x8003_0009))
    );
    assert_eq!(memory.transaction_version(), before);
    assert_eq!(word(&memory, 0x2000), 0xaabbccdd);
    let id = runtime.streams.entries[index].unwrap().id;
    assert_eq!(runtime.streams.store.position(id), Ok(0));
    assert_eq!(
        runtime.stream_call(
            &mut memory,
            index,
            StreamMethod32(3),
            &[pointer, 0x2004, 9, 0x2000]
        ),
        Ok(1)
    );
    assert_eq!(word(&memory, 0x2000), 6);
    assert_eq!(runtime.streams.store.position(id), Ok(6));
    let mut bytes = [0; 6];
    memory.read(GuestAddress(0x2004), &mut bytes).unwrap();
    assert_eq!(&bytes, b"abcdef");
}
#[test]
fn clone_identity_shares_contents_with_separate_cursor_and_literal_stat_layout() {
    let (mut runtime, mut memory, pointer, index) = runtime();
    assert_eq!(
        runtime.stream_call(&mut memory, index, StreamMethod32(13), &[pointer, 0x2000]),
        Ok(0)
    );
    let clone = word(&memory, 0x2000);
    assert_ne!(clone, pointer);
    let clone_index = runtime.stream_index(&memory, clone).unwrap();
    assert_eq!(
        runtime.stream_call(
            &mut memory,
            index,
            StreamMethod32(5),
            &[pointer, 2, 0, 0, 0]
        ),
        Ok(0)
    );
    memory.write(GuestAddress(0x2010), b"XY").unwrap();
    assert_eq!(
        runtime.stream_call(
            &mut memory,
            index,
            StreamMethod32(4),
            &[pointer, 0x2010, 2, 0]
        ),
        Ok(0)
    );
    assert_eq!(
        runtime.stream_call(
            &mut memory,
            clone_index,
            StreamMethod32(3),
            &[clone, 0x2020, 6, 0]
        ),
        Ok(0)
    );
    let mut bytes = [0; 6];
    memory.read(GuestAddress(0x2020), &mut bytes).unwrap();
    assert_eq!(&bytes, b"abXYef");
    assert_eq!(
        runtime.stream_call(
            &mut memory,
            clone_index,
            StreamMethod32(12),
            &[clone, 0x2040, 1]
        ),
        Ok(0)
    );
    assert_eq!(word(&memory, 0x2044), 2);
    assert_eq!(word(&memory, 0x2048), 6);
    assert_eq!(word(&memory, 0x2068), 2);
    assert_eq!(word(&memory, 0x206c), 0);
    memory.write(GuestAddress(0x2010), &IID_IUNKNOWN.0).unwrap();
    assert_eq!(
        runtime.stream_call(
            &mut memory,
            clone_index,
            StreamMethod32(0),
            &[clone, 0x2010, 0x2004]
        ),
        Ok(0)
    );
    let unknown = word(&memory, 0x2004);
    assert_eq!(
        runtime
            .com
            .query_target(&memory, GuestAddress(unknown), IID_ISTREAM),
        Ok(GuestAddress(clone))
    );
    assert_eq!(
        runtime.stream_call(&mut memory, clone_index, StreamMethod32(2), &[clone]),
        Ok(1)
    );
    assert_eq!(
        runtime.stream_call(&mut memory, clone_index, StreamMethod32(2), &[clone]),
        Ok(0)
    );
    assert_eq!(
        runtime.stream_call(&mut memory, index, StreamMethod32(2), &[pointer]),
        Ok(0)
    );
    assert_eq!(runtime.streams.store.reserved_bytes(), 0);
}
#[test]
fn invalid_write_counter_and_clone_output_do_not_mutate_stream_or_consume_capacity() {
    let (mut runtime, mut memory, pointer, index) = runtime();
    memory.write(GuestAddress(0x2000), b"XY").unwrap();
    memory
        .protect(
            PageRange::new(GuestAddress(0x3000), 1).unwrap(),
            Permissions::READ,
        )
        .unwrap();
    let id = runtime.streams.entries[index].unwrap().id;
    let before = runtime.streams.store.reserved_bytes();
    assert_eq!(
        runtime.stream_call(
            &mut memory,
            index,
            StreamMethod32(4),
            &[pointer, 0x2000, 2, 0x3000]
        ),
        Err(HResult32(0x8003_0009))
    );
    assert_eq!(runtime.streams.store.position(id), Ok(0));
    assert_eq!(
        runtime.stream_call(&mut memory, index, StreamMethod32(13), &[pointer, 0x3000]),
        Err(HResult32(0x8003_0009))
    );
    assert_eq!(runtime.streams.entries.iter().flatten().count(), 1);
    assert_eq!(runtime.streams.store.reserved_bytes(), before);
    assert_eq!(
        runtime.stream_call(
            &mut memory,
            index,
            StreamMethod32(10),
            &[pointer, 0, 0, 1, 0, 2]
        ),
        Err(HResult32(0x8003_0001))
    );
    assert_eq!(
        runtime.stream_call(
            &mut memory,
            index,
            StreamMethod32(3),
            &[pointer, 0x2010, 6, 0]
        ),
        Ok(0)
    );
    let mut bytes = [0; 6];
    memory.read(GuestAddress(0x2010), &mut bytes).unwrap();
    assert_eq!(&bytes, b"abcdef");
}
