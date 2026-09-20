#[path = "support/executable.rs"]
mod executable;

use ring3_core::execution::{Cpu32, Permissions, Register32, StopReason, load_pe32};

#[derive(Clone, Copy, Debug)]
enum Operation {
    Add,
    Sub,
    Cmp,
    Xor,
    Or,
    Test,
}
const OPERATIONS: [Operation; 6] = [
    Operation::Add,
    Operation::Sub,
    Operation::Cmp,
    Operation::Xor,
    Operation::Or,
    Operation::Test,
];

impl Operation {
    fn encoding(self) -> (u8, u8, u8) {
        match self {
            Self::Add => (0x04, 0x00, 0),
            Self::Sub => (0x2c, 0x28, 5),
            Self::Cmp => (0x3c, 0x38, 7),
            Self::Xor => (0x34, 0x30, 6),
            Self::Or => (0x0c, 0x08, 1),
            Self::Test => (0xa8, 0x84, 0),
        }
    }
    fn writes(self) -> bool {
        !matches!(self, Self::Cmp | Self::Test)
    }
}

fn oracle(operation: Operation, bits: u32, left: u32, right: u32) -> (u32, u32) {
    let modulus = 1_i64 << bits;
    let sign = modulus / 2;
    let signed = |v: u32| {
        if i64::from(v) >= sign {
            i64::from(v) - modulus
        } else {
            i64::from(v)
        }
    };
    let (wide, signed_result, carry, auxiliary) = match operation {
        Operation::Add => (
            i64::from(left) + i64::from(right),
            signed(left) + signed(right),
            u64::from(left) + u64::from(right) >= modulus.cast_unsigned(),
            (left % 16) + (right % 16) >= 16,
        ),
        Operation::Sub | Operation::Cmp => (
            i64::from(left) - i64::from(right),
            signed(left) - signed(right),
            left < right,
            left % 16 < right % 16,
        ),
        Operation::Xor => (i64::from(left ^ right), 0, false, false),
        Operation::Or => (i64::from(left | right), 0, false, false),
        Operation::Test => (i64::from(left & right), 0, false, false),
    };
    let result = u32::try_from(wide.rem_euclid(modulus)).unwrap();
    let flags = u32::from(carry)
        | (u32::from((result % 256).count_ones() % 2 == 0) << 2)
        | (u32::from(auxiliary) << 4)
        | (u32::from(result == 0) << 6)
        | (u32::from(i64::from(result) >= sign) << 7)
        | (u32::from(signed_result < -sign || signed_result >= sign) << 11);
    (result, flags)
}

fn immediate(code: &mut Vec<u8>, value: u32, word: bool) {
    code.extend_from_slice(&value.to_le_bytes()[..if word { 2 } else { 1 }]);
}

fn check(
    code: &[u8],
    operation: Operation,
    word: bool,
    left: u32,
    right: u32,
    memory_destination: bool,
) {
    let mask = if word { 0xffff } else { 0xff };
    let (result, flags) = oracle(operation, if word { 16 } else { 8 }, left, right);
    let mut image = load_pe32(&executable::pe32(code), 3).unwrap();
    image
        .memory
        .write(0x0040_2000, &(0xabcd_1200 & !mask | left).to_le_bytes())
        .unwrap();
    image
        .memory
        .write(0x0040_2004, &right.to_le_bytes())
        .unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_register(Register32::Eax, 0xabcd_1200 & !mask | left);
    cpu.set_register(Register32::Ebx, 0xabcd_5600 & !mask | right);
    cpu.eflags = 0xced7;
    let run = cpu.run(&mut image.memory, 1);
    assert_eq!(
        run.instructions, 1,
        "{operation:?} {code:02x?} {:?}",
        run.reason
    );
    let expected = if operation.writes() { result } else { left };
    assert_eq!(
        cpu.eflags & 0x8d5,
        flags,
        "{operation:?} {left:x} {right:x} {code:02x?}"
    );
    assert_eq!(cpu.eflags & !0x8d5, 0xced7 & !0x8d5);
    assert_eq!(cpu.register(Register32::Ebx), 0xabcd_5600 & !mask | right);
    let mut bytes = [0; 4];
    image.memory.read(0x0040_2000, &mut bytes).unwrap();
    let (register_value, memory_value) = if memory_destination {
        (left, expected)
    } else {
        (expected, left)
    };
    assert_eq!(
        cpu.register(Register32::Eax),
        0xabcd_1200 & !mask | register_value
    );
    assert_eq!(
        u32::from_le_bytes(bytes),
        0xabcd_1200 & !mask | memory_value
    );
}

#[test]
fn narrow_accumulator_flags_match_widened_math_at_signed_and_unsigned_edges() {
    for word in [false, true] {
        let mask = if word { 0xffff } else { 0xff };
        for operation in OPERATIONS {
            for left in [0, 1, 15, 127, 128, 255, 32767, 32768, 65535].map(|v| v & mask) {
                for right in [0, 1, 15, 127, 128, 255, 32767, 32768, 65535].map(|v| v & mask) {
                    let mut code = if word { vec![0x66] } else { vec![] };
                    code.push(operation.encoding().0 + u8::from(word));
                    immediate(&mut code, right, word);
                    check(&code, operation, word, left, right, false);
                }
            }
        }
    }
}

