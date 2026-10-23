use super::{BANK_POINTER, Output, double, ieee, mem, memory, span};
use crate::cpu::core::emitter::CTX;
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
    if state(out, operation, next) {
        return true;
    }
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
    out.c().block(BlockType::Empty).block(BlockType::Empty);
    span::enter(out);
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
            span::push(out);
            write_f32(out, TOP);
            clear_c1(out);
            set_top(out);
        }
        X87Operation::Store {
            format: format @ (X87Format::Float32 | X87Format::Float64),
            pop,
            ..
        } => {
            occupied(out, 0, X);
            span::materialize_slot(out, 0, X);
            ieee::pack(out, X, format);
            memory::prepare(out, address.unwrap(), format.size() as u32, true, false, 52);
            out.c().local_get(52).local_get(PACKED);
            if format == X87Format::Float32 {
                out.c().i32_wrap_i64().i32_store(mem(0, 0));
            } else {
                out.c().i64_store(mem(0, 0));
            }
            memory::commit(out);
            span::clear_status_recipe(out);
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
            span::push(out);
            transfer(out, index.wrapping_add(1) & 7, X, 0, TOP, Y);
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
            transfer(out, 0, X, index, TMP, Y);
            if pop {
                pop_stack(out);
            }
        }
        X87Operation::Exchange { index } => {
            register(out, 0, X);
            register(out, index, Y);
            span::valid(out, 0);
            out.c().i32_eqz().i32_eqz();
            span::valid(out, index);
            out.c().i32_eqz().i32_eqz().i32_and().if_(BlockType::Empty);
            span::load(out, 0, FA);
            span::load(out, index, FB);
            write_value(out, 0, TOP, FB);
            write_value(out, index, TMP, FA);
            out.c().else_();
            span::materialize_slot(out, 0, X);
            span::materialize_slot(out, index, Y);
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
            span::invalidate_slot(out, 0);
            span::invalidate_slot(out, index);
            value_tag(out, TOP, X);
            value_tag(out, TMP, Y);
            out.c().end();
            clear_c1(out);
        }
        X87Operation::ChangeSign | X87Operation::Absolute => {
            register(out, 0, X);
            span::valid(out, 0);
            out.c().if_(BlockType::Empty);
            span::load(out, 0, FA);
            out.c().local_get(FA);
            if matches!(operation, X87Operation::ChangeSign) {
                out.c().f64_neg();
            } else {
                out.c().f64_abs();
            }
            out.c().local_set(FA);
            write_value(out, 0, TOP, FA);
            out.c()
                .else_()
                .local_get(X)
                .local_get(X)
                .i32_load16_u(mem(8, 0));
            if matches!(operation, X87Operation::ChangeSign) {
                out.c().i32_const(0x8000).i32_xor();
            } else {
                out.c().i32_const(0x7fff).i32_and();
            }
            out.c().i32_store16(mem(8, 0));
            value_tag(out, TOP, X);
            out.c().end();
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
            operand(out, destination, X, FA);
            register(out, source, Y);
            operand(out, source, Y, FB);
            arithmetic(out, kind);
            out.c()
                .local_get(TOP)
                .i32_const(i32::from(destination))
                .i32_add()
                .i32_const(7)
                .i32_and()
                .local_set(TMP);
            write_value(out, destination, TMP, double::RESULT);
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
            operand(out, 0, X, FA);
            memory::prepare(out, address.unwrap(), 4, false, false, 52);
            out.c().local_get(52).i32_load(mem(0, 0)).local_set(BITS);
            normal_or_zero(out, BITS, 0);
            out.c()
                .local_get(BITS)
                .f32_reinterpret_i32()
                .f64_promote_f32()
                .local_set(FB);
            arithmetic(out, kind);
            write_value(out, 0, TOP, double::RESULT);
        }
        _ => unreachable!(),
    }
    span::commit(out, opcode, address);
    out.set_pc(next);
    out.c().br(1).end();
    true
}

