use ring3_engine::{
    abi::arena::{HELPER_OFFSET, SIMD_OFFSET, TRANSFER_OFFSET, YMM_OFFSET},
    cpu::x86::decode::{DecodeError, decode_one},
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
    process::{EngineInstance, HostError, StoreCompletion},
};

const OPERANDS: u32 = 1 | (2 << 3) | (3 << 9) | (1 << 12);

fn code(bytes: &[u8]) -> AddressSpace {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(0x1000), bytes).unwrap();
    memory
}

fn engine() -> EngineInstance {
    let mut engine = EngineInstance::new(1, 91).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
        .copy_from_slice(&[0xeb, 0xfe]);
    engine.upload(0x1000, 2).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[0, 0x10, 0, 0, 2, 0, 0, 0]);
    engine.compile(1).unwrap();
    engine
}

fn ram(engine: &EngineInstance) -> [u8; 4096] {
    let mut result = [0; 4096];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x1000), &mut result)
        .unwrap();
    result
}

fn admitted(bytes: &[u8]) -> ring3_engine::cpu::x86::ir::Operation {
    let memory = code(bytes);
    let instruction = decode_one(&memory, GuestAddress(0x1000)).unwrap();
    assert!(
        matches!(
            instruction.operation(),
            ring3_engine::cpu::x86::ir::Operation::Avx(_)
        ),
        "{bytes:02x?}"
    );
    assert_eq!(instruction.pc(), GuestAddress(0x1000));
    assert_eq!(instruction.length() as usize, bytes.len());
    assert_eq!(
        instruction.next_pc(),
        GuestAddress(0x1000 + bytes.len() as u32)
    );
    assert!(memory.is_code_current(instruction.code_snapshot()));
    *instruction.operation()
}

#[test]
fn every_gather_code_binds_while_existing_avx_loads_remain_available() {
    for control in [0x61, 0x65, 0xe1, 0xe5] {
        for opcode in 0x90..=0x93 {
            let bytes = [0xc4, 0xe2, control, opcode, 0x0c, 0x10];
            admitted(&bytes);
        }
    }
    for bytes in [&[0xc5, 0xf8, 0x10, 0x08][..], &[0xc5, 0xfe, 0x6f, 0x08]] {
        assert!(decode_one(&code(bytes), GuestAddress(0x1000)).is_ok());
    }
    for (control, sib) in [(0x71, 0x10), (0x61, 0x08), (0x61, 0x18)] {
        let bytes = [0xc4, 0xe2, control, 0x90, 0x0c, sib];
        crate::support_fetch::assert_decode_contract(
            decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
            DecodeError::InvalidEncoding,
        );
    }
}

#[test]
fn gather_state_request_errors_and_zero_selection_have_distinct_staged_results() {
    let mut engine = engine();
    let before_ram = ram(&engine);
    let arena = engine.arena_mut().unwrap();
    arena[TRANSFER_OFFSET..TRANSFER_OFFSET + 4096].fill(0xa5);
    arena[TRANSFER_OFFSET + 544..TRANSFER_OFFSET + 568].fill(0xff);
    arena[YMM_OFFSET + 12] = 1;
    arena[SIMD_OFFSET + 12] = 1;
    let malformed_state = arena.to_vec();
    for id in 1..=8 {
        engine
            .arena_mut()
            .unwrap()
            .copy_from_slice(&malformed_state);
        assert_eq!(
            engine.avx_execute(0x01001400 | id, OPERANDS, 0, 0, 0, 2),
            Err(HostError::Infrastructure)
        );
        assert_eq!(engine.arena(), malformed_state);
        assert_eq!(ram(&engine), before_ram);
        let arena = engine.arena_mut().unwrap();
        arena[YMM_OFFSET + 12] = 0;
        arena[SIMD_OFFSET + 12] = 0;
        let malformed_request = arena.to_vec();
        assert_eq!(
            engine.avx_execute(0x01001400 | id, OPERANDS, 0, 0, 0, 2),
            Err(HostError::InvalidRequest)
        );
        assert_eq!(engine.arena(), malformed_request);
        assert_eq!(ram(&engine), before_ram);
        let request =
            &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET + 544..TRANSFER_OFFSET + 568];
        request.fill(0);
        request[..12].copy_from_slice(&[b'R', b'3', b'G', b'A', 1, 0, 1, 0, 24, 0, 0, 0]);
        request[16..20].copy_from_slice(&3_u32.to_le_bytes());
        request[20..24].copy_from_slice(&u32::MAX.to_le_bytes());
        let before = engine.arena().to_vec();
        let mut expected = before.clone();
        expected[TRANSFER_OFFSET..TRANSFER_OFFSET + 64].fill(0);
        let packet = &mut expected[HELPER_OFFSET..HELPER_OFFSET + 40];
        packet.fill(0);
        packet[..12].copy_from_slice(&[b'R', b'3', b'G', b'H', 1, 0, 1, 0, 40, 0, 0, 0]);
        packet[36..40].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(
            engine.avx_execute(0x01001400 | id, OPERANDS, 0, 0, 0, 2),
            Ok(StoreCompletion::Complete)
        );
        assert_eq!(engine.arena(), expected);
        assert_eq!(ram(&engine), before_ram);
    }
}

#[test]
fn normal_open_helpers_and_unknown_geometry_controls_keep_their_authority_contract() {
    let mut engine = engine();
    let before_ram = ram(&engine);
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.avx_execute(0x01000201, 0, 0, 0, 0, 2),
        Ok(StoreCompletion::Complete)
    );
    let mut expected = before.clone();
    let mut packet = [0; 40];
    packet[..12].copy_from_slice(&[b'R', b'3', b'M', b'H', 7, 0, 1, 0, 40, 0, 0, 0]);
    expected[HELPER_OFFSET..HELPER_OFFSET + 40].copy_from_slice(&packet);
    expected[TRANSFER_OFFSET + 512..TRANSFER_OFFSET + 544].fill(0);
    expected[TRANSFER_OFFSET + 516..TRANSFER_OFFSET + 520].copy_from_slice(&2_u32.to_le_bytes());
    assert_eq!(engine.arena(), expected);
    assert_eq!(ram(&engine), before_ram);
    for descriptor in [0x01001400, 0x01001409, 0x01001481, 0x010014ff, 0x01001501] {
        let before = engine.arena().to_vec();
        assert_eq!(
            engine.avx_execute(descriptor, OPERANDS, 0, 0, 0, 2),
            Err(HostError::InvalidRequest)
        );
        assert_eq!(engine.arena(), before);
        assert_eq!(ram(&engine), before_ram);
    }
    engine.close();
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.avx_execute(0x01001401, OPERANDS, 0, 0, 0, 2),
        Err(HostError::Closed)
    );
    assert_eq!(engine.arena(), before);
}
