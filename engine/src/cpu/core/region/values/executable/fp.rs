//! Executable x87 values and effects in the common program arena.
//!
//! Payload representation, architectural tags and status are independent. In
//! particular, an empty slot still owns its payload and every rounded operation
//! owns its precision effect even when its value is subsequently overwritten.
use super::{
    AddressValue, EffectKind, ExecEffectId, ExecValueId, ExecutableBuilder, ExecutableProgram,
    ExecutableStep, FpEffectClass, Recovery, ValueNode,
};
use crate::cpu::core::region::{
    BlockId,
    values::{
        EffectId, FpFrame, FpValue, FrameStateId, ProgramPoint, ProofKind, RegionPlan, ValueType,
    },
};
use crate::cpu::x86::{X87Binary, X87Format, X87Operation, ir::Operation};

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::cpu::core) enum FpInput {
    Native(u8),
    Low(u8),
    High(u8),
    Valid(u8),
    Tag(u8),
    Top,
    Control,
    Status,
    Pending,
    PendingLeft,
    PendingRight,
    PendingResult,
    Fip,
    Fdp,
    Opcode,
    Cs,
    Ds,
    Selector(crate::cpu::x86::SegmentRegister),
}

#[derive(Clone, Debug)]
pub(in crate::cpu::core) enum FpValueNode {
    Input {
        point: ProgramPoint,
        field: FpInput,
    },
    I32(u32),
    EffectResult {
        effect: ExecEffectId,
        output: u8,
    },
    PrecisionMode {
        control: ExecValueId,
    },
    /// Snapshot a certificate for this complete actual input, before any
    /// projection can overwrite its physical record. Internal edges pass the
    /// resulting value with the payload rather than rereading that record.
    InputCertificate {
        point: ProgramPoint,
        logical: u8,
        control: ExecValueId,
        valid: ExecValueId,
    },
    /// Classification is a real value expression, not the incoming tag class.
    Classify(FpPayload),
}

