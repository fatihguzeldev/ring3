//! unrolled recovery for owners whose exact encoded representation is smaller.
use super::{
    Locals, State,
    program::{Color, SourceOp},
};
use crate::cpu::core::{
    emitter::{BlockType, Output},
    region::values::{ValueType, executable::Binary32},
};

fn mark(out: &mut Output<'_>, l: Locals, color: Color, marker: u32) {
    let tag = l.need(color.local);
    out.c()
        .local_get(tag)
        .i32_const(-(marker as i32))
        .i32_ne()
        .if_(BlockType::Empty)
        .i32_const(marker as i32)
        .local_set(tag)
        .end();
}

pub(super) fn emit(out: &mut Output<'_>, state: &State) {
    let l = state.locals;
    for (id, source) in state.program.reverse_refs() {
        let pending: Vec<_> = source
            .op
            .dependencies()
            .into_iter()
            .filter(|input| !matches!(state.program.get(*input).op, SourceOp::Available))
            .collect();
        // available dependencies already occupy their proven colors. with no
        // pending dependency there is no propagation or other work to guard.
        if pending.is_empty() {
            continue;
        }
        out.c()
            .local_get(l.need(source.color.local))
            .i32_const(id.0 as i32 + 1)
            .i32_eq()
            .if_(BlockType::Empty);
        for input in pending {
            let dependency = state.program.get(input);
            mark(out, l, dependency.color, input.0 + 1);
        }
        out.c().end();
    }
    super::leaves::emit(out, state);
    for (index, source) in state.program.refs().iter().enumerate() {
        if matches!(source.op, SourceOp::Available) || super::leaves::eligible(source) {
            continue;
        }
        let marker = index as i32 + 1;
        out.c()
            .local_get(l.need(source.color.local))
            .i32_const(marker)
            .i32_eq()
            .if_(BlockType::Empty);
        match &source.op {
            SourceOp::Available => {}
            SourceOp::PhysicalInput { local } => {
                out.c().local_get(*local).local_set(source.color.local);
            }
            SourceOp::Constant { bits } => {
                match source.color.ty {
                    ValueType::I32 => {
                        out.c().i32_const(*bits as i32);
                    }
                    ValueType::I64 => {
                        out.c().i64_const(*bits as i64);
                    }
                    ValueType::F64 => {
                        out.c().f64_const(f64::from_bits(*bits).into());
                    }
                }
                out.c().local_set(source.color.local);
            }
            SourceOp::Binary32 { kind, left, right } => {
                for id in [left, right] {
                    out.c().local_get(state.program.get(*id).color.local);
                }
                match kind {
                    Binary32::Add => {
                        out.c().i32_add();
                    }
                    Binary32::Sub => {
                        out.c().i32_sub();
                    }
                    Binary32::And => {
                        out.c().i32_and();
                    }
                    Binary32::Or => {
                        out.c().i32_or();
                    }
                    Binary32::Xor => {
                        out.c().i32_xor();
                    }
                }
                out.c().local_set(source.color.local);
            }
            SourceOp::ShiftLeft32 { value, amount } => {
                out.c()
                    .local_get(state.program.get(*value).color.local)
                    .i32_const(i32::from(*amount))
                    .i32_shl()
                    .local_set(source.color.local);
            }
            SourceOp::Fp(source_op) => {
                super::fp::emit_source(out, source_op, &state.program);
                out.c().local_set(source.color.local);
            }
        }
        out.c()
            .i32_const(-marker)
            .local_set(l.need(source.color.local))
            .end();
    }
}
