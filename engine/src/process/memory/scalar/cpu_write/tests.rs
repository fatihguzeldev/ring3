use crate::{
    abi::arena::{HELPER_OFFSET, TRANSFER_OFFSET},
    memory::GuestAddress,
    process::{EngineInstance, StoreCompletion},
};

fn engine() -> EngineInstance {
    let mut engine = EngineInstance::new(3, 7).unwrap();
    for address in [0x1000, 0xffff_f000, 0] {
        engine.map(address, 1, 7).unwrap();
    }
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
        .copy_from_slice(&[0xeb, 0xfe]);
    engine.upload(0x1000, 2).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[0, 0x10, 0, 0, 2, 0, 0, 0]);
    engine.compile(1).unwrap();
    let memory = engine.memory.as_mut().unwrap();
    memory
        .write(GuestAddress(0xffff_fffe), &[0x11, 0x22])
        .unwrap();
    memory.write(GuestAddress(0), &[0x33, 0x44]).unwrap();
    engine
}

fn ram(engine: &EngineInstance) -> [Vec<u8>; 2] {
    std::array::from_fn(|index| {
        let mut bytes = vec![0; 4096];
        engine
            .memory
            .as_ref()
            .unwrap()
            .read(
                GuestAddress(if index == 0 { 0xffff_f000 } else { 0 }),
                &mut bytes,
            )
            .unwrap();
        bytes
    })
}

fn expected(before: &[u8], fields: [u32; 6]) -> Vec<u8> {
    let mut arena = before.to_vec();
    for (index, word) in [0x484d_3352, 0x0001_000a, 40, 0]
        .into_iter()
        .chain(fields)
        .enumerate()
    {
        arena[HELPER_OFFSET + index * 4..HELPER_OFFSET + index * 4 + 4]
            .copy_from_slice(&word.to_le_bytes());
    }
    arena
}

#[test]
fn cpu_scalar_store_wraps_and_publishes_exact_v10_while_legacy_store_keeps_overflow() {
    let mut engine = engine();
    let before_ram = ram(&engine);
    assert_eq!(
        engine.store32(0xffff_fffe, 0x89ab_cdef),
        Ok(StoreCompletion::Complete)
    );
    assert_eq!(ram(&engine), before_ram);
    assert_eq!(
        u32::from_le_bytes(
            engine.arena()[HELPER_OFFSET + 16..HELPER_OFFSET + 20]
                .try_into()
                .unwrap()
        ),
        1
    );
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.cpu_store32(0xffff_fffe, 0x89ab_cdef),
        Ok(StoreCompletion::Complete)
    );
    assert_eq!(engine.arena(), expected(&before, [0, 0, 0, 0, 0, 4]));
    let mut expected_ram = before_ram;
    expected_ram[0][4094..].copy_from_slice(&[0xef, 0xcd]);
    expected_ram[1][..2].copy_from_slice(&[0xab, 0x89]);
    assert_eq!(ram(&engine), expected_ram);
}

#[test]
fn cpu_narrow_atomic_reply_is_the_old_width_bounded_value() {
    let mut engine = engine();
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.cpu_exchange16(u32::MAX, 0xdead_abcd),
        Ok(StoreCompletion::Complete)
    );
    assert_eq!(engine.arena(), expected(&before, [0, 0x3322, 0, 0, 0, 2]));
    let mut bytes = [0; 4];
    engine
        .memory
        .as_ref()
        .unwrap()
        .cpu_read(GuestAddress(0xffff_fffe), &mut bytes)
        .unwrap();
    assert_eq!(bytes, [0x11, 0xcd, 0xab, 0x44]);
}

#[test]
fn low_write_permission_fault_is_canonical_and_preserves_every_ram_byte() {
    let mut engine = engine();
    engine.protect(0, 1, 1).unwrap();
    let before = engine.arena().to_vec();
    let before_ram = ram(&engine);
    let version = engine.memory.as_ref().unwrap().code_validation_version();
    assert_eq!(
        engine.cpu_store32(0xffff_fffe, 0x89ab_cdef),
        Ok(StoreCompletion::Complete)
    );
    let mut reply = expected(&before, [9, 0, 7, 0, 2, 4]);
    reply[HELPER_OFFSET + 4..HELPER_OFFSET + 6].copy_from_slice(&15_u16.to_le_bytes());
    assert_eq!(engine.arena(), reply);
    assert_eq!(engine.cr2(), Some(GuestAddress(0)));
    assert_eq!(ram(&engine), before_ram);
    assert_eq!(
        engine.memory.as_ref().unwrap().code_validation_version(),
        version
    );
}

#[test]
fn stale_material_refuses_before_memory_or_v10_packet_publication() {
    let mut engine = engine();
    engine.write32(0x1000, 0x9090_9090).unwrap();
    let before = engine.arena().to_vec();
    let before_ram = ram(&engine);
    assert!(engine.cpu_store32(0xffff_fffe, 0x89ab_cdef).is_err());
    assert_eq!(engine.arena(), before);
    assert_eq!(ram(&engine), before_ram);
}
