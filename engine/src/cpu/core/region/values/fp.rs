use super::*;
use crate::cpu::x86::{X87Format, ir::Operation};

fn input(plan: &mut RegionPlan, block: BlockId, field: InputField, ty: ValueType) -> ValueId {
    plan.allocate_value(ty, ValueKind::Input { block, field })
}

pub(super) fn entry(
    plan: &mut RegionPlan,
    block: BlockId,
    pc: ResumePc,
    facts: flow::Facts,
) -> (FrameState, Vec<ProofId>) {
    let gpr = std::array::from_fn(|i| {
        let value = input(plan, block, InputField::Gpr(i as u8), ValueType::I32);
        plan.constant_facts[value.0] = facts.gpr[i];
        value
    });
    let flags_effect = plan.allocate_effect(EffectKind::Flags, None, vec![]);
    let memory_effect = plan.allocate_effect(EffectKind::Memory, None, vec![]);
    let control_epoch = plan.allocate_effect(EffectKind::Control, None, vec![]);
    let status_effect = plan.allocate_effect(EffectKind::Status, None, vec![]);
    let metadata_effect = plan.allocate_effect(EffectKind::Metadata, None, vec![]);
    let (mut fp, mut guards) = input_fp(
        plan,
        block,
        control_epoch,
        status_effect,
        metadata_effect,
        facts.native,
    );
    if facts.control_admitted {
        guards.push(plan.allocate_proof(Proof {
            value: None,
            kind: ProofKind::Control,
            epoch: control_epoch,
            at: ProgramPoint { block, prefix: 0 },
            conditional: None,
        }));
    }
    for (i, fact) in facts.tags.into_iter().enumerate() {
        if let Some(occupied) = fact {
            guards.push(plan.allocate_proof(Proof {
                value: Some(fp.payload[i].values()[0]),
                kind: if occupied {
                    ProofKind::Occupied
                } else {
                    ProofKind::Empty
                },
                epoch: control_epoch,
                at: ProgramPoint { block, prefix: 0 },
                conditional: None,
            }));
            if !occupied {
                fp.tags[i] = TagFact::Empty;
            }
        }
    }
    (
        FrameState {
            pc,
            retired: 0,
            gpr,
            flags_effect,
            memory_effect,
            fp,
        },
        guards,
    )
}

fn input_fp(
    plan: &mut RegionPlan,
    block: BlockId,
    control_epoch: EffectId,
    status_effect: EffectId,
    metadata_effect: EffectId,
    native_facts: [bool; 8],
) -> (FpFrame, Vec<ProofId>) {
    let point = ProgramPoint { block, prefix: 0 };
    let mut guards = Vec::new();
    let payload = std::array::from_fn(|i| {
        let cell = StorageCell(i as u8);
        let native = input(plan, block, InputField::FpNative(i as u8), ValueType::F64);
        plan.bind(native, cell, StorageLane::Native);
        let lo = input(plan, block, InputField::FpLow(i as u8), ValueType::I64);
        let hi = input(plan, block, InputField::FpHigh(i as u8), ValueType::I32);
        let valid = input(plan, block, InputField::FpValid(i as u8), ValueType::I32);
        plan.bind(lo, cell, StorageLane::Low);
        plan.bind(hi, cell, StorageLane::High);
        plan.bind(valid, cell, StorageLane::Valid);
        let proof = plan.allocate_proof(Proof {
            value: Some(native),
            kind: ProofKind::ExactF64,
            epoch: control_epoch,
            at: point,
            conditional: Some(valid),
        });
        // Internal predecessors can predict a native import, but public secondary entry
        // still owns arbitrary raw80. The conditional discriminator remains authoritative.
        if native_facts[i] {
            guards.push(proof);
        }
        FpValue::Tagged {
            lo,
            hi,
            native,
            valid,
            proof,
        }
    });
    let tags = std::array::from_fn(|i| {
        TagFact::Unknown(input(
            plan,
            block,
            InputField::FpTag(i as u8),
            ValueType::I32,
        ))
    });
    let top = input(plan, block, InputField::FpTop, ValueType::I32);
    (
        FpFrame {
            payload,
            tags,
            top,
            top_delta: 0,
            control_epoch,
            status_effect,
            metadata_effect,
            metadata: MetadataRecipe::default(),
        },
        guards,
    )
}

