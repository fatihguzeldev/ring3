//! executable values and ordered architectural effects share one program.

mod effects;
pub(in crate::cpu::core) mod fp;
#[cfg(test)]
mod tests;

use super::{BlockId, FrameStateId, ProgramPoint, Region, RegionPlan, ValueType};
use crate::cpu::{
    core::flags::FlagKind,
    x86::ir::{AddressSize, BinaryKind, EffectiveAddress, Location32, Operation, Value32},
};
pub(in crate::cpu::core) use effects::{ExactDefUse, FaultRecovery, FpEffectClass, MemoryEffect};
use fp::{FpEffect, FpRecovery, FpValueNode};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(in crate::cpu::core) struct ExecValueId(pub usize);
pub(in crate::cpu::core) type ExprId = ExecValueId;
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(in crate::cpu::core) struct ExecEffectId(pub usize);

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
    EffectResult {
        effect: ExecEffectId,
        register: u8,
    },
}

#[derive(Clone, Debug)]
pub(in crate::cpu::core) enum ValueNode {
    Scalar(Expr32),
    Fp(FpValueNode),
}
#[derive(Clone, Debug)]
pub(in crate::cpu::core) struct ExecutableValue {
    pub ty: ValueType,
    pub node: ValueNode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::cpu::core) enum FlagsOrigin {
    Entry(BlockId),
    Effect(ExecEffectId),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::cpu::core) enum FlagsRecipe {
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::cpu::core) struct FlagsState {
    pub origin: FlagsOrigin,
    pub recipe: FlagsRecipe,
}
impl FlagsState {
    pub fn roots(self) -> impl Iterator<Item = ExprId> {
        self.recipe.roots()
    }
    pub fn entry(block: BlockId) -> Self {
        Self {
            origin: FlagsOrigin::Entry(block),
            recipe: FlagsRecipe::Current,
        }
    }
}

#[derive(Clone, Debug)]
pub(in crate::cpu::core) struct Recovery {
    pub frame: FrameStateId,
    pub prefix: u32,
    pub gpr: [ExprId; 8],
    pub flags: FlagsState,
    pub fp: FpRecovery,
}
impl Recovery {
    pub fn scalar_roots(&self) -> Vec<ExprId> {
        self.gpr.into_iter().chain(self.flags.roots()).collect()
    }
    pub fn roots(&self) -> Vec<ExecValueId> {
        self.scalar_roots()
            .into_iter()
            .chain(self.fp.roots())
            .collect()
    }
}

#[derive(Clone, Debug)]
pub(in crate::cpu::core) struct PureRun {
    pub start_prefix: u32,
    pub end_prefix: u32,
    pub before: Recovery,
    pub after: Recovery,
    pub writes: u8,
    live: Vec<ExprId>,
}
impl PureRun {
    pub fn flags_changed(&self) -> bool {
        self.after.flags != self.before.flags
    }
}

