use ring3_engine::{
    abi::arena::{HELPER_OFFSET, SIMD_OFFSET, TRANSFER_OFFSET, YMM_OFFSET},
    cpu::{
        UnsupportedFeature,
        x86::decode::{DecodeError, decode_one},
    },
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

#[test]
fn every_gather_code_refuses_while_existing_avx_loads_remain_available() {
    for control in [0x61, 0x65, 0xe1, 0xe5] {
        for opcode in 0x90..=0x93 {
            let bytes = [0xc4, 0xe2, control, opcode, 0x0c, 0x10];
            assert_eq!(
                decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
                DecodeError::Unsupported(UnsupportedFeature::Simd)
            );
        }
    }
    for bytes in [&[0xc5, 0xf8, 0x10, 0x08][..], &[0xc5, 0xfe, 0x6f, 0x08]] {
        assert!(decode_one(&code(bytes), GuestAddress(0x1000)).is_ok());
    }
    for (control, sib) in [(0x71, 0x10), (0x61, 0x08), (0x61, 0x18)] {
        let bytes = [0xc4, 0xe2, control, 0x90, 0x0c, sib];
        assert_eq!(
            decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
            DecodeError::InvalidEncoding
        );
    }
}

#[test]
fn gather_helper_gate_precedes_malformed_state_request_and_staged_output() {
    let mut engine = engine();
    let before_ram = ram(&engine);
    let arena = engine.arena_mut().unwrap();
    arena[TRANSFER_OFFSET..TRANSFER_OFFSET + 4096].fill(0xa5);
    arena[TRANSFER_OFFSET + 544..TRANSFER_OFFSET + 568].fill(0xff);
    arena[YMM_OFFSET + 12] = 1;
    arena[SIMD_OFFSET + 12] = 1;
    let before = arena.to_vec();
    for id in 1..=8 {
        assert_eq!(
            engine.avx_execute(0x01001400 | id, OPERANDS, 0, 0, 0, 2),
            Err(HostError::InvalidRequest)
        );
        assert_eq!(engine.arena(), before);
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
