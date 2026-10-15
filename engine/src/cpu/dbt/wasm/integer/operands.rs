use super::{
    ByteRegister, ByteValue, ExtensionKind, InstructionSink, Location32, RESULT, Register32,
    SmallWidth, Value32, WordValue, memory, register,
};
use crate::cpu::dbt::wasm::imports::Imports;

pub(super) fn indirect_target(
    code: &mut InstructionSink<'_>,
    target: Location32,
    imports: Imports,
    exit_depth: u32,
) {
    match target {
        Location32::Register(target) => {
            code.local_get(register(target)).local_set(RESULT);
        }
        Location32::Memory(address) => {
            memory::load_result(code, address, imports, exit_depth);
        }
    }
}

fn byte_parent(byte: ByteRegister) -> (Register32, bool) {
    match byte {
        ByteRegister::Al => (Register32::Eax, false),
        ByteRegister::Cl => (Register32::Ecx, false),
        ByteRegister::Dl => (Register32::Edx, false),
        ByteRegister::Bl => (Register32::Ebx, false),
        ByteRegister::Ah => (Register32::Eax, true),
        ByteRegister::Ch => (Register32::Ecx, true),
        ByteRegister::Dh => (Register32::Edx, true),
        ByteRegister::Bh => (Register32::Ebx, true),
    }
}

pub(super) fn insert_byte(code: &mut InstructionSink<'_>, destination: ByteRegister) {
    let (parent, high) = byte_parent(destination);
    if high {
        code.i32_const(8).i32_shl();
    }
    code.local_get(register(parent))
        .i32_const(if high { !0xff00 } else { !0xff })
        .i32_and()
        .i32_or()
        .local_set(register(parent));
}

pub(super) fn word_value(code: &mut InstructionSink<'_>, value: WordValue) {
    match value {
        WordValue::Register(source) => {
            code.local_get(register(source)).i32_const(0xffff).i32_and();
        }
        WordValue::Immediate(value) => {
            code.i32_const(i32::from(value));
        }
    }
}

pub(super) fn byte_value(code: &mut InstructionSink<'_>, value: ByteValue) {
    match value {
        ByteValue::Immediate(value) => {
            code.i32_const(i32::from(value));
        }
        ByteValue::Register(source) => {
            let (parent, high) = byte_parent(source);
            code.local_get(register(parent));
            if high {
                code.i32_const(8).i32_shr_u();
            }
            code.i32_const(0xff).i32_and();
        }
    }
}

pub(super) fn value(code: &mut InstructionSink<'_>, value: Value32) {
    match value {
        Value32::Register(source) => {
            code.local_get(register(source));
        }
        Value32::Immediate(value) => {
            code.i32_const(value as i32);
        }
        Value32::Memory(_) => unreachable!("prepared region contains a memory operand"),
    }
}

pub(super) fn extend_byte_to_word(
    code: &mut InstructionSink<'_>,
    kind: ExtensionKind,
    destination: Register32,
) {
    if kind == ExtensionKind::Sign {
        extend_value(code, kind, SmallWidth::Byte);
        code.i32_const(0xffff).i32_and();
    }
    code.local_get(register(destination))
        .i32_const(!0xffff)
        .i32_and()
        .i32_or()
        .local_set(register(destination));
}

pub(super) fn extend_value(code: &mut InstructionSink<'_>, kind: ExtensionKind, width: SmallWidth) {
    let bits = match width {
        SmallWidth::Byte => 8,
        SmallWidth::Word => 16,
    };
    match kind {
        ExtensionKind::Zero => {
            code.i32_const((1 << bits) - 1).i32_and();
        }
        ExtensionKind::Sign => {
            code.i32_const(32 - bits)
                .i32_shl()
                .i32_const(32 - bits)
                .i32_shr_s();
        }
    }
}
