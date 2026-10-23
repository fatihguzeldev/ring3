mod access;
mod flow;
mod fp;
mod gpr;
mod lower;
#[cfg(test)]
mod tests;

use super::{BlockId, Region, ResumePc};
use crate::cpu::x86::{X87Operation, ir::EffectiveAddress};

pub(in crate::cpu::core) use access::{AccessGroup, AccessMember};

macro_rules! id {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub(in crate::cpu::core) struct $name(pub usize);
    };
}
id!(ValueId);
id!(ProofId);
id!(EffectId);
id!(FrameStateId);
id!(FpShapeId);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(in crate::cpu::core) struct StorageCell(pub u8);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::cpu::core) enum StorageLane {
    Native,
    Low,
    High,
    Valid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::cpu::core) struct Storage {
    pub cell: StorageCell,
    pub lane: StorageLane,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::cpu::core) struct ProgramPoint {
    pub block: BlockId,
    pub prefix: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::cpu::core) enum ValueType {
    I32,
    I64,
    F64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::cpu::core) enum InputField {
    Gpr(u8),
    FpLow(u8),
    FpHigh(u8),
    FpNative(u8),
    FpValid(u8),
    FpTag(u8),
    FpTop,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::cpu::core) enum ValueKind {
    Input { block: BlockId, field: InputField },
    Constant(u32),
    Instruction { point: ProgramPoint, output: u8 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::cpu::core) struct Value {
    pub ty: ValueType,
    pub kind: ValueKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::cpu::core) enum ProofKind {
    ExactF64,
    Native24,
    BoundedF64,
    Occupied,
    Empty,
    Control,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::cpu::core) struct Proof {
    pub value: Option<ValueId>,
    pub kind: ProofKind,
    pub epoch: EffectId,
    pub at: ProgramPoint,
    pub conditional: Option<ValueId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::cpu::core) enum EffectKind {
    Flags,
    Status,
    Metadata,
    Control,
    Memory,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::cpu::core) struct Effect {
    pub kind: EffectKind,
    pub at: Option<ProgramPoint>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::cpu::core) enum FpValue {
    ExactF64(ValueId, ProofId),
    Opaque80 {
        lo: ValueId,
        hi: ValueId,
    },
    Tagged {
        lo: ValueId,
        hi: ValueId,
        native: ValueId,
        valid: ValueId,
        proof: ProofId,
    },
}

impl FpValue {
    pub fn contains(self, value: ValueId) -> bool {
        match self {
            Self::ExactF64(native, _) => native == value,
            Self::Opaque80 { lo, hi } => lo == value || hi == value,
            Self::Tagged {
                lo,
                hi,
                native,
                valid,
                ..
            } => [lo, hi, native, valid].contains(&value),
        }
    }
    pub fn values(self) -> Vec<ValueId> {
        match self {
            Self::ExactF64(value, _) => vec![value],
            Self::Opaque80 { lo, hi } => vec![lo, hi],
            Self::Tagged {
                lo,
                hi,
                native,
                valid,
                ..
            } => vec![lo, hi, native, valid],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::cpu::core) enum TagFact {
    Unknown(ValueId),
    Empty,
    Occupied,
    Zero,
    Normal,
    Special,
    Value(ValueId),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::cpu::core) struct FpFrame {
    // Empty tags still own their physical payload; stack renaming never drops it.
    pub payload: [FpValue; 8],
    pub tags: [TagFact; 8],
    pub top: ValueId,
    pub top_delta: i8,
    pub control_epoch: EffectId,
    pub status_effect: EffectId,
    pub metadata_effect: EffectId,
    pub metadata: MetadataRecipe,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::cpu::core) struct FrameState {
    pub pc: ResumePc,
    pub retired: u32,
    pub gpr: [ValueId; 8],
    pub flags_effect: EffectId,
    pub memory_effect: EffectId,
    pub fp: FpFrame,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::cpu::core) struct AddressPlan {
    pub address: EffectiveAddress,
    pub base: Option<ValueId>,
    pub index: Option<ValueId>,
    pub memory_epoch: EffectId,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::cpu::core) struct MetadataRecipe {
    pub instruction: Option<(u32, u16)>,
    pub data: Option<AddressPlan>,
    pub canonical: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(in crate::cpu::core) struct FpShape {
    pub cells: [StorageCell; 8],
    pub inherited_dirty: [Option<u8>; 8],
    pub written: u8,
    pub tag_written: u8,
    pub tags: [TagRecipe; 8],
    pub top_delta: i8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(in crate::cpu::core) enum TagRecipe {
    Entry(u8),
    Empty,
    Zero,
    Normal,
    Special,
    Value(StorageCell),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::cpu::core) struct InstructionPlan {
    pub before: FrameStateId,
    pub after: FrameStateId,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::cpu::core) struct EdgePlan {
    pub target: BlockId,
    pub frame: FrameStateId,
    pub fp_rebase: bool,
    pub fp_transfer: FpTransfer,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::cpu::core) struct FpTransfer {
    // Simultaneous source cells for the target's eight unique entry cells.
    pub sources: [StorageCell; 8],
    pub tags: [TagRecipe; 8],
    pub written: u8,
    pub tag_writes: u8,
    pub top_delta: i8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::cpu::core) struct EntryContract {
    pub frame: FrameStateId,
    pub external: bool,
    pub guards: Vec<ProofId>,
    pub gpr_constants: [Option<u32>; 8],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::cpu::core) struct BlockPlan {
    pub entry: FrameStateId,
    pub exit: FrameStateId,
    pub instructions: Vec<InstructionPlan>,
    pub contract: EntryContract,
    pub edges: Vec<EdgePlan>,
}

pub(in crate::cpu::core) struct RegionPlan {
    blocks: Vec<BlockPlan>,
    access: Vec<access::BlockAccessPlan>,
    values: Vec<Value>,
    proofs: Vec<Proof>,
    effects: Vec<Effect>,
    frames: Vec<FrameState>,
    storage: Vec<Option<Storage>>,
    shapes: Vec<FpShape>,
    frame_shapes: Vec<FpShapeId>,
    frame_proofs: Vec<Vec<ProofId>>,
    frame_outputs: Vec<Option<StorageCell>>,
    shape_index: std::collections::HashMap<FpShape, FpShapeId>,
    constants: std::collections::HashMap<u32, ValueId>,
    constant_facts: Vec<Option<u32>>,
}

impl RegionPlan {
    pub fn analyze(region: &Region) -> Self {
        lower::analyze(region)
    }
    pub fn block(&self, block: BlockId) -> &BlockPlan {
        &self.blocks[block.0]
    }
    pub fn instruction(&self, block: BlockId, prefix: u32) -> &InstructionPlan {
        &self.blocks[block.0].instructions[prefix as usize]
    }
    pub fn access_groups(&self, block: BlockId) -> &[AccessGroup] {
        &self.access[block.0].groups
    }
    pub fn has_access_groups(&self) -> bool {
        self.access.iter().any(|block| !block.groups.is_empty())
    }
    pub fn access_member(
        &self,
        block: BlockId,
        prefix: u32,
        address: EffectiveAddress,
        width: u32,
        write: bool,
    ) -> Option<&AccessMember> {
        self.access[block.0].member(prefix, address, width, write)
    }
    pub fn frame(&self, frame: FrameStateId) -> &FrameState {
        &self.frames[frame.0]
    }
    pub fn value(&self, value: ValueId) -> &Value {
        &self.values[value.0]
    }
    pub fn proof(&self, proof: ProofId) -> &Proof {
        &self.proofs[proof.0]
    }
    pub fn effect(&self, effect: EffectId) -> &Effect {
        &self.effects[effect.0]
    }
    pub fn constant(&self, value: ValueId) -> Option<u32> {
        self.constant_facts[value.0]
    }
    pub fn same_value(&self, left: ValueId, right: ValueId) -> bool {
        left == right
    }
    pub fn values(&self) -> &[Value] {
        &self.values
    }
    pub fn frames(&self) -> &[FrameState] {
        &self.frames
    }
    pub fn proofs_at(&self, frame: FrameStateId) -> &[ProofId] {
        &self.frame_proofs[frame.0]
    }
    pub fn fp_output_cell(&self, frame: FrameStateId) -> Option<StorageCell> {
        self.frame_outputs[frame.0]
    }
    pub fn storage(&self, value: ValueId) -> Option<Storage> {
        self.storage[value.0]
    }
    pub fn shape_id(&self, frame: FrameStateId) -> FpShapeId {
        self.frame_shapes[frame.0]
    }
    pub fn shape(&self, frame: FrameStateId) -> &FpShape {
        &self.shapes[self.shape_id(frame).0]
    }
    pub fn shapes(&self) -> &[FpShape] {
        &self.shapes
    }
    pub fn metadata(&self, frame: FrameStateId) -> &MetadataRecipe {
        &self.frame(frame).fp.metadata
    }
    pub fn fp_input_origin(&self, frame: FrameStateId, logical: u8) -> Option<u8> {
        self.frame(frame).fp.payload[logical as usize]
            .values()
            .into_iter()
            .find_map(|value| match self.value(value).kind {
                ValueKind::Input {
                    field:
                        InputField::FpNative(index)
                        | InputField::FpLow(index)
                        | InputField::FpHigh(index)
                        | InputField::FpValid(index),
                    ..
                } => Some(index),
                _ => None,
            })
    }
    fn allocate_value(&mut self, ty: ValueType, kind: ValueKind) -> ValueId {
        let fact = if let ValueKind::Constant(value) = kind {
            Some(value)
        } else {
            None
        };
        let id = ValueId(self.values.len());
        self.values.push(Value { ty, kind });
        self.storage.push(None);
        self.constant_facts.push(fact);
        id
    }
    fn allocate_effect(&mut self, kind: EffectKind, at: Option<ProgramPoint>) -> EffectId {
        let id = EffectId(self.effects.len());
        self.effects.push(Effect { kind, at });
        id
    }
    fn allocate_frame(&mut self, frame: FrameState) -> FrameStateId {
        let cells = frame.fp.payload.map(|payload| {
            self.storage(payload.values()[0])
                .expect("FP values have bounded storage")
                .cell
        });
        let shape = FpShape {
            cells,
            inherited_dirty: std::array::from_fn(|i| Some(i as u8)),
            written: 0,
            tag_written: 0,
            tags: std::array::from_fn(|i| self.tag_recipe(frame.fp.tags[i], cells[i])),
            top_delta: frame.fp.top_delta,
        };
        self.allocate_frame_with_shape(frame, shape)
    }
    fn allocate_frame_with_shape(&mut self, frame: FrameState, shape: FpShape) -> FrameStateId {
        let shape_id = if let Some(id) = self.shape_index.get(&shape) {
            *id
        } else {
            let id = FpShapeId(self.shapes.len());
            self.shape_index.insert(shape.clone(), id);
            self.shapes.push(shape);
            id
        };
        let id = FrameStateId(self.frames.len());
        self.frames.push(frame);
        self.frame_shapes.push(shape_id);
        self.frame_proofs.push(Vec::new());
        self.frame_outputs.push(None);
        id
    }
    fn bind(&mut self, value: ValueId, cell: StorageCell, lane: StorageLane) {
        self.storage[value.0] = Some(Storage { cell, lane });
    }
    fn tag_recipe(&self, tag: TagFact, fallback: StorageCell) -> TagRecipe {
        match tag {
            TagFact::Empty => TagRecipe::Empty,
            TagFact::Zero => TagRecipe::Zero,
            TagFact::Normal => TagRecipe::Normal,
            TagFact::Special => TagRecipe::Special,
            TagFact::Value(value) => {
                TagRecipe::Value(self.storage(value).map_or(fallback, |s| s.cell))
            }
            TagFact::Unknown(value) => match self.value(value).kind {
                ValueKind::Input {
                    field: InputField::FpTag(index),
                    ..
                } => TagRecipe::Entry(index),
                _ => TagRecipe::Entry(fallback.0),
            },
            TagFact::Occupied => TagRecipe::Value(fallback),
        }
    }
    fn allocate_proof(&mut self, proof: Proof) -> ProofId {
        let id = ProofId(self.proofs.len());
        self.proofs.push(proof);
        id
    }
    fn constant_value(&mut self, value: u32) -> ValueId {
        if let Some(id) = self.constants.get(&value) {
            return *id;
        }
        let id = self.allocate_value(ValueType::I32, ValueKind::Constant(value));
        self.constants.insert(value, id);
        id
    }
}
