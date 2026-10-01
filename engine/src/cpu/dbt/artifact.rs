use super::region::prepare_embedded_region;
use super::{BlockSpec, CompileError, CompileLimits, PreparedRegion, prepare_region, wasm};
use crate::{
    abi::{ABI_VERSION, X86_INTEGER_PROFILE},
    memory::AddressSpace,
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
    bytes: Vec<u8>,
}

impl CompiledRegion {
    pub fn wasm_bytes(&self, memory: &AddressSpace) -> Result<&[u8], ArtifactError> {
        if !self.prepared.is_current(memory) {
            return Err(ArtifactError::CodeInvalidated);
        }
        Ok(&self.bytes)
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
    compile(memory, specs, limits, None)
}

pub(crate) fn compile_embedded_region(
    memory: &AddressSpace,
    specs: &[BlockSpec],
    limits: CompileLimits,
    key: u64,
    generation: u32,
) -> Result<CompiledRegion, CompileError> {
    compile(
        memory,
        specs,
        limits,
        Some(wasm::EmbeddedBinding { key, generation }),
    )
}

fn compile(
    memory: &AddressSpace,
    specs: &[BlockSpec],
    limits: CompileLimits,
    binding: Option<wasm::EmbeddedBinding>,
) -> Result<CompiledRegion, CompileError> {
    let prepared = if binding.is_some() {
        prepare_embedded_region(memory, specs, limits)?
    } else {
        prepare_region(memory, specs, limits)?
    };
    let bytes = wasm::emit(&prepared.blocks, binding);
    if bytes.len() > limits.wasm_bytes {
        return Err(CompileError::WasmLimit);
    }
    Ok(CompiledRegion { prepared, bytes })
}
