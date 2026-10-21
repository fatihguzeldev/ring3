use super::{BANK_POINTER, Output, double, ieee, mem, memory};
use crate::cpu::core::emitter::{CTX, LOGICAL_ADDRESS};
use crate::cpu::x86::{X87Binary, X87Format, X87Operation, ir::Operation};
use wasm_encoder::BlockType;

const BANK: u32 = 37;
const TOP: u32 = 38;
const X: u32 = 39;
const Y: u32 = 40;
const STATUS: u32 = 41;
const TAG: u32 = 42;
const BITS: u32 = 43;
const EXP: u32 = 44;
const TMP: u32 = 45;
const SIG: u32 = 46;
const PACKED: u32 = 47;
const FA: u32 = 48;
const FB: u32 = 49;
const EXACT: u32 = 50;
const ROUNDED: u32 = 51;
const DOUBLE: u32 = 54;

pub(super) fn begin(out: &mut Output<'_>, operation: Operation, next: u32) -> bool {
    let Operation::X87 {
        operation,
        address,
        opcode,
        ..
    } = operation
    else {
        return false;
    };
    if !matches!(
        operation,
        X87Operation::Load(X87Format::Float32)
            | X87Operation::Store {
                format: X87Format::Float32 | X87Format::Float64,
                truncate: false,
                ..
            }
            | X87Operation::LoadRegister { .. }
            | X87Operation::StoreRegister { .. }
            | X87Operation::Exchange { .. }
            | X87Operation::Binary { .. }
            | X87Operation::BinaryMemory {
                format: X87Format::Float32,
                ..
            }
            | X87Operation::ChangeSign
            | X87Operation::Absolute
    ) {
        return false;
    }
    out.c()
        .block(BlockType::Empty)
        .block(BlockType::Empty)
        .local_get(CTX)
        .i32_load(mem(BANK_POINTER, 2))
        .local_tee(BANK)
        .i32_eqz()
        .br_if(0)
        .local_get(BANK)
        .i32_load16_u(mem(0, 1))
        .i32_const(63)
        .i32_and()
        .i32_const(63)
        .i32_ne()
        .br_if(0)
        .local_get(BANK)
        .i32_load16_u(mem(2, 1))
        .local_tee(STATUS)
        .i32_const(11)
        .i32_shr_u()
        .i32_const(7)
        .i32_and()
        .local_set(TOP)
        .local_get(BANK)
        .i32_load16_u(mem(4, 1))
        .local_set(TAG);
    if let Some(address) = address {
        out.c()
            .local_get(BANK)
            .i32_load16_u(mem(100 + address.resolved_segment().index() as u64 * 2, 1))
            .i32_const(!3)
            .i32_and()
            .i32_eqz()
            .br_if(0);
        if operation.write_size() != 0
            && address.resolved_segment() == crate::cpu::x86::SegmentRegister::Cs
        {
            out.c().br(0);
        }
    }
    match operation {
        X87Operation::Load(X87Format::Float32) => {
            let address = address.unwrap();
            memory::prepare(out, address, 4, false, false, 52);
            out.c().local_get(52).i32_load(mem(0, 0)).local_set(BITS);
            normal_or_zero(out, BITS, 0);
            out.c()
                .local_get(TOP)
                .i32_const(1)
                .i32_sub()
                .i32_const(7)
                .i32_and()
                .local_set(TOP);
            tag(out, TOP);
            out.c().i32_const(3).i32_ne().br_if(0);
            pointer(out, TOP, 0, X);
            write_f32(out, X, TOP);
            clear_c1(out);
            set_top(out);
        }
        X87Operation::Store {
            format: format @ (X87Format::Float32 | X87Format::Float64),
            pop,
            ..
        } => {
            register(out, 0, X);
            ieee::pack(out, X, format);
            memory::prepare(out, address.unwrap(), format.size() as u32, true, false, 52);
            out.c().local_get(52).local_get(PACKED);
            if format == X87Format::Float32 {
                out.c().i32_wrap_i64().i32_store(mem(0, 0));
            } else {
                out.c().i64_store(mem(0, 0));
            }
            memory::commit(out);
            if pop {
                pop_stack(out);
            }
        }
        X87Operation::LoadRegister { index } => {
            register(out, index, X);
            out.c()
                .local_get(TOP)
                .i32_const(1)
                .i32_sub()
                .i32_const(7)
                .i32_and()
                .local_set(TOP);
            tag(out, TOP);
            out.c().i32_const(3).i32_ne().br_if(0);
            pointer(out, TOP, 0, Y);
            copy(out, X, Y);
            value_tag(out, TOP, Y);
            set_top(out);
        }
        X87Operation::StoreRegister { index, pop } => {
            register(out, 0, X);
            out.c()
                .local_get(TOP)
                .i32_const(i32::from(index))
                .i32_add()
                .i32_const(7)
                .i32_and()
                .local_set(TMP);
            pointer(out, TMP, 0, Y);
            copy(out, X, Y);
            value_tag(out, TMP, Y);
            if pop {
                pop_stack(out);
            }
        }
        X87Operation::Exchange { index } => {
            register(out, 0, X);
            register(out, index, Y);
            out.c()
                .local_get(X)
                .i64_load(mem(0, 0))
                .local_set(SIG)
                .local_get(X)
                .i32_load16_u(mem(8, 0))
                .local_set(EXP);
            copy(out, Y, X);
            out.c()
                .local_get(Y)
                .local_get(SIG)
                .i64_store(mem(0, 0))
                .local_get(Y)
                .local_get(EXP)
                .i32_store16(mem(8, 0));
            value_tag(out, TOP, X);
            value_tag(out, TMP, Y);
            clear_c1(out);
        }
        X87Operation::ChangeSign | X87Operation::Absolute => {
            register(out, 0, X);
            out.c().local_get(X).local_get(X).i32_load16_u(mem(8, 0));
            if matches!(operation, X87Operation::ChangeSign) {
                out.c().i32_const(0x8000).i32_xor();
            } else {
                out.c().i32_const(0x7fff).i32_and();
            }
            out.c().i32_store16(mem(8, 0));
            value_tag(out, TOP, X);
            clear_c1(out);
        }
        X87Operation::Binary {
            kind,
            destination,
            source,
            pop,
        } => {
            arithmetic_control(out);
            register(out, destination, X);
            operand(out, X, FA);
            register(out, source, Y);
            operand(out, Y, FB);
            arithmetic(out, kind, false);
            out.c()
                .local_get(TOP)
                .i32_const(i32::from(destination))
                .i32_add()
                .i32_const(7)
                .i32_and()
                .local_set(TMP);
            write_f64(out, X, TMP);
            if pop {
                pop_stack(out);
            }
        }
        X87Operation::BinaryMemory {
            kind,
            format: X87Format::Float32,
        } => {
            arithmetic_control(out);
            register(out, 0, X);
            operand(out, X, FA);
            memory::prepare(out, address.unwrap(), 4, false, false, 52);
            out.c().local_get(52).i32_load(mem(0, 0)).local_set(BITS);
            normal_or_zero(out, BITS, 0);
            out.c()
                .local_get(BITS)
                .f32_reinterpret_i32()
                .f64_promote_f32()
                .local_set(FB);
            arithmetic(out, kind, true);
            write_f64(out, X, TOP);
        }
        _ => unreachable!(),
    }
    out.c()
        .local_get(BANK)
        .local_get(STATUS)
        .i32_store16(mem(2, 1))
        .local_get(BANK)
        .local_get(TAG)
        .i32_store16(mem(4, 1));
    let pc = out.pc;
    out.c()
        .local_get(BANK)
        .i32_const(pc as i32)
        .i32_store(mem(8, 2))
        .local_get(BANK)
        .i32_const(i32::from(opcode))
        .i32_store16(mem(6, 1))
        .local_get(BANK)
        .local_get(BANK)
        .i32_load16_u(mem(102, 1))
        .i32_store16(mem(16, 1));
    if let Some(address) = address {
        out.c()
            .local_get(BANK)
            .local_get(LOGICAL_ADDRESS)
            .i32_store(mem(12, 2))
            .local_get(BANK)
            .local_get(BANK)
            .i32_load16_u(mem(100 + address.resolved_segment().index() as u64 * 2, 1))
            .i32_store16(mem(18, 1));
    }
    out.set_pc(next);
    out.c().br(1).end();
    true
}