#[derive(Clone, Debug)]
pub(in crate::cpu::core) struct AddressValue {
    pub address: EffectiveAddress,
    pub offset: ExprId,
    pub base: Option<ExprId>,
    pub index: Option<ExprId>,
}
#[derive(Clone, Debug)]
pub(in crate::cpu::core) enum EffectKind {
    PlainRead32 {
        address: AddressValue,
        result: ExprId,
    },
    Exact(ExactDefUse),
    Fp(FpEffect),
}
#[derive(Clone, Debug)]
pub(in crate::cpu::core) struct OrderedEffect {
    pub id: ExecEffectId,
    pub point: ProgramPoint,
    pub operation: Option<Operation>,
    pub gate: Option<u32>,
    pub kind: EffectKind,
    pub contract: ExactDefUse,
    pub before: Recovery,
    pub after: Recovery,
}
impl OrderedEffect {
    pub fn inputs(&self) -> Vec<ExecValueId> {
        match &self.kind {
            EffectKind::PlainRead32 { address, .. } => vec![address.offset],
            EffectKind::Fp(effect) => effect.inputs(),
            EffectKind::Exact(_) => (0..8)
                .filter(|&r| self.contract.gpr_reads & (1 << r) != 0)
                .map(|r| self.before.gpr[r])
                .chain(
                    if self.contract.flags_read != 0 || self.contract.flags_write != 0 {
                        self.before.flags.roots().collect::<Vec<_>>()
                    } else {
                        Vec::new()
                    },
                )
                .collect(),
        }
    }
    pub fn definitions(&self) -> Vec<ExecValueId> {
        let mut result: Vec<_> = (0..8)
            .filter(|&r| self.contract.gpr_writes & (1 << r) != 0)
            .map(|r| self.after.gpr[r])
            .collect();
        if let EffectKind::Fp(effect) = &self.kind {
            result.extend(effect.definitions());
        } else if matches!(self.contract.fp, FpEffectClass::Observe) {
            result.extend(self.after.fp.status.roots());
        }
        result
    }
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
    pub fn recovery(&self, prefix: u32) -> &Recovery {
        &self.recoveries[prefix as usize]
    }
    pub fn effects(&self) -> impl Iterator<Item = &OrderedEffect> {
        self.steps.iter().filter_map(|s| {
            if let ExecutableStep::Effect(e) = s {
                Some(e)
            } else {
                None
            }
        })
    }
}
pub(in crate::cpu::core) struct ExecutableProgram {
    values: Vec<ExecutableValue>,
    blocks: Vec<ExecutableBlock>,
    effects: Vec<OrderedEffect>,
    fp_parameters: Vec<fp::FpParameters>,
}
pub(in crate::cpu::core) type ExecutablePlan = ExecutableProgram;
impl ExecutableProgram {
    pub fn analyze(region: &Region, values: &RegionPlan) -> Self {
        let mut builder = ExecutableBuilder::default();
        let mut blocks = Vec::with_capacity(region.blocks.len());
        for (index, block) in region.blocks.iter().enumerate() {
            let id = BlockId(index);
            let entry_frame = values.block(id).entry;
            let gpr = builder.inputs(id, 0);
            let fp = FpRecovery::entry(&mut builder, id, entry_frame, values);
            let mut current = Recovery {
                frame: entry_frame,
                prefix: 0,
                gpr,
                flags: FlagsState::entry(id),
                fp,
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
                        .is_some_and(|op| builder.apply(op, &mut current));
                if pure {
                    run.get_or_insert(before);
                    current.frame = analyzed.after;
                    current.prefix = instruction.prefix + 1;
                } else {
                    finish_run(&builder, &mut steps, run.take(), &before);
                    let effect = builder.reserve_effect();
                    let contract = effects::classify(instruction.operation, instruction.gate);
                    current.frame = analyzed.after;
                    current.prefix = instruction.prefix + 1;
                    let kind = if instruction.gate.is_none() {
                        if let Some(Operation::Move {
                            destination: Location32::Register(register),
                            source: Value32::Memory(address),
                        }) = instruction.operation
                        {
                            let address = builder
                                .address_value(address, &before.gpr)
                                .expect("decoded effective address");
                            let result = builder.effect_result(effect, register.index() as u8);
                            current.gpr[register.index()] = result;
                            EffectKind::PlainRead32 { address, result }
                        } else if let Some((fp_effect, recovery)) =
                            instruction.operation.and_then(|op| {
                                fp::lower_effect(
                                    &mut builder,
                                    op,
                                    ProgramPoint {
                                        block: id,
                                        prefix: instruction.prefix,
                                    },
                                    &before,
                                    analyzed.after,
                                    values,
                                    effect,
                                )
                            })
                        {
                            current.fp = recovery;
                            for r in 0..8 {
                                if contract.gpr_writes & (1 << r) != 0 {
                                    current.gpr[r] = builder.effect_result(effect, r as u8);
                                }
                            }
                            if contract.flags_write != 0 {
                                current.flags = FlagsState {
                                    origin: FlagsOrigin::Effect(effect),
                                    recipe: FlagsRecipe::Current,
                                };
                            }
                            EffectKind::Fp(fp_effect)
                        } else {
                            apply_exact(
                                &mut builder,
                                &mut current,
                                &before,
                                ProgramPoint {
                                    block: id,
                                    prefix: instruction.prefix,
                                },
                                analyzed.after,
                                values,
                                effect,
                                contract,
                            );
                            EffectKind::Exact(contract)
                        }
                    } else {
                        apply_exact(
                            &mut builder,
                            &mut current,
                            &before,
                            ProgramPoint {
                                block: id,
                                prefix: instruction.prefix,
                            },
                            analyzed.after,
                            values,
                            effect,
                            contract,
                        );
                        EffectKind::Exact(contract)
                    };
                    let ordered = OrderedEffect {
                        id: effect,
                        point: ProgramPoint {
                            block: id,
                            prefix: instruction.prefix,
                        },
                        operation: instruction.operation,
                        gate: instruction.gate,
                        kind,
                        contract,
                        before,
                        after: current.clone(),
                    };
                    builder.finish_effect(ordered.clone());
                    steps.push(ExecutableStep::Effect(ordered));
                }
                recoveries.push(current.clone());
            }
            finish_run(&builder, &mut steps, run, &current);
            blocks.push(ExecutableBlock {
                steps,
                entry,
                exit: current,
                recoveries,
            });
        }
        let mut program = Self {
            values: builder.values,
            blocks,
            effects: builder
                .effects
                .into_iter()
                .map(|e| e.expect("reserved effect completed"))
                .collect(),
            fp_parameters: Vec::new(),
        };
        program.fp_parameters = fp::analyze_parameters(&program, values);
        program
    }
    pub fn block(&self, id: BlockId) -> &ExecutableBlock {
        &self.blocks[id.0]
    }
    pub fn fp_parameters(&self, id: BlockId) -> &fp::FpParameters {
        &self.fp_parameters[id.0]
    }
    pub fn expression(&self, id: ExprId) -> &Expr32 {
        match &self.values[id.0].node {
            ValueNode::Scalar(value) => value,
            _ => panic!("scalar expression required"),
        }
    }
    pub fn value(&self, id: ExecValueId) -> &ExecutableValue {
        &self.values[id.0]
    }
    pub fn effect(&self, id: ExecEffectId) -> &OrderedEffect {
        &self.effects[id.0]
    }
    pub fn expression_count(&self) -> usize {
        self.values.len()
    }
    pub fn value_count(&self) -> usize {
        self.values.len()
    }
    pub fn live_expressions<'a>(&self, run: &'a PureRun) -> &'a [ExprId] {
        &run.live
    }
    pub fn dependency_order(&self, roots: &[ExecValueId]) -> Vec<ExecValueId> {
        dependency_order(&self.values, roots)
    }
}
fn apply_exact(
    builder: &mut ExecutableBuilder,
    current: &mut Recovery,
    before: &Recovery,
    point: ProgramPoint,
    after: FrameStateId,
    values: &RegionPlan,
    effect: ExecEffectId,
    contract: ExactDefUse,
) {
    for r in 0..8 {
        if contract.gpr_writes & (1 << r) != 0 {
            current.gpr[r] = builder.effect_result(effect, r as u8);
        }
    }
    if contract.flags_write != 0 {
        current.flags = FlagsState {
            origin: FlagsOrigin::Effect(effect),
            recipe: FlagsRecipe::Current,
        };
    }
    current.fp =
        before
            .fp
            .after_exact(builder, point, &values.frame(after).fp, contract.fp, effect);
}
fn finish_run(
    builder: &ExecutableBuilder,
    steps: &mut Vec<ExecutableStep>,
    before: Option<Recovery>,
    after: &Recovery,
) {
    let Some(before) = before else { return };
    let writes = (0..8).fold(0, |mask, r| {
        mask | (u8::from(before.gpr[r] != after.gpr[r]) << r)
    });
    let mut roots: Vec<_> = (0..8)
        .filter(|&r| writes & (1 << r) != 0)
        .map(|r| after.gpr[r])
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
        live: builder.live(&roots),
    }));
}
fn dependency_order(values: &[ExecutableValue], roots: &[ExecValueId]) -> Vec<ExecValueId> {
    let mut live = BTreeSet::new();
    let mut pending = roots.to_vec();
    while let Some(id) = pending.pop() {
        if live.insert(id) {
            pending.extend(value_dependencies(&values[id.0].node));
        }
    }
    live.into_iter().collect()
}
fn value_dependencies(node: &ValueNode) -> Vec<ExecValueId> {
    match node {
        ValueNode::Scalar(Expr32::Binary { left, right, .. }) => vec![*left, *right],
        ValueNode::Scalar(Expr32::ShiftLeft { value, .. }) => vec![*value],
        ValueNode::Scalar(_) => Vec::new(),
        ValueNode::Fp(node) => node.dependencies(),
    }
}
#[derive(Default)]
pub(in crate::cpu::core) struct ExecutableBuilder {
    values: Vec<ExecutableValue>,
    interned: BTreeMap<Expr32, ExprId>,
    effects: Vec<Option<OrderedEffect>>,
}

