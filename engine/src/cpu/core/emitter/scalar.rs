use super::{CARRY_IN, FLAGS, FULL_BLOCK, LEFT, Output, RIGHT, VALUE};
use crate::cpu::core::{
    flags::{self, FlagKind},
    region::values::scalar::{FlagRecipe, ReadAvailability},
};
use crate::cpu::x86::ir::Condition;
use wasm_encoder::{BlockType, ValType};

const CACHE_BASE: u32 = 172;

fn recipe(out: &Output<'_>) -> Option<FlagRecipe> {
    out.before_frame
        .and_then(|frame| out.scalar_plan.recipe(frame))
}

pub(super) fn try_condition(out: &mut Output<'_>, condition: Condition) -> bool {
    let Some(recipe) = recipe(out) else {
        return false;
    };
    out.c()
        .local_get(FULL_BLOCK)
        .if_(BlockType::Result(ValType::I32));
    emit_condition(out, recipe, condition);
    out.c().else_();
    out.condition_fallback(condition);
    out.c().end();
    true
}

pub(super) fn try_materialize(out: &mut Output<'_>, mask: u32) -> bool {
    let Some(recipe) = recipe(out) else {
        return false;
    };
    out.c().local_get(FULL_BLOCK).if_(BlockType::Empty);
    let written = recipe.written() & mask;
    out.c()
        .local_get(FLAGS)
        .i32_const(!written as i32)
        .i32_and();
    for bit in [
        flags::CF,
        flags::PF,
        flags::AF,
        flags::ZF,
        flags::SF,
        flags::OF,
    ] {
        if written & bit != 0 {
            emit_flag(out, recipe, bit);
            if bit != flags::CF {
                out.c().i32_const(bit.trailing_zeros() as i32).i32_shl();
            }
            out.c().i32_or();
        }
    }
    out.c().i32_const(2).i32_or().local_set(FLAGS).else_();
    out.materialize_fallback(mask);
    out.c().end();
    true
}

fn operand(out: &mut Output<'_>, recipe: FlagRecipe, left: bool) {
    if left && recipe.kind == FlagKind::Neg {
        out.c().i32_const(0);
    } else if !left && matches!(recipe.kind, FlagKind::Inc | FlagKind::Dec) {
        out.c().i32_const(1);
    } else {
        let local = if left || recipe.kind == FlagKind::Neg {
            LEFT
        } else {
            RIGHT
        };
        out.c().local_get(local);
        narrow(out, recipe.width);
    }
}

fn result(out: &mut Output<'_>, recipe: FlagRecipe) {
    out.c().local_get(VALUE);
    narrow(out, recipe.width);
}

fn narrow(out: &mut Output<'_>, width: u32) {
    if width != 32 {
        out.c()
            .i32_const((u32::MAX >> (32 - width)) as i32)
            .i32_and();
    }
}