fn tag(out: &mut Output<'_>, physical: u32) {
    out.c()
        .local_get(TAG)
        .local_get(physical)
        .i32_const(1)
        .i32_shl()
        .i32_shr_u()
        .i32_const(3)
        .i32_and();
}
fn clear_tag(out: &mut Output<'_>, physical: u32) {
    out.c()
        .local_get(TAG)
        .i32_const(3)
        .local_get(physical)
        .i32_const(1)
        .i32_shl()
        .i32_shl()
        .i32_const(-1)
        .i32_xor()
        .i32_and()
        .local_set(TAG);
}
fn value_tag(out: &mut Output<'_>, physical: u32, pointer: u32) {
    clear_tag(out, physical);
    out.c()
        .local_get(TAG)
        .local_get(pointer)
        .i64_load(mem(0, 0))
        .i64_eqz()
        .local_get(physical)
        .i32_const(1)
        .i32_shl()
        .i32_shl()
        .i32_or()
        .local_set(TAG);
}
fn pointer(out: &mut Output<'_>, physical: u32, index: u8, destination: u32) {
    out.c()
        .local_get(BANK)
        .local_get(physical)
        .i32_const(i32::from(index))
        .i32_add()
        .i32_const(7)
        .i32_and()
        .i32_const(10)
        .i32_mul()
        .i32_add()
        .i32_const(20)
        .i32_add()
        .local_set(destination);
}
fn register(out: &mut Output<'_>, index: u8, destination: u32) {
    assert!(index < 8);
    out.c()
        .local_get(TOP)
        .i32_const(i32::from(index))
        .i32_add()
        .i32_const(7)
        .i32_and()
        .local_set(TMP);
    pointer(out, TOP, index, destination);
    tag(out, TMP);
    out.c().i32_const(3).i32_eq().br_if(0);
    out.c()
        .local_get(destination)
        .i32_load16_u(mem(8, 0))
        .i32_const(0x7fff)
        .i32_and()
        .local_set(EXP)
        .local_get(destination)
        .i64_load(mem(0, 0))
        .local_set(SIG)
        .local_get(EXP)
        .i32_eqz()
        .local_get(SIG)
        .i64_eqz()
        .i32_and()
        .local_get(EXP)
        .i32_const(1)
        .i32_sub()
        .i32_const(0x7ffe)
        .i32_lt_u()
        .local_get(SIG)
        .i64_const(0)
        .i64_lt_s()
        .i32_and()
        .i32_or()
        .i32_eqz()
        .br_if(0);
}
fn normal_or_zero(out: &mut Output<'_>, bits: u32, depth: u32) {
    out.c()
        .local_get(bits)
        .i32_const(0x7fff_ffff)
        .i32_and()
        .i32_eqz()
        .i32_eqz()
        .local_get(bits)
        .i32_const(23)
        .i32_shr_u()
        .i32_const(255)
        .i32_and()
        .i32_const(1)
        .i32_sub()
        .i32_const(254)
        .i32_ge_u()
        .i32_and()
        .br_if(depth);
}
fn to_f64(out: &mut Output<'_>, pointer: u32, value: u32) {
    out.c()
        .local_get(pointer)
        .i64_load(mem(0, 0))
        .local_tee(SIG)
        .i64_const(0x7ff)
        .i64_and()
        .i64_const(0)
        .i64_ne()
        .br_if(0)
        .local_get(pointer)
        .i32_load16_u(mem(8, 0))
        .local_tee(EXP)
        .i32_const(0x7fff)
        .i32_and()
        .i32_const(15361)
        .i32_sub()
        .i32_const(2045)
        .i32_gt_u()
        .local_get(SIG)
        .i64_eqz()
        .i32_eqz()
        .i32_and()
        .br_if(0)
        .local_get(SIG)
        .i64_eqz()
        .if_(BlockType::Result(wasm_encoder::ValType::I64))
        .i64_const(0)
        .else_()
        .local_get(SIG)
        .i64_const(11)
        .i64_shr_u()
        .i64_const(0x000f_ffff_ffff_ffff)
        .i64_and()
        .local_get(EXP)
        .i32_const(0x7fff)
        .i32_and()
        .i32_const(15360)
        .i32_sub()
        .i64_extend_i32_u()
        .i64_const(52)
        .i64_shl()
        .i64_or()
        .end()
        .local_get(EXP)
        .i32_const(0x8000)
        .i32_and()
        .i64_extend_i32_u()
        .i64_const(48)
        .i64_shl()
        .i64_or()
        .f64_reinterpret_i64()
        .local_set(value);
}
fn operand(out: &mut Output<'_>, pointer: u32, value: u32) {
    to_f64(out, pointer, value);
    out.c()
        .local_get(SIG)
        .i64_const(0xff_ffff_ffff)
        .i64_and()
        .i64_const(0)
        .i64_ne()
        .local_get(DOUBLE)
        .i32_eqz()
        .i32_and()
        .br_if(0);
    out.c()
        .local_get(EXP)
        .i32_const(0x7fff)
        .i32_and()
        .i32_const(16257)
        .i32_sub()
        .i32_const(253)
        .i32_gt_u()
        .local_get(SIG)
        .i64_eqz()
        .i32_eqz()
        .i32_and()
        .local_get(DOUBLE)
        .i32_eqz()
        .i32_and()
        .br_if(0);
    double::bound(out, value, 0);
}
fn copy(out: &mut Output<'_>, source: u32, destination: u32) {
    out.c()
        .local_get(destination)
        .local_get(source)
        .i64_load(mem(0, 0))
        .i64_store(mem(0, 0))
        .local_get(destination)
        .local_get(source)
        .i32_load16_u(mem(8, 0))
        .i32_store16(mem(8, 0));
}
fn clear_c1(out: &mut Output<'_>) {
    out.c()
        .local_get(STATUS)
        .i32_const(!0x200)
        .i32_and()
        .local_set(STATUS);
}
fn set_top(out: &mut Output<'_>) {
    out.c()
        .local_get(STATUS)
        .i32_const(!0x3800)
        .i32_and()
        .local_get(TOP)
        .i32_const(11)
        .i32_shl()
        .i32_or()
        .local_set(STATUS);
}
fn pop_stack(out: &mut Output<'_>) {
    out.c()
        .local_get(TAG)
        .i32_const(3)
        .local_get(TOP)
        .i32_const(1)
        .i32_shl()
        .i32_shl()
        .i32_or()
        .local_set(TAG)
        .local_get(TOP)
        .i32_const(1)
        .i32_add()
        .i32_const(7)
        .i32_and()
        .local_set(TOP);
    set_top(out);
}
fn write_f32(out: &mut Output<'_>, pointer: u32, physical: u32) {
    clear_tag(out, physical);
    out.c()
        .local_get(BITS)
        .i32_const(0x7fff_ffff)
        .i32_and()
        .if_(BlockType::Empty)
        .local_get(pointer)
        .local_get(BITS)
        .i32_const(0x7fffff)
        .i32_and()
        .i32_const(0x800000)
        .i32_or()
        .i64_extend_i32_u()
        .i64_const(40)
        .i64_shl()
        .i64_store(mem(0, 0))
        .local_get(BITS)
        .i32_const(23)
        .i32_shr_u()
        .i32_const(255)
        .i32_and()
        .i32_const(16256)
        .i32_add()
        .local_set(EXP)
        .else_()
        .local_get(pointer)
        .i64_const(0)
        .i64_store(mem(0, 0))
        .local_get(TAG)
        .i32_const(1)
        .local_get(physical)
        .i32_const(1)
        .i32_shl()
        .i32_shl()
        .i32_or()
        .local_set(TAG)
        .i32_const(0)
        .local_set(EXP)
        .end()
        .local_get(pointer)
        .local_get(EXP)
        .local_get(BITS)
        .i32_const(16)
        .i32_shr_u()
        .i32_const(0x8000)
        .i32_and()
        .i32_or()
        .i32_store16(mem(8, 0));
}
fn write_f64(out: &mut Output<'_>, pointer: u32, physical: u32) {
    clear_tag(out, physical);
    out.c()
        .local_get(double::RESULT)
        .i64_reinterpret_f64()
        .local_tee(PACKED)
        .i64_const(i64::MAX)
        .i64_and()
        .i64_const(0)
        .i64_ne()
        .if_(BlockType::Empty)
        .local_get(pointer)
        .local_get(PACKED)
        .i64_const(0x000f_ffff_ffff_ffff)
        .i64_and()
        .i64_const(0x0010_0000_0000_0000)
        .i64_or()
        .i64_const(11)
        .i64_shl()
        .i64_store(mem(0, 0))
        .local_get(PACKED)
        .i64_const(52)
        .i64_shr_u()
        .i32_wrap_i64()
        .i32_const(2047)
        .i32_and()
        .i32_const(15360)
        .i32_add()
        .local_set(EXP)
        .else_()
        .local_get(pointer)
        .i64_const(0)
        .i64_store(mem(0, 0))
        .local_get(TAG)
        .i32_const(1)
        .local_get(physical)
        .i32_const(1)
        .i32_shl()
        .i32_shl()
        .i32_or()
        .local_set(TAG)
        .i32_const(0)
        .local_set(EXP)
        .end()
        .local_get(pointer)
        .local_get(EXP)
        .local_get(PACKED)
        .i64_const(48)
        .i64_shr_u()
        .i32_wrap_i64()
        .i32_const(0x8000)
        .i32_and()
        .i32_or()
        .i32_store16(mem(8, 0));
}
fn arithmetic_control(out: &mut Output<'_>) {
    out.c()
        .local_get(BANK)
        .i32_load16_u(mem(0, 1))
        .i32_const(0xf3f)
        .i32_and()
        .local_tee(TMP)
        .i32_const(63)
        .i32_eq()
        .local_get(TMP)
        .i32_const(0x23f)
        .i32_eq()
        .i32_or()
        .i32_eqz()
        .br_if(0)
        .local_get(TMP)
        .i32_const(0x23f)
        .i32_eq()
        .local_set(DOUBLE);
}
fn arithmetic(out: &mut Output<'_>, kind: X87Binary, single_source: bool) {
    out.c().local_get(DOUBLE).if_(BlockType::Empty);
    double::arithmetic(out, kind, single_source, 1);
    out.c().else_();
    arithmetic_single(out, kind, 1);
    out.c().end();
}
fn arithmetic_single(out: &mut Output<'_>, kind: X87Binary, depth: u32) {
    if matches!(
        kind,
        X87Binary::Add | X87Binary::Subtract | X87Binary::ReverseSubtract
    ) {
        // bounded operand distance makes the binary32 sum exact in binary64.
        out.c()
            .local_get(FA)
            .i64_reinterpret_f64()
            .i64_const(52)
            .i64_shr_u()
            .i32_wrap_i64()
            .i32_const(2047)
            .i32_and()
            .local_get(FB)
            .i64_reinterpret_f64()
            .i64_const(52)
            .i64_shr_u()
            .i32_wrap_i64()
            .i32_const(2047)
            .i32_and()
            .i32_sub()
            .local_tee(EXP)
            .i32_const(24)
            .i32_gt_s()
            .local_get(EXP)
            .i32_const(-24)
            .i32_lt_s()
            .i32_or()
            .local_get(FA)
            .f64_const(0.0.into())
            .f64_ne()
            .i32_and()
            .local_get(FB)
            .f64_const(0.0.into())
            .f64_ne()
            .i32_and()
            .br_if(depth);
    }
    let reverse = matches!(kind, X87Binary::ReverseSubtract | X87Binary::ReverseDivide);
    out.c()
        .local_get(if reverse { FB } else { FA })
        .local_get(if reverse { FA } else { FB });
    match kind {
        X87Binary::Add => {
            out.c().f64_add();
        }
        X87Binary::Multiply => {
            out.c().f64_mul();
        }
        X87Binary::Subtract | X87Binary::ReverseSubtract => {
            out.c().f64_sub();
        }
        X87Binary::Divide | X87Binary::ReverseDivide => {
            out.c().f64_div();
        }
    }
    out.c()
        .local_set(EXACT)
        .local_get(EXACT)
        .f64_abs()
        .f64_const((2.0f64.powi(-126)).into())
        .f64_lt()
        .local_get(EXACT)
        .f64_const(0.0.into())
        .f64_ne()
        .i32_and()
        .br_if(depth)
        .local_get(EXACT)
        .f32_demote_f64()
        .i32_reinterpret_f32()
        .local_set(BITS);
    normal_or_zero(out, BITS, depth);
    out.c()
        .local_get(BITS)
        .f32_reinterpret_i32()
        .f64_promote_f32()
        .local_tee(ROUNDED)
        .local_set(double::RESULT);
    clear_c1(out);
    if matches!(kind, X87Binary::Divide | X87Binary::ReverseDivide) {
        out.c()
            .local_get(ROUNDED)
            .local_get(if reverse { FA } else { FB })
            .f64_mul()
            .f64_abs()
            .local_set(EXACT)
            .local_get(if reverse { FB } else { FA })
            .f64_abs()
            .local_set(ROUNDED);
    } else {
        out.c()
            .local_get(EXACT)
            .f64_abs()
            .local_set(EXACT)
            .local_get(ROUNDED)
            .f64_abs()
            .local_set(ROUNDED);
    }
    out.c()
        .local_get(STATUS)
        .local_get(EXACT)
        .local_get(ROUNDED)
        .f64_ne()
        .i32_const(5)
        .i32_shl()
        .i32_or();
    if matches!(kind, X87Binary::Divide | X87Binary::ReverseDivide) {
        out.c().local_get(EXACT).local_get(ROUNDED).f64_gt();
    } else {
        out.c().local_get(ROUNDED).local_get(EXACT).f64_gt();
    }
    out.c().i32_const(9).i32_shl().i32_or().local_set(STATUS);
}
