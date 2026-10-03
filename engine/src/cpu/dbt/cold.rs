use super::{
    BlockSpec, CompileError, CompileLimits, GateSpec, InstructionError, PreparedRegion, gate,
    region::{self, CompiledBlock, GUEST_END, MAX_BLOCKS},
};
use crate::{
    cpu::x86::decode::decode_one,
    memory::{AddressSpace, GuestAddress},
};

pub fn prepare_entry_region(
    memory: &AddressSpace,
    entries: &[GuestAddress],
    limits: CompileLimits,
) -> Result<PreparedRegion, CompileError> {
    prepare(memory, entries, limits, false, &[])
}

pub(super) fn prepare_embedded_entry_region(
    memory: &AddressSpace,
    entries: &[GuestAddress],
    limits: CompileLimits,
    gates: &[GateSpec],
) -> Result<PreparedRegion, CompileError> {
    prepare(memory, entries, limits, true, gates)
}

pub(super) fn prepare_resident_entry_region(
    memory: &AddressSpace,
    entries: &[GuestAddress],
    limits: CompileLimits,
    gates: &[GateSpec],
) -> Result<PreparedRegion, CompileError> {
    prepare_embedded_entry_region(memory, entries, limits, gates)
}

fn prepare(
    memory: &AddressSpace,
    entries: &[GuestAddress],
    limits: CompileLimits,
    embedded: bool,
    gates: &[GateSpec],
) -> Result<PreparedRegion, CompileError> {
    region::validate_limits(limits)?;
    if entries.is_empty() || entries.len() > limits.blocks {
        return Err(CompileError::InvalidBlocks);
    }
    let mut specs = [BlockSpec {
        entry: GuestAddress(0),
        byte_length: 0,
    }; MAX_BLOCKS];
    for (spec, entry) in specs.iter_mut().zip(entries) {
        *spec = BlockSpec {
            entry: *entry,
            byte_length: if gates.iter().any(|gate| gate.entry == *entry) {
                2
            } else {
                1
            },
        };
    }
    let specs = &mut specs[..entries.len()];
    region::validate_blocks(specs, limits.blocks)?;
    gate::validate(specs, gates)?;
    let mut blocks = Vec::new();
    blocks
        .try_reserve_exact(entries.len())
        .map_err(|_| CompileError::Allocation)?;
    let mut instruction_count = 0;
    for (index, entry) in entries.iter().enumerate() {
        if let Some(gate) = gates.iter().find(|gate| gate.entry == *entry) {
            if instruction_count == limits.instructions {
                return Err(CompileError::InstructionLimit);
            }
            blocks.push(CompiledBlock {
                instructions: Vec::new(),
                gate: Some(gate::prepare(memory, *gate)?),
            });
            instruction_count += 1;
            continue;
        }
        let start = u64::from(entry.0);
        let boundary = entries
            .iter()
            .map(|seed| u64::from(seed.0))
            .filter(|seed| *seed > start)
            .min()
            .unwrap_or(GUEST_END);
        let mut cursor = start;
        let mut instructions = Vec::new();
        while cursor < boundary {
            if instruction_count == limits.instructions {
                return Err(CompileError::InstructionLimit);
            }
            let pc = GuestAddress(cursor as u32);
            let instruction = decode_one(memory, pc)
                .map_err(|error| region::instruction_error(pc, InstructionError::Decode(error)))?;
            let next = cursor + u64::from(instruction.length());
            if next > boundary {
                return Err(CompileError::InvalidBlocks);
            }
            if !region::supports(&instruction, embedded) {
                return Err(region::instruction_error(
                    pc,
                    InstructionError::BackendUnsupported,
                ));
            }
            let terminates = region::terminates(&instruction, embedded);
            instructions
                .try_reserve(1)
                .map_err(|_| CompileError::Allocation)?;
            instructions.push(instruction);
            instruction_count += 1;
            cursor = next;
            if terminates {
                break;
            }
        }
        specs[index].byte_length = (cursor - start) as u32;
        blocks.push(CompiledBlock {
            instructions,
            gate: None,
        });
    }
    region::validate_blocks(specs, limits.blocks)?;
    Ok(PreparedRegion { blocks })
}

#[cfg(test)]
#[path = "cold_tests.rs"]
mod tests;
