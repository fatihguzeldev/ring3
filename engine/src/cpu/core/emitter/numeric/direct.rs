use super::{BANK_POINTER, Output, double, frame, ieee, mem, memory, pool, span};
use crate::cpu::core::emitter::CTX;
use crate::cpu::core::region::values::{FpValue, ProofKind, TagRecipe};
use crate::cpu::x86::{X87Binary, X87Format, X87Operation, ir::Operation};
use wasm_encoder::BlockType;

const BANK: u32 = 37;
const X: u32 = 39;
const STATUS: u32 = 41;
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum StoreValue {
    Canonical,
    Native,
    NativeInSingle,
}

pub(super) fn begin(out: &mut Output<'_>, operation: Operation, _next: u32) -> bool {
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
    prepare(out);
    match operation {
        X87Operation::Load(X87Format::Float32) => {
            memory::prepare(out, address.unwrap(), 4, false, false, 52);
            out.c().local_get(52).i32_load(mem(0, 0)).local_set(BITS);
            normal_or_zero(out, BITS, 0);
            empty(out, 7);
            out.c()
                .local_get(BITS)
                .f32_reinterpret_i32()
                .f64_promote_f32()
                .local_set(double::RESULT);
            span::write(out, 0, double::RESULT);
            clear_c1(out);
        }
        X87Operation::Store {
            format: format @ (X87Format::Float32 | X87Format::Float64),
            ..
        } => {
            let native = native_store(out, format);
            if native == StoreValue::Native {
                span::load(out, 0, FA);
            } else if native == StoreValue::NativeInSingle {
                // A store can be the first consumer in this block. Read the
                // imported CW, not arithmetic scratch DOUBLE from another path.
                out.c()
                    .local_get(span::CONTROL)
                    .i32_const(0x300)
                    .i32_and()
                    .i32_eqz()
                    .if_(BlockType::Empty);
                span::load(out, 0, FA);
                out.c()
                    .local_get(FA)
                    .f32_demote_f64()
                    .i32_reinterpret_f32()
                    .i64_extend_i32_u()
                    .local_set(PACKED)
                    .local_get(STATUS)
                    .i32_const(!0x200)
                    .i32_and()
                    .local_set(STATUS)
                    .else_();
                frame::load_raw(out, 0, SIG, X);
                ieee::pack(out, SIG, X, format);
                out.c().end();
            } else {
                frame::load_raw(out, 0, SIG, X);
                ieee::pack(out, SIG, X, format);
            }
            memory::prepare(out, address.unwrap(), format.size() as u32, true, false, 52);
            out.c().local_get(52);
            if native == StoreValue::Native {
                out.c().local_get(FA);
                if format == X87Format::Float32 {
                    out.c().f32_demote_f64().f32_store(mem(0, 0));
                } else {
                    out.c().f64_store(mem(0, 0));
                }
            } else {
                out.c().local_get(PACKED);
                if format == X87Format::Float32 {
                    out.c().i32_wrap_i64().i32_store(mem(0, 0));
                } else {
                    out.c().i64_store(mem(0, 0));
                }
            }
            memory::commit(out);
            if native == StoreValue::Native {
                // An exact store overwrites C1, but a rejected store must preserve
                // the preceding completed instruction's deferred C1 recipe.
                out.c()
                    .local_get(STATUS)
                    .i32_const(!0x200)
                    .i32_and()
                    .local_set(STATUS);
            }
            span::clear_status_recipe(out);
        }
        X87Operation::LoadRegister { .. } => {
            empty(out, 7);
            // The typed step aliases the source value and renames all eight payload references.
        }
        X87Operation::StoreRegister { .. } => {}
        X87Operation::Exchange { .. } => {
            clear_c1(out);
        }
        X87Operation::ChangeSign | X87Operation::Absolute => {
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
            span::write(out, 0, FA);
            out.c().else_();
            frame::write_raw_sign(out, matches!(operation, X87Operation::Absolute));
            out.c().end();
            clear_c1(out);
        }
        X87Operation::Binary {
            kind,
            destination,
            source,
            ..
        } => {
            // Every scheduled input has been prepared before either capture.
            operand(out, destination, FA);
            operand(out, source, FB);
            arithmetic(out, kind);
            span::write(out, destination, double::RESULT);
        }
        X87Operation::BinaryMemory {
            kind,
            format: X87Format::Float32,
        } => {
            operand(out, 0, FA);
            memory::prepare(out, address.unwrap(), 4, false, false, 52);
            out.c().local_get(52).i32_load(mem(0, 0)).local_set(BITS);
            normal_or_zero(out, BITS, 0);
            out.c()
                .local_get(BITS)
                .f32_reinterpret_i32()
                .f64_promote_f32()
                .local_set(FB);
            arithmetic(out, kind);
            span::write(out, 0, double::RESULT);
        }
        _ => unreachable!(),
    }
    span::commit(out, opcode, address);
    out.c().br(1).end();
    true
}