fn state(out: &mut Output<'_>, operation: Operation, next: u32) -> bool {
    let waiting = match operation {
        Operation::X87StatusToAx => false,
        Operation::X87 {
            operation: X87Operation::Wait,
            address: None,
            waiting: true,
            ..
        } => true,
        _ => return false,
    };
    out.c()
        .block(BlockType::Empty)
        .block(BlockType::Empty)
        .local_get(CTX)
        .i32_load(mem(BANK_POINTER, 2))
        .local_tee(BANK)
        .i32_eqz()
        .br_if(0);
    span::status(out);
    if waiting {
        out.c()
            .local_get(STATUS)
            .local_get(BANK)
            .i32_load16_u(mem(0, 1))
            .i32_const(-1)
            .i32_xor()
            .i32_and()
            .i32_const(63)
            .i32_and()
            .br_if(0);
    } else {
        out.c()
            .local_get(2)
            .i32_const(-65536)
            .i32_and()
            .local_get(STATUS)
            .i32_or()
            .local_set(2);
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
fn occupied(out: &mut Output<'_>, index: u8, destination: u32) {
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
}
fn register(out: &mut Output<'_>, index: u8, destination: u32) {
    occupied(out, index, destination);
    span::valid(out, index);
    out.c().i32_eqz().if_(BlockType::Empty);
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
        .br_if(1)
        .end();
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
fn to_f64(out: &mut Output<'_>, pointer: u32, value: u32, depth: u32) {
    out.c()
        .local_get(pointer)
        .i64_load(mem(0, 0))
        .local_tee(SIG)
        .i64_const(0x7ff)
        .i64_and()
        .i64_const(0)
        .i64_ne()
        .br_if(depth)
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
        .br_if(depth)
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
fn operand(out: &mut Output<'_>, index: u8, pointer: u32, value: u32) {
    span::valid(out, index);
    out.c().if_(BlockType::Empty);
    span::load(out, index, value);
    out.c().else_();
    to_f64(out, pointer, value, 1);
    span::remember(out, index, value);
    out.c().end();
    // precision and exponent admission applies equally to newly decoded and retained values.
    out.c()
        .local_get(value)
        .i64_reinterpret_f64()
        .i64_const(0x1fff_ffff)
        .i64_and()
        .i64_eqz()
        .i32_eqz()
        .local_get(DOUBLE)
        .i32_eqz()
        .i32_and()
        .br_if(0)
        .local_get(value)
        .i64_reinterpret_f64()
        .i64_const(52)
        .i64_shr_u()
        .i32_wrap_i64()
        .i32_const(2047)
        .i32_and()
        .i32_const(897)
        .i32_sub()
        .i32_const(253)
        .i32_gt_u()
        .local_get(value)
        .f64_const(0.0.into())
        .f64_ne()
        .i32_and()
        .local_get(DOUBLE)
        .i32_eqz()
        .i32_and()
        .br_if(0);
    double::bound(out, value, 0);
}
fn transfer(
    out: &mut Output<'_>,
    source_index: u8,
    source: u32,
    destination_index: u8,
    destination_physical: u32,
    destination: u32,
) {
    span::valid(out, source_index);
    out.c().if_(BlockType::Empty);
    span::load(out, source_index, FA);
    write_value(out, destination_index, destination_physical, FA);
    out.c().else_();
    copy(out, source, destination);
    span::invalidate_slot(out, destination_index);
    value_tag(out, destination_physical, destination);
    out.c().end();
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
    span::clear_status_recipe(out);
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
    span::pop(out);
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
fn write_value(out: &mut Output<'_>, index: u8, physical: u32, value: u32) {
    clear_tag(out, physical);
    out.c()
        .local_get(TAG)
        .local_get(value)
        .f64_const(0.0.into())
        .f64_eq()
        .local_get(physical)
        .i32_const(1)
        .i32_shl()
        .i32_shl()
        .i32_or()
        .local_set(TAG);
    span::write(out, index, value);
}
fn write_f32(out: &mut Output<'_>, physical: u32) {
    out.c()
        .local_get(BITS)
        .f32_reinterpret_i32()
        .f64_promote_f32()
        .local_set(double::RESULT);
    write_value(out, 0, physical, double::RESULT);
}
fn arithmetic_control(out: &mut Output<'_>) {
    out.c()
        .local_get(span::CONTROL)
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
fn arithmetic(out: &mut Output<'_>, kind: X87Binary) {
    out.c().local_get(DOUBLE).if_(BlockType::Empty);
    double::arithmetic(out, kind, 1);
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
