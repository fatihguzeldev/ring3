//! A dense immutable map for dependency-free I32 recovery leaves. A NeedTag
//! selects the exact source reference, never just the color it happens to use.
use super::{
    Locals, State,
    program::{Color, Program, SourceOp, SourceRef},
};
use crate::cpu::core::{
    emitter::{BlockType, Output, mem},
    region::values::{ValueType, executable::fp::FpInput},
};
use std::collections::BTreeSet;
use wasm_encoder::ValType;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Leaf {
    Constant(u32),
    Selector(u32),
}

impl Leaf {
    fn word(self) -> i64 {
        let (kind, value) = match self {
            Self::Constant(value) => (1u64, value),
            Self::Selector(segment) => (2u64, segment),
        };
        ((kind << 32) | u64::from(value)) as i64
    }
}

fn classify(source: &SourceRef) -> Option<Leaf> {
    if source.color.ty != ValueType::I32 {
        return None;
    }
    match source.op {
        SourceOp::Constant { bits } => Some(Leaf::Constant(bits as u32)),
        SourceOp::Fp(super::fp::Source::Input {
            field: FpInput::Selector(segment),
            top: None,
        }) => Some(Leaf::Selector(segment.index() as u32)),
        _ => None,
    }
}

pub(super) fn eligible(source: &SourceRef) -> bool {
    classify(source).is_some()
}

pub(super) fn append(program: &Program, words: &mut Vec<i64>) -> u32 {
    let base = u32::try_from(words.len()).expect("bounded immutable leaf map");
    base.checked_add(u32::try_from(program.refs().len()).expect("bounded immutable leaf count"))
        .expect("bounded immutable leaf map end");
    words.extend(
        program
            .refs()
            .iter()
            .map(|source| classify(source).map_or(0, Leaf::word)),
    );
    base
}

fn color(out: &mut Output<'_>, state: &State, color: Color, l: Locals) {
    let tag = l.need(color.local);
    let getter = super::super::support::owner_defined(9, !out.native.is_empty());
    out.c()
        .local_get(tag)
        .i32_const(0)
        .i32_gt_s()
        .if_(BlockType::Empty)
        .local_get(tag)
        .i32_const(1)
        .i32_sub()
        .i32_const(state.leaf_base as i32)
        .i32_add()
        .call(getter)
        .local_tee(l.wide(0))
        .i64_const(32)
        .i64_shr_u()
        .i32_wrap_i64()
        .local_tee(l.at(15))
        .if_(BlockType::Empty)
        .local_get(l.at(15))
        .i32_const(2)
        .i32_eq()
        .if_(BlockType::Result(ValType::I32))
        .local_get(96)
        .local_get(l.wide(0))
        .i32_wrap_i64()
        .i32_const(1)
        .i32_shl()
        .i32_add()
        .i32_load16_u(mem(100, 1))
        .else_()
        .local_get(l.wide(0))
        .i32_wrap_i64()
        .end()
        .local_set(color.local)
        .local_get(tag)
        .i32_const(-1)
        .i32_mul()
        .local_set(tag)
        .end()
        .end();
}

pub(super) fn emit(out: &mut Output<'_>, state: &State) {
    let eligible: BTreeSet<_> = state
        .program
        .refs()
        .iter()
        .filter(|source| eligible(source))
        .map(|source| source.color.local)
        .collect();
    for candidate in state
        .program
        .colors()
        .filter(|color| eligible.contains(&color.local))
    {
        color(out, state, candidate, state.locals);
    }
}

#[cfg(test)]
mod tests {
    use super::super::program::{RefId, ValueKey};
    use super::*;
    use crate::cpu::{core::region::values::executable::ExecValueId, x86::SegmentRegister};

    fn source(value: usize, local: u32, ty: ValueType, op: SourceOp) -> SourceRef {
        SourceRef {
            key: ValueKey::Executable(ExecValueId(value)),
            color: Color { local, ty },
            op,
        }
    }
    fn selector(segment: SegmentRegister, top: Option<RefId>) -> SourceOp {
        SourceOp::Fp(super::super::fp::Source::Input {
            field: FpInput::Selector(segment),
            top,
        })
    }

    #[test]
    fn classification_is_restricted_to_exact_dependency_free_i32_leaves() {
        assert_eq!(
            classify(&source(
                0,
                200,
                ValueType::I32,
                SourceOp::Constant { bits: 0xffff_ffff }
            )),
            Some(Leaf::Constant(u32::MAX))
        );
        assert_eq!(
            classify(&source(
                0,
                200,
                ValueType::I32,
                selector(SegmentRegister::Fs, None)
            )),
            Some(Leaf::Selector(SegmentRegister::Fs.index() as u32))
        );
        for op in [
            SourceOp::Available,
            SourceOp::PhysicalInput { local: 2 },
            selector(SegmentRegister::Cs, Some(RefId(0))),
            SourceOp::Fp(super::super::fp::Source::Input {
                field: FpInput::Control,
                top: None,
            }),
            SourceOp::ShiftLeft32 {
                value: RefId(0),
                amount: 1,
            },
        ] {
            assert!(!eligible(&source(0, 200, ValueType::I32, op)));
        }
        for ty in [ValueType::I64, ValueType::F64] {
            assert!(!eligible(&source(
                0,
                200,
                ty,
                SourceOp::Constant { bits: 7 }
            )));
        }
    }

    #[test]
    fn dense_map_keeps_ref_identity_and_zero_entries_across_shared_color_source_modes() {
        let mut program = Program::new(16);
        let literal = program
            .intern(source(
                0,
                200,
                ValueType::I32,
                SourceOp::Constant { bits: 0x8000_0001 },
            ))
            .unwrap();
        let captured = program
            .intern(source(1, 200, ValueType::I32, SourceOp::Available))
            .unwrap();
        let physical = program
            .intern(source(
                2,
                200,
                ValueType::I32,
                SourceOp::PhysicalInput { local: 2 },
            ))
            .unwrap();
        let cs = program
            .intern(source(
                3,
                200,
                ValueType::I32,
                selector(SegmentRegister::Cs, None),
            ))
            .unwrap();
        let same = program
            .intern(source(
                999,
                200,
                ValueType::I32,
                SourceOp::Constant { bits: 0x8000_0001 },
            ))
            .unwrap();
        assert_eq!(same, literal);
        let mut words = vec![99, 100];
        let base = append(&program, &mut words) as usize;
        assert_eq!(base, 2);
        assert_eq!(&words[..base], &[99, 100]);
        assert_eq!(words.len(), base + program.refs().len());
        assert_eq!(words[base + literal.0 as usize], (1i64 << 32) | 0x8000_0001);
        assert_eq!(words[base + captured.0 as usize], 0);
        assert_eq!(words[base + physical.0 as usize], 0);
        assert_eq!(
            words[base + cs.0 as usize],
            (2i64 << 32) | SegmentRegister::Cs.index() as i64
        );
        assert!(program.validate_roots(&[literal]).is_ok());
        assert!(program.validate_roots(&[cs]).is_ok());
        assert!(
            program.validate_roots(&[literal, cs]).is_err(),
            "only a validated one-value closure may choose the shared physical color"
        );
    }
}
