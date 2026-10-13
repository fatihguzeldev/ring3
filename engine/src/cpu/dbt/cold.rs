use super::{
    BlockSpec, CompileError, CompileLimits, GateSpec, InstructionError, PreparedRegion, gate,
    region::{self, BlockSource, CompiledBlock, GUEST_END, MAX_BLOCKS},
};
use crate::{
    cpu::x86::decode::{BoundDecode, CodeFetchContext, DecodePolicy},
    memory::{AddressSpace, GuestAddress},
};

pub fn prepare_entry_region(
    memory: &AddressSpace,
    entries: &[GuestAddress],
    limits: CompileLimits,
) -> Result<PreparedRegion, CompileError> {
    prepare(
        memory,
        entries,
        limits,
        false,
        &[],
        DecodePolicy::Strict,
        None,
        None,
    )
}

pub(super) fn prepare_embedded_entry_region(
    memory: &AddressSpace,
    entries: &[GuestAddress],
    limits: CompileLimits,
    gates: &[GateSpec],
    cs: Option<CodeFetchContext>,
    bank: Option<crate::cpu::x86::UserSystemRegisterBank32>,
) -> Result<PreparedRegion, CompileError> {
    prepare(
        memory,
        entries,
        limits,
        true,
        gates,
        DecodePolicy::Strict,
        cs,
        bank,
    )
}

pub(super) fn prepare_resident_entry_region(
    memory: &AddressSpace,
    entries: &[GuestAddress],
    limits: CompileLimits,
    gates: &[GateSpec],
    cs: Option<CodeFetchContext>,
    bank: Option<crate::cpu::x86::UserSystemRegisterBank32>,
) -> Result<PreparedRegion, CompileError> {
    prepare_embedded_entry_region(memory, entries, limits, gates, cs, bank)
}

pub(super) fn prepare_runtime_entry(
    memory: &AddressSpace,
    entry: GuestAddress,
    limits: CompileLimits,
    cs: Option<CodeFetchContext>,
    bank: Option<crate::cpu::x86::UserSystemRegisterBank32>,
) -> Result<PreparedRegion, CompileError> {
    prepare(
        memory,
        &[entry],
        limits,
        true,
        &[],
        DecodePolicy::Execution,
        cs,
        bank,
    )
}

#[allow(clippy::too_many_arguments)]
fn prepare(
    memory: &AddressSpace,
    entries: &[GuestAddress],
    limits: CompileLimits,
    embedded: bool,
    gates: &[GateSpec],
    decode_policy: DecodePolicy,
    cs: Option<CodeFetchContext>,
    bank: Option<crate::cpu::x86::UserSystemRegisterBank32>,
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
    let specs = &specs[..entries.len()];
    region::validate_blocks(specs, limits.blocks)?;
    gate::validate(specs, gates)?;
    if entries.len() > limits.instructions {
        return Err(CompileError::InstructionLimit);
    }
    let mut blocks = Vec::new();
    blocks
        .try_reserve_exact(entries.len())
        .map_err(|_| CompileError::Allocation)?;
    let mut instruction_count = 0;
    for (index, entry) in entries.iter().enumerate() {
        let entry_limit = limits.instructions - (entries.len() - index - 1);
        if let Some(gate) = gates.iter().find(|gate| gate.entry == *entry) {
            if instruction_count == limits.instructions {
                return Err(CompileError::InstructionLimit);
            }
            blocks.push(CompiledBlock {
                instructions: Vec::new(),
                source: BlockSource::Gate(gate::prepare(memory, *gate, cs)?),
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
        let mut terminal = None;
        while cursor < boundary {
            if instruction_count == entry_limit {
                break;
            }
            let pc = GuestAddress(cursor as u32);
            let decoded = crate::cpu::x86::decode::decode_with_system_registers(
                memory,
                pc,
                decode_policy,
                cs,
                bank,
            )
            .map_err(|error| region::instruction_error(pc, InstructionError::Decode(error)))?;
            let instruction = match decoded {
                BoundDecode::Instruction(instruction) => instruction,
                BoundDecode::CodeFault(mut fault) => {
                    let consumed_length =
                        (cursor - start) as usize + usize::from(fault.consumed_prefix_length);
                    if start + consumed_length as u64 > boundary {
                        return Err(CompileError::InvalidBlocks);
                    }
                    let consumed = if instructions.is_empty() {
                        fault.prefix.take()
                    } else {
                        fault.prefix = None;
                        Some(region::prepare_block_snapshot(
                            memory,
                            *entry,
                            consumed_length,
                            cs,
                        )?)
                    };
                    terminal = Some(BlockSource::ContextFault { fault, consumed });
                    instruction_count += 1;
                    break;
                }
            };
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
        let source = match terminal {
            Some(source) => source,
            None => BlockSource::Instructions(region::prepare_block_snapshot(
                memory,
                *entry,
                (cursor - start) as usize,
                cs,
            )?),
        };
        blocks.push(CompiledBlock {
            source,
            instructions,
        });
    }

    Ok(PreparedRegion::new(memory, blocks, cs))
}

// automatic emission can shorten a candidate without dropping a requested entry.
pub(super) fn shorten_entry_region(
    memory: &AddressSpace,
    prepared: &mut PreparedRegion,
    cs: Option<CodeFetchContext>,
) -> Result<bool, CompileError> {
    for block in prepared.blocks.iter_mut().rev() {
        let remove_fault = block.context_fault().is_some() && !block.instructions.is_empty();
        if !remove_fault {
            if block.instructions.len() <= 1 {
                continue;
            }
            block.instructions.pop();
        }
        let entry = block.instructions[0].pc();
        let length = block
            .instructions
            .iter()
            .map(|instruction| usize::from(instruction.length()))
            .sum();
        block.source =
            BlockSource::Instructions(region::prepare_block_snapshot(memory, entry, length, cs)?);
        return Ok(true);
    }
    Ok(false)
}

#[cfg(test)]
#[path = "cold/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "cold/tests/capacity.rs"]
mod capacity;