pub(super) fn operation(
    operation: Operation,
) -> Option<(X87Operation, Option<EffectiveAddress>, u16)> {
    Some(match operation {
        Operation::X87 {
            operation,
            address,
            opcode,
            ..
        } => (operation, address, opcode),
        Operation::X87StatusToAx => (X87Operation::StatusToAx, None, 0x7e0),
        Operation::InitializeX87 => (X87Operation::Initialize, None, 0x3e3),
        Operation::ClearX87Exceptions => (X87Operation::ClearExceptions, None, 0x3e2),
        Operation::X87StatusToMemory { address } => (X87Operation::StoreStatus, Some(address), 0),
        Operation::X87ControlToMemory { address } => (X87Operation::StoreControl, Some(address), 0),
        _ => return None,
    })
}

pub(super) fn canonical_numeric(operation: Operation) -> bool {
    if let Some((operation, ..)) = self::operation(operation) {
        !flow::direct_fp(operation)
            && !matches!(operation, X87Operation::Wait | X87Operation::StatusToAx)
    } else {
        crate::cpu::core::emitter::is_native(&operation)
    }
}

pub(super) fn kill(
    plan: &mut RegionPlan,
    frame: &mut FrameState,
    shape: &mut FpShape,
    point: ProgramPoint,
) {
    let control = plan.allocate_effect(
        EffectKind::Control,
        Some(point),
        vec![frame.fp.control_epoch],
    );
    let status = plan.allocate_effect(
        EffectKind::Status,
        Some(point),
        vec![frame.fp.status_effect],
    );
    let metadata = plan.allocate_effect(
        EffectKind::Metadata,
        Some(point),
        vec![frame.fp.metadata_effect],
    );
    frame.fp = input_fp(plan, point.block, control, status, metadata, [false; 8]).0;
    frame.fp.metadata.canonical = true;
    *shape = FpShape {
        cells: std::array::from_fn(|i| StorageCell(i as u8)),
        inherited_dirty: std::array::from_fn(|i| Some(i as u8)),
        written: 0,
        tags: std::array::from_fn(|i| TagRecipe::Entry(i as u8)),
        top_delta: 0,
    };
}

fn proof(
    plan: &mut RegionPlan,
    frame: &FrameState,
    point: ProgramPoint,
    available: &mut Vec<ProofId>,
    value: Option<ValueId>,
    kind: ProofKind,
) -> ProofId {
    if let Some(&id) = available.iter().find(|&&id| {
        let old = plan.proof(id);
        old.value == value
            && old.kind == kind
            && old.epoch == frame.fp.control_epoch
            && old.conditional.is_none()
    }) {
        return id;
    }
    let id = plan.allocate_proof(Proof {
        value,
        kind,
        epoch: frame.fp.control_epoch,
        at: point,
        conditional: None,
    });
    available.push(id);
    id
}

fn occupied(
    plan: &mut RegionPlan,
    frame: &FrameState,
    point: ProgramPoint,
    available: &mut Vec<ProofId>,
    index: usize,
) {
    let value = frame.fp.payload[index].values()[0];
    proof(
        plan,
        frame,
        point,
        available,
        Some(value),
        ProofKind::Occupied,
    );
}

fn admit(
    plan: &mut RegionPlan,
    frame: &mut FrameState,
    point: ProgramPoint,
    available: &mut Vec<ProofId>,
    index: usize,
) -> FpValue {
    occupied(plan, frame, point, available, index);
    let native = match frame.fp.payload[index] {
        FpValue::ExactF64(native, _) | FpValue::Tagged { native, .. } => native,
        FpValue::Opaque80 { lo, .. } => {
            let cell = plan.storage(lo).unwrap().cell;
            let native = lower::definition(plan, point, 40 + index as u8, ValueType::F64);
            plan.bind(native, cell, StorageLane::Native);
            native
        }
    };
    let exact = proof(
        plan,
        frame,
        point,
        available,
        Some(native),
        ProofKind::ExactF64,
    );
    proof(
        plan,
        frame,
        point,
        available,
        Some(native),
        ProofKind::BoundedF64,
    );
    frame.fp.payload[index] = FpValue::ExactF64(native, exact);
    proof(
        plan,
        frame,
        point,
        available,
        Some(native),
        ProofKind::Occupied,
    );
    frame.fp.payload[index]
}

fn free_cell(shape: &FpShape, destination: usize) -> StorageCell {
    (0..8)
        .map(StorageCell)
        .find(|cell| {
            !shape
                .cells
                .iter()
                .enumerate()
                .any(|(i, occupied)| i != destination && occupied == cell)
        })
        .expect("eight live payloads fit eight cells after replacing one destination")
}

