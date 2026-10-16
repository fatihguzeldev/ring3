use super::*;
use crate::{
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
    windows::{
        com::{IID_IUNKNOWN, InterfaceId},
        ole32::{HResult32, IID_ICLASSFACTORY, KnownClass32},
    },
};
fn runtime() -> (Ole32Runtime32, AddressSpace) {
    let mut runtime = Ole32Runtime32::new(Ole32Config32 {
        objects: PageRange::new(GuestAddress(0x1000_0000), 32).unwrap(),
        max_objects: 8,
        max_threads: 4,
    })
    .unwrap();
    runtime.methods = std::array::from_fn(|i| GuestAddress(0x9000 + i as u32 * 4));
    let mut memory = AddressSpace::new(64).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x2000), 1).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    (runtime, memory)
}
fn read(memory: &AddressSpace, address: u32) -> u32 {
    let mut bytes = [0; 4];
    memory.read(GuestAddress(address), &mut bytes).unwrap();
    u32::from_le_bytes(bytes)
}
#[test]
fn factory_query_identity_references_and_final_release_are_real() {
    let (mut runtime, mut memory) = runtime();
    runtime
        .publish_factory(
            &mut memory,
            KnownClass32::DirectSound8,
            IID_ICLASSFACTORY,
            0x2000,
        )
        .unwrap();
    let pointer = read(&memory, 0x2000);
    assert_eq!(
        runtime.factory_class(&memory, pointer),
        Ok(KnownClass32::DirectSound8)
    );
    assert_eq!(
        runtime.query_factory(&mut memory, pointer, IID_IUNKNOWN, 0x2004),
        Ok(0)
    );
    assert_eq!(read(&memory, 0x2004), pointer);
    assert_eq!(
        runtime.query_factory(&mut memory, pointer, InterfaceId([0xee; 16]), 0x2008),
        Ok(0x8000_4002)
    );
    assert_eq!(read(&memory, 0x2008), 0);
    assert_eq!(runtime.release_factory(&mut memory, pointer), Ok(1));
    assert_eq!(runtime.release_factory(&mut memory, pointer), Ok(0));
    assert!(runtime.factory_class(&memory, pointer).is_err());
}
#[test]
fn invalid_output_and_server_lock_exhaustion_do_not_publish_or_increment() {
    let (mut runtime, mut memory) = runtime();
    let before = memory.transaction_version();
    assert_eq!(
        runtime.publish_factory(
            &mut memory,
            KnownClass32::DirectInput8,
            IID_ICLASSFACTORY,
            0xffff_ffff
        ),
        Err(HResult32::POINTER)
    );
    assert_eq!(memory.transaction_version(), before);
    assert!(runtime.factories.iter().all(Option::is_none));
    assert_eq!(runtime.lock_server(KnownClass32::DirectInput8, true), Ok(0));
    assert_eq!(
        runtime.lock_server(KnownClass32::DirectInput8, false),
        Ok(0)
    );
    assert_eq!(
        runtime.lock_server(KnownClass32::DirectInput8, false),
        Err(HResult32::UNEXPECTED)
    );
    runtime.server_locks[6] = u32::MAX;
    assert_eq!(
        runtime.lock_server(KnownClass32::DirectInput8, true),
        Err(HResult32::OUT_OF_MEMORY)
    );
    assert_eq!(runtime.server_locks[6], u32::MAX);
}
