use ring3_engine::{
    cpu::x86::{
        Register32, SegmentRegister,
        bits::BitManipulationKind,
        decode::{DecodeError, decode_one},
        ir::{AddressSize, EffectiveAddress, Operation, Value32},
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};

#[derive(Clone, Copy, Debug)]
enum Form {
    AndNot,
    Extract,
    IsolateLowest,
    MaskLowest,
    ResetLowest,
}

const FORMS: [Form; 5] = [
    Form::AndNot,
    Form::Extract,
    Form::IsolateLowest,
    Form::MaskLowest,
    Form::ResetLowest,
];

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
    Operation::BitManipulation {
        kind: match form {
            Form::AndNot => BitManipulationKind::AndNot {
                left: REGISTERS[usize::from(control)],
            },
            Form::Extract => BitManipulationKind::Extract {
                control: REGISTERS[usize::from(control)],
            },
            Form::IsolateLowest => BitManipulationKind::IsolateLowest,
            Form::MaskLowest => BitManipulationKind::MaskLowest,
            Form::ResetLowest => BitManipulationKind::ResetLowest,
        },
        destination: REGISTERS[usize::from(destination)],
        source: if memory {
            Value32::Memory(EffectiveAddress {
                base: Some(REGISTERS[usize::from(source)]),
                ..Default::default()
            })
        } else {
            Value32::Register(REGISTERS[usize::from(source)])
        },
    }
}

fn encoded(form: Form, destination: u8, source: u8, control: u8, memory: bool, w: bool) -> Vec<u8> {
    let (opcode, vvvv, reg) = match form {
        Form::AndNot => (0xf2, control, destination),
        Form::Extract => (0xf7, control, destination),
        Form::IsolateLowest => (0xf3, destination, 3),
        Form::MaskLowest => (0xf3, destination, 2),
        Form::ResetLowest => (0xf3, destination, 1),
    };
    vec![
        0xc4,
        0xe2,
        (u8::from(w) << 7) | ((!vvvv & 15) << 3),
        opcode,
        (if memory { 0 } else { 0xc0 }) | (reg << 3) | source,
    ]
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
fn bmi1_register_forms_cover_every_operand_alias_and_ignored_vex_w() {
    let mut cases = 0;
    for form in FORMS {
        let controls = if matches!(form, Form::AndNot | Form::Extract) {
            8
        } else {
            1
        };
        for destination in 0..8 {
            for source in 0..8 {
                for control in 0..controls {
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
    assert_eq!(cases, 2432);
}

#[test]
fn bmi1_memory_forms_decode_without_reading_the_operand() {
    let mut cases = 0;
    for form in FORMS {
        let controls = if matches!(form, Form::AndNot | Form::Extract) {
            8
        } else {
            1
        };
        for destination in 0..8 {
            for control in 0..controls {
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
    assert_eq!(cases, 304);
}

#[test]
fn bmi1_rejects_vex_l_and_legacy_mandatory_or_lock_prefixes() {
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
}

#[test]
fn bmi1_address_and_segment_prefixes_preserve_dword_sources() {
    for form in FORMS {
        for (prefix, address) in [
            (
                vec![0x67],
                EffectiveAddress {
                    base: Some(Register32::Ebp),
                    index: Some(Register32::Edi),
                    address_size: AddressSize::Bits16,
                    ..Default::default()
                },
            ),
            (
                vec![0x64],
                EffectiveAddress {
                    base: Some(Register32::Ebx),
                    segment: Some(SegmentRegister::Fs),
                    ..Default::default()
                },
            ),
            (
                vec![0x65, 0x67],
                EffectiveAddress {
                    base: Some(Register32::Ebp),
                    index: Some(Register32::Edi),
                    address_size: AddressSize::Bits16,
                    segment: Some(SegmentRegister::Gs),
                    ..Default::default()
                },
            ),
        ] {
            let mut bytes = prefix;
            bytes.extend(encoded(form, 0, 3, 1, true, false));
            let mut operation = expected(form, 0, 3, 1, true);
            if let Operation::BitManipulation { source, .. } = &mut operation {
                *source = Value32::Memory(address);
            }
            assert_eq!(decode(&bytes), Ok(operation), "{form:?}: {bytes:02x?}");
        }
    }
}