fn native_result(
    plan: &mut RegionPlan,
    frame: &mut FrameState,
    shape: &mut FpShape,
    point: ProgramPoint,
    available: &mut Vec<ProofId>,
    destination: usize,
    native24: bool,
) {
    let cell = free_cell(shape, destination);
    let value = lower::definition(plan, point, 16 + destination as u8, ValueType::F64);
    plan.bind(value, cell, StorageLane::Native);
    let exact = proof(
        plan,
        frame,
        point,
        available,
        Some(value),
        ProofKind::ExactF64,
    );
    proof(
        plan,
        frame,
        point,
        available,
        Some(value),
        ProofKind::BoundedF64,
    );
    if native24 {
        proof(
            plan,
            frame,
            point,
            available,
            Some(value),
            ProofKind::Native24,
        );
    }
    frame.fp.payload[destination] = FpValue::ExactF64(value, exact);
    frame.fp.tags[destination] = TagFact::Value(value);
    shape.cells[destination] = cell;
    written(shape, destination);
}

fn written(shape: &mut FpShape, destination: usize) {
    shape.written |= 1 << destination;
    shape.inherited_dirty[destination] = None;
}

fn push(frame: &mut FrameState, shape: &mut FpShape) {
    frame.fp.payload.rotate_right(1);
    frame.fp.tags.rotate_right(1);
    shape.cells.rotate_right(1);
    shape.inherited_dirty.rotate_right(1);
    shape.written = shape.written.rotate_left(1);
    frame.fp.top_delta = (frame.fp.top_delta - 1).rem_euclid(8);
}

fn pop(frame: &mut FrameState, shape: &mut FpShape) {
    // The popped physical register keeps its exact payload even though its tag is empty.
    frame.fp.tags[0] = TagFact::Empty;
    frame.fp.payload.rotate_left(1);
    frame.fp.tags.rotate_left(1);
    shape.cells.rotate_left(1);
    shape.inherited_dirty.rotate_left(1);
    shape.written = shape.written.rotate_right(1);
    frame.fp.top_delta = (frame.fp.top_delta + 1).rem_euclid(8);
}

