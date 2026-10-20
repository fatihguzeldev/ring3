use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_IDENTITY: AtomicU64 = AtomicU64::new(1);

use crate::{
    cpu::x86::{
        decode::{BoundDecode, DecodeAuthority, DecodeError, DecodePolicy, decode_for_compilation},
        ir::{BranchTarget, Operation},
    },
    memory::{AddressSpace, CodeSnapshot, GuestAddress, MemoryError},
};

#[derive(Clone, Copy, Debug)]
pub struct CompileOptions {
    pub instructions: usize,
    pub blocks: usize,
    pub wasm_bytes: usize,
}

impl Default for CompileOptions {
    fn default() -> Self {
        Self {
            instructions: 2048,
            blocks: 128,
            wasm_bytes: 1 << 20,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompileError {
    InvalidLimits,
    Decode(DecodeError),
    Memory(MemoryError),
    WasmLimit,
}

#[derive(Debug)]
pub struct CompiledCode {
    identity: u64,
    wasm: Vec<u8>,
    sources: Vec<CodeSnapshot>,
    entries: Vec<GuestAddress>,
    instructions: usize,
}

impl CompiledCode {
    pub fn identity(&self) -> u64 {
        self.identity
    }
    pub fn wasm(&self) -> &[u8] {
        &self.wasm
    }
    pub fn entrypoints(&self) -> &[GuestAddress] {
        &self.entries
    }
    pub fn instruction_count(&self) -> usize {
        self.instructions
    }
    pub fn is_current(&self, memory: &AddressSpace) -> bool {
        self.sources
            .iter()
            .all(|source| memory.is_code_current(source))
    }
}

#[derive(Clone, Copy)]
pub(super) struct Instruction {
    pub pc: u32,
    pub next: u32,
    pub operation: Option<Operation>,
}

pub(super) struct Block {
    pub entry: u32,
    pub instructions: Vec<Instruction>,
}

pub fn compile(
    memory: &AddressSpace,
    entry: GuestAddress,
    options: CompileOptions,
) -> Result<CompiledCode, CompileError> {
    if options.instructions == 0
        || options.instructions > 16384
        || options.blocks == 0
        || options.blocks > 1024
        || options.wasm_bytes < 128
        || options.wasm_bytes > 16 << 20
    {
        return Err(CompileError::InvalidLimits);
    }
    let mut pending = VecDeque::from([entry.0]);
    let mut nodes = BTreeMap::new();
    let mut starts = BTreeSet::from([entry.0]);
    let mut sources = Vec::new();
    while let Some(mut pc) = pending.pop_front() {
        if nodes.len() >= options.instructions || starts.len() > options.blocks {
            break;
        }
        while !nodes.contains_key(&pc) && nodes.len() < options.instructions {
            let decoded = decode_for_compilation(
                memory,
                GuestAddress(pc),
                DecodePolicy::Execution,
                None,
                DecodeAuthority::default(),
            );
            let decoded = match decoded {
                Ok(BoundDecode::Instruction(decoded)) => decoded,
                Ok(BoundDecode::CodeFault(_)) => {
                    unreachable!("flat fetch has no segment authority")
                }
                Err(error) if nodes.is_empty() => return Err(CompileError::Decode(error)),
                Err(_) => break,
            };
            let next = decoded.next_pc().0;
            sources.push(
                memory
                    .snapshot_code(decoded.pc(), usize::from(decoded.length()))
                    .map_err(CompileError::Memory)?,
            );
            let operation = if decoded.segment_accesses() & (1 << 5) == 0
                && !decoded.is_privileged_instruction()
                && super::emitter::supports(decoded.operation())
            {
                Some(*decoded.operation())
            } else {
                None
            };
            nodes.insert(
                pc,
                Instruction {
                    pc,
                    next,
                    operation,
                },
            );
            let mut enqueue = |target| {
                if starts.len() < options.blocks {
                    starts.insert(target);
                    pending.push_back(target);
                }
            };
            match operation {
                Some(
                    Operation::ConditionalJump { target, .. }
                    | Operation::CountBranch { target, .. },
                ) => {
                    enqueue(target.0);
                    enqueue(next);
                    break;
                }
                Some(Operation::Jump {
                    target: BranchTarget::Direct(target),
                }) => {
                    enqueue(target.0);
                    break;
                }
                Some(Operation::Call { target }) => {
                    if let BranchTarget::Direct(target) = target {
                        enqueue(target.0);
                    }
                    enqueue(next);
                    break;
                }
                Some(Operation::Jump { .. } | Operation::Return { .. } | Operation::Control(_))
                | None => break,
                _ => pc = next,
            }
        }
    }
    // a target may enter the middle of an already decoded linear span.
    let mut blocks = Vec::new();
    let mut current: Option<Block> = None;
    for (pc, instruction) in nodes {
        if current.as_ref().is_some_and(|block| {
            starts.contains(&pc)
                || block
                    .instructions
                    .last()
                    .is_some_and(|last| last.next != pc || terminates(last.operation))
        }) {
            blocks.push(current.take().expect("current block"));
        }
        current
            .get_or_insert_with(|| Block {
                entry: pc,
                instructions: Vec::new(),
            })
            .instructions
            .push(instruction);
    }
    if let Some(block) = current {
        blocks.push(block);
    }
    let identity = NEXT_IDENTITY
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
            value.checked_add(1)
        })
        .map_err(|_| CompileError::Memory(MemoryError::VersionExhausted))?;
    let wasm = super::emitter::emit(&blocks, identity);
    if wasm.len() > options.wasm_bytes {
        return Err(CompileError::WasmLimit);
    }
    Ok(CompiledCode {
        identity,
        entries: blocks
            .iter()
            .map(|block| GuestAddress(block.entry))
            .collect(),
        instructions: blocks.iter().map(|block| block.instructions.len()).sum(),
        wasm,
        sources,
    })
}

fn terminates(operation: Option<Operation>) -> bool {
    matches!(
        operation,
        None | Some(
            Operation::Jump { .. }
                | Operation::ConditionalJump { .. }
                | Operation::CountBranch { .. }
                | Operation::Call { .. }
                | Operation::Return { .. }
                | Operation::Control(_)
        )
    )
}
