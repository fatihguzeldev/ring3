use std::sync::Arc;

use super::{CompileError, X87GroupSpan32};
use crate::cpu::{
    dbt::{X87PairStep32, region::CompiledBlock},
    x86::{SegmentRegister, ir::Operation},
};

#[derive(Clone, Copy, Debug)]
pub(crate) struct X87PreparedStep32 {
    pub(crate) operation: crate::cpu::x86::X87Operation,
    pub(crate) pc: u32,
    pub(crate) opcode: u16,
    pub(crate) segment: SegmentRegister,
    pub(crate) waiting: bool,
    pub(crate) instruction_length: u8,
    pub(crate) read_size: u8,
    pub(crate) write_size: u8,
}

impl X87PreparedStep32 {
    fn checked(step: X87PairStep32) -> Option<Self> {
        let segment = (step.control >> 8) & 7;
        if step.opcode > 0x7ff || step.control & !0x701 != 0 || segment > 6 {
            return None;
        }
        Some(Self {
            operation: step.operation,
            pc: step.pc,
            opcode: step.opcode,
            segment: if segment == 0 {
                SegmentRegister::Ds
            } else {
                [
                    SegmentRegister::Es,
                    SegmentRegister::Cs,
                    SegmentRegister::Ss,
                    SegmentRegister::Ds,
                    SegmentRegister::Fs,
                    SegmentRegister::Gs,
                ][(segment - 1) as usize]
            },
            waiting: step.control & 1 != 0,
            instruction_length: step.length,
            read_size: u8::try_from(step.operation.read_size()).ok()?,
            write_size: u8::try_from(step.operation.write_size()).ok()?,
        })
    }
}

#[derive(Clone, Copy, Debug)]
struct NativeSpan32 {
    first: u16,
    length: u8,
}

#[derive(Clone, Copy, Debug)]
struct NativePair32 {
    pc: u32,
    span: Option<NativeSpan32>,
}

#[derive(Clone, Debug)]
pub(crate) struct X87NativePlan32 {
    steps: Arc<[X87PreparedStep32]>,
    span: NativeSpan32,
}

impl X87NativePlan32 {
    pub(crate) fn steps(&self) -> &[X87PreparedStep32] {
        let first = usize::from(self.span.first);
        &self.steps[first..first + usize::from(self.span.length)]
    }
}

#[derive(Debug)]
pub(super) struct NativeX87Catalog32 {
    steps: Arc<[X87PreparedStep32]>,
    pairs: Vec<NativePair32>,
    groups: Vec<Option<NativeSpan32>>,
}

fn span(positions: &[Option<u16>], first: usize, length: usize) -> Option<NativeSpan32> {
    let selected = positions.get(first..first.checked_add(length)?)?;
    let start = selected.first().copied().flatten()?;
    for (index, position) in selected.iter().enumerate() {
        if *position != Some(start.checked_add(u16::try_from(index).ok()?)?) {
            return None;
        }
    }
    Some(NativeSpan32 {
        first: start,
        length: u8::try_from(length).ok()?,
    })
}

impl NativeX87Catalog32 {
    pub(super) fn new(
        blocks: &[CompiledBlock],
        pairs: &[[X87PairStep32; 2]],
        groups: &[X87GroupSpan32],
    ) -> Result<Self, CompileError> {
        let instruction_count = blocks
            .iter()
            .flat_map(|block| &block.instructions)
            .filter(|instruction| matches!(instruction.operation(), Operation::X87 { .. }))
            .count();
        let mut steps = Vec::new();
        steps
            .try_reserve_exact(instruction_count)
            .map_err(|_| CompileError::Allocation)?;
        let mut positions = Vec::new();
        positions
            .try_reserve_exact(blocks.len())
            .map_err(|_| CompileError::Allocation)?;
        for block in blocks {
            let mut selected = Vec::new();
            selected
                .try_reserve_exact(block.instructions.len())
                .map_err(|_| CompileError::Allocation)?;
            for instruction in &block.instructions {
                let prepared = X87PairStep32::from_instruction(instruction, true)
                    .and_then(X87PreparedStep32::checked);
                let position = if let Some(prepared) = prepared {
                    let position =
                        u16::try_from(steps.len()).map_err(|_| CompileError::InvalidLimits)?;
                    steps.push(prepared);
                    Some(position)
                } else {
                    None
                };
                selected.push(position);
            }
            positions.push(selected);
        }
        let mut candidates = Vec::new();
        candidates
            .try_reserve_exact(pairs.len())
            .map_err(|_| CompileError::Allocation)?;
        for (block_index, block) in blocks.iter().enumerate() {
            let mut cursor = 0;
            for pair in crate::cpu::dbt::x87_pairs(&block.instructions) {
                while block.instructions[cursor].pc().0 != pair[0].pc {
                    cursor += 1;
                }
                candidates.push((pair, span(&positions[block_index], cursor, 2)));
                cursor += 2;
            }
        }
        candidates.sort_unstable_by_key(|(pair, _)| pair[0].pc);
        let mut native_pairs = Vec::new();
        native_pairs
            .try_reserve_exact(pairs.len())
            .map_err(|_| CompileError::Allocation)?;
        for pair in pairs {
            let first = candidates.partition_point(|(candidate, _)| candidate[0].pc < pair[0].pc);
            let selected = candidates[first..]
                .iter()
                .take_while(|(candidate, _)| candidate[0].pc == pair[0].pc)
                .find(|(candidate, _)| candidate == pair)
                .and_then(|(_, selected)| *selected);
            native_pairs.push(NativePair32 {
                pc: pair[0].pc,
                span: selected,
            });
        }
        let mut native_groups = Vec::new();
        native_groups
            .try_reserve_exact(groups.len())
            .map_err(|_| CompileError::Allocation)?;
        for group in groups {
            native_groups.push(span(
                &positions[usize::from(group.block)],
                usize::from(group.first),
                usize::from(group.length),
            ));
        }
        Ok(Self {
            steps: steps.into(),
            pairs: native_pairs,
            groups: native_groups,
        })
    }

    fn plan(&self, span: Option<NativeSpan32>) -> Option<X87NativePlan32> {
        Some(X87NativePlan32 {
            steps: self.steps.clone(),
            span: span?,
        })
    }

    pub(super) fn pair(&self, first_pc: u32) -> Option<X87NativePlan32> {
        let index = self
            .pairs
            .binary_search_by_key(&first_pc, |pair| pair.pc)
            .ok()?;
        self.plan(self.pairs[index].span)
    }

    pub(super) fn group(&self, ordinal: u32, wide: bool) -> Option<X87NativePlan32> {
        let span = *self.groups.get(ordinal as usize)?;
        let length = usize::from(span?.length);
        if wide != (length > crate::cpu::dbt::X87_GROUP_MAX_STEPS) {
            return None;
        }
        self.plan(span)
    }
}
