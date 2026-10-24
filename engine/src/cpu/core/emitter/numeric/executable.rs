//! Consume typed FP values/effects. Architectural cell publication is confined
//! to recovery; successful native producers define F64 SSA values directly.
use super::super::{CTX, Output, mem};
use super::{direct, double, frame, ieee, memory, pool, span};
use crate::cpu::core::region::values::executable::fp::{
    FpEffect, FpEffectKind, FpInput, FpObligation, FpOperand, FpPayload, FpRecovery, FpStatus,
    FpValueNode, NativeDomain, StatusEffect,
};
use crate::cpu::core::region::values::executable::{
    EffectKind, ExecValueId, FpEffectClass, OrderedEffect,
};
use crate::cpu::core::{StopReason, fp_pool as p};
use crate::cpu::x86::X87Format;
use std::collections::BTreeSet;
use wasm_encoder::{BlockType, ValType};

type Availability = BTreeSet<ExecValueId>;
fn otherwise(out: &mut Output<'_>, before: &Availability) -> Availability {
    let first = out.available.clone();
    out.available = before.clone();
    out.c().else_();
    first
}
fn joined(out: &mut Output<'_>, first: Availability) {
    out.c().end();
    out.available = out.available.intersection(&first).copied().collect();
}

const BANK: u32 = 37;
const HIGH: u32 = 39;
const STATUS: u32 = 41;
const BITS: u32 = 43;
const EXP: u32 = 44;
const SIG: u32 = 46;
const PACKED: u32 = 47;
const LEFT: u32 = 48;
const RIGHT: u32 = 49;
const POINTER: u32 = 52;

fn take(out: &mut Output<'_>, value: ExecValueId, local: u32) {
    out.demand(value);
    out.c().local_set(local);
}
fn put(out: &mut Output<'_>, value: ExecValueId, local: u32) {
    out.c().local_get(local);
    out.define(value);
}

fn import_header(out: &mut Output<'_>) {
    out.c()
        .local_get(CTX)
        .i32_load(mem(super::BANK_POINTER, 2))
        .local_tee(96)
        .local_tee(BANK)
        .i32_eqz()
        .br_if(1);
    pool::initialize(out);
    for (offset, local) in [
        (p::CONTROL, 87),
        (p::STATUS, 88),
        (p::TAG, 89),
        (p::TOP, 90),
        (p::FIP, 91),
        (p::FDP, 92),
        (p::OPCODE, 93),
        (p::CS, 94),
        (p::DS, 95),
        (p::PENDING, 100),
    ] {
        out.c()
            .local_get(pool::POINTER)
            .i32_load(mem(offset, 2))
            .local_set(local);
    }
    for (offset, local) in [
        (p::PENDING_LEFT, 101),
        (p::PENDING_RIGHT, 102),
        (p::PENDING_RESULT, 103),
    ] {
        out.c()
            .local_get(pool::POINTER)
            .f64_load(mem(offset, 3))
            .local_set(local);
    }
    out.c().i32_const(1).local_set(84);
}

fn input_top(out: &mut Output<'_>, point: crate::cpu::core::region::values::ProgramPoint) {
    if point.prefix == 0 {
        let top = out.executable.block(point.block).entry.fp.top;
        out.demand(top);
    } else {
        out.c().local_get(pool::POINTER).i32_load(mem(p::TOP, 2));
    }
}

fn record(
    out: &mut Output<'_>,
    point: crate::cpu::core::region::values::ProgramPoint,
    logical: u8,
) {
    out.c().local_get(pool::POINTER);
    input_top(out, point);
    out.c()
        .i32_const(i32::from(logical))
        .i32_add()
        .i32_const(7)
        .i32_and()
        .i32_const(5)
        .i32_shl()
        .i32_add()
        .i32_const(p::RECORDS as i32)
        .i32_add()
        .local_set(HIGH);
}