#[allow(clippy::too_many_arguments)]
pub(super) fn lower(
    plan: &mut RegionPlan,
    frame: &mut FrameState,
    shape: &mut FpShape,
    operation: X87Operation,
    address: Option<EffectiveAddress>,
    opcode: u16,
    point: ProgramPoint,
    available: &mut Vec<ProofId>,
    proofs: &mut Vec<ProofId>,
    inputs: &mut Vec<ValueId>,
    outputs: &mut Vec<ValueId>,
    effects: &mut Vec<EffectId>,
) -> FpStep {
    let mut step = FpStep {
        operation,
        address,
        opcode,
        inputs: vec![],
        outputs: vec![],
        kills_proofs: false,
    };
    if matches!(operation, X87Operation::Wait | X87Operation::StatusToAx) {
        return step;
    }
    if !flow::direct_fp(operation) {
        kill(plan, frame, shape, point);
        available.clear();
        effects.extend([
            frame.fp.control_epoch,
            frame.fp.status_effect,
            frame.fp.metadata_effect,
        ]);
        step.kills_proofs = true;
        return step;
    }
    match operation {
        X87Operation::Load(X87Format::Float32) => {
            proof(
                plan,
                frame,
                point,
                available,
                Some(frame.fp.payload[7].values()[0]),
                ProofKind::Empty,
            );
            push(frame, shape);
            native_result(plan, frame, shape, point, available, 0, true);
            step.outputs.push(frame.fp.payload[0]);
        }
        X87Operation::LoadRegister { index } => {
            let index = index as usize;
            occupied(plan, frame, point, available, index);
            proof(
                plan,
                frame,
                point,
                available,
                Some(frame.fp.payload[7].values()[0]),
                ProofKind::Empty,
            );
            let payload = frame.fp.payload[index];
            let tag = frame.fp.tags[index];
            let cell = shape.cells[index];
            step.inputs.push(payload);
            push(frame, shape);
            frame.fp.payload[0] = payload;
            frame.fp.tags[0] = tag;
            shape.cells[0] = cell;
            written(shape, 0);
            step.outputs.push(payload);
        }
        X87Operation::StoreRegister {
            index,
            pop: should_pop,
        } => {
            occupied(plan, frame, point, available, 0);
            let index = index as usize;
            let payload = frame.fp.payload[0];
            step.inputs.push(payload);
            frame.fp.payload[index] = payload;
            frame.fp.tags[index] = frame.fp.tags[0];
            shape.cells[index] = shape.cells[0];
            written(shape, index);
            step.outputs.push(payload);
            if should_pop {
                pop(frame, shape);
            }
        }
        X87Operation::Exchange { index } => {
            let index = index as usize;
            occupied(plan, frame, point, available, 0);
            occupied(plan, frame, point, available, index);
            step.inputs
                .extend([frame.fp.payload[0], frame.fp.payload[index]]);
            frame.fp.payload.swap(0, index);
            frame.fp.tags.swap(0, index);
            shape.cells.swap(0, index);
            shape.inherited_dirty.swap(0, index);
            let a = shape.written & 1 != 0;
            let b = shape.written & (1 << index) != 0;
            shape.written &= !(1 | (1 << index));
            shape.written |= u8::from(a) << index | u8::from(b);
            written(shape, 0);
            written(shape, index);
            step.outputs
                .extend([frame.fp.payload[0], frame.fp.payload[index]]);
        }
        X87Operation::Binary {
            destination,
            source,
            pop: should_pop,
            ..
        } => {
            proof(plan, frame, point, available, None, ProofKind::Control);
            let destination = destination as usize;
            let source = source as usize;
            step.inputs
                .push(admit(plan, frame, point, available, destination));
            step.inputs
                .push(admit(plan, frame, point, available, source));
            native_result(plan, frame, shape, point, available, destination, false);
            step.outputs.push(frame.fp.payload[destination]);
            if should_pop {
                pop(frame, shape);
            }
        }
        X87Operation::BinaryMemory {
            format: X87Format::Float32,
            ..
        } => {
            proof(plan, frame, point, available, None, ProofKind::Control);
            step.inputs.push(admit(plan, frame, point, available, 0));
            native_result(plan, frame, shape, point, available, 0, false);
            step.outputs.push(frame.fp.payload[0]);
        }
        X87Operation::ChangeSign | X87Operation::Absolute => {
            occupied(plan, frame, point, available, 0);
            step.inputs.push(frame.fp.payload[0]);
            let cell = free_cell(shape, 0);
            // The raw80 branch changes only its sign bit; a native cached payload changes
            // in parallel. Keeping the discriminator avoids inventing an exact-f64 proof.
            let lo = lower::definition(plan, point, 24, ValueType::I64);
            let hi = lower::definition(plan, point, 25, ValueType::I32);
            let native = lower::definition(plan, point, 26, ValueType::F64);
            let valid = lower::definition(plan, point, 27, ValueType::I32);
            for (value, lane) in [
                (lo, StorageLane::Low),
                (hi, StorageLane::High),
                (native, StorageLane::Native),
                (valid, StorageLane::Valid),
            ] {
                plan.bind(value, cell, lane);
            }
            let exact = plan.allocate_proof(Proof {
                value: Some(native),
                kind: ProofKind::ExactF64,
                epoch: frame.fp.control_epoch,
                at: point,
                conditional: Some(valid),
            });
            let payload = FpValue::Tagged {
                lo,
                hi,
                native,
                valid,
                proof: exact,
            };
            frame.fp.payload[0] = payload;
            frame.fp.tags[0] = TagFact::Value(native);
            shape.cells[0] = cell;
            written(shape, 0);
            step.outputs.push(payload);
        }
        X87Operation::Store {
            pop: should_pop, ..
        } => {
            occupied(plan, frame, point, available, 0);
            step.inputs.push(frame.fp.payload[0]);
            if should_pop {
                pop(frame, shape);
            }
        }
        _ => unreachable!("direct FP classification and transfer agree"),
    }
    let pc = match frame.pc {
        ResumePc::Exact(pc) => pc,
        ResumePc::Computed => 0,
    };
    frame.fp.metadata.instruction = Some((pc, opcode));
    frame.fp.metadata.canonical = false;
    if let Some(address) = address {
        frame.fp.metadata.data = Some(AddressPlan {
            address,
            base: address.base.map(|r| frame.gpr[r.index()]),
            index: address.index.map(|r| frame.gpr[r.index()]),
            memory_epoch: frame.memory_effect,
        });
    }
    frame.fp.status_effect = plan.allocate_effect(
        EffectKind::Status,
        Some(point),
        vec![frame.fp.status_effect],
    );
    frame.fp.metadata_effect = plan.allocate_effect(
        EffectKind::Metadata,
        Some(point),
        vec![frame.fp.metadata_effect],
    );
    effects.extend([frame.fp.status_effect, frame.fp.metadata_effect]);
    inputs.extend(step.inputs.iter().flat_map(|value| value.values()));
    outputs.extend(step.outputs.iter().flat_map(|value| value.values()));
    // These are success-prefix proofs. Consumers ask proofs_at(before) so a faulting
    // admission never grants its future fact to the current instruction or a side exit.
    *proofs = available.clone();
    step
}