impl ExecutableBuilder {
    pub fn push_value(&mut self, ty: ValueType, node: ValueNode) -> ExecValueId {
        let id = ExecValueId(self.values.len());
        debug_assert!(value_dependencies(&node).iter().all(|v| v.0 < id.0));
        self.values.push(ExecutableValue { ty, node });
        id
    }
    pub fn value_type(&self, id: ExecValueId) -> ValueType {
        self.values[id.0].ty
    }
    pub fn node(&self, id: ExecValueId) -> &ValueNode {
        &self.values[id.0].node
    }
    pub fn dependencies(&self, id: ExecValueId) -> Vec<ExecValueId> {
        value_dependencies(self.node(id))
    }
    pub fn reserve_effect(&mut self) -> ExecEffectId {
        let id = ExecEffectId(self.effects.len());
        self.effects.push(None);
        id
    }
    pub fn finish_effect(&mut self, effect: OrderedEffect) {
        let id = effect.id;
        assert!(self.effects[id.0].is_none());
        self.effects[id.0] = Some(effect);
    }
    pub fn effect_result(&mut self, effect: ExecEffectId, register: u8) -> ExprId {
        self.intern(Expr32::EffectResult { effect, register })
    }
    pub fn address_value(
        &mut self,
        address: EffectiveAddress,
        gpr: &[ExprId; 8],
    ) -> Option<AddressValue> {
        self.address(address, gpr).map(|offset| AddressValue {
            address,
            offset,
            base: address.base.map(|r| gpr[r.index()]),
            index: address.index.map(|r| gpr[r.index()]),
        })
    }
    fn scalar(&self, id: ExprId) -> Expr32 {
        match &self.values[id.0].node {
            ValueNode::Scalar(value) => *value,
            _ => panic!("scalar value required"),
        }
    }
    fn intern(&mut self, expression: Expr32) -> ExprId {
        if let Some(&id) = self.interned.get(&expression) {
            return id;
        }
        let id = self.push_value(ValueType::I32, ValueNode::Scalar(expression));
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
        if let Expr32::Constant(value) = self.scalar(id) {
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
                } = self.scalar(base)
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
                state.flags.recipe = FlagsRecipe::Arithmetic {
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
        dependency_order(&self.values, roots)
    }
}
