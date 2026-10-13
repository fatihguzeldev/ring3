use ring3_engine::{
    cpu::x86::{
        decode::{DecodeError, decode_one},
        ir::Operation,
    },
    memory::{
        Access, AddressSpace, FaultReason, GuestAddress, MemoryFault, PageRange, Permissions,
    },
};

// (data bytes, index bytes, VEX.L), independently transcribed from the opcode tables.
const GEOMETRY: &[(u8, u8, bool)] = &[
    (4, 4, false),
    (4, 4, true),
    (8, 4, false),
    (8, 4, true),
    (4, 8, false),
    (4, 8, true),
    (8, 8, false),
    (8, 8, true),
];

fn bytes(geometry: (u8, u8, bool), floating: bool, index: u8, scale: u8) -> Vec<u8> {
    let (data, index_bytes, wide) = geometry;
    let opcode = 0x90 | u8::from(index_bytes == 8) | (u8::from(floating) << 1);
    vec![
        0xc4,
        0xe2,
        0x61 | (u8::from(wide) << 2) | (u8::from(data == 8) << 7),
        opcode,
        0x0c,
        (scale << 6) | (index << 3),
    ]
}

fn memory(bytes: &[u8], pc: u32) -> AddressSpace {
    let mut memory = AddressSpace::new(1).unwrap();
    let page = PageRange::new(GuestAddress(pc & !0xfff), 1).unwrap();
    memory.map_zeroed(page, Permissions::ALL).unwrap();
    memory.write(GuestAddress(pc), bytes).unwrap();
    memory.protect(page, Permissions::EXECUTE).unwrap();
    memory
}

fn decode(bytes: &[u8], pc: u32) -> Result<Operation, DecodeError> {
    let memory = memory(bytes, pc);
    let instruction = decode_one(&memory, GuestAddress(pc))?;
    assert_eq!(instruction.pc(), GuestAddress(pc));
    assert_eq!(instruction.length() as usize, bytes.len());
    assert_eq!(instruction.next_pc(), GuestAddress(pc + bytes.len() as u32));
    assert!(memory.is_code_current(instruction.code_snapshot()));
    Ok(*instruction.operation())
}

#[test]
fn all_sixteen_codes_bind_vsib_scale_and_raw_alias_geometry() {
    for &geometry in GEOMETRY {
        for scale in 0..4 {
            let integer = decode(&bytes(geometry, false, 2, scale), 0x1000).unwrap();
            let floating = decode(&bytes(geometry, true, 2, scale), 0x1000).unwrap();
            assert!(matches!(integer, Operation::Avx(_)));
            assert_eq!(integer, floating, "raw aliases {geometry:?} scale{scale}");
            // SIB index4 is vector register4, never the scalar no-index encoding.
            assert_ne!(
                integer,
                decode(&bytes(geometry, false, 4, scale), 0x1000).unwrap()
            );
            assert_ne!(
                integer,
                decode(&bytes(geometry, false, 2, (scale + 1) % 4), 0x1000).unwrap()
            );
        }
    }
}

#[test]
fn valid_vsib_fetches_full_sib_displacement_and_keeps_default_base_identity() {
    for &geometry in GEOMETRY {
        let plain = bytes(geometry, false, 2, 0);
        assert!(decode(&plain, 0x2000 - plain.len() as u32).is_ok());
        let mut displaced = plain.clone();
        displaced[4] |= 0x40;
        displaced.push(0x20);
        assert!(decode(&displaced, 0x1000).is_ok());
        let mut no_base = plain.clone();
        no_base[5] |= 5;
        no_base.extend(0x12345678_u32.to_le_bytes());
        assert!(decode(&no_base, 0x1000).is_ok());
        let mut stack = plain.clone();
        stack[4] |= 0x40;
        stack[5] |= 5;
        stack.push(0);
        assert!(decode(&stack, 0x1000).is_ok());
        let mut fs = vec![0x64];
        fs.extend(plain);
        assert_ne!(
            decode(&fs, 0x1000).unwrap(),
            decode(&stack, 0x1000).unwrap()
        );
    }
}

#[test]
fn malformed_vsib_and_pairwise_aliases_refuse_before_operand_access() {
    for &geometry in GEOMETRY {
        for floating in [false, true] {
            let valid = bytes(geometry, floating, 2, 0);
            for prefix in [0x66, 0xf0, 0xf2, 0xf3, 0x67] {
                let mut malformed = vec![prefix];
                malformed.extend(&valid);
                assert_eq!(
                    decode(&malformed, 0x1000),
                    Err(DecodeError::InvalidEncoding)
                );
            }
            let mut variants = Vec::new();
            let mut no_sib = valid.clone();
            no_sib[4] = 0x08;
            variants.push(no_sib);
            let mut register = valid.clone();
            register[4] = 0xcc;
            variants.push(register);
            let mut destination_is_mask = valid.clone();
            destination_is_mask[2] = (valid[2] & !0x78) | (14 << 3);
            variants.push(destination_is_mask);
            for alias_index in [1, 3] {
                let mut alias = valid.clone();
                alias[5] = alias_index << 3;
                variants.push(alias);
            }
            for malformed in variants {
                assert_eq!(
                    decode(&malformed, 0x1000),
                    Err(DecodeError::InvalidEncoding)
                );
            }
        }
    }
}

#[test]
fn missing_final_sib_is_a_fetch_fault_and_existing_avx_loads_stay_available() {
    let valid = bytes(GEOMETRY[0], false, 2, 0);
    assert_eq!(
        decode(&valid[..5], 0x1ffb),
        Err(DecodeError::MemoryFault {
            pc: GuestAddress(0x1ffb),
            fault: MemoryFault {
                address: GuestAddress(0x2000),
                access: Access::Execute,
                reason: FaultReason::Unmapped,
            },
            length: 6,
        })
    );
    for ordinary in [&[0xc5, 0xf8, 0x10, 0x08][..], &[0xc5, 0xfe, 0x6f, 0x08]] {
        assert!(matches!(
            decode(ordinary, 0x1000).unwrap(),
            Operation::Avx(_)
        ));
    }
}