impl FpValueNode {
    pub fn dependencies(&self) -> Vec<ExecValueId> {
        match self {
            Self::Classify(value) => value.roots(),
            Self::PrecisionMode { control } => vec![*control],
            Self::InputCertificate { control, valid, .. } => vec![*control, *valid],
            _ => Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::cpu::core) enum NativeDomain {
    Exact,
    Bounded,
    InSingle,
    Native24,
}
impl NativeDomain {
    pub fn proof(self) -> ProofKind {
        match self {
            Self::Exact => ProofKind::ExactF64,
            Self::Bounded => ProofKind::BoundedF64,
            Self::InSingle => ProofKind::Native24InSingle,
            Self::Native24 => ProofKind::Native24,
        }
    }
    pub fn implies(self, other: Self) -> bool {
        self == other
            || other == Self::Exact
            || self == Self::Native24
            || (self == Self::InSingle && other == Self::Bounded)
    }
    /// These bits describe this native value under its actual control epoch.
    pub fn certificate_bits(self) -> u32 {
        match self {
            Self::Exact => 1,
            Self::Bounded => 3,
            Self::InSingle => 11,
            Self::Native24 => 15,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::cpu::core) enum FpPayload {
    Native {
        value: ExecValueId,
        domain: NativeDomain,
        epoch: EffectId,
    },
    Opaque {
        low: ExecValueId,
        high: ExecValueId,
    },
    Tagged {
        low: ExecValueId,
        high: ExecValueId,
        native: ExecValueId,
        valid: ExecValueId,
        certificate: ExecValueId,
    },
}
impl FpPayload {
    pub fn roots(self) -> Vec<ExecValueId> {
        match self {
            Self::Native { value, .. } => vec![value],
            Self::Opaque { low, high } => vec![low, high],
            Self::Tagged {
                low,
                high,
                native,
                valid,
                certificate,
            } => vec![low, high, native, valid, certificate],
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(in crate::cpu::core) struct FpStatus {
    pub bits: ExecValueId,
    pub pending: ExecValueId,
    pub left: ExecValueId,
    pub right: ExecValueId,
    pub result: ExecValueId,
}
impl FpStatus {
    pub fn roots(self) -> Vec<ExecValueId> {
        vec![self.bits, self.pending, self.left, self.right, self.result]
    }
}

#[derive(Clone, Copy, Debug)]
pub(in crate::cpu::core) struct FpMetadata {
    pub fip: ExecValueId,
    pub fdp: ExecValueId,
    pub opcode: ExecValueId,
    pub cs: ExecValueId,
    pub ds: ExecValueId,
}

#[derive(Clone, Copy, Debug)]
pub(in crate::cpu::core) enum FpObligation {
    Native {
        logical: u8,
        input: FpPayload,
        value: ExecValueId,
        domain: NativeDomain,
    },
    Occupied {
        logical: u8,
        tag: ExecValueId,
    },
    Empty {
        logical: u8,
        tag: ExecValueId,
    },
}

#[derive(Clone, Debug)]
pub(in crate::cpu::core) struct FpRecovery {
    pub payload: [FpPayload; 8],
    pub tags: [ExecValueId; 8],
    pub top: ExecValueId,
    pub top_delta: i8,
    pub control: ExecValueId,
    pub mode: ExecValueId,
    pub epoch: EffectId,
    pub status: FpStatus,
    pub metadata: FpMetadata,
    /// Logical changed destinations relative to this block's actual TOP.
    pub written: u8,
    pub tag_written: u8,
    /// These are executed by external ingress, never granted by entry hints.
    pub ingress: Vec<FpObligation>,
}

fn input(
    builder: &mut ExecutableBuilder,
    point: ProgramPoint,
    field: FpInput,
    ty: ValueType,
) -> ExecValueId {
    builder.push_value(ty, ValueNode::Fp(FpValueNode::Input { point, field }))
}
fn constant(builder: &mut ExecutableBuilder, value: u32) -> ExecValueId {
    builder.push_value(ValueType::I32, ValueNode::Fp(FpValueNode::I32(value)))
}
fn result(
    builder: &mut ExecutableBuilder,
    effect: ExecEffectId,
    output: u8,
    ty: ValueType,
) -> ExecValueId {
    builder.push_value(
        ty,
        ValueNode::Fp(FpValueNode::EffectResult { effect, output }),
    )
}
fn tagged(
    builder: &mut ExecutableBuilder,
    point: ProgramPoint,
    logical: u8,
    control: ExecValueId,
) -> FpPayload {
    let valid = input(builder, point, FpInput::Valid(logical), ValueType::I32);
    let certificate = builder.push_value(
        ValueType::I32,
        ValueNode::Fp(FpValueNode::InputCertificate {
            point,
            logical,
            control,
            valid,
        }),
    );
    FpPayload::Tagged {
        low: input(builder, point, FpInput::Low(logical), ValueType::I64),
        high: input(builder, point, FpInput::High(logical), ValueType::I32),
        native: input(builder, point, FpInput::Native(logical), ValueType::F64),
        valid,
        certificate,
    }
}
fn domain(values: &RegionPlan, frame: FrameStateId, logical: usize) -> NativeDomain {
    let state = values.frame(frame);
    let payload = state.fp.payload[logical];
    let has = |kind| {
        values.proofs_at(frame).iter().any(|&id| {
            let proof = values.proof(id);
            proof.kind == kind
                && proof.conditional.is_none()
                && proof.epoch == state.fp.control_epoch
                && proof.value.is_some_and(|value| payload.contains(value))
        })
    };
    if has(ProofKind::Native24) {
        NativeDomain::Native24
    } else if has(ProofKind::Native24InSingle) {
        NativeDomain::InSingle
    } else if has(ProofKind::BoundedF64) {
        NativeDomain::Bounded
    } else {
        NativeDomain::Exact
    }
}

impl FpRecovery {
    pub fn entry(
        builder: &mut ExecutableBuilder,
        block: BlockId,
        entry: FrameStateId,
        values: &RegionPlan,
    ) -> Self {
        let frame = &values.frame(entry).fp;
        let point = ProgramPoint { block, prefix: 0 };
        let mut recovery = Self::unknown(builder, point, frame);
        for logical in 0..8 {
            if matches!(frame.payload[logical], FpValue::ExactF64(..)) {
                let input = recovery.payload[logical];
                let FpPayload::Tagged { native: value, .. } = input else {
                    unreachable!()
                };
                let domain = domain(values, entry, logical);
                recovery.payload[logical] = FpPayload::Native {
                    value,
                    domain,
                    epoch: frame.control_epoch,
                };
                recovery.ingress.push(FpObligation::Native {
                    logical: logical as u8,
                    input,
                    value,
                    domain,
                });
            }
        }
        recovery
    }

    fn unknown(builder: &mut ExecutableBuilder, point: ProgramPoint, frame: &FpFrame) -> Self {
        let control = input(builder, point, FpInput::Control, ValueType::I32);
        let payload = std::array::from_fn(|i| tagged(builder, point, i as u8, control));
        let tags =
            std::array::from_fn(|i| input(builder, point, FpInput::Tag(i as u8), ValueType::I32));
        let status = FpStatus {
            bits: input(builder, point, FpInput::Status, ValueType::I32),
            pending: input(builder, point, FpInput::Pending, ValueType::I32),
            left: input(builder, point, FpInput::PendingLeft, ValueType::F64),
            right: input(builder, point, FpInput::PendingRight, ValueType::F64),
            result: input(builder, point, FpInput::PendingResult, ValueType::F64),
        };
        let metadata = FpMetadata {
            fip: input(builder, point, FpInput::Fip, ValueType::I32),
            fdp: input(builder, point, FpInput::Fdp, ValueType::I32),
            opcode: input(builder, point, FpInput::Opcode, ValueType::I32),
            cs: input(builder, point, FpInput::Cs, ValueType::I32),
            ds: input(builder, point, FpInput::Ds, ValueType::I32),
        };
        let mode = builder.push_value(
            ValueType::I32,
            ValueNode::Fp(FpValueNode::PrecisionMode { control }),
        );
        Self {
            payload,
            tags,
            top: input(builder, point, FpInput::Top, ValueType::I32),
            top_delta: 0,
            control,
            mode,
            epoch: frame.control_epoch,
            status,
            metadata,
            written: 0,
            tag_written: 0,
            ingress: Vec::new(),
        }
    }

    pub fn after_exact(
        &self,
        builder: &mut ExecutableBuilder,
        point: ProgramPoint,
        frame: &FpFrame,
        class: FpEffectClass,
        effect: ExecEffectId,
    ) -> Self {
        match class {
            FpEffectClass::Barrier => Self::unknown(
                builder,
                ProgramPoint {
                    prefix: point.prefix + 1,
                    ..point
                },
                frame,
            ),
            FpEffectClass::Preserve => self.clone(),
            FpEffectClass::Observe => {
                let mut after = self.clone();
                after.status = FpStatus {
                    bits: result(builder, effect, 4, ValueType::I32),
                    pending: result(builder, effect, 5, ValueType::I32),
                    left: result(builder, effect, 6, ValueType::F64),
                    right: result(builder, effect, 7, ValueType::F64),
                    result: result(builder, effect, 8, ValueType::F64),
                };
                after
            }
        }
    }

    pub fn roots(&self) -> Vec<ExecValueId> {
        self.payload
            .into_iter()
            .flat_map(FpPayload::roots)
            .chain(self.tags)
            .chain([self.top, self.control, self.mode])
            .chain(self.status.roots())
            .chain([
                self.metadata.fip,
                self.metadata.fdp,
                self.metadata.opcode,
                self.metadata.cs,
                self.metadata.ds,
            ])
            .collect()
    }

    fn write(&mut self, builder: &mut ExecutableBuilder, logical: usize, payload: FpPayload) {
        self.payload[logical] = payload;
        self.tags[logical] = builder.push_value(
            ValueType::I32,
            ValueNode::Fp(FpValueNode::Classify(payload)),
        );
        self.written |= 1 << logical;
        self.tag_written |= 1 << logical;
    }
    fn push(&mut self) {
        self.payload.rotate_right(1);
        self.tags.rotate_right(1);
        self.written = self.written.rotate_left(1);
        self.tag_written = self.tag_written.rotate_left(1);
        self.top_delta = (self.top_delta - 1).rem_euclid(8);
    }
    fn pop(&mut self, builder: &mut ExecutableBuilder) {
        self.tags[0] = constant(builder, 3);
        self.tag_written |= 1;
        self.payload.rotate_left(1);
        self.tags.rotate_left(1);
        self.written = self.written.rotate_right(1);
        self.tag_written = self.tag_written.rotate_right(1);
        self.top_delta = (self.top_delta + 1).rem_euclid(8);
    }
}

#[derive(Clone, Debug)]
pub(in crate::cpu::core) enum FpOperand {
    Payload(FpPayload),
    Memory32(AddressValue),
}
impl FpOperand {
    fn roots(&self) -> Vec<ExecValueId> {
        match self {
            Self::Payload(p) => p.roots(),
            Self::Memory32(a) => vec![a.offset],
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub(in crate::cpu::core) enum StatusEffect {
    Preserve,
    ClearC1,
    Arithmetic,
    Store,
    Control,
}

#[derive(Clone, Debug)]
pub(in crate::cpu::core) enum FpEffectKind {
    LoadControl {
        address: AddressValue,
        result: ExecValueId,
    },
    StoreControl {
        address: AddressValue,
    },
    IntegerLoad {
        address: AddressValue,
        format: X87Format,
        result: FpPayload,
    },
    IntegerStore {
        address: AddressValue,
        source: FpPayload,
        format: X87Format,
        truncate: bool,
    },
    Load32 {
        address: AddressValue,
        result: ExecValueId,
    },
    Binary {
        kind: X87Binary,
        left: FpPayload,
        right: FpOperand,
        result: ExecValueId,
        exact_product: bool,
    },
    Transfer {
        source: FpPayload,
    },
    Exchange {
        left: FpPayload,
        right: FpPayload,
    },
    Sign {
        source: FpPayload,
        absolute: bool,
        result: FpPayload,
    },
    Store {
        address: AddressValue,
        source: FpPayload,
        format: X87Format,
    },
}

#[derive(Clone, Debug)]
pub(in crate::cpu::core) struct FpEffect {
    pub id: ExecEffectId,
    pub point: ProgramPoint,
    pub before_frame: FrameStateId,
    pub after_frame: FrameStateId,
    pub kind: FpEffectKind,
    pub waiting: bool,
    pub control: ExecValueId,
    pub mode: ExecValueId,
    pub epoch: EffectId,
    pub occupied: Vec<(u8, ExecValueId)>,
    pub empty: Vec<(u8, ExecValueId)>,
    pub status_before: FpStatus,
    pub status_after: FpStatus,
    pub status_effect: StatusEffect,
    pub outputs: Vec<ExecValueId>,
}
impl FpEffect {
    pub fn inputs(&self) -> Vec<ExecValueId> {
        let mut roots = vec![self.control, self.mode];
        roots.extend(self.status_before.roots());
        roots.extend(self.occupied.iter().chain(&self.empty).map(|(_, tag)| *tag));
        match &self.kind {
            FpEffectKind::Load32 { address, .. }
            | FpEffectKind::LoadControl { address, .. }
            | FpEffectKind::StoreControl { address }
            | FpEffectKind::IntegerLoad { address, .. } => roots.push(address.offset),
            FpEffectKind::IntegerStore {
                address, source, ..
            } => {
                roots.push(address.offset);
                roots.extend(source.roots());
            }
            FpEffectKind::Binary { left, right, .. } => {
                roots.extend(left.roots());
                roots.extend(right.roots());
            }
            FpEffectKind::Transfer { source } | FpEffectKind::Sign { source, .. } => {
                roots.extend(source.roots())
            }
            FpEffectKind::Exchange { left, right } => {
                roots.extend(left.roots());
                roots.extend(right.roots());
            }
            FpEffectKind::Store {
                address, source, ..
            } => {
                roots.push(address.offset);
                roots.extend(source.roots());
            }
        }
        roots.sort_unstable();
        roots.dedup();
        roots
    }
    pub fn definitions(&self) -> Vec<ExecValueId> {
        self.outputs.clone()
    }
}

/// The same arguments are supplied by every internal predecessor and by the
/// external adapter. Compile-time emitter availability is not this contract.
#[derive(Clone, Debug)]
pub(in crate::cpu::core) struct FpParameters {
    pub block: BlockId,
    pub payloads: u8,
    pub tags: u8,
    pub header: bool,
    pub imports: bool,
    pub arithmetic: bool,
}
impl FpParameters {
    pub fn roots(&self, entry: &FpRecovery) -> Vec<ExecValueId> {
        let mut roots = Vec::new();
        for i in 0..8 {
            if self.payloads & (1 << i) != 0 {
                roots.extend(entry.payload[i].roots());
            }
            if self.tags & (1 << i) != 0 {
                roots.push(entry.tags[i]);
            }
        }
        if self.header {
            roots.extend([entry.top, entry.control, entry.mode]);
            roots.extend(entry.status.roots());
            roots.extend([
                entry.metadata.fip,
                entry.metadata.fdp,
                entry.metadata.opcode,
                entry.metadata.cs,
                entry.metadata.ds,
            ]);
        }
        roots.sort_unstable();
        roots.dedup();
        roots
    }
}

pub(in crate::cpu::core) fn analyze_parameters(
    program: &ExecutableProgram,
    values: &RegionPlan,
) -> Vec<FpParameters> {
    let count = program.blocks.len();
    let mut active = vec![false; count];
    let mut dirty = vec![0u8; count];
    let mut tag_dirty = vec![0u8; count];
    loop {
        let mut changed = false;
        for index in 0..count {
            let body = &program.blocks[index];
            let mut outgoing_active = active[index];
            let mut reset = false;
            for step in &body.steps {
                if let ExecutableStep::Effect(effect) = step {
                    if matches!(effect.kind, EffectKind::Exact(_))
                        && matches!(effect.contract.fp, FpEffectClass::Barrier)
                    {
                        outgoing_active = false;
                        reset = true;
                    }
                    if matches!(effect.kind, EffectKind::Fp(_)) {
                        outgoing_active = true;
                    }
                }
            }
            let delta = body.exit.fp.top_delta.rem_euclid(8) as u32;
            let outgoing = (if reset {
                0
            } else {
                dirty[index].rotate_right(delta)
            }) | body.exit.fp.written;
            let outgoing_tags = (if reset {
                0
            } else {
                tag_dirty[index].rotate_right(delta)
            }) | body.exit.fp.tag_written;
            for edge in &values.block(BlockId(index)).edges {
                let target = edge.target.0;
                let next = (
                    active[target] | outgoing_active,
                    dirty[target] | outgoing,
                    tag_dirty[target] | outgoing_tags,
                );
                if next != (active[target], dirty[target], tag_dirty[target]) {
                    (active[target], dirty[target], tag_dirty[target]) = next;
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    let mut parameters:Vec<_>=(0..count).map(|index| {
    let block=BlockId(index);let body=&program.blocks[index];let entry=&body.entry.fp;
    let roots:Vec<_>=body.effects().flat_map(|effect|effect.inputs()).collect();
    let live=program.dependency_order(&roots);
    let mut payloads=dirty[block.0];let mut tags=tag_dirty[block.0];
    for logical in 0..8 {
        if entry.payload[logical].roots().iter().any(|value|live.contains(value)) {payloads|=1<<logical;}
        if live.contains(&entry.tags[logical]) {tags|=1<<logical;}
    }
    let uses_fp=body.effects().any(|effect|matches!(effect.kind,EffectKind::Fp(_)));
    let imports=uses_fp||body.effects().any(|effect|matches!(effect.contract.fp,FpEffectClass::Observe|FpEffectClass::Barrier));
    let arithmetic=body.effects().any(|effect|matches!(&effect.kind,EffectKind::Fp(fp) if matches!(fp.kind,FpEffectKind::Binary {..})));
    FpParameters {block,payloads,tags,header:active[block.0]||imports,imports,arithmetic}
    }).collect();
    carry_parameters(program, values, &mut parameters);
    parameters
}

#[derive(Clone, Copy, Default)]
struct EntryDemand {
    payloads: u8,
    tags: u8,
    header: bool,
}
impl EntryDemand {
    fn merge(&mut self, other: Self) {
        self.payloads |= other.payloads;
        self.tags |= other.tags;
        self.header |= other.header;
    }
    fn from_roots(program: &ExecutableProgram, entry: &FpRecovery, roots: &[ExecValueId]) -> Self {
        let live = program.dependency_order(roots);
        let mut result = Self::default();
        for logical in 0..8 {
            if entry.payload[logical]
                .roots()
                .iter()
                .any(|id| live.contains(id))
            {
                result.payloads |= 1 << logical;
            }
            if live.contains(&entry.tags[logical]) {
                result.tags |= 1 << logical;
            }
        }
        let header = [
            entry.top,
            entry.control,
            entry.mode,
            entry.status.bits,
            entry.status.pending,
            entry.status.left,
            entry.status.right,
            entry.status.result,
            entry.metadata.fip,
            entry.metadata.fdp,
            entry.metadata.opcode,
            entry.metadata.cs,
            entry.metadata.ds,
        ];
        result.header =
            result.payloads != 0 || result.tags != 0 || header.iter().any(|id| live.contains(id));
        result
    }
}

/// A clean value is still an argument when a downstream block consumes it.
/// Never reconstruct an unpassed predicted-native input from a raw pool lane.
/// Dependencies are computed once; only the bounded argument masks iterate.
fn carry_parameters(
    program: &ExecutableProgram,
    values: &RegionPlan,
    parameters: &mut [FpParameters],
) {
    let count = parameters.len();
    let transfers: Vec<_> = (0..count)
        .map(|index| {
            let body = &program.blocks[index];
            let entry = &body.entry.fp;
            let exit = &body.exit.fp;
            let payloads = std::array::from_fn::<_, 8, _>(|i| {
                EntryDemand::from_roots(program, entry, &exit.payload[i].roots())
            });
            let tags = std::array::from_fn::<_, 8, _>(|i| {
                EntryDemand::from_roots(program, entry, &[exit.tags[i]])
            });
            let header = EntryDemand::from_roots(
                program,
                entry,
                &[
                    exit.top,
                    exit.control,
                    exit.mode,
                    exit.status.bits,
                    exit.status.pending,
                    exit.status.left,
                    exit.status.right,
                    exit.status.result,
                    exit.metadata.fip,
                    exit.metadata.fdp,
                    exit.metadata.opcode,
                    exit.metadata.cs,
                    exit.metadata.ds,
                ],
            );
            (payloads, tags, header)
        })
        .collect();
    let mut predecessors = vec![Vec::new(); count];
    for source in 0..count {
        for edge in &values.block(BlockId(source)).edges {
            predecessors[edge.target.0].push(source);
        }
    }
    let mut queued = vec![true; count];
    let mut pending: std::collections::VecDeque<_> = (0..count).collect();
    while let Some(target) = pending.pop_front() {
        queued[target] = false;
        for &source in &predecessors[target] {
            let required = &parameters[target];
            let (payloads, tags, header) = &transfers[source];
            let mut demand = EntryDemand::default();
            for logical in 0..8 {
                if required.payloads & (1 << logical) != 0 {
                    demand.merge(payloads[logical]);
                }
                if required.tags & (1 << logical) != 0 {
                    demand.merge(tags[logical]);
                }
            }
            if required.header {
                demand.merge(*header);
            }
            let old = &parameters[source];
            let next = (
                old.payloads | demand.payloads,
                old.tags | demand.tags,
                old.header | demand.header,
            );
            if next != (old.payloads, old.tags, old.header) {
                let current = &mut parameters[source];
                (current.payloads, current.tags, current.header) = next;
                if !queued[source] {
                    queued[source] = true;
                    pending.push_back(source);
                }
            }
        }
    }
}

/// Lower supported x87 instructions to actual typed operands/results/effects.
/// Each Binary remains a distinct ordered rounding point; it is not interned.
pub(in crate::cpu::core) fn lower_effect(
    builder: &mut ExecutableBuilder,
    operation: Operation,
    point: ProgramPoint,
    before: &Recovery,
    after_frame: FrameStateId,
    values: &RegionPlan,
    effect: ExecEffectId,
) -> Option<(FpEffect, FpRecovery)> {
    let (operation, address, opcode, waiting) = match operation {
        Operation::X87 {
            operation,
            address,
            opcode,
            waiting,
        } => (operation, address, opcode, waiting),
        Operation::X87ControlToMemory { address } => {
            (X87Operation::StoreControl, Some(address), 0, false)
        }
        _ => return None,
    };
    let mut after = before.fp.clone();
    let mut occupied = Vec::new();
    let mut empty = Vec::new();
    let mut outputs = Vec::new();
    let mut status_effect = StatusEffect::Preserve;
    let mut read = |logical: usize| {
        if !occupied.iter().any(|&(i, _)| i == logical as u8) {
            occupied.push((logical as u8, before.fp.tags[logical]));
        }
        before.fp.payload[logical]
    };
    let native = |builder: &mut ExecutableBuilder,
                  outputs: &mut Vec<ExecValueId>,
                  output: u8,
                  logical: usize| {
        let value = result(builder, effect, output, ValueType::F64);
        outputs.push(value);
        FpPayload::Native {
            value,
            domain: domain(values, after_frame, logical),
            epoch: values.frame(after_frame).fp.control_epoch,
        }
    };
    let kind = match operation {
        X87Operation::LoadControl => {
            let address = builder.address_value(address?, &before.gpr)?;
            let control = result(builder, effect, 0, ValueType::I32);
            outputs.push(control);
            after.control = control;
            after.mode = builder.push_value(
                ValueType::I32,
                ValueNode::Fp(FpValueNode::PrecisionMode { control }),
            );
            after.epoch = values.frame(after_frame).fp.control_epoch;
            let no_certificate = constant(builder, 0);
            for payload in &mut after.payload {
                *payload = match *payload {
                    FpPayload::Native { value, .. } => FpPayload::Native {
                        value,
                        domain: NativeDomain::Exact,
                        epoch: after.epoch,
                    },
                    FpPayload::Tagged {
                        low,
                        high,
                        native,
                        valid,
                        ..
                    } => FpPayload::Tagged {
                        low,
                        high,
                        native,
                        valid,
                        certificate: no_certificate,
                    },
                    opaque => opaque,
                };
            }
            status_effect = StatusEffect::Control;
            FpEffectKind::LoadControl {
                address,
                result: control,
            }
        }
        X87Operation::StoreControl => FpEffectKind::StoreControl {
            address: builder.address_value(address?, &before.gpr)?,
        },
        X87Operation::Load(
            format @ (X87Format::Integer16 | X87Format::Integer32 | X87Format::Integer64),
        ) => {
            let address = builder.address_value(address?, &before.gpr)?;
            empty.push((7, before.fp.tags[7]));
            after.push();
            let payload = if format == X87Format::Integer64 {
                let low = result(builder, effect, 0, ValueType::I64);
                let high = result(builder, effect, 1, ValueType::I32);
                let native = result(builder, effect, 2, ValueType::F64);
                let valid = result(builder, effect, 3, ValueType::I32);
                let certificate = result(builder, effect, 9, ValueType::I32);
                outputs.extend([low, high, native, valid, certificate]);
                FpPayload::Tagged {
                    low,
                    high,
                    native,
                    valid,
                    certificate,
                }
            } else {
                let value = result(builder, effect, 0, ValueType::F64);
                outputs.push(value);
                FpPayload::Native {
                    value,
                    domain: if format == X87Format::Integer16 {
                        NativeDomain::Native24
                    } else {
                        NativeDomain::Bounded
                    },
                    epoch: after.epoch,
                }
            };
            after.write(builder, 0, payload);
            status_effect = StatusEffect::ClearC1;
            FpEffectKind::IntegerLoad {
                address,
                format,
                result: payload,
            }
        }
        X87Operation::Store {
            format: format @ (X87Format::Integer16 | X87Format::Integer32 | X87Format::Integer64),
            truncate,
            pop,
        } => {
            let source = read(0);
            let address = builder.address_value(address?, &before.gpr)?;
            if pop {
                after.pop(builder);
            }
            status_effect = StatusEffect::Store;
            FpEffectKind::IntegerStore {
                address,
                source,
                format,
                truncate,
            }
        }
        X87Operation::Load(X87Format::Float32) => {
            let address = builder.address_value(address?, &before.gpr)?;
            empty.push((7, before.fp.tags[7]));
            after.push();
            let payload = native(builder, &mut outputs, 0, 0);
            after.write(builder, 0, payload);
            let FpPayload::Native { value: result, .. } = payload else {
                unreachable!()
            };
            status_effect = StatusEffect::ClearC1;
            FpEffectKind::Load32 { address, result }
        }
        X87Operation::LoadRegister { index } => {
            let source = read(index as usize);
            empty.push((7, before.fp.tags[7]));
            after.push();
            after.write(builder, 0, source);
            FpEffectKind::Transfer { source }
        }
        X87Operation::StoreRegister { index, pop } => {
            let source = read(0);
            after.write(builder, index as usize, source);
            if pop {
                after.pop(builder);
            }
            FpEffectKind::Transfer { source }
        }
        X87Operation::Exchange { index } => {
            let left = read(0);
            let right = read(index as usize);
            after.write(builder, 0, right);
            after.write(builder, index as usize, left);
            status_effect = StatusEffect::ClearC1;
            FpEffectKind::Exchange { left, right }
        }
        X87Operation::Binary {
            kind,
            destination,
            source,
            pop,
        } => {
            let left = read(destination as usize);
            let right = read(source as usize);
            // The after-frame is already in post-pop logical coordinates.
            // Its neighboring preserved value must never certify this result.
            let result_logical = (usize::from(destination) + if pop { 7 } else { 0 }) & 7;
            let payload = native(builder, &mut outputs, 0, result_logical);
            after.write(builder, destination as usize, payload);
            if pop {
                after.pop(builder);
            }
            let FpPayload::Native { value: result, .. } = payload else {
                unreachable!()
            };
            let exact_product = kind == X87Binary::Multiply
                && matches!(
                    left,
                    FpPayload::Native {
                        domain: NativeDomain::Native24,
                        ..
                    }
                )
                && matches!(
                    right,
                    FpPayload::Native {
                        domain: NativeDomain::Native24,
                        ..
                    }
                );
            status_effect = StatusEffect::Arithmetic;
            FpEffectKind::Binary {
                kind,
                left,
                right: FpOperand::Payload(right),
                result,
                exact_product,
            }
        }
        X87Operation::BinaryMemory {
            kind,
            format: X87Format::Float32,
        } => {
            let left = read(0);
            let address = builder.address_value(address?, &before.gpr)?;
            let payload = native(builder, &mut outputs, 0, 0);
            after.write(builder, 0, payload);
            let FpPayload::Native { value: result, .. } = payload else {
                unreachable!()
            };
            status_effect = StatusEffect::Arithmetic;
            FpEffectKind::Binary {
                kind,
                left,
                right: FpOperand::Memory32(address),
                result,
                exact_product: false,
            }
        }
        X87Operation::ChangeSign | X87Operation::Absolute => {
            let source = read(0);
            let payload = match source {
                FpPayload::Native { domain, epoch, .. } => {
                    let value = result(builder, effect, 0, ValueType::F64);
                    outputs.push(value);
                    FpPayload::Native {
                        value,
                        domain,
                        epoch,
                    }
                }
                _ => {
                    let low = result(builder, effect, 0, ValueType::I64);
                    let high = result(builder, effect, 1, ValueType::I32);
                    let native = result(builder, effect, 2, ValueType::F64);
                    let valid = result(builder, effect, 3, ValueType::I32);
                    outputs.extend([low, high, native, valid]);
                    FpPayload::Tagged {
                        low,
                        high,
                        native,
                        valid,
                        certificate: constant(builder, 0),
                    }
                }
            };
            after.write(builder, 0, payload);
            status_effect = StatusEffect::ClearC1;
            FpEffectKind::Sign {
                source,
                absolute: matches!(operation, X87Operation::Absolute),
                result: payload,
            }
        }
        X87Operation::Store {
            format: format @ (X87Format::Float32 | X87Format::Float64),
            truncate: false,
            pop,
        } => {
            let source = read(0);
            let address = builder.address_value(address?, &before.gpr)?;
            if pop {
                after.pop(builder);
            }
            status_effect = StatusEffect::Store;
            FpEffectKind::Store {
                address,
                source,
                format,
            }
        }
        _ => return None,
    };
    if !matches!(status_effect, StatusEffect::Preserve) {
        let bits = result(builder, effect, 4, ValueType::I32);
        let pending = result(builder, effect, 5, ValueType::I32);
        outputs.extend([bits, pending]);
        after.status = if matches!(status_effect, StatusEffect::Arithmetic) {
            let left = result(builder, effect, 6, ValueType::F64);
            let right = result(builder, effect, 7, ValueType::F64);
            let result = result(builder, effect, 8, ValueType::F64);
            outputs.extend([left, right, result]);
            FpStatus {
                bits,
                pending,
                left,
                right,
                result,
            }
        } else {
            FpStatus {
                bits,
                pending,
                ..before.fp.status
            }
        };
    }
    if operation.is_computation() {
        after.metadata.fip = constant(
            builder,
            match values.frame(before.frame).pc {
                crate::cpu::core::region::ResumePc::Exact(pc) => pc,
                _ => 0,
            },
        );
        after.metadata.opcode = constant(builder, u32::from(opcode));
        // Segment selector reads are immutable value inputs, not guest memory loads.
        after.metadata.cs = input(
            builder,
            point,
            FpInput::Selector(crate::cpu::x86::SegmentRegister::Cs),
            ValueType::I32,
        );
        let address = match &kind {
            FpEffectKind::Load32 { address, .. }
            | FpEffectKind::Store { address, .. }
            | FpEffectKind::IntegerLoad { address, .. }
            | FpEffectKind::IntegerStore { address, .. } => Some(address),
            FpEffectKind::Binary {
                right: FpOperand::Memory32(address),
                ..
            } => Some(address),
            _ => None,
        };
        if let Some(address) = address {
            after.metadata.fdp = address.offset;
            after.metadata.ds = input(
                builder,
                point,
                FpInput::Selector(address.address.resolved_segment()),
                ValueType::I32,
            );
        }
    }
    Some((
        FpEffect {
            id: effect,
            point,
            before_frame: before.frame,
            after_frame,
            kind,
            waiting,
            control: before.fp.control,
            mode: before.fp.mode,
            epoch: before.fp.epoch,
            occupied,
            empty,
            status_before: before.fp.status,
            status_after: after.status,
            status_effect,
            outputs,
        },
        after,
    ))
}
