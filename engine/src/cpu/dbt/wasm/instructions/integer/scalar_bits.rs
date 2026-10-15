use crate::cpu::dbt::wasm::imports::Imports;
use crate::cpu::{
    dbt::wasm::instructions::integer::{
        ByteValue, FLAGS, LHS, RESULT, Register32, SmallWidth, byte_value, memory, register,
    },
    x86::{
        bits::{BitCountKind, Crc32Source, Crc32Width, ScalarWidth},
        ir::{Location32, Value32},
    },
};
use wasm_encoder::InstructionSink;
pub(super) fn crc32(
    code: &mut InstructionSink<'_>,
    destination: Register32,
    source: Crc32Source,
    imports: Imports,
    exit_depth: u32,
) {
    let bits = match source {
        Crc32Source::ByteRegister(source) => {
            byte_value(code, ByteValue::Register(source));
            8
        }
        Crc32Source::WordRegister(source) => {
            code.local_get(register(source)).i32_const(0xffff).i32_and();
            16
        }
        Crc32Source::DwordRegister(source) => {
            code.local_get(register(source));
            32
        }
        Crc32Source::Memory { width, address } => match width {
            Crc32Width::Byte => {
                memory::load_narrow_value(code, address, SmallWidth::Byte, imports, exit_depth);
                8
            }
            Crc32Width::Word => {
                memory::load_narrow_value(code, address, SmallWidth::Word, imports, exit_depth);
                16
            }
            Crc32Width::Dword => {
                memory::load_result(code, address, imports, exit_depth);
                code.local_get(RESULT);
                32
            }
        },
    };
    code.local_get(register(destination)).i32_xor();
    // unroll the fixed reflected CRC32C update without guest branches or helper calls.
    for _ in 0..bits {
        code.local_tee(RESULT)
            .i32_const(1)
            .i32_shr_u()
            .local_get(RESULT)
            .i32_const(31)
            .i32_shl()
            .i32_const(31)
            .i32_shr_s()
            .i32_const(0x82f6_3b78_u32 as i32)
            .i32_and()
            .i32_xor();
    }
    code.local_set(register(destination));
}

pub(super) fn bit_count(
    code: &mut InstructionSink<'_>,
    kind: BitCountKind,
    width: ScalarWidth,
    destination: Register32,
    source: Value32,
    imports: Imports,
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
    imports: Imports,
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
    imports: Imports,
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
