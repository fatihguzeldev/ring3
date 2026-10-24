//! Executable scalar values between exact architectural effects.
//!
//! A pure run has no observable intermediate execution point. Its prefix maps
//! remain recovery recipes, while only the live exit expressions are emitted.
//! Every unmigrated operation is an ordered effect, including unused loads.

use super::{BlockId, FrameStateId, ProgramPoint, Region, RegionPlan};
use crate::cpu::{
    core::flags::FlagKind,
    x86::ir::{AddressSize, BinaryKind, EffectiveAddress, Location32, Operation, Value32},
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::cpu::core) struct ExprId(pub usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::cpu::core) enum Binary32 {
    Add,
    Sub,
    And,
    Or,
    Xor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::cpu::core) enum Expr32 {
    /// Actual architectural local at this pure run's entry, not a prediction.
    Input {
        block: BlockId,
        prefix: u32,
        register: u8,
    },
    Constant(u32),
    Binary {
        kind: Binary32,
        left: ExprId,
        right: ExprId,
    },
    ShiftLeft {
        value: ExprId,
        amount: u8,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::cpu::core) enum FlagsRecipe {
    /// Preserve the actual current materialized/lazy tuple, including CF and DF.
    Current,
    Arithmetic {
        kind: FlagKind,
        width: u32,
        left: ExprId,
        right: ExprId,
        result: ExprId,
    },
}

impl FlagsRecipe {
    pub fn roots(self) -> impl Iterator<Item = ExprId> {
        match self {
            Self::Current => [None; 3],
            Self::Arithmetic {
                left,
                right,
                result,
                ..
            } => [Some(left), Some(right), Some(result)],
        }
        .into_iter()
        .flatten()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::cpu::core) struct Recovery {
    pub frame: FrameStateId,
    /// Number of completed guest instructions in the block.
    pub prefix: u32,
    pub gpr: [ExprId; 8],
    pub flags: FlagsRecipe,
}

#[derive(Clone, Debug)]
pub(in crate::cpu::core) struct PureRun {
    pub start_prefix: u32,
    pub end_prefix: u32,
    pub before: Recovery,
    pub after: Recovery,
    /// Parallel writes: evaluate all live expressions before modifying a GPR.
    pub writes: u8,
    live: Vec<ExprId>,
}

impl PureRun {
    pub fn flags_changed(&self) -> bool {
        self.after.flags != self.before.flags
    }
}

#[derive(Clone, Debug)]
pub(in crate::cpu::core) struct OrderedEffect {
    pub point: ProgramPoint,
    pub operation: Option<Operation>,
    pub gate: Option<u32>,
    pub before: Recovery,
    /// New actual-local inputs after the existing exact lowering has completed.
    pub after: Recovery,
}

#[derive(Clone, Debug)]
pub(in crate::cpu::core) enum ExecutableStep {
    Pure(PureRun),
    Effect(OrderedEffect),
}

pub(in crate::cpu::core) struct ExecutableBlock {
    pub steps: Vec<ExecutableStep>,
    pub entry: Recovery,
    pub exit: Recovery,
    recoveries: Vec<Recovery>,
}

impl ExecutableBlock {
    pub fn recovery(&self, completed_prefix: u32) -> &Recovery {
        &self.recoveries[completed_prefix as usize]
    }
}

pub(in crate::cpu::core) struct ExecutablePlan {
    expressions: Vec<Expr32>,
    blocks: Vec<ExecutableBlock>,
}

impl ExecutablePlan {
    pub fn analyze(region: &Region, values: &RegionPlan) -> Self {
        let mut dag = Dag::default();
        let mut blocks = Vec::with_capacity(region.blocks.len());
        for (index, block) in region.blocks.iter().enumerate() {
            let id = BlockId(index);
            let mut current = Recovery {
                frame: values.block(id).entry,
                prefix: 0,
                gpr: dag.inputs(id, 0),
                flags: FlagsRecipe::Current,
            };
            let entry = current.clone();
            let mut recoveries = vec![current.clone()];
            let mut steps = Vec::new();
            let mut run = None;
            for instruction in &block.instructions {
                let analyzed = values.instruction(id, instruction.prefix);
                debug_assert_eq!(current.frame, analyzed.before);
                let before = current.clone();
                let pure = instruction.gate.is_none()
                    && instruction
                        .operation
                        .is_some_and(|operation| dag.apply(operation, &mut current));
                if pure {
                    run.get_or_insert(before);
                    current.frame = analyzed.after;
                    current.prefix = instruction.prefix + 1;
                } else {
                    finish_run(&dag, &mut steps, run.take(), &before);
                    current = Recovery {
                        frame: analyzed.after,
                        prefix: instruction.prefix + 1,
                        gpr: dag.inputs(id, instruction.prefix + 1),
                        flags: FlagsRecipe::Current,
                    };
                    steps.push(ExecutableStep::Effect(OrderedEffect {
                        point: ProgramPoint {
                            block: id,
                            prefix: instruction.prefix,
                        },
                        operation: instruction.operation,
                        gate: instruction.gate,
                        before,
                        after: current.clone(),
                    }));
                }
                recoveries.push(current.clone());
            }
            finish_run(&dag, &mut steps, run, &current);
            blocks.push(ExecutableBlock {
                steps,
                entry,
                exit: current,
                recoveries,
            });
        }
        Self {
            expressions: dag.expressions,
            blocks,
        }
    }

    pub fn block(&self, id: BlockId) -> &ExecutableBlock {
        &self.blocks[id.0]
    }

    pub fn expression(&self, id: ExprId) -> &Expr32 {
        &self.expressions[id.0]
    }

    pub fn expression_count(&self) -> usize {
        self.expressions.len()
    }

    /// Deterministic dependency order. Unused intermediate definitions are absent.
    pub fn live_expressions<'a>(&self, run: &'a PureRun) -> &'a [ExprId] {
        &run.live
    }
}