pub(super) fn state(out: &mut Output<'_>, operation: Operation) -> bool {
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
    out.c().br(1).end();
    true
}

fn tag_recipe(out: &Output<'_>, logical: u8) -> TagRecipe {
    out.plan
        .shape(out.before_frame.expect("numeric tag has a before frame"))
        .tags[logical as usize]
}

fn occupied(out: &mut Output<'_>, logical: u8) {
    match tag_recipe(out, logical) {
        TagRecipe::Empty => {
            out.c().br(0);
        }
        TagRecipe::Entry(_) => {
            frame::tag(out, logical);
            out.c().i32_const(3).i32_eq().br_if(0);
        }
        _ => {}
    }
}

fn empty(out: &mut Output<'_>, logical: u8) {
    if out.fp_schedule.at(out.before_frame.unwrap()).empty & (1 << logical) == 0 {
        return;
    }
    match tag_recipe(out, logical) {
        TagRecipe::Empty => {}
        TagRecipe::Entry(_) => {
            frame::tag(out, logical);
            out.c().i32_const(3).i32_ne().br_if(0);
        }
        _ => {
            out.c().br(0);
        }
    }
}

fn prepare(out: &mut Output<'_>) {
    let before = out
        .before_frame
        .expect("scheduled FP instruction has a frame");
    let guards = out.fp_schedule.at(before).clone();
    if guards.enter {
        span::enter(out);
    } else {
        span::resume(out);
    }
    if guards.control {
        arithmetic_control(out);
    }
    for input in guards.inputs {
        if input.occupied {
            occupied(out, input.logical);
        }
        if input.load {
            let cell = out.plan.shape(before).cells[input.logical as usize];
            pool::ensure_cell(out, before, input.logical, cell);
        }
        if let Some(kind) = input.admit {
            pool::admit_input(out, input.logical, kind);
        }
        if input.format {
            let cell = u32::from(frame::input_cell(out, input.logical));
            span::valid(out, input.logical);
            out.c().i32_eqz().if_(BlockType::Empty);
            raw_normal_or_zero(out, cell, 1);
            out.c().end();
        }
    }
}

