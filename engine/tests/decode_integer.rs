use ring3_engine::{
    cpu::{
        UnsupportedFeature,
        x86::{
            Register32,
            decode::{DecodeError, decode_one},
            ir::{BinaryKind, EffectiveAddress, Location32, Operation, UnaryKind, Value32},
        },
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};

const PC: GuestAddress = GuestAddress(0x1234);
const ADDRESS: EffectiveAddress = EffectiveAddress {
    base: Some(Register32::Ebx),
    index: Some(Register32::Ecx),
    scale: 4,
    displacement: 0xffff_ffe0,
};

fn executable_memory() -> AddressSpace {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory
}

fn assert_operation(memory: &mut AddressSpace, bytes: &[u8], expected: Operation) {
    memory.write(PC, bytes).unwrap();
    let decoded = decode_one(memory, PC).unwrap();
    assert_eq!(*decoded.operation(), expected, "bytes {bytes:02x?}");
    assert_eq!(decoded.pc(), PC);
    assert_eq!(decoded.length() as usize, bytes.len(), "bytes {bytes:02x?}");
    assert_eq!(decoded.next_pc(), GuestAddress(PC.0 + bytes.len() as u32));
}

#[test]
fn binary_forms_preserve_direction_address_and_immediate_bits() {
    use BinaryKind::{Add, And, Cmp, Or, Sub, Test, Xor};

    let memory_destination = Location32::Memory(ADDRESS);
    let register_destination = Location32::Register(Register32::Edx);
    let accumulator = Location32::Register(Register32::Eax);
    let memory_source = Value32::Memory(ADDRESS);
    let register_source = Value32::Register(Register32::Edx);
    let immediate32 = Value32::Immediate(0x8000_0001);
    let signed_immediate8 = Value32::Immediate(0xffff_ff80);
    // these literal bytes were assembled independently with clang's i386 assembler.
    let cases: [(BinaryKind, &[u8], Location32, Value32); 33] = [
        (
            Add,
            &[0x01, 0x54, 0x8b, 0xe0],
            memory_destination,
            register_source,
        ),
        (
            Add,
            &[0x03, 0x54, 0x8b, 0xe0],
            register_destination,
            memory_source,
        ),
        (
            Add,
            &[0x05, 0x01, 0x00, 0x00, 0x80],
            accumulator,
            immediate32,
        ),
        (
            Add,
            &[0x81, 0x44, 0x8b, 0xe0, 0x01, 0x00, 0x00, 0x80],
            memory_destination,
            immediate32,
        ),
        (
            Add,
            &[0x83, 0x44, 0x8b, 0xe0, 0x80],
            memory_destination,
            signed_immediate8,
        ),
        (
            Sub,
            &[0x29, 0x54, 0x8b, 0xe0],
            memory_destination,
            register_source,
        ),
        (
            Sub,
            &[0x2b, 0x54, 0x8b, 0xe0],
            register_destination,
            memory_source,
        ),
        (
            Sub,
            &[0x2d, 0x01, 0x00, 0x00, 0x80],
            accumulator,
            immediate32,
        ),
        (
            Sub,
            &[0x81, 0x6c, 0x8b, 0xe0, 0x01, 0x00, 0x00, 0x80],
            memory_destination,
            immediate32,
        ),
        (
            Sub,
            &[0x83, 0x6c, 0x8b, 0xe0, 0x80],
            memory_destination,
            signed_immediate8,
        ),
        (
            Cmp,
            &[0x39, 0x54, 0x8b, 0xe0],
            memory_destination,
            register_source,
        ),
        (
            Cmp,
            &[0x3b, 0x54, 0x8b, 0xe0],
            register_destination,
            memory_source,
        ),
        (
            Cmp,
            &[0x3d, 0x01, 0x00, 0x00, 0x80],
            accumulator,
            immediate32,
        ),
        (
            Cmp,
            &[0x81, 0x7c, 0x8b, 0xe0, 0x01, 0x00, 0x00, 0x80],
            memory_destination,
            immediate32,
        ),
        (
            Cmp,
            &[0x83, 0x7c, 0x8b, 0xe0, 0x80],
            memory_destination,
            signed_immediate8,
        ),
        (
            Test,
            &[0x85, 0x54, 0x8b, 0xe0],
            memory_destination,
            register_source,
        ),
        (
            Test,
            &[0xa9, 0x01, 0x00, 0x00, 0x80],
            accumulator,
            immediate32,
        ),
        (
            Test,
            &[0xf7, 0x44, 0x8b, 0xe0, 0x01, 0x00, 0x00, 0x80],
            memory_destination,
            immediate32,
        ),
        (
            And,
            &[0x21, 0x54, 0x8b, 0xe0],
            memory_destination,
            register_source,
        ),
        (
            And,
            &[0x23, 0x54, 0x8b, 0xe0],
            register_destination,
            memory_source,
        ),
        (
            And,
            &[0x25, 0x01, 0x00, 0x00, 0x80],
            accumulator,
            immediate32,
        ),
        (
            And,
            &[0x81, 0x64, 0x8b, 0xe0, 0x01, 0x00, 0x00, 0x80],
            memory_destination,
            immediate32,
        ),
        (
            And,
            &[0x83, 0x64, 0x8b, 0xe0, 0x80],
            memory_destination,
            signed_immediate8,
        ),
        (
            Or,
            &[0x09, 0x54, 0x8b, 0xe0],
            memory_destination,
            register_source,
        ),
        (
            Or,
            &[0x0b, 0x54, 0x8b, 0xe0],
            register_destination,
            memory_source,
        ),
        (
            Or,
            &[0x0d, 0x01, 0x00, 0x00, 0x80],
            accumulator,
            immediate32,
        ),
        (
            Or,
            &[0x81, 0x4c, 0x8b, 0xe0, 0x01, 0x00, 0x00, 0x80],
            memory_destination,
            immediate32,
        ),
        (
            Or,
            &[0x83, 0x4c, 0x8b, 0xe0, 0x80],
            memory_destination,
            signed_immediate8,
        ),
        (
            Xor,
            &[0x31, 0x54, 0x8b, 0xe0],
            memory_destination,
            register_source,
        ),
        (
            Xor,
            &[0x33, 0x54, 0x8b, 0xe0],
            register_destination,
            memory_source,
        ),
        (
            Xor,
            &[0x35, 0x01, 0x00, 0x00, 0x80],
            accumulator,
            immediate32,
        ),
        (
            Xor,
            &[0x81, 0x74, 0x8b, 0xe0, 0x01, 0x00, 0x00, 0x80],
            memory_destination,
            immediate32,
        ),
        (
            Xor,
            &[0x83, 0x74, 0x8b, 0xe0, 0x80],
            memory_destination,
            signed_immediate8,
        ),
    ];
    let mut memory = executable_memory();
    for (kind, bytes, destination, source) in cases {
        assert_operation(
            &mut memory,
            bytes,
            Operation::Binary {
                kind,
                destination,
                source,
            },
        );
    }
}

#[test]
fn unary_forms_preserve_register_and_memory_destinations() {
    use UnaryKind::{Dec, Inc, Neg, Not};

    let register = Location32::Register(Register32::Edx);
    let memory = Location32::Memory(ADDRESS);
    let cases: [(UnaryKind, &[u8], Location32); 8] = [
        (Inc, &[0x42], register),
        (Inc, &[0xff, 0x44, 0x8b, 0xe0], memory),
        (Dec, &[0x4a], register),
        (Dec, &[0xff, 0x4c, 0x8b, 0xe0], memory),
        (Not, &[0xf7, 0xd2], register),
        (Not, &[0xf7, 0x54, 0x8b, 0xe0], memory),
        (Neg, &[0xf7, 0xda], register),
        (Neg, &[0xf7, 0x5c, 0x8b, 0xe0], memory),
    ];
    let mut executable = executable_memory();
    for (kind, bytes, destination) in cases {
        assert_operation(
            &mut executable,
            bytes,
            Operation::Unary { kind, destination },
        );
    }
}

#[test]
fn adjacent_unsupported_integer_instructions_remain_rejected() {
    let cases: [(&str, &[u8]); 11] = [
        ("operand-prefixed add byte memory", &[0x66, 0x00, 0x10]),
        ("add word", &[0x66, 0x01, 0xd0]),
        ("operand-prefixed adc byte memory", &[0x66, 0x10, 0x10]),
        ("operand-prefixed sbb byte memory", &[0x66, 0x18, 0x10]),
        ("rol word", &[0x66, 0xd1, 0xc0]),
        ("ror word", &[0x66, 0xd1, 0xc8]),
        ("mul word", &[0x66, 0xf7, 0xe2]),
        ("imul word", &[0x66, 0x0f, 0xaf, 0xc2]),
        ("idiv", &[0xf7, 0xfa]),
        ("operand-prefixed inc byte memory", &[0x66, 0xfe, 0x00]),
        ("inc word", &[0x66, 0x40]),
    ];
    let mut memory = executable_memory();
    for (name, bytes) in cases {
        memory.write(PC, bytes).unwrap();
        assert!(
            matches!(
                decode_one(&memory, PC),
                Err(DecodeError::Unsupported(UnsupportedFeature::Opcode))
            ),
            "{name}: {bytes:02x?}"
        );
    }
}
