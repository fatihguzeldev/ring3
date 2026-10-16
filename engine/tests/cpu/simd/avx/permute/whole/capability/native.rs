use ring3_engine::{
    abi::arena::{HELPER_OFFSET, TRANSFER_OFFSET, YMM_OFFSET},
    cpu::x86::decode::{DecodeError, decode_one},
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
    process::{EngineInstance, HostError, StoreCompletion},
};
const FORMS: [(u8, u8, u8, bool); 4] = [
    (2, 0x6d, 0x16, false),
    (2, 0x6d, 0x36, false),
    (3, 0xfd, 0, true),
    (3, 0xfd, 1, true),
];
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
fn encoding(form: (u8, u8, u8, bool), operand: u8) -> Vec<u8> {
    let (map, control, opcode, immediate) = form;
    let mut bytes = vec![0xc4, 0xe0 | map, control, opcode, operand];
    if immediate {
        bytes.push(0x1b);
    }
    bytes
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

fn zero_register_result(before: &[u8]) -> Vec<u8> {
    let mut expected = before.to_vec();
    let mut packet = [0; 40];
    packet[..12].copy_from_slice(&[b'R', b'3', b'M', b'H', 7, 0, 1, 0, 40, 0, 0, 0]);
    expected[HELPER_OFFSET..HELPER_OFFSET + 40].copy_from_slice(&packet);
    expected[TRANSFER_OFFSET + 512..TRANSFER_OFFSET + 544].fill(0);
    expected[TRANSFER_OFFSET + 516..TRANSFER_OFFSET + 520].copy_from_slice(&2_u32.to_le_bytes());
    expected
}

#[test]
fn whole_and_lane_local_forms_bind_distinct_operations() {
    for form in FORMS {
        for operand in [0xcb, 0x08] {
            admitted(&encoding(form, operand));
        }
    }
    for bytes in [
        &[0xc4, 0xe2, 0x75, 0x0c, 0xc2][..],
        &[0xc4, 0xe3, 0x7d, 0x05, 0xc1, 0x1b][..],
    ] {
        assert!(decode_one(&code(bytes), GuestAddress(0x1000)).is_ok());
    }
}
#[test]
fn wrong_w_l_and_reserved_immediate_vvvv_refuse_before_private_gate() {
    for form in FORMS {
        let bytes = encoding(form, 0xcb);
        for changed in [bytes[2] ^ 0x80, bytes[2] & !4] {
            let mut invalid = bytes.clone();
            invalid[2] = changed;
            crate::support_fetch::assert_decode_contract(
                decode_one(&code(&invalid), GuestAddress(0x1000)).unwrap_err(),
                DecodeError::InvalidEncoding,
            );
        }
        if form.3 {
            let mut invalid = bytes.clone();
            invalid[2] &= !8;
            crate::support_fetch::assert_decode_contract(
                decode_one(&code(&invalid), GuestAddress(0x1000)).unwrap_err(),
                DecodeError::InvalidEncoding,
            );
        }
        for prefix in [0x66, 0xf0, 0xf2, 0xf3] {
            let mut invalid = vec![prefix];
            invalid.extend(&bytes);
            crate::support_fetch::assert_decode_contract(
                decode_one(&code(&invalid), GuestAddress(0x1000)).unwrap_err(),
                DecodeError::InvalidEncoding,
            );
        }
    }
}
#[test]
fn whole_descriptors_validate_state_metadata_and_exact_register_publication() {
    let mut engine = EngineInstance::new(1, 91).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
        .copy_from_slice(&[0xeb, 0xfe]);
    engine.upload(0x1000, 2).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[0, 0x10, 0, 0, 2, 0, 0, 0]);
    engine.compile(1).unwrap();
    let mut before_ram = [0; 4096];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x1000), &mut before_ram)
        .unwrap();
    for (descriptor, operands, immediate) in [
        (0x0100041f, 1 | (2 << 3) | (3 << 6), 0),
        (0x0100041f, 1 | (2 << 3) | (1 << 12), 0),
        (0x01000420, 1 | (3 << 6), 255),
        (0x01000420, 1 | (1 << 12), 255),
    ] {
        let flags = 2;
        let valid_register = operands & (1 << 12) == 0;
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + 544..TRANSFER_OFFSET + 568].fill(0xff);
        engine.arena_mut().unwrap()[YMM_OFFSET + 12] = 1;
        let malformed_state = engine.arena().to_vec();
        assert_eq!(
            engine.avx_execute(descriptor, operands, immediate, 0, 0, flags),
            Err(HostError::Infrastructure)
        );
        assert_eq!(engine.arena(), malformed_state);

        let mut after_ram = [0; 4096];
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(0x1000), &mut after_ram)
            .unwrap();
        assert_eq!(&after_ram[..], &before_ram[..]);
        assert_eq!(engine.arena(), malformed_state);
        engine.arena_mut().unwrap()[YMM_OFFSET + 12] = 0;
        let before = engine.arena().to_vec();
        let result = engine.avx_execute(descriptor, operands, immediate, 0, 0, flags);
        if valid_register {
            assert_eq!(result, Ok(StoreCompletion::Complete));
            assert_eq!(engine.arena(), zero_register_result(&before));
        } else {
            assert_eq!(result, Err(HostError::InvalidRequest));
            assert_eq!(engine.arena(), before);
        }

        let mut after_ram = [0; 4096];
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(0x1000), &mut after_ram)
            .unwrap();
        assert_eq!(&after_ram[..], &before_ram[..]);
        if valid_register {
            assert_eq!(engine.arena(), zero_register_result(&before));
        } else {
            assert_eq!(engine.arena(), before);
        }
    }
}
