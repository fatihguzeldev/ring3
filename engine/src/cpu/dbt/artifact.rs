use super::{
    BlockSpec, CompileError, CompileLimits, GateSpec, PreparedRegion, prepare_region, wasm,
};
use super::{
    cold::{prepare_embedded_entry_region, prepare_entry_region, prepare_runtime_entry},
    region::prepare_embedded_region,
};
use crate::{
    abi::{ABI_VERSION, X86_INTEGER_PROFILE},
    cpu::x86::decode::{CapturedCodeFault, CodeDecodeKey, CodeFetchContext},
    memory::{AddressSpace, GuestAddress},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArtifactError {
    CodeInvalidated,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegionMetadata {
    pub backend_version: u16,
    pub abi_version: u16,
    pub profile: u16,
    pub blocks: usize,
    pub instructions: usize,
}

#[derive(Debug)]
pub struct CompiledRegion {
    prepared: PreparedRegion,
    tier: super::CompilationTier32,
    bytes: Vec<u8>,
    entry_start: u32,
    entry_end: u32,
    has_indexed_entry: bool,
    x87_pairs: Vec<[super::X87PairStep32; 2]>,
    x87_groups: Vec<X87GroupSpan32>,
}

#[derive(Clone, Copy, Debug)]
struct X87GroupSpan32 {
    block: u16,
    first: u16,
    length: u8,
}

impl CompiledRegion {
    fn new(
        prepared: PreparedRegion,
        bytes: Vec<u8>,
        tier: super::CompilationTier32,
        binding: Option<wasm::EmbeddedBinding>,
    ) -> Result<Self, CompileError> {
        let mut x87_pairs = Vec::new();
        let mut x87_groups = Vec::new();
        if matches!(binding, Some(wasm::EmbeddedBinding::Resident { .. })) {
            // final retained instructions are the descriptor source after any shortening.
            let pairs = || {
                prepared
                    .blocks
                    .iter()
                    .flat_map(|block| super::x87_pairs(&block.instructions))
            };
            x87_pairs
                .try_reserve_exact(pairs().count())
                .map_err(|_| CompileError::Allocation)?;
            x87_pairs.extend(pairs());
            x87_pairs.sort_unstable_by_key(|pair| pair[0].pc);

            // ordinals follow final block order, including overlapping entries whose
            // first pc agrees but whose selected group length differs.
            let groups = || {
                prepared
                    .blocks
                    .iter()
                    .enumerate()
                    .flat_map(|(block, retained)| {
                        super::x87_groups(&retained.instructions)
                            .map(move |plan| (block, plan.first, plan.length))
                    })
            };
            let wide_groups = || {
                prepared
                    .blocks
                    .iter()
                    .enumerate()
                    .flat_map(|(block, retained)| {
                        super::x87_wide_groups(&retained.instructions)
                            .map(move |plan| (block, plan.first, plan.length))
                    })
            };
            x87_groups
                .try_reserve_exact(groups().chain(wide_groups()).count())
                .map_err(|_| CompileError::Allocation)?;
            // legacy ordinals are a stable prefix; wider spans are appended only
            // after both catalogs have been derived from the final retained ir.
            for (block, first, length) in groups().chain(wide_groups()) {
                x87_groups.push(X87GroupSpan32 {
                    block: u16::try_from(block).map_err(|_| CompileError::InvalidLimits)?,
                    first: u16::try_from(first).map_err(|_| CompileError::InvalidLimits)?,
                    length: length as u8,
                });
            }
        }
        // this describes final immutable ir, not current source or decode authority.
        let has_indexed_entry = prepared
            .blocks
            .iter()
            .any(|block| block.indexed_span().is_some());
        let mut region = Self {
            prepared,
            bytes,
            tier,
            entry_start: u32::MAX,
            entry_end: 0,
            has_indexed_entry,
            x87_pairs,
            x87_groups,
        };
        let (start, end) = region
            .executable_addresses()
            .fold((u32::MAX, 0), |(start, end), address| {
                (start.min(address.0), end.max(address.0))
            });
        region.entry_start = start;
        region.entry_end = end;
        Ok(region)
    }

    fn may_contain_entry(&self, pc: u32) -> bool {
        self.entry_start <= pc && pc <= self.entry_end
    }

    pub fn tier(&self) -> super::CompilationTier32 {
        self.tier
    }

    pub(crate) fn x87_pair(&self, first_pc: u32) -> Option<[super::X87PairStep32; 2]> {
        self.x87_pairs
            .binary_search_by_key(&first_pc, |pair| pair[0].pc)
            .ok()
            .map(|index| self.x87_pairs[index])
    }

    pub(crate) fn x87_group(&self, ordinal: u32) -> Option<super::X87Group32> {
        let span = self.x87_groups.get(ordinal as usize)?;
        let block = self.prepared.blocks.get(usize::from(span.block))?;
        super::x87_group::X87GroupPlan32 {
            first: usize::from(span.first),
            length: usize::from(span.length),
        }
        .descriptors(&block.instructions)
    }

    pub(crate) fn x87_group32(&self, ordinal: u32) -> Option<super::X87WideGroup32<'_>> {
        let span = self.x87_groups.get(ordinal as usize)?;
        let block = self.prepared.blocks.get(usize::from(span.block))?;
        super::x87_group::X87WideGroupPlan32 {
            first: usize::from(span.first),
            length: usize::from(span.length),
        }
        .view(&block.instructions)
    }
    pub(crate) fn supports_indexed_entry(&self) -> bool {
        self.has_indexed_entry
    }

    pub(crate) fn decode_key(&self) -> CodeDecodeKey {
        self.prepared.decode_key.unwrap_or(CodeDecodeKey::FLAT)
    }

    pub(crate) fn matches_decode_key(&self, key: CodeDecodeKey) -> bool {
        self.decode_key() == key
    }

    pub(crate) fn contains_material_in(&self, pc: u32, key: CodeDecodeKey) -> bool {
        self.matches_decode_key(key) && self.contains_material(pc)
    }

    pub(crate) fn context_fault_in(
        &self,
        pc: u32,
        key: CodeDecodeKey,
    ) -> Option<&CapturedCodeFault> {
        self.matches_decode_key(key)
            .then(|| self.context_fault(pc))
            .flatten()
    }

    pub(super) fn executable_addresses(&self) -> impl Iterator<Item = GuestAddress> + '_ {
        self.prepared.blocks.iter().flat_map(|block| {
            block
                .instructions
                .iter()
                .map(|instruction| instruction.pc())
                .chain(block.gate().into_iter().map(|gate| gate.entry))
                .chain(block.context_fault().into_iter().map(|fault| fault.pc))
        })
    }

    pub(crate) fn contains_executable(&self, pc: u32) -> bool {
        self.may_contain_entry(pc) && self.executable_addresses().any(|entry| entry.0 == pc)
    }

    pub(crate) fn contains_instruction(&self, pc: u32) -> bool {
        self.prepared.blocks.iter().any(|block| {
            block
                .instructions
                .iter()
                .any(|instruction| instruction.pc().0 == pc)
        })
    }

    /// immutable decode provenance, including after a consumed entry changes its own source.
    #[cfg_attr(
        not(any(target_arch = "wasm32", test)),
        expect(
            dead_code,
            reason = "generated exception detection is enabled for wasm32"
        )
    )]
    pub(crate) fn is_privileged_instruction_at(&self, pc: u32) -> bool {
        self.prepared.blocks.iter().any(|block| {
            block.instructions.iter().any(|instruction| {
                instruction.pc().0 == pc && instruction.is_privileged_instruction()
            })
        })
    }

    pub(crate) fn matches_gate(&self, entry: u32, id: u32) -> bool {
        self.prepared.blocks.iter().any(|block| {
            block
                .gate()
                .is_some_and(|gate| gate.entry.0 == entry && gate.id == id)
        })
    }

    pub(crate) fn contains_material(&self, pc: u32) -> bool {
        self.may_contain_entry(pc)
            && self.prepared.blocks.iter().any(|block| {
                block
                    .instructions
                    .iter()
                    .any(|instruction| instruction.pc().0 == pc)
                    || block.gate().is_some_and(|gate| gate.entry.0 == pc)
            })
    }

    pub(crate) fn context_fault(&self, pc: u32) -> Option<&CapturedCodeFault> {
        if !self.may_contain_entry(pc) {
            return None;
        }
        self.prepared
            .blocks
            .iter()
            .filter_map(|block| block.context_fault())
            .find(|fault| fault.pc.0 == pc)
    }

    pub fn wasm_bytes(&self, memory: &AddressSpace) -> Result<&[u8], ArtifactError> {
        if !self.prepared.is_current(memory) {
            return Err(ArtifactError::CodeInvalidated);
        }
        Ok(&self.bytes)
    }

    pub(crate) fn retained_wasm_byte_len(&self) -> usize {
        self.bytes.len()
    }

    pub(crate) fn first_entry(&self) -> Option<u32> {
        self.executable_addresses().next().map(|entry| entry.0)
    }

    #[cfg(test)]
    pub(super) fn retained_wasm_bytes_for_test(&self) -> &[u8] {
        &self.bytes
    }

    pub fn metadata(&self) -> RegionMetadata {
        RegionMetadata {
            backend_version: 1,
            abi_version: ABI_VERSION,
            profile: X86_INTEGER_PROFILE,
            blocks: self.prepared.block_count(),
            instructions: self.prepared.instruction_count(),
        }
    }
}