pub(in crate::cpu::core::emitter) fn emit_value(out: &mut Output<'_>, node: &FpValueNode) {
    match node {
        FpValueNode::I32(value) => {
            out.c().i32_const(*value as i32);
        }
        FpValueNode::EffectResult { .. } => {
            panic!("an FP effect result must be defined by its ordered effect")
        }
        FpValueNode::PrecisionMode { control } => {
            out.demand(*control);
            out.c().i32_const(0x300).i32_and().i32_const(0x200).i32_eq();
        }
        FpValueNode::InputCertificate {
            point,
            logical,
            control,
            valid,
        } => {
            // Required external parameters are all snapshotted before any
            // effect. A later consumer must use this immutable value, never
            // the certificate of a possibly overwritten physical record.
            if point.prefix == 0 {
                out.demand(*valid);
                out.demand(*control);
                record(out, *point, *logical);
                out.c()
                    .local_get(HIGH)
                    .i32_load(mem(p::PROOF_CONTROL, 2))
                    .i32_eq()
                    .i32_and()
                    .if_(BlockType::Result(ValType::I32))
                    .local_get(HIGH)
                    .i32_load(mem(p::PROOFS, 2))
                    .i32_const(15)
                    .i32_and()
                    .else_()
                    .i32_const(0)
                    .end();
            } else {
                // A fresh same-block barrier input has no external snapshot
                // contract. Keep its genuine first-use domain admission.
                out.c().i32_const(0);
            }
        }
        FpValueNode::Classify(payload) => classify(out, *payload),
        FpValueNode::Input { point, field } => match *field {
            FpInput::Top => {
                out.c().local_get(pool::POINTER).i32_load(mem(p::TOP, 2));
            }
            FpInput::Native(logical) => {
                record(out, *point, logical);
                out.c().local_get(HIGH).f64_load(mem(p::NATIVE, 3));
            }
            FpInput::Low(logical) => {
                record(out, *point, logical);
                out.c().local_get(HIGH).i64_load(mem(p::LOW, 3));
            }
            FpInput::High(logical) => {
                record(out, *point, logical);
                out.c().local_get(HIGH).i32_load(mem(p::HIGH, 2));
            }
            FpInput::Valid(logical) => {
                record(out, *point, logical);
                out.c().local_get(HIGH).i32_load(mem(p::VALID, 2));
            }
            FpInput::Tag(logical) => {
                out.c().local_get(pool::POINTER).i32_load(mem(p::TAG, 2));
                input_top(out, *point);
                out.c()
                    .i32_const(i32::from(logical))
                    .i32_add()
                    .i32_const(7)
                    .i32_and()
                    .i32_const(1)
                    .i32_shl()
                    .i32_shr_u()
                    .i32_const(3)
                    .i32_and();
            }
            FpInput::Selector(segment) => {
                out.c()
                    .local_get(96)
                    .i32_load16_u(mem(100 + segment.index() as u64 * 2, 1));
            }
            field => {
                let (offset, wide) = match field {
                    FpInput::Control => (p::CONTROL, false),
                    FpInput::Status => (p::STATUS, false),
                    FpInput::Pending => (p::PENDING, false),
                    FpInput::PendingLeft => (p::PENDING_LEFT, true),
                    FpInput::PendingRight => (p::PENDING_RIGHT, true),
                    FpInput::PendingResult => (p::PENDING_RESULT, true),
                    FpInput::Fip => (p::FIP, false),
                    FpInput::Fdp => (p::FDP, false),
                    FpInput::Opcode => (p::OPCODE, false),
                    FpInput::Cs => (p::CS, false),
                    FpInput::Ds => (p::DS, false),
                    _ => unreachable!(),
                };
                out.c().local_get(pool::POINTER);
                if wide {
                    out.c().f64_load(mem(offset, 3));
                } else {
                    out.c().i32_load(mem(offset, 2));
                }
            }
        },
    }
}

fn raw_class(out: &mut Output<'_>, low: ExecValueId, high: ExecValueId) {
    out.demand(low);
    out.c().i64_eqz();
    out.demand(high);
    out.c()
        .i32_const(0x7fff)
        .i32_and()
        .i32_eqz()
        .i32_and()
        .if_(BlockType::Result(ValType::I32))
        .i32_const(1)
        .else_();
    out.demand(high);
    out.c()
        .i32_const(0x7fff)
        .i32_and()
        .i32_const(1)
        .i32_sub()
        .i32_const(0x7ffe)
        .i32_lt_u();
    out.demand(low);
    out.c()
        .i64_const(0)
        .i64_lt_s()
        .i32_and()
        .if_(BlockType::Result(ValType::I32))
        .i32_const(0)
        .else_()
        .i32_const(2)
        .end()
        .end();
}
fn classify(out: &mut Output<'_>, payload: FpPayload) {
    match payload {
        FpPayload::Native { value, .. } => {
            out.demand(value);
            out.c().f64_const(0.0.into()).f64_eq();
        }
        FpPayload::Opaque { low, high } => raw_class(out, low, high),
        FpPayload::Tagged {
            low,
            high,
            native,
            valid,
            ..
        } => {
            out.demand(valid);
            out.c().if_(BlockType::Result(ValType::I32));
            let before = out.available.clone();
            out.demand(native);
            out.c().f64_const(0.0.into()).f64_eq();
            let first = otherwise(out, &before);
            raw_class(out, low, high);
            joined(out, first);
        }
    }
}

