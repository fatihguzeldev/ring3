use ring3_engine::{
    cpu::x86::{
        Register32,
        bits::BitManipulationKind,
        decode::{DecodeError, decode_one},
        ir::{EffectiveAddress, Operation, Value32},
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};

#[derive(Clone, Copy, Debug)]
enum Form {
    ZeroHigh,
    Deposit,
    ExtractMasked,
    MultiplyExtended,
    ShiftArithmeticRight,
    ShiftLeft,
    ShiftLogicalRight,
    RotateRight,
}

const FORMS: [Form; 8] = [
    Form::ZeroHigh,
    Form::Deposit,
    Form::ExtractMasked,
    Form::MultiplyExtended,
    Form::ShiftArithmeticRight,
    Form::ShiftLeft,
    Form::ShiftLogicalRight,
    Form::RotateRight,
];
const COUNTS: [u8; 7] = [0, 1, 31, 32, 63, 64, 255];

const REGISTERS: [Register32; 8] = [
    Register32::Eax,
    Register32::Ecx,
    Register32::Edx,
    Register32::Ebx,
    Register32::Esp,
    Register32::Ebp,
    Register32::Esi,
    Register32::Edi,
];

fn expected(form: Form, destination: u8, source: u8, control: u8, memory: bool) -> Operation {
    let source = if memory {
        Value32::Memory(EffectiveAddress {
            base: Some(REGISTERS[usize::from(source)]),
            ..Default::default()
        })
    } else {
        Value32::Register(REGISTERS[usize::from(source)])
    };
    if matches!(form, Form::MultiplyExtended) {
        return Operation::MultiplyExtended {
            high: REGISTERS[usize::from(destination)],
            low: REGISTERS[usize::from(control)],
            source,
        };
    }
    Operation::BitManipulation {
        kind: match form {
            Form::ZeroHigh => BitManipulationKind::ZeroHigh {
                index: REGISTERS[usize::from(control)],
            },
            Form::Deposit => BitManipulationKind::Deposit {
                value: REGISTERS[usize::from(control)],
            },
            Form::ExtractMasked => BitManipulationKind::ExtractMasked {
                value: REGISTERS[usize::from(control)],
            },
            Form::ShiftArithmeticRight => BitManipulationKind::ShiftArithmeticRight {
                count: REGISTERS[usize::from(control)],
            },
            Form::ShiftLeft => BitManipulationKind::ShiftLeft {
                count: REGISTERS[usize::from(control)],
            },
            Form::ShiftLogicalRight => BitManipulationKind::ShiftLogicalRight {
                count: REGISTERS[usize::from(control)],
            },
            Form::RotateRight => BitManipulationKind::RotateRight { count: control },
            Form::MultiplyExtended => unreachable!(),
        },
        destination: REGISTERS[usize::from(destination)],
        source,
    }
}

fn encoded(form: Form, destination: u8, source: u8, control: u8, memory: bool, w: bool) -> Vec<u8> {
    let (map, pp, opcode, vvvv) = match form {
        Form::ZeroHigh => (2, 0, 0xf5, control),
        Form::Deposit => (2, 3, 0xf5, control),
        Form::ExtractMasked => (2, 2, 0xf5, control),
        Form::MultiplyExtended => (2, 3, 0xf6, control),
        Form::ShiftArithmeticRight => (2, 2, 0xf7, control),
        Form::ShiftLeft => (2, 1, 0xf7, control),
        Form::ShiftLogicalRight => (2, 3, 0xf7, control),
        Form::RotateRight => (3, 3, 0xf0, 0),
    };
    let mut bytes = vec![
        0xc4,
        0xe0 | map,
        (u8::from(w) << 7) | ((!vvvv & 15) << 3) | pp,
        opcode,
        (if memory { 0 } else { 0xc0 }) | (destination << 3) | source,
    ];
    if matches!(form, Form::RotateRight) {
        bytes.push(control);
    }
    bytes
}

fn decode(bytes: &[u8]) -> Result<Operation, DecodeError> {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(0x1000), bytes).unwrap();
    memory
        .protect(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::EXECUTE,
        )
        .unwrap();
    let instruction = decode_one(&memory, GuestAddress(0x1000))?;
    assert_eq!(usize::from(instruction.length()), bytes.len());
    assert_eq!(
        instruction.next_pc(),
        GuestAddress(0x1000 + bytes.len() as u32)
    );
    Ok(*instruction.operation())
}

#[test]
fn bmi2_register_forms_cover_operand_aliases_immediates_and_ignored_vex_w() {
    let mut cases = 0;
    for form in FORMS {
        let controls: Vec<u8> = if matches!(form, Form::RotateRight) {
            COUNTS.to_vec()
        } else {
            (0..8).collect()
        };
        for destination in 0..8 {
            for source in 0..8 {
                for &control in &controls {
                    for w in [false, true] {
                        let bytes = encoded(form, destination, source, control, false, w);
                        assert_eq!(
                            decode(&bytes),
                            Ok(expected(form, destination, source, control, false)),
                            "{form:?}: {bytes:02x?}"
                        );
                        cases += 1;
                    }
                }
            }
        }
    }
    assert_eq!(cases, 8064);
}

#[test]
fn bmi2_memory_forms_decode_without_reading_the_operand() {
    let mut cases = 0;
    for form in FORMS {
        let controls: Vec<u8> = if matches!(form, Form::RotateRight) {
            COUNTS.to_vec()
        } else {
            (0..8).collect()
        };
        for destination in 0..8 {
            for &control in &controls {
                for w in [false, true] {
                    let bytes = encoded(form, destination, 3, control, true, w);
                    assert_eq!(
                        decode(&bytes),
                        Ok(expected(form, destination, 3, control, true)),
                        "{form:?}: {bytes:02x?}"
                    );
                    cases += 1;
                }
            }
        }
    }
    assert_eq!(cases, 1008);
}

#[test]
fn bmi2_rejects_vex_l_reserved_vvvv_and_legacy_mandatory_or_lock_prefixes() {
    for form in FORMS {
        let bytes = encoded(form, 0, 2, 1, false, false);
        let mut vector_length = bytes.clone();
        vector_length[2] |= 4;
        assert_eq!(decode(&vector_length), Err(DecodeError::InvalidEncoding));
        for prefix in [0x66, 0xf2, 0xf3, 0xf0] {
            let mut prefixed = vec![prefix];
            prefixed.extend_from_slice(&bytes);
            assert_eq!(decode(&prefixed), Err(DecodeError::InvalidEncoding));
        }
    }
    for w in [false, true] {
        let mut rotate = encoded(Form::RotateRight, 0, 2, 31, false, w);
        rotate[2] &= !8;
        assert_eq!(decode(&rotate), Err(DecodeError::InvalidEncoding));
    }
}
