use super::{
    BlockSpec, CompileError, CompileLimits, GateSpec, PreparedRegion, prepare_region, wasm,
};
use super::{
    cold::{
        prepare_embedded_entry_region, prepare_entry_region, prepare_runtime_entry,
        shorten_entry_region,
    },
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
}

impl CompiledRegion {
    pub fn tier(&self) -> super::CompilationTier32 {
        self.tier
    }
    pub(crate) fn supports_indexed_entry(&self) -> bool {
        self.prepared
            .blocks
            .iter()
            .any(|block| block.indexed_span().is_some())
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
        self.executable_addresses().any(|entry| entry.0 == pc)
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
        self.prepared.blocks.iter().any(|block| {
            block
                .instructions
                .iter()
                .any(|instruction| instruction.pc().0 == pc)
                || block.gate().is_some_and(|gate| gate.entry.0 == pc)
        })
    }

    pub(crate) fn context_fault(&self, pc: u32) -> Option<&CapturedCodeFault> {
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
    let bytes = wasm::emit(&prepared.blocks, binding, prepared.decode_key);
    if bytes.len() > limits.wasm_bytes {
        return Err(CompileError::WasmLimit);
    }
    Ok(CompiledRegion {
        prepared,
        bytes,
        tier: super::CompilationTier32::Baseline,
    })
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
    mut prepared: PreparedRegion,
    limits: CompileLimits,
    binding: Option<wasm::EmbeddedBinding>,
    cs: Option<CodeFetchContext>,
    tier: super::CompilationTier32,
) -> Result<CompiledRegion, CompileError> {
    loop {
        let applied_tier = if tier == super::CompilationTier32::Optimized
            && prepared
                .blocks
                .iter()
                .any(|block| block.indexed_span().is_some())
        {
            tier
        } else {
            super::CompilationTier32::Baseline
        };
        let bytes =
            wasm::emit_with_tier(&prepared.blocks, binding, prepared.decode_key, applied_tier);
        if bytes.len() <= limits.wasm_bytes {
            return Ok(CompiledRegion {
                prepared,
                bytes,
                tier: applied_tier,
            });
        }
        if !shorten_entry_region(memory, &mut prepared, cs)? {
            return Err(CompileError::WasmLimit);
        }
    }
}
