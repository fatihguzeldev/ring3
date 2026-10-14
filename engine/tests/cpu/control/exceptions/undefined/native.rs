use super::support_fetch as fetch;
use ring3_engine::{
    cpu::{
        dbt::{BlockSpec, CompileLimits, compile_entry_region, compile_region},
        x86::{
            ControlOperation,
            decode::{DecodeError, decode_one},
            ir::Operation,
        },
    },
    memory::{AddressSpace, FaultReason, GuestAddress, PageRange, Permissions},
};

fn executable(pc: u32, bytes: &[u8]) -> AddressSpace {
    let mut memory = AddressSpace::new(1).unwrap();
    let page = PageRange::new(GuestAddress(pc & !0xfff), 1).unwrap();
    memory.map_zeroed(page, Permissions::ALL).unwrap();
    memory.write(GuestAddress(pc), bytes).unwrap();
    memory.protect(page, Permissions::EXECUTE).unwrap();
    memory
}

fn encoding(opcode: u8, modrm: u8, word: bool) -> Vec<u8> {
    let mut bytes = Vec::new();
    if word {
        bytes.push(0x66);
    }
    bytes.extend([0x0f, opcode, modrm]);
    let mode = modrm >> 6;
    let base = modrm & 7;
    if mode != 3 && base == 4 {
        bytes.push(0x24);
    }
    match mode {
        0 if base == 5 => bytes.extend(u32::MAX.to_le_bytes()),
        1 => bytes.push(0x7f),
        2 => bytes.extend(0x1234_5678_u32.to_le_bytes()),
        _ => {}
    }
    bytes
}

#[test]
fn every_modrm_and_operand_width_is_a_fault_without_operand_effects() {
    for opcode in [0xff, 0xb9] {
        for word in [false, true] {
            for modrm in 0..=u8::MAX {
                let bytes = encoding(opcode, modrm, word);
                let memory = executable(0x1000, &bytes);
                let decoded = decode_one(&memory, GuestAddress(0x1000))
                    .unwrap_or_else(|error| panic!("{bytes:02x?}: {error:?}"));
                assert_eq!(
                    decoded.operation(),
                    &Operation::Control(ControlOperation::InvalidOpcode)
                );
                assert!(!ControlOperation::InvalidOpcode.reads_memory());
                assert_eq!(usize::from(decoded.length()), bytes.len());
                assert_eq!(decoded.next_pc(), GuestAddress(0x1000 + bytes.len() as u32));
                assert!(memory.is_code_current(decoded.code_snapshot()));
            }
        }
    }
}

#[test]
fn ignored_single_prefixes_and_invalid_neighbors_keep_the_existing_profile() {
    for opcode in [0xff, 0xb9] {
        for prefix in [0x66, 0x67, 0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65, 0xf2, 0xf3] {
            let bytes = [prefix, 0x0f, opcode, 0x00];
            let decoded = decode_one(&executable(0x1000, &bytes), GuestAddress(0x1000)).unwrap();
            assert_eq!(
                decoded.operation(),
                &Operation::Control(ControlOperation::InvalidOpcode)
            );
            assert_eq!(decoded.length(), 4);
        }
        let locked = [0xf0, 0x0f, opcode, 0xc0];
        assert_eq!(
            decode_one(&executable(0x1000, &locked), GuestAddress(0x1000)).unwrap_err(),
            DecodeError::InvalidEncoding
        );
        for prefix in [[0x66, 0x66], [0xf2, 0xf3]] {
            let bytes = [prefix.as_slice(), &[0x0f, opcode, 0xc0]].concat();
            assert!(matches!(
                decode_one(&executable(0x1000, &bytes), GuestAddress(0x1000)),
                Err(DecodeError::Unsupported(_))
            ));
        }
    }
}

#[test]
fn both_compilation_modes_end_at_the_undefined_instruction() {
    for opcode in [0xff, 0xb9] {
        for word in [false, true] {
            for modrm in [0xc0, 0x05] {
                let bytes = encoding(opcode, modrm, word);
                let memory = executable(0x1000, &bytes);
                for compiled in [
                    compile_region(
                        &memory,
                        &[BlockSpec {
                            entry: GuestAddress(0x1000),
                            byte_length: bytes.len() as u32,
                        }],
                        CompileLimits::default(),
                    )
                    .unwrap(),
                    compile_entry_region(
                        &memory,
                        &[GuestAddress(0x1000)],
                        CompileLimits::default(),
                    )
                    .unwrap(),
                ] {
                    assert_eq!(compiled.metadata().instructions, 1);
                    assert_eq!(
                        &compiled.wasm_bytes(&memory).unwrap()[..8],
                        b"\0asm\x01\0\0\0"
                    );
                }
            }
        }
    }
}

#[test]
fn fetch_requires_complete_modrm_and_displacement_but_no_following_byte() {
    for opcode in [0xff, 0xb9] {
        for bytes in [encoding(opcode, 0xc0, false), encoding(opcode, 0x05, true)] {
            let pc = 0x2000 - bytes.len() as u32;
            let decoded = decode_one(&executable(pc, &bytes), GuestAddress(pc)).unwrap();
            assert_eq!(decoded.length() as usize, bytes.len());
            assert_eq!(decoded.next_pc(), GuestAddress(0x2000));
        }
        for bytes in [
            vec![0x0f, opcode],
            vec![0x0f, opcode, 0x05, 0xff, 0xff, 0xff],
        ] {
            let pc = GuestAddress(0x2000 - bytes.len() as u32);
            fetch::assert_error(
                decode_one(&executable(pc.0, &bytes), pc).unwrap_err(),
                pc.0,
                0x2000,
                FaultReason::Unmapped,
                bytes.len() as u32 + 1,
            );
        }
    }
}