// Each source is either this literal writer's immutable operand tuple or the
// materialized FLAGS bit preserved by INC/DEC. No runtime kind/width decoding.
fn emit_flag(out: &mut Output<'_>, recipe: FlagRecipe, bit: u32) {
    if recipe.written() & bit == 0 {
        out.c().local_get(FLAGS).i32_const(bit as i32).i32_and();
        if bit != flags::CF {
            out.c().i32_const(bit.trailing_zeros() as i32).i32_shr_u();
        }
        return;
    }
    if recipe.kind == FlagKind::Logical && matches!(bit, flags::CF | flags::AF | flags::OF) {
        out.c().i32_const(0);
        return;
    }
    match bit {
        flags::CF => {
            if matches!(recipe.kind, FlagKind::Add | FlagKind::Adc) {
                operand(out, recipe, true);
                out.c().i64_extend_i32_u();
                operand(out, recipe, false);
                out.c().i64_extend_i32_u().i64_add();
                if recipe.kind == FlagKind::Adc {
                    out.c()
                        .local_get(CARRY_IN)
                        .i32_const(1)
                        .i32_and()
                        .i64_extend_i32_u()
                        .i64_add();
                }
                out.c()
                    .i64_const(i64::from(u32::MAX >> (32 - recipe.width)))
                    .i64_gt_u();
            } else {
                operand(out, recipe, false);
                out.c().i64_extend_i32_u();
                if recipe.kind == FlagKind::Sbb {
                    out.c()
                        .local_get(CARRY_IN)
                        .i32_const(1)
                        .i32_and()
                        .i64_extend_i32_u()
                        .i64_add();
                }
                operand(out, recipe, true);
                out.c().i64_extend_i32_u().i64_gt_u();
            }
        }
        flags::PF => {
            result(out, recipe);
            out.c()
                .i32_const(0xff)
                .i32_and()
                .i32_popcnt()
                .i32_const(1)
                .i32_and()
                .i32_eqz();
        }
        flags::AF => {
            operand(out, recipe, true);
            operand(out, recipe, false);
            out.c().i32_xor();
            result(out, recipe);
            out.c()
                .i32_xor()
                .i32_const(4)
                .i32_shr_u()
                .i32_const(1)
                .i32_and();
        }
        flags::ZF => {
            result(out, recipe);
            out.c().i32_eqz();
        }
        flags::SF => {
            result(out, recipe);
            out.c()
                .i32_const((recipe.width - 1) as i32)
                .i32_shr_u()
                .i32_const(1)
                .i32_and();
        }
        flags::OF => {
            operand(out, recipe, true);
            operand(out, recipe, false);
            out.c().i32_xor();
            if matches!(recipe.kind, FlagKind::Add | FlagKind::Adc | FlagKind::Inc) {
                out.c().i32_const(-1).i32_xor();
            }
            operand(out, recipe, true);
            result(out, recipe);
            out.c()
                .i32_xor()
                .i32_and()
                .i32_const((recipe.width - 1) as i32)
                .i32_shr_u()
                .i32_const(1)
                .i32_and();
        }
        _ => unreachable!("arithmetic flag"),
    }
}

fn emit_condition(out: &mut Output<'_>, recipe: FlagRecipe, condition: Condition) {
    if (recipe.kind == FlagKind::Sub && subtraction_condition(out, recipe, condition))
        || (recipe.kind == FlagKind::Logical && logical_condition(out, recipe, condition))
    {
        return;
    }
    match condition {
        Condition::Overflow | Condition::NotOverflow => emit_flag(out, recipe, flags::OF),
        Condition::Below | Condition::AboveOrEqual => emit_flag(out, recipe, flags::CF),
        Condition::Equal | Condition::NotEqual => emit_flag(out, recipe, flags::ZF),
        Condition::Sign | Condition::NotSign => emit_flag(out, recipe, flags::SF),
        Condition::Parity | Condition::NotParity => emit_flag(out, recipe, flags::PF),
        Condition::BelowOrEqual | Condition::Above => {
            emit_flag(out, recipe, flags::CF);
            emit_flag(out, recipe, flags::ZF);
            out.c().i32_or();
        }
        Condition::Less
        | Condition::GreaterOrEqual
        | Condition::LessOrEqual
        | Condition::Greater => {
            emit_flag(out, recipe, flags::SF);
            emit_flag(out, recipe, flags::OF);
            out.c().i32_xor();
            if matches!(condition, Condition::LessOrEqual | Condition::Greater) {
                emit_flag(out, recipe, flags::ZF);
                out.c().i32_or();
            }
        }
    }
    if matches!(
        condition,
        Condition::NotOverflow
            | Condition::AboveOrEqual
            | Condition::NotEqual
            | Condition::Above
            | Condition::NotSign
            | Condition::NotParity
            | Condition::GreaterOrEqual
            | Condition::Greater
    ) {
        out.c().i32_eqz();
    }
}