fn raw_check(out: &mut Output<'_>, depth: u32) {
    out.c()
        .local_get(HIGH)
        .i32_const(0x7fff)
        .i32_and()
        .local_set(EXP)
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
fn domain_guard(
    out: &mut Output<'_>,
    value: u32,
    domain: NativeDomain,
    control: ExecValueId,
    depth: u32,
) {
    if matches!(domain, NativeDomain::Native24 | NativeDomain::InSingle) {
        if domain == NativeDomain::InSingle {
            out.demand(control);
            out.c()
                .i32_const(0x300)
                .i32_and()
                .i32_eqz()
                .if_(BlockType::Empty);
        }
        let nested = depth + u32::from(domain == NativeDomain::InSingle);
        out.c()
            .local_get(value)
            .i64_reinterpret_f64()
            .i64_const(0x1fff_ffff)
            .i64_and()
            .i64_eqz()
            .i32_eqz()
            .br_if(nested)
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
            .br_if(nested);
        if domain == NativeDomain::InSingle {
            out.c().end();
        }
    }
    if domain != NativeDomain::Exact {
        double::bound(out, value, depth);
    }
}

fn certificate_satisfies(
    out: &mut Output<'_>,
    certificate: ExecValueId,
    domain: NativeDomain,
    control: ExecValueId,
) {
    let mask = match domain {
        NativeDomain::Exact => 15,
        NativeDomain::Bounded => 14,
        NativeDomain::InSingle => 12,
        NativeDomain::Native24 => 4,
    };
    out.demand(certificate);
    out.c().i32_const(mask).i32_and().i32_eqz().i32_eqz();
    if domain == NativeDomain::Native24 {
        // The conditional single-precision proof is unconditional Native24
        // only under this same value's still-current PC24 control epoch.
        out.demand(certificate);
        out.c().i32_const(8).i32_and().i32_eqz().i32_eqz();
        out.demand(control);
        out.c()
            .i32_const(0x300)
            .i32_and()
            .i32_eqz()
            .i32_and()
            .i32_or();
    }
}

fn native(
    out: &mut Output<'_>,
    payload: FpPayload,
    destination: u32,
    domain: NativeDomain,
    control: ExecValueId,
    depth: u32,
) {
    match payload {
        FpPayload::Native {
            value,
            domain: known,
            ..
        } => {
            take(out, value, destination);
            if !known.implies(domain) {
                domain_guard(out, destination, domain, control, depth);
            }
        }
        FpPayload::Opaque { low, high } => {
            take(out, low, SIG);
            take(out, high, HIGH);
            raw_check(out, depth);
            direct::raw_to_f64(out, SIG, HIGH, destination, depth);
            domain_guard(out, destination, domain, control, depth);
        }
        FpPayload::Tagged {
            low,
            high,
            native: value,
            valid,
            certificate,
        } => {
            out.demand(valid);
            out.c().if_(BlockType::Empty);
            let before = out.available.clone();
            take(out, value, destination);
            let first = otherwise(out, &before);
            take(out, low, SIG);
            take(out, high, HIGH);
            raw_check(out, depth + 1);
            direct::raw_to_f64(out, SIG, HIGH, destination, depth + 1);
            joined(out, first);
            if domain != NativeDomain::Exact {
                certificate_satisfies(out, certificate, domain, control);
                out.c().i32_eqz().if_(BlockType::Empty);
                let before_guard = out.available.clone();
                domain_guard(out, destination, domain, control, depth + 1);
                out.c().end();
                out.available = before_guard;
            }
        }
    }
}

fn native_raw(out: &mut Output<'_>, value: ExecValueId) {
    out.demand(value);
    out.c()
        .i64_reinterpret_f64()
        .local_set(PACKED)
        .local_get(PACKED)
        .i64_const(i64::MAX)
        .i64_and()
        .i64_eqz()
        .if_(BlockType::Empty)
        .i64_const(0)
        .local_set(SIG)
        .i32_const(0)
        .local_set(HIGH)
        .else_()
        .local_get(PACKED)
        .i64_const(0x000f_ffff_ffff_ffff)
        .i64_and()
        .i64_const(0x0010_0000_0000_0000)
        .i64_or()
        .i64_const(11)
        .i64_shl()
        .local_set(SIG)
        .local_get(PACKED)
        .i64_const(52)
        .i64_shr_u()
        .i32_wrap_i64()
        .i32_const(2047)
        .i32_and()
        .i32_const(15360)
        .i32_add()
        .local_set(HIGH)
        .end()
        .local_get(HIGH)
        .local_get(PACKED)
        .i64_const(48)
        .i64_shr_u()
        .i32_wrap_i64()
        .i32_const(0x8000)
        .i32_and()
        .i32_or()
        .local_set(HIGH);
}
fn raw(out: &mut Output<'_>, payload: FpPayload) {
    match payload {
        FpPayload::Native { value, .. } => native_raw(out, value),
        FpPayload::Opaque { low, high } => {
            take(out, low, SIG);
            take(out, high, HIGH);
        }
        FpPayload::Tagged {
            low,
            high,
            native,
            valid,
            ..
        } => {
            out.demand(valid);
            out.c().if_(BlockType::Empty);
            let before = out.available.clone();
            native_raw(out, native);
            let first = otherwise(out, &before);
            take(out, low, SIG);
            take(out, high, HIGH);
            joined(out, first);
        }
    }
}

fn format_guard(out: &mut Output<'_>, payload: FpPayload, depth: u32) {
    match payload {
        FpPayload::Native { .. } => {}
        FpPayload::Opaque { low, high } => {
            take(out, low, SIG);
            take(out, high, HIGH);
            raw_check(out, depth);
        }
        FpPayload::Tagged {
            low, high, valid, ..
        } => {
            out.demand(valid);
            out.c().i32_eqz().if_(BlockType::Empty);
            let before = out.available.clone();
            take(out, low, SIG);
            take(out, high, HIGH);
            raw_check(out, depth + 1);
            out.c().end();
            out.available = before;
        }
    }
}

fn load_status(out: &mut Output<'_>, status: FpStatus) {
    for (value, local) in [
        (status.bits, STATUS),
        (status.pending, 100),
        (status.left, 101),
        (status.right, 102),
        (status.result, 103),
    ] {
        take(out, value, local);
    }
}
fn define_status(out: &mut Output<'_>, effect: &FpEffect) {
    if matches!(effect.status_effect, StatusEffect::Preserve) {
        return;
    }
    put(out, effect.status_after.bits, STATUS);
    put(out, effect.status_after.pending, 100);
    if matches!(effect.status_effect, StatusEffect::Arithmetic) {
        for (value, local) in [
            (effect.status_after.left, 101),
            (effect.status_after.right, 102),
            (effect.status_after.result, 103),
        ] {
            put(out, value, local);
        }
    }
}
fn clear_c1(out: &mut Output<'_>) {
    out.c()
        .local_get(STATUS)
        .i32_const(!0x200)
        .i32_and()
        .local_set(STATUS)
        .i32_const(0)
        .local_set(100);
}

fn arithmetic_control(out: &mut Output<'_>, depth: u32) {
    out.c()
        .local_get(pool::ADMITTED)
        .i32_const(256)
        .i32_and()
        .i32_eqz()
        .if_(BlockType::Empty);
    direct::control_guard(out, depth + 1);
    out.c()
        .local_get(pool::ADMITTED)
        .i32_const(256)
        .i32_or()
        .local_set(pool::ADMITTED)
        .end();
}

fn admit_parameter(
    out: &mut Output<'_>,
    entry: &FpRecovery,
    logical: u8,
    input: FpPayload,
    value: ExecValueId,
    domain: NativeDomain,
    depth: u32,
) {
    let point = crate::cpu::core::region::values::ProgramPoint {
        block: out.current_block.expect("FP parameter belongs to an entry"),
        prefix: 0,
    };
    // This adapter precedes every typed operation in the block. The actual
    // physical input record is authoritative, including its independent tag.
    record(out, point, logical);
    out.c()
        .local_get(HIGH)
        .i32_load(mem(p::VALID, 2))
        .local_get(HIGH)
        .i32_load(mem(p::PROOFS, 2))
        .i32_const(pool::proof_bit(domain.proof()))
        .i32_and()
        .i32_eqz()
        .i32_eqz()
        .i32_and()
        .local_get(HIGH)
        .i32_load(mem(p::PROOF_CONTROL, 2))
        .local_get(87)
        .i32_eq()
        .i32_and()
        .if_(BlockType::Empty);
    let before = out.available.clone();
    out.c()
        .local_get(HIGH)
        .f64_load(mem(p::NATIVE, 3))
        .local_set(LEFT);
    let first = otherwise(out, &before);
    native(out, input, LEFT, domain, entry.control, depth + 1);
    joined(out, first);
    put(out, value, LEFT);
}

/// The sole supported-operation consumer. It never switches on original IR
/// Operation and never publishes a producer through VALID/LOADED/token masks.
pub(in crate::cpu::core::emitter) fn emit_effect(out: &mut Output<'_>, effect: &FpEffect) {
    let before_available = out.available.clone();
    out.c().block(BlockType::Empty).block(BlockType::Empty);
    out.c().local_get(96).local_set(BANK);
    take(out, effect.control, 87);
    take(out, effect.mode, 54);
    load_status(out, effect.status_before);
    for (_, tag) in &effect.occupied {
        out.demand(*tag);
        out.c().i32_const(3).i32_eq().br_if(0);
    }
    for (_, tag) in &effect.empty {
        out.demand(*tag);
        out.c().i32_const(3).i32_ne().br_if(0);
    }
    match &effect.kind {
        FpEffectKind::Load32 { address, result } => {
            memory::prepare_value(out, address, 4, false, false, POINTER);
            out.c()
                .local_get(POINTER)
                .i32_load(mem(0, 0))
                .local_set(BITS);
            direct::normal_or_zero(out, BITS, 0);
            out.c()
                .local_get(BITS)
                .f32_reinterpret_i32()
                .f64_promote_f32();
            out.define(*result);
            clear_c1(out);
        }
        FpEffectKind::Binary {
            kind,
            left,
            right,
            result,
            exact_product,
        } => {
            // Admission can use LEFT scratch. Prepare both values before their
            // final captures so a second raw conversion never destroys LEFT.
            native(out, *left, LEFT, NativeDomain::InSingle, effect.control, 0);
            out.c().local_get(LEFT).local_set(73);
            match right {
                FpOperand::Payload(payload) => native(
                    out,
                    *payload,
                    RIGHT,
                    NativeDomain::InSingle,
                    effect.control,
                    0,
                ),
                FpOperand::Memory32(address) => {
                    memory::prepare_value(out, address, 4, false, false, POINTER);
                    out.c()
                        .local_get(POINTER)
                        .i32_load(mem(0, 0))
                        .local_set(BITS);
                    direct::normal_or_zero(out, BITS, 0);
                    out.c()
                        .local_get(BITS)
                        .f32_reinterpret_i32()
                        .f64_promote_f32()
                        .local_set(RIGHT);
                }
            }
            out.c().local_get(73).local_set(LEFT);
            direct::arithmetic_plan(out, *kind, *exact_product);
            put(out, *result, double::RESULT);
        }
        FpEffectKind::Transfer { source } => format_guard(out, *source, 0),
        FpEffectKind::Exchange { left, right } => {
            format_guard(out, *left, 0);
            format_guard(out, *right, 0);
            clear_c1(out);
        }
        FpEffectKind::Sign {
            source,
            absolute,
            result,
        } => {
            match *result {
                FpPayload::Native { value, .. } => {
                    let FpPayload::Native { value: source, .. } = *source else {
                        unreachable!()
                    };
                    out.demand(source);
                    if *absolute {
                        out.c().f64_abs();
                    } else {
                        out.c().f64_neg();
                    }
                    out.define(value);
                }
                FpPayload::Tagged {
                    low,
                    high,
                    native: result_native,
                    valid: result_valid,
                    ..
                } => {
                    let source_valid = match *source {
                        FpPayload::Tagged { valid, .. } => Some(valid),
                        _ => None,
                    };
                    if let Some(valid) = source_valid {
                        out.demand(valid);
                    } else {
                        out.c().i32_const(0);
                    }
                    out.c().local_tee(EXP);
                    out.define(result_valid);
                    out.c().local_get(EXP).if_(BlockType::Empty);
                    let branch = out.available.clone();
                    if let FpPayload::Tagged {
                        native: source_native,
                        ..
                    } = *source
                    {
                        out.demand(source_native);
                    } else {
                        out.c().f64_const(0.0.into());
                    }
                    if *absolute {
                        out.c().f64_abs();
                    } else {
                        out.c().f64_neg();
                    }
                    out.define(result_native);
                    out.c().i64_const(0);
                    out.define(low);
                    out.c().i32_const(0);
                    out.define(high);
                    let first = otherwise(out, &branch);
                    raw(out, *source);
                    raw_check(out, 1);
                    out.c().local_get(SIG);
                    out.define(low);
                    out.c().local_get(HIGH);
                    if *absolute {
                        out.c().i32_const(0x7fff).i32_and();
                    } else {
                        out.c().i32_const(0x8000).i32_xor();
                    }
                    out.define(high);
                    out.c().f64_const(0.0.into());
                    out.define(result_native);
                    joined(out, first);
                }
                _ => unreachable!(),
            }
            clear_c1(out);
        }
        FpEffectKind::Store {
            address,
            source,
            format,
        } => {
            let direct = matches!(
                source,
                FpPayload::Native {
                    domain: NativeDomain::Native24,
                    ..
                }
            ) || (*format == X87Format::Float64
                && matches!(source, FpPayload::Native { .. }));
            if direct {
                let FpPayload::Native { value, .. } = source else {
                    unreachable!()
                };
                take(out, *value, LEFT);
            } else if *format == X87Format::Float32
                && matches!(
                    source,
                    FpPayload::Native {
                        domain: NativeDomain::InSingle,
                        ..
                    }
                )
            {
                let FpPayload::Native { value, .. } = *source else {
                    unreachable!()
                };
                out.demand(effect.control);
                out.c()
                    .i32_const(0x300)
                    .i32_and()
                    .i32_eqz()
                    .if_(BlockType::Empty);
                let before = out.available.clone();
                out.demand(value);
                out.c()
                    .f32_demote_f64()
                    .i32_reinterpret_f32()
                    .i64_extend_i32_u()
                    .local_set(PACKED);
                clear_c1(out);
                let first = otherwise(out, &before);
                raw(out, *source);
                ieee::pack_at(out, SIG, HIGH, *format, 1);
                joined(out, first);
            } else {
                raw(out, *source);
                ieee::pack(out, SIG, HIGH, *format);
            }
            memory::prepare_value(out, address, format.size() as u32, true, false, POINTER);
            out.c().local_get(POINTER);
            if direct {
                out.c().local_get(LEFT);
                if *format == X87Format::Float32 {
                    out.c().f32_demote_f64().f32_store(mem(0, 0));
                } else {
                    out.c().f64_store(mem(0, 0));
                }
            } else {
                out.c().local_get(PACKED);
                if *format == X87Format::Float32 {
                    out.c().i32_wrap_i64().i32_store(mem(0, 0));
                } else {
                    out.c().i64_store(mem(0, 0));
                }
            }
            memory::commit(out);
            if direct {
                clear_c1(out);
            } else {
                out.c().i32_const(0).local_set(100);
            }
        }
    }
    if matches!(effect.status_effect, StatusEffect::ClearC1) {
        clear_c1(out);
    }
    define_status(out, effect);
    let success_available = out.available.clone();
    out.c().br(1).end();
    out.available = before_available;
    out.exit(StopReason::ExactPrefix, Some(out.pc), out.prefix);
    out.c().end();
    out.available = success_available;
}

/// External adapter. Native block parameters are validated from their complete
/// payloads; a reciprocal typed internal edge can enter with already defined SSA
/// arguments. The runtime local-view branch distinguishes those actual paths.
pub(in crate::cpu::core::emitter) fn emit_ingress(out: &mut Output<'_>, entry: &FpRecovery) {
    if out.native.is_empty() {
        return;
    }
    let block = out.current_block.expect("FP entry has a block");
    let parameters = out.executable.fp_parameters(block).clone();
    let required = parameters.roots(entry);
    let before = out.available.clone();
    let marker = out.fp_entry_marker;
    out.c()
        .block(BlockType::Empty)
        .block(BlockType::Empty)
        .local_get(marker)
        .i32_const(block.0 as i32 + 1)
        .i32_ne()
        .if_(BlockType::Empty);
    if parameters.imports {
        import_header(out);
        take(out, entry.control, 87);
        out.c()
            .local_get(87)
            .i32_const(63)
            .i32_and()
            .i32_const(63)
            .i32_ne()
            .br_if(1);
        if parameters.arithmetic {
            arithmetic_control(out, 1);
        }
        for obligation in &entry.ingress {
            if let FpObligation::Native {
                logical,
                input,
                value,
                domain,
            } = *obligation
            {
                if parameters.payloads & (1 << logical) != 0 {
                    admit_parameter(out, entry, logical, input, value, domain, 1);
                }
            }
        }
        for &value in &required {
            out.demand(value);
            out.c().drop();
        }
    } else {
        // An external scalar bridge owns no local FP view. A subsequent typed
        // edge will use the external adapter rather than read undefined args.
        out.c().i32_const(0).local_set(84);
    }
    out.c().end().i32_const(0).local_set(marker).br(1).end();
    out.available = before.clone();
    // No operation completed on admission failure; the authoritative pool must
    // stay untouched, even when the failed predicted parameter was native.
    out.c()
        .i32_const(0)
        .local_set(84)
        .i32_const(0)
        .local_set(frame::SELECTED)
        .i32_const(0)
        .local_set(pool::ADMITTED);
    out.exit(StopReason::ExactPrefix, Some(out.pc), 0);
    out.c().end();
    out.available = before;
    out.available.extend(required);
}

/// A short-quota target is rejected before its public adapter. Only an actual
/// typed predecessor marker grants its already supplied entry arguments here.
pub(in crate::cpu::core::emitter) fn recover_ingress(
    out: &mut Output<'_>,
    entry: &FpRecovery,
    block: crate::cpu::core::region::BlockId,
) {
    if out.native.is_empty() {
        return;
    }
    let parameters = out.executable.fp_parameters(block).clone();
    if !parameters.header {
        return;
    }
    let available = out.available.clone();
    let previous_frame = out.frame;
    let marker = out.fp_entry_marker;
    out.c()
        .local_get(marker)
        .i32_const(block.0 as i32 + 1)
        .i32_eq()
        .if_(BlockType::Empty);
    out.available.extend(parameters.roots(entry));
    out.frame = Some(out.plan.block(block).entry);
    recover(out, entry);
    out.c().end();
    out.frame = previous_frame;
    out.available = available;
}

fn stage_payload(
    out: &mut Output<'_>,
    source: FpPayload,
    target: FpPayload,
    control: ExecValueId,
    targets: &mut Vec<ExecValueId>,
    depth: u32,
) {
    match target {
        FpPayload::Native { value, domain, .. } => {
            native(out, source, LEFT, domain, control, depth);
            out.c().local_get(LEFT);
            targets.push(value);
        }
        FpPayload::Tagged {
            low,
            high,
            native: target_native,
            valid: target_valid,
            certificate: target_certificate,
        } => match source {
            FpPayload::Native { value, domain, .. } => {
                out.c().i64_const(0).i32_const(0);
                out.demand(value);
                out.c()
                    .i32_const(1)
                    .i32_const(domain.certificate_bits() as i32);
                targets.extend([low, high, target_native, target_valid, target_certificate]);
            }
            FpPayload::Opaque {
                low: source_low,
                high: source_high,
            } => {
                out.demand(source_low);
                out.demand(source_high);
                out.c().f64_const(0.0.into()).i32_const(0).i32_const(0);
                targets.extend([low, high, target_native, target_valid, target_certificate]);
            }
            FpPayload::Tagged {
                low: source_low,
                high: source_high,
                native: source_native,
                valid: source_valid,
                certificate: source_certificate,
            } => {
                out.demand(source_low);
                out.demand(source_high);
                out.demand(source_native);
                out.demand(source_valid);
                out.demand(source_certificate);
                targets.extend([low, high, target_native, target_valid, target_certificate]);
            }
        },
        FpPayload::Opaque { low, high } => {
            raw(out, source);
            out.c().local_get(SIG).local_get(HIGH);
            targets.extend([low, high]);
        }
    }
}

fn transfer_tag_word(out: &mut Output<'_>, source: &FpRecovery) {
    for logical in 0..8 {
        if source.tag_written & (1 << logical) == 0 {
            continue;
        }
        out.demand(source.top);
        out.c()
            .i32_const(i32::from(source.top_delta) + logical as i32)
            .i32_add()
            .i32_const(7)
            .i32_and()
            .i32_const(1)
            .i32_shl()
            .local_set(38)
            .local_get(89)
            .i32_const(3)
            .local_get(38)
            .i32_shl()
            .i32_const(-1)
            .i32_xor()
            .i32_and();
        out.demand(source.tags[logical]);
        out.c().local_get(38).i32_shl().i32_or().local_set(89);
    }
}

pub(in crate::cpu::core::emitter) fn emit_edge(
    out: &mut Output<'_>,
    source: &FpRecovery,
    target: &FpRecovery,
    target_block: crate::cpu::core::region::BlockId,
) {
    if out.native.is_empty() {
        return;
    }
    let before = out.available.clone();
    let marker = out.fp_entry_marker;
    let parameters = out.executable.fp_parameters(target_block).clone();
    out.c()
        .block(BlockType::Empty)
        .block(BlockType::Empty)
        .i32_const(0)
        .local_set(marker)
        .local_get(84)
        .if_(BlockType::Empty);
    let mut targets = Vec::new();
    if parameters.header {
        take(out, source.control, 87);
        // ACTIVE was granted only by an actual masked-CW adapter. Neither a
        // typed producer nor an internal edge changes that same control epoch.
        if parameters.arithmetic {
            arithmetic_control(out, 1);
        }
        for logical in 0..8 {
            if parameters.payloads & (1 << logical) != 0 {
                stage_payload(
                    out,
                    source.payload[logical],
                    target.payload[logical],
                    source.control,
                    &mut targets,
                    1,
                );
            }
            if parameters.tags & (1 << logical) != 0 {
                out.demand(source.tags[logical]);
                targets.push(target.tags[logical]);
            }
        }
        for (from, to) in [
            (source.control, target.control),
            (source.mode, target.mode),
            (source.status.bits, target.status.bits),
            (source.status.pending, target.status.pending),
            (source.status.left, target.status.left),
            (source.status.right, target.status.right),
            (source.status.result, target.status.result),
            (source.metadata.fip, target.metadata.fip),
            (source.metadata.fdp, target.metadata.fdp),
            (source.metadata.opcode, target.metadata.opcode),
            (source.metadata.cs, target.metadata.cs),
            (source.metadata.ds, target.metadata.ds),
        ] {
            out.demand(from);
            targets.push(to);
        }
        out.demand(source.top);
        out.c()
            .i32_const(i32::from(source.top_delta))
            .i32_add()
            .i32_const(7)
            .i32_and();
        targets.push(target.top);
        // Read the source tag view before a self-edge replaces its parameters.
        transfer_tag_word(out, source);
        // Parallel assignment also covers self-loop FXCH cycles and aliases.
        for target in targets.into_iter().rev() {
            out.define(target);
        }
    }
    let delta = i32::from(source.top_delta).rem_euclid(8);
    if delta != 0 {
        out.c()
            .local_get(86)
            .i32_const(delta)
            .i32_shr_u()
            .local_get(86)
            .i32_const(8 - delta)
            .i32_shl()
            .i32_or()
            .i32_const(255)
            .i32_and();
    } else {
        out.c().local_get(86);
    }
    out.c()
        .i32_const(i32::from(source.written))
        .i32_or()
        .local_set(86)
        .i32_const(-1)
        .local_set(frame::SELECTED)
        .i32_const(target_block.0 as i32 + 1)
        .local_set(marker)
        .end()
        .br(1)
        .end();
    out.available = before.clone();
    // Source COUNT has already completed at this point. Recover the source
    // state, then execute the target's current instruction exactly once.
    let source_frame = out.plan.block(out.current_block.unwrap()).exit;
    out.frame = Some(source_frame);
    recover(out, source);
    frame::emit_frame(out, source_frame);
    let pc = match out.plan.frame(out.plan.block(target_block).entry).pc {
        crate::cpu::core::region::ResumePc::Exact(pc) => pc,
        _ => unreachable!(),
    };
    out.exit(StopReason::ExactPrefix, Some(pc), 0);
    out.c().end();
    out.available = before;
}

/// Side-exit/publication projection only. Known native lanes get one constant
/// validity mask here, rather than generic bookkeeping after every producer.
pub(in crate::cpu::core::emitter) fn recover(out: &mut Output<'_>, recovery: &FpRecovery) {
    if out.native.is_empty() {
        return;
    }
    let available = out.available.clone();
    out.c().local_get(84).if_(BlockType::Empty);
    let shape = out
        .plan
        .shape(out.frame.expect("FP recovery has an architectural frame"))
        .clone();
    let mut certificates = 0i64;
    for logical in 0..8 {
        if let FpPayload::Native { domain, .. } = recovery.payload[logical] {
            let bits = pool::FORMAT_TOKEN | i64::from(domain.certificate_bits());
            certificates |= bits << (u32::from(shape.cells[logical].0) * pool::TOKEN_BITS);
        }
    }
    // Actual domains attach to the full projected value and actual CW only at
    // this publication boundary. Native producers need no runtime token writes.
    out.c()
        .i64_const(certificates)
        .local_set(super::super::FP_PAYLOAD_TOKENS);
    out.c().i32_const(0).local_set(85);
    let delta = i32::from(recovery.top_delta).rem_euclid(8);
    if delta != 0 {
        out.c()
            .local_get(86)
            .i32_const(delta)
            .i32_shr_u()
            .local_get(86)
            .i32_const(8 - delta)
            .i32_shl()
            .i32_or()
            .i32_const(255)
            .i32_and();
    } else {
        out.c().local_get(86);
    }
    out.c()
        .i32_const(i32::from(recovery.written))
        .i32_or()
        .local_set(158);
    let mut seen = 0u8;
    for logical in 0..8 {
        let cell = shape.cells[logical].0;
        if seen & (1 << cell) != 0 {
            continue;
        }
        seen |= 1 << cell;
        let aliases = (0..8)
            .filter(|&i| shape.cells[i].0 == cell)
            .fold(0u8, |mask, i| mask | (1 << i));
        for alias in 0..8 {
            if aliases & (1 << alias) != 0 {
                debug_assert_eq!(recovery.payload[logical], recovery.payload[alias]);
            }
        }
        out.c()
            .local_get(158)
            .i32_const(i32::from(aliases))
            .i32_and()
            .if_(BlockType::Empty);
        let before = out.available.clone();
        match recovery.payload[logical] {
            FpPayload::Native { value, .. } => {
                take(out, value, 76 + u32::from(cell));
                out.c()
                    .local_get(85)
                    .i32_const(1 << cell)
                    .i32_or()
                    .local_set(85);
            }
            FpPayload::Opaque { low, high } => {
                take(out, low, frame::LOW + u32::from(cell));
                take(out, high, frame::HIGH + u32::from(cell));
            }
            FpPayload::Tagged {
                low,
                high,
                native,
                valid,
                ..
            } => {
                out.demand(valid);
                out.c().if_(BlockType::Empty);
                let branch = out.available.clone();
                take(out, native, 76 + u32::from(cell));
                out.c()
                    .local_get(85)
                    .i32_const(1 << cell)
                    .i32_or()
                    .local_set(85);
                let first = otherwise(out, &branch);
                take(out, low, frame::LOW + u32::from(cell));
                take(out, high, frame::HIGH + u32::from(cell));
                joined(out, first);
            }
        }
        out.c().end();
        out.available = before;
    }
    for (value, local) in [
        (recovery.control, 87),
        (recovery.status.bits, 88),
        (recovery.top, 90),
        (recovery.metadata.fip, 91),
        (recovery.metadata.fdp, 92),
        (recovery.metadata.opcode, 93),
        (recovery.metadata.cs, 94),
        (recovery.metadata.ds, 95),
        (recovery.status.pending, 100),
        (recovery.status.left, 101),
        (recovery.status.right, 102),
        (recovery.status.result, 103),
    ] {
        take(out, value, local);
    }
    // The existing exact FrameState descriptor owns tag/TOP/ghost publication.
    out.c().end();
    out.available = available;
}

/// Exact kernels that can observe/canonicalize x87 state receive the real
/// completed SSA prefix. Ordinary scalar kernels never touch an FP view.
pub(in crate::cpu::core::emitter) fn prepare_exact(out: &mut Output<'_>, effect: &OrderedEffect) {
    if out.native.is_empty() || !matches!(effect.kind, EffectKind::Exact(_)) {
        return;
    }
    if !matches!(effect.contract.fp, FpEffectClass::Preserve)
        || effect
            .operation
            .as_ref()
            .is_some_and(super::super::is_native)
    {
        recover(out, &effect.before.fp);
        frame::emit_frame(out, effect.before.frame);
    }
}

pub(in crate::cpu::core::emitter) fn finish_exact(out: &mut Output<'_>, effect: &OrderedEffect) {
    if out.native.is_empty() || !matches!(effect.kind, EffectKind::Exact(_)) {
        return;
    }
    if matches!(effect.contract.fp, FpEffectClass::Observe) {
        for (value, local) in [
            (effect.after.fp.status.bits, 88),
            (effect.after.fp.status.pending, 100),
            (effect.after.fp.status.left, 101),
            (effect.after.fp.status.right, 102),
            (effect.after.fp.status.result, 103),
        ] {
            put(out, value, local);
        }
    } else if matches!(effect.contract.fp, FpEffectClass::Barrier) {
        // The actual canonical kernel is now authoritative. Later FP inputs
        // are distinct post-effect nodes, never inherited proof parameters.
        let marker = out.fp_entry_marker;
        out.c()
            .i32_const(0)
            .local_set(84)
            .i32_const(0)
            .local_set(frame::SELECTED)
            .i32_const(0)
            .local_set(marker)
            .i32_const(0)
            .local_set(pool::ADMITTED);
    }
}