fn finish_run(
    dag: &Dag,
    steps: &mut Vec<ExecutableStep>,
    before: Option<Recovery>,
    after: &Recovery,
) {
    let Some(before) = before else {
        return;
    };
    let writes = (0..8).fold(0, |mask, register| {
        mask | (u8::from(before.gpr[register] != after.gpr[register]) << register)
    });
    let mut roots: Vec<_> = (0..8)
        .filter(|&register| writes & (1 << register) != 0)
        .map(|register| after.gpr[register])
        .collect();
    if before.flags != after.flags {
        roots.extend(after.flags.roots());
    }
    steps.push(ExecutableStep::Pure(PureRun {
        start_prefix: before.prefix,
        end_prefix: after.prefix,
        before,
        after: after.clone(),
        writes,
        live: dag.live(&roots),
    }));
}

#[derive(Default)]
struct Dag {
    expressions: Vec<Expr32>,
    interned: BTreeMap<Expr32, ExprId>,
}

impl Dag {
    fn intern(&mut self, expression: Expr32) -> ExprId {
        if let Some(&id) = self.interned.get(&expression) {
            return id;
        }
        let id = ExprId(self.expressions.len());
        self.expressions.push(expression);
        self.interned.insert(expression, id);
        id
    }

    fn inputs(&mut self, block: BlockId, prefix: u32) -> [ExprId; 8] {
        std::array::from_fn(|register| {
            self.intern(Expr32::Input {
                block,
                prefix,
                register: register as u8,
            })
        })
    }

    fn constant(&mut self, value: u32) -> ExprId {
        self.intern(Expr32::Constant(value))
    }
    fn value(&self, id: ExprId) -> Option<u32> {
        if let Expr32::Constant(value) = self.expressions[id.0] {
            Some(value)
        } else {
            None
        }
    }

    fn binary(&mut self, kind: Binary32, mut left: ExprId, mut right: ExprId) -> ExprId {
        let a = self.value(left);
        let b = self.value(right);
        if let (Some(a), Some(b)) = (a, b) {
            return self.constant(match kind {
                Binary32::Add => a.wrapping_add(b),
                Binary32::Sub => a.wrapping_sub(b),
                Binary32::And => a & b,
                Binary32::Or => a | b,
                Binary32::Xor => a ^ b,
            });
        }
        if left == right {
            match kind {
                Binary32::Sub | Binary32::Xor => return self.constant(0),
                Binary32::And | Binary32::Or => return left,
                Binary32::Add => {}
            }
        }
        match kind {
            Binary32::Add | Binary32::Or | Binary32::Xor if a == Some(0) => return right,
            Binary32::Add | Binary32::Sub | Binary32::Or | Binary32::Xor if b == Some(0) => {
                return left;
            }
            Binary32::And if a == Some(0) || b == Some(0) => return self.constant(0),
            Binary32::And if a == Some(u32::MAX) => return right,
            Binary32::And if b == Some(u32::MAX) => return left,
            Binary32::Or if a == Some(u32::MAX) || b == Some(u32::MAX) => {
                return self.constant(u32::MAX);
            }
            _ => {}
        }
        if kind == Binary32::Add {
            let (base, constant) = if let Some(value) = b {
                (left, value)
            } else if let Some(value) = a {
                (right, value)
            } else {
                (left, 0)
            };
            if a.is_some() || b.is_some() {
                if let Expr32::Binary {
                    kind: Binary32::Add,
                    left: x,
                    right: y,
                } = self.expressions[base.0]
                {
                    let inner = self
                        .value(x)
                        .map(|c| (y, c))
                        .or_else(|| self.value(y).map(|c| (x, c)));
                    if let Some((input, value)) = inner {
                        let combined = self.constant(value.wrapping_add(constant));
                        return self.binary(Binary32::Add, input, combined);
                    }
                }
            }
        }
        if kind != Binary32::Sub && right < left {
            std::mem::swap(&mut left, &mut right);
        }
        self.intern(Expr32::Binary { kind, left, right })
    }

