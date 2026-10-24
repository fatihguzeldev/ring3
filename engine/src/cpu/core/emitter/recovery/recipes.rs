//! Immutable source operations for the bounded, shared recovery VM.
//!
//! RefIds remain the instruction addresses of this program. In particular, two
//! values that reuse a local at different recovery sites must not be merged by
//! their encoded color or operation. Request-closure validation stays in Program.

use super::{
    fp::{Payload, Source},
    program::{Program, RefId, SourceOp, SourceRef},
};
use crate::cpu::core::region::values::{
    ValueType,
    executable::{Binary32, fp::FpInput},
};

#[cfg(test)]
mod tests;

pub(super) const WORDS: u32 = 4;
pub(super) const TYPE_I32: u32 = 0;
pub(super) const TYPE_I64: u32 = 1;
pub(super) const TYPE_F64: u32 = 2;
pub(super) const KIND_MASK: u32 = 0xff;
pub(super) const TYPE_SHIFT: u32 = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub(super) enum Kind {
    Available = 0,
    PhysicalInput = 1,
    Constant = 2,
    Add = 3,
    Sub = 4,
    And = 5,
    Or = 6,
    Xor = 7,
    ShiftLeft32 = 8,
    FpInput = 9,
    Precision = 10,
    Certificate = 11,
    ClassNative = 12,
    ClassRaw = 13,
    ClassTagged = 14,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Recipes {
    pub base: u32,
    pub count: u32,
    pub kinds: u32,
}

/// Append one four-word instruction per immutable SourceRef. Validate the whole
/// program first, so an invalid source cannot leave a partially appended table.
pub(super) fn append(program: &Program, words: &mut Vec<i64>, expression_base: u32) -> Recipes {
    let base = u32::try_from(words.len()).expect("recovery word offset exceeds u32");
    let count = u32::try_from(program.refs().len()).expect("recovery source count exceeds u32");
    assert!(count <= i32::MAX as u32, "recovery NeedTag exceeds i32");
    checked_end(base, count);
    let mut encoded = Vec::with_capacity(program.refs().len());
    let mut kinds = 0;
    for (index, source) in program.refs().iter().enumerate() {
        let (kind, record) = encode(program, index, source, expression_base);
        kinds |= 1 << kind as u32;
        encoded.push(record);
    }
    for record in encoded {
        words.extend(record);
    }
    Recipes { base, count, kinds }
}

fn checked_end(base: u32, count: u32) -> u32 {
    base.checked_add(
        count
            .checked_mul(WORDS)
            .expect("recovery source table exceeds u32"),
    )
    .expect("recovery word range exceeds u32")
}

fn pair(low: u32, high: u32) -> u64 {
    u64::from(low) | (u64::from(high) << 32)
}

fn type_code(ty: ValueType) -> u32 {
    match ty {
        ValueType::I32 => TYPE_I32,
        ValueType::I64 => TYPE_I64,
        ValueType::F64 => TYPE_F64,
    }
}

fn dependency(program: &Program, index: usize, id: RefId, ty: ValueType) -> u32 {
    assert!(
        (id.0 as usize) < index,
        "recovery dependency must precede its user"
    );
    assert_eq!(
        program.get(id).color.ty,
        ty,
        "recovery dependency type mismatch"
    );
    id.0.checked_add(1)
        .expect("recovery dependency exceeds u32")
}

fn input(field: FpInput) -> (u32, u32, ValueType) {
    let (kind, argument, ty) = match field {
        FpInput::Native(i) => (0, u32::from(i), ValueType::F64),
        FpInput::Low(i) => (1, u32::from(i), ValueType::I64),
        FpInput::High(i) => (2, u32::from(i), ValueType::I32),
        FpInput::Valid(i) => (3, u32::from(i), ValueType::I32),
        FpInput::Tag(i) => (4, u32::from(i), ValueType::I32),
        FpInput::Top => (5, 0, ValueType::I32),
        FpInput::Control => (6, 0, ValueType::I32),
        FpInput::Status => (7, 0, ValueType::I32),
        FpInput::Pending => (8, 0, ValueType::I32),
        FpInput::PendingLeft => (9, 0, ValueType::F64),
        FpInput::PendingRight => (10, 0, ValueType::F64),
        FpInput::PendingResult => (11, 0, ValueType::F64),
        FpInput::Fip => (12, 0, ValueType::I32),
        FpInput::Fdp => (13, 0, ValueType::I32),
        FpInput::Opcode => (14, 0, ValueType::I32),
        FpInput::Cs => (15, 0, ValueType::I32),
        FpInput::Ds => (16, 0, ValueType::I32),
        FpInput::Selector(segment) => (17, segment.index() as u32, ValueType::I32),
    };
    if kind <= 4 {
        assert!(
            argument < 8,
            "recovery logical FP index exceeds eight cells"
        );
    }
    (kind, argument, ty)
}

fn encode(
    program: &Program,
    index: usize,
    source: &SourceRef,
    expression_base: u32,
) -> (Kind, [i64; WORDS as usize]) {
    let destination = source
        .color
        .local
        .checked_sub(expression_base)
        .expect("recovery source color precedes expression locals");
    let mut argument = 0;
    let mut dependencies = [0; 4];
    let mut expected = source.color.ty;
    let kind = match &source.op {
        SourceOp::Available => Kind::Available,
        SourceOp::PhysicalInput { local } => {
            // Program::value only exposes the eight whole architectural GPRs.
            // The VM deliberately has no arbitrary-local physical reader.
            assert!(
                (2..=9).contains(local),
                "unsupported recovery physical input local"
            );
            expected = ValueType::I32;
            argument = u64::from(*local);
            Kind::PhysicalInput
        }
        SourceOp::Constant { bits } => {
            argument = *bits;
            Kind::Constant
        }
        SourceOp::Binary32 { kind, left, right } => {
            expected = ValueType::I32;
            dependencies[0] = dependency(program, index, *left, ValueType::I32);
            dependencies[1] = dependency(program, index, *right, ValueType::I32);
            match kind {
                Binary32::Add => Kind::Add,
                Binary32::Sub => Kind::Sub,
                Binary32::And => Kind::And,
                Binary32::Or => Kind::Or,
                Binary32::Xor => Kind::Xor,
            }
        }
        SourceOp::ShiftLeft32 { value, amount } => {
            expected = ValueType::I32;
            argument = u64::from(*amount);
            dependencies[0] = dependency(program, index, *value, ValueType::I32);
            Kind::ShiftLeft32
        }
        SourceOp::Fp(Source::Input { field, top }) => {
            let (field_kind, field_argument, ty) = input(*field);
            expected = ty;
            argument = pair(field_kind, field_argument);
            if let Some(top) = top {
                dependencies[0] = dependency(program, index, *top, ValueType::I32);
            }
            Kind::FpInput
        }
        SourceOp::Fp(Source::PrecisionMode { control }) => {
            expected = ValueType::I32;
            dependencies[0] = dependency(program, index, *control, ValueType::I32);
            Kind::Precision
        }
        SourceOp::Fp(Source::Certificate {
            logical,
            top,
            control,
            valid,
        }) => {
            assert!(
                *logical < 8,
                "recovery certificate logical index exceeds eight cells"
            );
            expected = ValueType::I32;
            argument = u64::from(*logical);
            dependencies[0] = dependency(program, index, *top, ValueType::I32);
            dependencies[1] = dependency(program, index, *control, ValueType::I32);
            dependencies[2] = dependency(program, index, *valid, ValueType::I32);
            Kind::Certificate
        }
        SourceOp::Fp(Source::Classify(payload)) => {
            expected = ValueType::I32;
            match *payload {
                Payload::Native { value } => {
                    dependencies[0] = dependency(program, index, value, ValueType::F64);
                    Kind::ClassNative
                }
                Payload::Opaque { low, high } => {
                    dependencies[0] = dependency(program, index, low, ValueType::I64);
                    dependencies[1] = dependency(program, index, high, ValueType::I32);
                    Kind::ClassRaw
                }
                Payload::Tagged {
                    valid,
                    native,
                    low,
                    high,
                } => {
                    dependencies[0] = dependency(program, index, valid, ValueType::I32);
                    dependencies[1] = dependency(program, index, native, ValueType::F64);
                    dependencies[2] = dependency(program, index, low, ValueType::I64);
                    dependencies[3] = dependency(program, index, high, ValueType::I32);
                    Kind::ClassTagged
                }
            }
        }
    };
    assert_eq!(
        source.color.ty, expected,
        "recovery source result type mismatch"
    );
    let header = kind as u32 | (type_code(source.color.ty) << TYPE_SHIFT);
    (
        kind,
        [
            pair(header, destination) as i64,
            argument as i64,
            pair(dependencies[0], dependencies[1]) as i64,
            pair(dependencies[2], dependencies[3]) as i64,
        ],
    )
}