fn sign_extend(out: &mut Output<'_>, width: u32) {
    if width != 32 {
        let shift = (32 - width) as i32;
        out.c()
            .i32_const(shift)
            .i32_shl()
            .i32_const(shift)
            .i32_shr_s();
    }
}

fn subtraction_condition(out: &mut Output<'_>, recipe: FlagRecipe, condition: Condition) -> bool {
    // Plain subtraction's CF is unsigned lhs<rhs, ZF is lhs==rhs, and
    // SF xor OF is signed lhs<rhs, including the signed-overflow cases.
    let (signed, inverse) = match condition {
        Condition::Below | Condition::Equal | Condition::Above => (false, false),
        Condition::AboveOrEqual | Condition::NotEqual | Condition::BelowOrEqual => (false, true),
        Condition::Less | Condition::Greater => (true, false),
        Condition::GreaterOrEqual | Condition::LessOrEqual => (true, true),
        _ => return false,
    };
    operand(out, recipe, true);
    if signed {
        sign_extend(out, recipe.width);
    }
    operand(out, recipe, false);
    if signed {
        sign_extend(out, recipe.width);
    }
    match condition {
        Condition::Below | Condition::AboveOrEqual => {
            out.c().i32_lt_u();
        }
        Condition::Equal | Condition::NotEqual => {
            out.c().i32_eq();
        }
        Condition::BelowOrEqual | Condition::Above => {
            out.c().i32_gt_u();
        }
        Condition::Less | Condition::GreaterOrEqual => {
            out.c().i32_lt_s();
        }
        Condition::LessOrEqual | Condition::Greater => {
            out.c().i32_gt_s();
        }
        _ => unreachable!("selected subtraction comparison"),
    }
    if inverse {
        out.c().i32_eqz();
    }
    true
}

fn logical_condition(out: &mut Output<'_>, recipe: FlagRecipe, condition: Condition) -> bool {
    // Logical writers clear CF/OF. Their combined predicates therefore depend
    // only on the width-correct result, rather than two reconstructed flags.
    let inverse = match condition {
        Condition::Overflow | Condition::Below => {
            out.c().i32_const(0);
            return true;
        }
        Condition::NotOverflow | Condition::AboveOrEqual => {
            out.c().i32_const(1);
            return true;
        }
        Condition::Equal | Condition::BelowOrEqual => false,
        Condition::NotEqual | Condition::Above => true,
        Condition::Sign | Condition::Less | Condition::Greater => false,
        Condition::NotSign | Condition::GreaterOrEqual | Condition::LessOrEqual => true,
        _ => return false,
    };
    result(out, recipe);
    match condition {
        Condition::Equal | Condition::NotEqual | Condition::BelowOrEqual | Condition::Above => {
            out.c().i32_eqz();
        }
        _ => {
            sign_extend(out, recipe.width);
            out.c().i32_const(0);
            if matches!(condition, Condition::LessOrEqual | Condition::Greater) {
                out.c().i32_gt_s();
            } else {
                out.c().i32_lt_s();
            }
        }
    }
    if inverse {
        out.c().i32_eqz();
    }
    true
}

fn read(out: &Output<'_>, width: u32, prepare: bool) -> Option<ReadAvailability> {
    if prepare || !matches!(width, 1 | 2 | 4) || out.before_frame.is_none() {
        return None;
    }
    out.scalar_plan.read(
        out.current_block?,
        out.prefix,
        out.address_fact?.address,
        width,
    )
}

pub(super) fn reusable_read(out: &Output<'_>, width: u32, prepare: bool) -> Option<u32> {
    read(out, width, prepare)
        .filter(|read| !read.first)
        .map(|read| CACHE_BASE + read.slot)
}

pub(super) fn capture_read(out: &mut Output<'_>, width: u32, destination: u32, prepare: bool) {
    if let Some(read) = read(out, width, prepare) {
        out.c()
            .local_get(destination)
            .local_set(CACHE_BASE + read.slot);
    }
}
