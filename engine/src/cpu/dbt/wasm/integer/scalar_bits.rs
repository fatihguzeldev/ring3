use wasm_encoder::InstructionSink;

use super::{FLAGS, LHS, RESULT, Register32, SmallWidth, memory, register};
use crate::cpu::x86::{
    bits::{BitCountKind, ScalarWidth},
    ir::{Location32, Value32},
};

pub(super) fn bit_count(
    code: &mut InstructionSink<'_>,
    kind: BitCountKind,
    width: ScalarWidth,
    destination: Register32,
    source: Value32,
    imports: memory::Imports,
    exit_depth: u32,
) {
    load_source(code, width, source, imports, exit_depth);
    code.local_get(RESULT).local_set(LHS).local_get(LHS);
    match kind {
        BitCountKind::TrailingZero => {
            if width == ScalarWidth::Word {
                code.i32_const(0x10000).i32_or();
            }
            code.i32_ctz();
        }
        BitCountKind::LeadingZero => {
            code.i32_clz();
            if width == ScalarWidth::Word {
                code.i32_const(16).i32_sub();
            }
        }
        BitCountKind::Population => {
            code.i32_popcnt();
        }
    }
    code.local_set(RESULT)
        .local_get(FLAGS)
        .i32_const(!0x8d5)
        .i32_and();
    match kind {
        BitCountKind::TrailingZero | BitCountKind::LeadingZero => {
            code.local_get(LHS)
                .i32_eqz()
                .i32_or()
                .local_get(RESULT)
                .i32_eqz();
        }
        BitCountKind::Population => {
            code.local_get(LHS).i32_eqz();
        }
    }
    code.i32_const(6).i32_shl().i32_or().local_set(FLAGS);
    write_register(code, width, destination);
}

pub(super) fn move_byte_swap(
    code: &mut InstructionSink<'_>,
    width: ScalarWidth,
    destination: Location32,
    source: Value32,
    imports: memory::Imports,
    exit_depth: u32,
) -> bool {
    load_source(code, width, source, imports, exit_depth);
    code.local_get(RESULT).local_set(LHS);
    match width {
        ScalarWidth::Word => {
            code.local_get(LHS)
                .i32_const(8)
                .i32_shl()
                .local_get(LHS)
                .i32_const(8)
                .i32_shr_u()
                .i32_or()
                .i32_const(0xffff)
                .i32_and();
        }
        ScalarWidth::Dword => {
            code.local_get(LHS)
                .i32_const(0x00ff_00ff)
                .i32_and()
                .i32_const(8)
                .i32_shl()
                .local_get(LHS)
                .i32_const(8)
                .i32_shr_u()
                .i32_const(0x00ff_00ff)
                .i32_and()
                .i32_or()
                .i32_const(16)
                .i32_rotl();
        }
    }
    code.local_set(RESULT);
    match destination {
        Location32::Register(destination) => {
            write_register(code, width, destination);
            false
        }
        Location32::Memory(address) => {
            match width {
                ScalarWidth::Word => memory::store_word_result(code, address, imports, exit_depth),
                ScalarWidth::Dword => memory::store_result(code, address, imports, exit_depth),
            }
            true
        }
    }
}

fn load_source(
    code: &mut InstructionSink<'_>,
    width: ScalarWidth,
    source: Value32,
    imports: memory::Imports,
    exit_depth: u32,
) {
    match source {
        Value32::Register(source) => {
            code.local_get(register(source)).local_set(RESULT);
        }
        Value32::Immediate(value) => {
            code.i32_const(value as i32).local_set(RESULT);
        }
        Value32::Memory(address) => match width {
            ScalarWidth::Word => {
                memory::load_narrow_value(code, address, SmallWidth::Word, imports, exit_depth);
                code.local_set(RESULT);
            }
            ScalarWidth::Dword => memory::load_result(code, address, imports, exit_depth),
        },
    }
    if width == ScalarWidth::Word {
        code.local_get(RESULT)
            .i32_const(0xffff)
            .i32_and()
            .local_set(RESULT);
    }
}

fn write_register(code: &mut InstructionSink<'_>, width: ScalarWidth, destination: Register32) {
    if width == ScalarWidth::Word {
        code.local_get(register(destination))
            .i32_const(!0xffff)
            .i32_and()
            .local_get(RESULT)
            .i32_or();
    } else {
        code.local_get(RESULT);
    }
    code.local_set(register(destination));
}
