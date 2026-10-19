use super::{CompileError, CompileLimits, CompiledRegion, PreparedRegion, wasm};
use crate::{
    cpu::{
        dbt::{
            CompilationTier32,
            cold::shorten_entry_region,
            region::{self, BlockSource},
        },
        x86::decode::{CodeFetchContext, DecodedOperation},
    },
    memory::AddressSpace,
};

struct Emission {
    bytes: Vec<u8>,
    tier: CompilationTier32,
}

fn emit_bytes(
    prepared: &PreparedRegion,
    binding: Option<wasm::EmbeddedBinding>,
    tier: CompilationTier32,
) -> Emission {
    let tier = if tier == CompilationTier32::Optimized
        && prepared
            .blocks
            .iter()
            .any(|block| block.indexed_span().is_some())
    {
        tier
    } else {
        CompilationTier32::Baseline
    };
    #[cfg(feature = "observability")]
    let _scope = ring3_observability::current().scope(
        ring3_observability::Level::Detailed,
        ring3_observability::categories::ENGINE_CPU_CODEGEN,
        "emit_attempt",
        &[
            ring3_observability::Field::unsigned("blocks", prepared.block_count() as u64),
            ring3_observability::Field::unsigned(
                "instructions",
                prepared.instruction_count() as u64,
            ),
            ring3_observability::Field::unsigned("tier", tier as u64),
        ],
    );
    Emission {
        bytes: wasm::emit_with_tier(&prepared.blocks, binding, prepared.decode_key, tier),
        tier,
    }
}

pub(super) fn emit(
    memory: &AddressSpace,
    mut prepared: PreparedRegion,
    limits: CompileLimits,
    binding: Option<wasm::EmbeddedBinding>,
    cs: Option<CodeFetchContext>,
    tier: CompilationTier32,
) -> Result<CompiledRegion, CompileError> {
    let full = emit_bytes(&prepared, binding, tier);
    if full.bytes.len() <= limits.wasm_bytes {
        return CompiledRegion::new(prepared, full.bytes, full.tier, binding);
    }
    drop(full);
    let mut journal = TrimJournal::new(&prepared)?;
    if journal.limit == 0 {
        return Err(CompileError::WasmLimit);
    }
    let fitting = search(journal.limit, limits.wasm_bytes, |position| {
        journal.seek(memory, &mut prepared, cs, position)?;
        Ok(emit_bytes(&prepared, binding, tier))
    })?;
    if let Some(fitting) = fitting {
        journal.seek(memory, &mut prepared, cs, fitting.position)?;
        journal.finalize(memory, &mut prepared, cs)?;
        return CompiledRegion::new(
            prepared,
            fitting.emission.bytes,
            fitting.emission.tier,
            binding,
        );
    }

    // a larger prefix can be smaller after helper/type/tier changes. a failed minimum
    // therefore cannot establish WasmLimit: preserve the original exhaustive fallback.
    journal.seek(memory, &mut prepared, cs, 0)?;
    drop(journal);
    while shorten_entry_region(memory, &mut prepared, cs)? {
        let emission = emit_bytes(&prepared, binding, tier);
        if emission.bytes.len() <= limits.wasm_bytes {
            return CompiledRegion::new(prepared, emission.bytes, emission.tier, binding);
        }
    }
    Err(CompileError::WasmLimit)
}

struct Fitting {
    position: usize,
    emission: Emission,
}

fn search(
    limit: usize,
    budget: usize,
    mut emit: impl FnMut(usize) -> Result<Emission, CompileError>,
) -> Result<Option<Fitting>, CompileError> {
    let minimum = emit(limit)?;
    if minimum.bytes.len() > budget {
        return Ok(None);
    }
    let mut fitting = Fitting {
        position: limit,
        emission: minimum,
    };
    let mut failed = 0;
    while fitting.position - failed > 1 {
        let position = failed + (fitting.position - failed) / 2;
        let emission = emit(position)?;
        if emission.bytes.len() <= budget {
            fitting = Fitting { position, emission };
        } else {
            failed = position;
        }
    }
    // only tested bytes are retained. nonmonotonic sizes can hide a larger fitting
    // prefix, so this bounded search deliberately makes no maximal-prefix promise.
    Ok(Some(fitting))
}

enum Removed {
    Instruction {
        block: usize,
        operation: DecodedOperation,
    },
    Fault {
        block: usize,
        source: BlockSource,
    },
}

struct TrimJournal {
    removed: Vec<Removed>,
    limit: usize,
}

impl TrimJournal {
    fn new(prepared: &PreparedRegion) -> Result<Self, CompileError> {
        let limit = prepared
            .blocks
            .iter()
            .map(|block| {
                block.instructions.len().saturating_sub(1)
                    + usize::from(block.context_fault().is_some() && !block.instructions.is_empty())
            })
            .sum();
        let mut removed = Vec::new();
        removed
            .try_reserve_exact(limit)
            .map_err(|_| CompileError::Allocation)?;
        Ok(Self { removed, limit })
    }

    fn seek(
        &mut self,
        memory: &AddressSpace,
        prepared: &mut PreparedRegion,
        cs: Option<CodeFetchContext>,
        position: usize,
    ) -> Result<(), CompileError> {
        assert!(position <= self.limit);
        while self.removed.len() > position {
            match self.removed.pop().unwrap() {
                Removed::Instruction { block, operation } => {
                    // pop never shrinks the original allocation, so restoration cannot allocate.
                    prepared.blocks[block].instructions.push(operation);
                }
                Removed::Fault { block, source } => prepared.blocks[block].source = source,
            }
        }
        while self.removed.len() < position {
            let (index, block) = prepared
                .blocks
                .iter_mut()
                .enumerate()
                .rev()
                .find(|(_, block)| {
                    block.instructions.len() > 1
                        || block.context_fault().is_some() && !block.instructions.is_empty()
                })
                .expect("prepared trim count preserves one entry per block");
            let removed = if block.context_fault().is_some() {
                let snapshot = region::prepare_block_snapshot(
                    memory,
                    block.instructions[0].pc(),
                    prefix_length(block),
                    cs,
                )?;
                Removed::Fault {
                    block: index,
                    source: std::mem::replace(
                        &mut block.source,
                        BlockSource::Instructions(snapshot),
                    ),
                }
            } else {
                Removed::Instruction {
                    block: index,
                    operation: block.instructions.pop().unwrap(),
                }
            };
            self.removed.push(removed);
        }
        Ok(())
    }

    fn finalize(
        &self,
        memory: &AddressSpace,
        prepared: &mut PreparedRegion,
        cs: Option<CodeFetchContext>,
    ) -> Result<(), CompileError> {
        let mut changed = [false; region::MAX_COHORT_BLOCKS];
        for removed in &self.removed {
            let block = match removed {
                Removed::Instruction { block, .. } | Removed::Fault { block, .. } => *block,
            };
            changed[block] = true;
        }
        for (index, block) in prepared.blocks.iter_mut().enumerate() {
            if changed[index] {
                // trial snapshots may cover removed suffix bytes, but none can be published.
                let snapshot = region::prepare_block_snapshot(
                    memory,
                    block.instructions[0].pc(),
                    prefix_length(block),
                    cs,
                )?;
                block.source = BlockSource::Instructions(snapshot);
            }
        }
        Ok(())
    }
}

fn prefix_length(block: &region::CompiledBlock) -> usize {
    block
        .instructions
        .iter()
        .map(|instruction| usize::from(instruction.length()))
        .sum()
}

#[cfg(test)]
mod tests;
