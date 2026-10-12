use ring3_engine::{
    abi::arena::{ARENA_SIZE, HELPER_OFFSET, TRANSFER_OFFSET},
    cpu::dbt::RegistryError,
    memory::GuestAddress,
    process::{EngineInstance, HostError, StoreCompletion},
};

const KEY: u64 = 0x9234_5678_9abc_def0;
const FIRST: u32 = 0x1000;
const SECOND: u32 = 0x2000;

fn upload(engine: &mut EngineInstance, address: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(address, bytes.len() as u32).unwrap();
}

fn ram(engine: &EngineInstance) -> Vec<u8> {
    let mut bytes = vec![0; 8192];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(FIRST), &mut bytes)
        .unwrap();
    bytes
}

fn store(
    engine: &mut EngineInstance,
    resident: bool,
    id: u64,
    width: usize,
    address: u32,
    value: u32,
) -> Result<StoreCompletion, HostError> {
    match (resident, width) {
        (false, 2) => engine.store16(address, value),
        (false, 4) => engine.store32(address, value),
        (true, 2) => engine.store_resident16(KEY, id, address, value),
        (true, 4) => engine.store_resident32(KEY, id, address, value),
        _ => unreachable!(),
    }
}

fn success_packet(width: usize) -> [u8; 40] {
    let mut bytes = [0; 40];
    bytes[..16].copy_from_slice(&[
        0x52,
        0x33,
        0x4d,
        0x48,
        if width == 2 { 4 } else { 1 },
        0,
        1,
        0,
        40,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
    ]);
    if width == 2 {
        bytes[36..40].copy_from_slice(&2_u32.to_le_bytes());
    }
    bytes
}

#[test]
fn same_byte_mixed_executable_and_data_stores_commit_then_reject_stale_owners() {
    for resident in [false, true] {
        for width in [2, 4] {
            for executable_first in [false, true] {
                let case = (resident, width, executable_first);
                let mut engine = EngineInstance::new(2, KEY).unwrap();
                engine.map(FIRST, 2, 3).unwrap();
                let executable = if executable_first { FIRST } else { SECOND };
                let data = if executable_first { SECOND } else { FIRST };
                let entry = executable + 0x20;
                let address = SECOND - (width / 2) as u32;
                let payload = [0x11, 0x22, 0x33, 0x44];
                upload(&mut engine, entry, &[0x90]);
                upload(&mut engine, address - 1, &[0xa5]);
                upload(&mut engine, address, &payload[..width]);
                upload(&mut engine, address + width as u32, &[0x5a]);
                engine.protect(executable, 1, 7).unwrap();
                let descriptor =
                    &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
                descriptor[..4].copy_from_slice(&entry.to_le_bytes());
                descriptor[4..].copy_from_slice(&1_u32.to_le_bytes());
                let id = if resident {
                    engine.compile_resident(1).unwrap().get()
                } else {
                    u64::from(engine.compile(1).unwrap())
                };
                if resident {
                    engine.guard_resident(KEY, id).unwrap();
                } else {
                    engine.guard(KEY, id as u32).unwrap();
                }
                engine.arena_mut().unwrap()[HELPER_OFFSET..HELPER_OFFSET + 40].fill(0xa5);
                let before_arena = engine.arena().to_vec();
                let before_ram = ram(&engine);
                let value = if width == 2 {
                    u32::from(u16::from_le_bytes([0x11, 0x22]))
                } else {
                    u32::from_le_bytes(payload)
                };
                assert_eq!(
                    store(&mut engine, resident, id, width, address, value),
                    Ok(StoreCompletion::CodeInvalidated),
                    "{case:?}"
                );
                assert_eq!(ram(&engine), before_ram, "{case:?}");
                let mut expected_arena = before_arena;
                expected_arena[HELPER_OFFSET..HELPER_OFFSET + 40]
                    .copy_from_slice(&success_packet(width));
                assert_eq!(engine.arena().len(), ARENA_SIZE);
                assert_eq!(engine.arena(), expected_arena, "{case:?}");

                let expected_error = if resident {
                    HostError::Resident(RegistryError::CodeInvalidated)
                } else {
                    HostError::CodeInvalidated
                };
                assert_eq!(
                    store(&mut engine, resident, id, width, data + 0x100, 0xdead_beef),
                    Err(expected_error),
                    "{case:?}"
                );
                assert_eq!(ram(&engine), before_ram, "{case:?}");
                assert_eq!(engine.arena(), expected_arena, "{case:?}");
            }
        }
    }
}