#[test]
fn register_memory_and_immediate_forms_share_width_and_sign_extension() {
    for word in [false, true] {
        let mask = if word { 0xffff } else { 0xff };
        for operation in OPERATIONS {
            let (_, register_opcode, group) = operation.encoding();
            let opcode = register_opcode + u8::from(word);
            let immediate_opcode = if matches!(operation, Operation::Test) {
                0xf6
            } else {
                0x80
            } + u8::from(word);
            for (tail, memory_destination) in [
                (vec![opcode, 0xd8], false),
                (vec![opcode, 0x1d, 0, 0x20, 0x40, 0], true),
                (vec![immediate_opcode, 0xc0 + group * 8], false),
                (
                    vec![immediate_opcode, 0x05 + group * 8, 0, 0x20, 0x40, 0],
                    true,
                ),
            ] {
                let is_immediate = tail[0] == immediate_opcode;
                let mut code = if word { vec![0x66] } else { vec![] };
                code.extend_from_slice(&tail);
                if is_immediate {
                    immediate(&mut code, 0x8011 & mask, word);
                }
                check(
                    &code,
                    operation,
                    word,
                    0x9234 & mask,
                    0x8011 & mask,
                    memory_destination,
                );
            }
            if !matches!(operation, Operation::Test) {
                let mut code = if word { vec![0x66] } else { vec![] };
                code.extend_from_slice(&[opcode + 2, 0x05, 4, 0x20, 0x40, 0]);
                check(&code, operation, word, 0x9234 & mask, 0x8011 & mask, false);
                if word {
                    check(
                        &[0x66, 0x83, 0xc0 + group * 8, 0xff],
                        operation,
                        true,
                        1,
                        0xffff,
                        false,
                    );
                }
            }
        }
    }
}

#[test]
fn readonly_comparisons_and_faulting_writes_preserve_operands() {
    for opcode in [0x38, 0x84, 0x08] {
        for word in [false, true] {
            let mut code = if word { vec![0x66] } else { vec![] };
            code.extend_from_slice(&[opcode + u8::from(word), 0x1d, 0xff, 0x2f, 0x40, 0]);
            let mut image = load_pe32(&executable::pe32(&code), 3).unwrap();
            image
                .memory
                .protect(0x0040_2000, 4096, Permissions::READ)
                .unwrap();
            let mut cpu = Cpu32::new(image.entry_point);
            cpu.eflags = 0xced7;
            let before = cpu;
            let result = cpu.run(&mut image.memory, 1);
            if word || opcode == 0x08 {
                assert!(matches!(result.reason, StopReason::MemoryFault(_)));
                assert_eq!(cpu, before);
            } else {
                assert_eq!(result.instructions, 1);
            }
            let mut byte = [0];
            image.memory.read(0x0040_2fff, &mut byte).unwrap();
            assert_eq!(byte, [0]);
        }
    }
}

#[test]
fn high_byte_alias_and_word_loop_use_the_same_resumable_cpu() {
    let code = [
        0xb8, 1, 0x80, 0x34, 0x12, 0x00, 0xc4, 0x66, 0x0d, 1, 0x80, 0x66, 0xb9, 3, 0, 0x66, 0x83,
        0xe9, 1, 0x75, 0xfa, 0xcc,
    ];
    let mut whole = load_pe32(&executable::pe32(&code), 3).unwrap();
    let mut stepped = load_pe32(&executable::pe32(&code), 3).unwrap();
    let mut a = Cpu32::new(whole.entry_point);
    let mut b = a;
    let result = a.run(&mut whole.memory, 100);
    assert_eq!(result.reason, StopReason::Breakpoint);
    let mut count = 0;
    loop {
        let step = b.run(&mut stepped.memory, 1);
        count += step.instructions;
        if step.reason != StopReason::InstructionLimit {
            assert_eq!(step.reason, StopReason::Breakpoint);
            break;
        }
    }
    assert_eq!(a, b);
    assert_eq!(result.instructions, count);
    assert_eq!(a.register(Register32::Eax), 0x1234_8101);
}

#[test]
fn narrow_spans_can_end_exactly_at_four_gib_and_aliases_stay_unsupported() {
    for word in [false, true] {
        let address = if word { 0xffff_fffe_u32 } else { u32::MAX };
        let mut code = if word { vec![0x66] } else { vec![] };
        code.extend_from_slice(&[if word { 0x81 } else { 0x80 }, 0x0d]);
        code.extend_from_slice(&address.to_le_bytes());
        immediate(&mut code, if word { 0x8001 } else { 0x81 }, word);
        let mut image = load_pe32(&executable::pe32(&code), 4).unwrap();
        image
            .memory
            .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
            .unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        assert_eq!(cpu.run(&mut image.memory, 1).instructions, 1);
        let mut bytes = [0; 2];
        image
            .memory
            .read(u64::from(address), &mut bytes[..if word { 2 } else { 1 }])
            .unwrap();
        assert_eq!(bytes, if word { [1, 0x80] } else { [0x81, 0] });
    }
    for code in [
        &[0x82, 0xc0, 1][..],
        &[0xf3, 0x66, 0x0d, 1, 0][..],
        &[0x66, 0x67, 0x09, 0][..],
    ] {
        let mut image = load_pe32(&executable::pe32(code), 3).unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        let before = cpu;
        assert_eq!(
            cpu.run(&mut image.memory, 1).reason,
            StopReason::UnsupportedInstruction
        );
        assert_eq!(cpu, before);
    }
}