fn raw_normal_or_zero(out: &mut Output<'_>, cell: u32, depth: u32) {
    out.c()
        .local_get(frame::HIGH + cell)
        .i32_const(0x7fff)
        .i32_and()
        .local_set(EXP)
        .local_get(frame::LOW + cell)
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
        .br_if(depth);
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

fn to_f64(out: &mut Output<'_>, logical: u8, value: u32, depth: u32) {
    let cell = u32::from(frame::input_cell(out, logical));
    out.c()
        .local_get(frame::LOW + cell)
        .local_tee(SIG)
        .i64_const(0x7ff)
        .i64_and()
        .i64_const(0)
        .i64_ne()
        .br_if(depth)
        .local_get(frame::HIGH + cell)
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

fn native_store(out: &Output<'_>, format: X87Format) -> StoreValue {
    let before = out.before_frame.expect("store has a before frame");
    let frame = out.plan.frame(before);
    let FpValue::ExactF64(value, _) = frame.fp.payload[0] else {
        return StoreValue::Canonical;
    };
    let has = |kind| {
        out.plan.proofs_at(before).iter().any(|id| {
            let proof = out.plan.proof(*id);
            proof.value == Some(value)
                && proof.kind == kind
                && proof.epoch == frame.fp.control_epoch
                && proof.conditional.is_none()
        })
    };
    // These successful-prefix facts certify finite normal values or signed zero.
    // A conditional imported native lane alone cannot authorize an exact store.
    if !has(ProofKind::ExactF64) {
        StoreValue::Canonical
    } else if format == X87Format::Float64 || has(ProofKind::Native24) {
        StoreValue::Native
    } else if has(ProofKind::Native24InSingle) {
        StoreValue::NativeInSingle
    } else {
        StoreValue::Canonical
    }
}

pub(super) fn admit_loaded(out: &mut Output<'_>, logical: u8, kind: ProofKind, depth: u32) {
    pool::has_certificate(out, logical, kind);
    out.c().i32_eqz().if_(BlockType::Empty);
    admit_loaded_uncertified(out, logical, kind, depth + 1);
    pool::certify_input(out, logical, kind);
    out.c().end();
}

fn admit_loaded_uncertified(out: &mut Output<'_>, logical: u8, kind: ProofKind, depth: u32) {
    let cell = u32::from(frame::input_cell(out, logical));
    out.c()
        .local_get(85)
        .i32_const(1 << cell)
        .i32_and()
        .if_(BlockType::Empty)
        .local_get(76 + cell)
        .local_set(FA)
        .else_();
    raw_normal_or_zero(out, cell, depth + 1);
    to_f64(out, logical, FA, depth + 1);
    out.c()
        .local_get(FA)
        .local_set(76 + cell)
        .local_get(85)
        .i32_const(1 << cell)
        .i32_or()
        .local_set(85)
        .end();
    if kind == ProofKind::Native24InSingle {
        out.c()
            .local_get(span::CONTROL)
            .i32_const(0x300)
            .i32_and()
            .i32_eqz()
            .if_(BlockType::Empty);
    }
    if matches!(kind, ProofKind::Native24 | ProofKind::Native24InSingle) {
        let depth = depth + u32::from(kind == ProofKind::Native24InSingle);
        out.c()
            .local_get(FA)
            .i64_reinterpret_f64()
            .i64_const(0x1fff_ffff)
            .i64_and()
            .i64_eqz()
            .i32_eqz()
            .br_if(depth)
            .local_get(FA)
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
            .local_get(FA)
            .f64_const(0.0.into())
            .f64_ne()
            .i32_and()
            .br_if(depth);
    }
    if kind == ProofKind::Native24InSingle {
        out.c().end();
    }
    if matches!(
        kind,
        ProofKind::Native24 | ProofKind::Native24InSingle | ProofKind::BoundedF64
    ) {
        double::bound(out, FA, depth);
    }
}

fn operand(out: &mut Output<'_>, logical: u8, value: u32) {
    // prepare() admitted every distinct arithmetic input before either capture.
    // The execution plan owns this proof, including unknown external entries.
    span::load(out, logical, value);
}

fn clear_c1(out: &mut Output<'_>) {
    span::clear_status_recipe(out);
    out.c()
        .local_get(STATUS)
        .i32_const(!0x200)
        .i32_and()
        .local_set(STATUS);
}

fn arithmetic_control(out: &mut Output<'_>) {
    out.c()
        .local_get(pool::ADMITTED)
        .i32_const(256)
        .i32_and()
        .i32_eqz()
        .if_(BlockType::Empty);
    control_guard(out, 1);
    out.c()
        .local_get(pool::ADMITTED)
        .i32_const(256)
        .i32_or()
        .local_set(pool::ADMITTED)
        .end();
}

pub(super) fn control_guard(out: &mut Output<'_>, depth: u32) {
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
        .br_if(depth)
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