pub fn compile_region(
    memory: &AddressSpace,
    specs: &[BlockSpec],
    limits: CompileLimits,
) -> Result<CompiledRegion, CompileError> {
    #[cfg(feature = "observability")]
    let _scope = ring3_observability::current().scope(
        ring3_observability::Level::Detailed,
        ring3_observability::categories::ENGINE_CPU_CODEGEN,
        "compile_region",
        &[],
    );
    compile(
        memory,
        specs,
        limits,
        None,
        &[],
        None,
        crate::cpu::x86::decode::DecodeAuthority::default(),
    )
}

pub fn compile_entry_region(
    memory: &AddressSpace,
    entries: &[GuestAddress],
    limits: CompileLimits,
) -> Result<CompiledRegion, CompileError> {
    #[cfg(feature = "observability")]
    let _scope = ring3_observability::current().scope(
        ring3_observability::Level::Detailed,
        ring3_observability::categories::ENGINE_CPU_CODEGEN,
        "compile_entries",
        &[],
    );
    let prepared = prepare_entry_region(memory, entries, limits)?;
    emit_entry_prepared(memory, prepared, limits, None, None)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn compile_embedded_entry_region(
    memory: &AddressSpace,
    entries: &[GuestAddress],
    limits: CompileLimits,
    key: u64,
    generation: u32,
    gates: &[GateSpec],
    cs: CodeFetchContext,
    authority: crate::cpu::x86::decode::DecodeAuthority,
) -> Result<CompiledRegion, CompileError> {
    let prepared =
        prepare_embedded_entry_region(memory, entries, limits, gates, Some(cs), authority)?;
    emit_entry_prepared(
        memory,
        prepared,
        limits,
        Some(wasm::EmbeddedBinding::Replacement { key, generation }),
        Some(cs),
    )
}

pub(crate) fn compile_runtime_entry(
    memory: &AddressSpace,
    entry: GuestAddress,
    limits: CompileLimits,
    key: u64,
    generation: u32,
    cs: CodeFetchContext,
    authority: crate::cpu::x86::decode::DecodeAuthority,
) -> Result<CompiledRegion, CompileError> {
    let prepared = prepare_runtime_entry(memory, entry, limits, Some(cs), authority)?;
    emit_entry_prepared(
        memory,
        prepared,
        limits,
        Some(wasm::EmbeddedBinding::Replacement { key, generation }),
        Some(cs),
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn compile_embedded_region(
    memory: &AddressSpace,
    specs: &[BlockSpec],
    limits: CompileLimits,
    key: u64,
    generation: u32,
    gates: &[GateSpec],
    cs: CodeFetchContext,
    authority: crate::cpu::x86::decode::DecodeAuthority,
) -> Result<CompiledRegion, CompileError> {
    compile(
        memory,
        specs,
        limits,
        Some(wasm::EmbeddedBinding::Replacement { key, generation }),
        gates,
        Some(cs),
        authority,
    )
}

fn compile(
    memory: &AddressSpace,
    specs: &[BlockSpec],
    limits: CompileLimits,
    binding: Option<wasm::EmbeddedBinding>,
    gates: &[GateSpec],
    cs: Option<CodeFetchContext>,
    authority: crate::cpu::x86::decode::DecodeAuthority,
) -> Result<CompiledRegion, CompileError> {
    let prepared = if binding.is_some() {
        prepare_embedded_region(memory, specs, limits, gates, cs, authority)?
    } else {
        prepare_region(memory, specs, limits)?
    };
    emit_prepared(prepared, limits, binding)
}

pub(super) fn emit_prepared(
    prepared: PreparedRegion,
    limits: CompileLimits,
    binding: Option<wasm::EmbeddedBinding>,
) -> Result<CompiledRegion, CompileError> {
    #[cfg(feature = "observability")]
    let _scope = ring3_observability::current().scope(
        ring3_observability::Level::Detailed,
        ring3_observability::categories::ENGINE_CPU_CODEGEN,
        "emit",
        &[],
    );
    let bytes = wasm::emit(&prepared.blocks, binding, prepared.decode_key);
    if bytes.len() > limits.wasm_bytes {
        return Err(CompileError::WasmLimit);
    }
    CompiledRegion::new(prepared, bytes, super::CompilationTier32::Baseline, binding)
}

pub(super) fn emit_entry_prepared(
    memory: &AddressSpace,
    prepared: PreparedRegion,
    limits: CompileLimits,
    binding: Option<wasm::EmbeddedBinding>,
    cs: Option<CodeFetchContext>,
) -> Result<CompiledRegion, CompileError> {
    emit_entry_prepared_tier(
        memory,
        prepared,
        limits,
        binding,
        cs,
        super::CompilationTier32::Baseline,
    )
}
pub(super) fn emit_entry_prepared_tier(
    memory: &AddressSpace,
    prepared: PreparedRegion,
    limits: CompileLimits,
    binding: Option<wasm::EmbeddedBinding>,
    cs: Option<CodeFetchContext>,
    tier: super::CompilationTier32,
) -> Result<CompiledRegion, CompileError> {
    shortening::emit(memory, prepared, limits, binding, cs, tier)
}

mod shortening;

#[cfg(test)]
#[path = "artifact/tests/bounds.rs"]
mod bounds_tests;

#[cfg(test)]
#[path = "artifact/tests/pairs.rs"]
mod pair_tests;

#[cfg(test)]
#[path = "artifact/tests/groups.rs"]
mod group_tests;

#[cfg(test)]
#[path = "artifact/tests/wide_groups.rs"]
mod wide_group_tests;

#[cfg(test)]
#[path = "artifact/tests/indexed/mod.rs"]
mod indexed_tests;