    fn operand(&mut self, source: Value32, gpr: &[ExprId; 8]) -> Option<ExprId> {
        match source {
            Value32::Register(register) => Some(gpr[register.index()]),
            Value32::Immediate(value) => Some(self.constant(value)),
            Value32::Memory(_) => None,
        }
    }

    fn address(&mut self, address: EffectiveAddress, gpr: &[ExprId; 8]) -> Option<ExprId> {
        if !matches!(address.scale, 1 | 2 | 4 | 8) {
            return None;
        }
        let mut result = self.constant(address.displacement);
        if let Some(base) = address.base {
            result = self.binary(Binary32::Add, result, gpr[base.index()]);
        }
        if let Some(index) = address.index {
            let value = gpr[index.index()];
            let scaled = if address.scale == 1 {
                value
            } else if let Some(value) = self.value(value) {
                self.constant(value.wrapping_mul(u32::from(address.scale)))
            } else {
                self.intern(Expr32::ShiftLeft {
                    value,
                    amount: address.scale.trailing_zeros() as u8,
                })
            };
            result = self.binary(Binary32::Add, result, scaled);
        }
        if address.address_size == AddressSize::Bits16 {
            let mask = self.constant(0xffff);
            result = self.binary(Binary32::And, result, mask);
        }
        Some(result)
    }

    fn apply(&mut self, operation: Operation, state: &mut Recovery) -> bool {
        match operation {
            Operation::Nop => {}
            Operation::Move {
                destination: Location32::Register(register),
                source,
            } => {
                let Some(value) = self.operand(source, &state.gpr) else {
                    return false;
                };
                state.gpr[register.index()] = value;
            }
            Operation::Lea {
                destination,
                address,
            } => {
                let Some(value) = self.address(address, &state.gpr) else {
                    return false;
                };
                state.gpr[destination.index()] = value;
            }
            Operation::Binary {
                kind,
                destination: Location32::Register(register),
                source,
            } => {
                let (operator, flags) = match kind {
                    BinaryKind::Add => (Binary32::Add, FlagKind::Add),
                    BinaryKind::Sub | BinaryKind::Cmp => (Binary32::Sub, FlagKind::Sub),
                    BinaryKind::And | BinaryKind::Test => (Binary32::And, FlagKind::Logical),
                    BinaryKind::Or => (Binary32::Or, FlagKind::Logical),
                    BinaryKind::Xor => (Binary32::Xor, FlagKind::Logical),
                    BinaryKind::Adc | BinaryKind::Sbb => return false,
                };
                let Some(right) = self.operand(source, &state.gpr) else {
                    return false;
                };
                let left = state.gpr[register.index()];
                let result = self.binary(operator, left, right);
                if !matches!(kind, BinaryKind::Cmp | BinaryKind::Test) {
                    state.gpr[register.index()] = result;
                }
                let (left, right) = if flags == FlagKind::Logical {
                    // Logical flags depend only on the result; AF follows the
                    // existing defined-zero policy. Dead operands need no code.
                    let zero = self.constant(0);
                    (zero, zero)
                } else {
                    (left, right)
                };
                state.flags = FlagsRecipe::Arithmetic {
                    kind: flags,
                    width: 32,
                    left,
                    right,
                    result,
                };
            }
            _ => return false,
        }
        true
    }

    fn live(&self, roots: &[ExprId]) -> Vec<ExprId> {
        let mut live = BTreeSet::new();
        let mut pending = roots.to_vec();
        while let Some(id) = pending.pop() {
            if !live.insert(id) {
                continue;
            }
            match self.expressions[id.0] {
                Expr32::Binary { left, right, .. } => pending.extend([left, right]),
                Expr32::ShiftLeft { value, .. } => pending.push(value),
                Expr32::Input { .. } | Expr32::Constant(_) => {}
            }
        }
        // Interning only accepts existing operands, so ID order is topological.
        live.into_iter().collect()
    }
}

#[cfg(test)]
mod tests;
