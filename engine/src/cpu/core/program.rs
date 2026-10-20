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
                _ if next < pc => {
                    enqueue(next);
                    break;
                }
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
    if blocks.len() > options.blocks {
        let requested = blocks
            .binary_search_by_key(&entry.0, |block| block.entry)
            .expect("requested entry was decoded");
        if requested >= options.blocks {
            blocks.swap(options.blocks - 1, requested);
        }
        blocks.truncate(options.blocks);
        blocks.sort_unstable_by_key(|block| block.entry);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{PageRange, Permissions};

    fn memory(pages: &[u32]) -> AddressSpace {
        let mut memory = AddressSpace::new(pages.len() as u32).unwrap();
        for &page in pages {
            memory
                .map_zeroed(
                    PageRange::new(GuestAddress(page), 1).unwrap(),
                    Permissions::ALL,
                )
                .unwrap();
        }
        memory
    }

    fn limits(instructions: usize, blocks: usize) -> CompileOptions {
        CompileOptions {
            instructions,
            blocks,
            ..CompileOptions::default()
        }
    }

    fn wrapping_memory() -> AddressSpace {
        let mut memory = memory(&[0, 0xffff_f000]);
        memory.write(GuestAddress(u32::MAX), &[0x90]).unwrap();
        memory.write(GuestAddress(0), &[0x0f, 0x0b]).unwrap();
        memory
    }

    #[test]
    fn natural_wrap_cannot_add_a_block_beyond_the_limit() {
        let memory = wrapping_memory();
        let code = compile(&memory, GuestAddress(u32::MAX), limits(8, 1)).unwrap();
        assert_eq!(code.entrypoints(), &[GuestAddress(u32::MAX)]);
        assert_eq!(code.instruction_count(), 1);
        assert!(code.is_current(&memory));
    }

    #[test]
    fn natural_wrap_can_enter_zero_when_another_block_is_available() {
        let memory = wrapping_memory();
        let code = compile(&memory, GuestAddress(u32::MAX), limits(8, 2)).unwrap();
        assert_eq!(
            code.entrypoints(),
            &[GuestAddress(0), GuestAddress(u32::MAX)]
        );
        assert_eq!(code.instruction_count(), 2);
        assert!(code.is_current(&memory));
    }

    #[test]
    fn a_nonzero_entry_keeps_its_backward_target_in_the_sorted_blocks() {
        let mut memory = memory(&[0x1000]);
        memory
            .write(GuestAddress(0x1000), &[0x90, 0x0f, 0x0b])
            .unwrap();
        memory.write(GuestAddress(0x1010), &[0xeb, 0xee]).unwrap();
        let code = compile(&memory, GuestAddress(0x1010), limits(8, 2)).unwrap();
        assert_eq!(
            code.entrypoints(),
            &[GuestAddress(0x1000), GuestAddress(0x1010)]
        );
        assert_eq!(code.instruction_count(), 3);
    }

    #[test]
    fn overlapping_decode_streams_obey_the_final_block_limit() {
        let mut memory = memory(&[0x1000]);
        memory
            .write(
                GuestAddress(0x1000),
                &[0xb8, 0x00, 0xb8, 0x11, 0x22, 0x75, 0xfb, 0x0f, 0x0b],
            )
            .unwrap();
        let code = compile(&memory, GuestAddress(0x1000), limits(8, 2)).unwrap();
        assert_eq!(
            code.entrypoints(),
            &[GuestAddress(0x1000), GuestAddress(0x1002)]
        );
        assert_eq!(code.instruction_count(), 2);
        assert!(!code.entrypoints().contains(&GuestAddress(0x1005)));
        assert!(!code.entrypoints().contains(&GuestAddress(0x1007)));
        assert!(code.is_current(&memory));
    }

    #[test]
    fn the_final_block_limit_keeps_a_higher_requested_entry() {
        let mut memory = memory(&[0x1000]);
        memory
            .write(
                GuestAddress(0x1000),
                &[0xb8, 0x00, 0xb8, 0x11, 0x22, 0x75, 0xfb, 0x0f, 0x0b],
            )
            .unwrap();
        memory.write(GuestAddress(0x1010), &[0xeb, 0xee]).unwrap();
        let code = compile(&memory, GuestAddress(0x1010), limits(8, 3)).unwrap();
        assert_eq!(
            code.entrypoints(),
            &[
                GuestAddress(0x1000),
                GuestAddress(0x1002),
                GuestAddress(0x1010),
            ]
        );
        assert_eq!(code.instruction_count(), 3);
        assert!(!code.entrypoints().contains(&GuestAddress(0x1005)));
        assert!(!code.entrypoints().contains(&GuestAddress(0x1007)));
        assert!(code.is_current(&memory));
    }

    #[test]
    fn the_instruction_limit_applies_to_linear_and_queued_followups() {
        let mut linear = memory(&[0x1000]);
        linear
            .write(GuestAddress(0x1000), &[0x90, 0x90, 0x90, 0x0f, 0x0b])
            .unwrap();
        let code = compile(&linear, GuestAddress(0x1000), limits(2, 8)).unwrap();
        assert_eq!(code.entrypoints(), &[GuestAddress(0x1000)]);
        assert_eq!(code.instruction_count(), 2);

        let wrapping = wrapping_memory();
        let code = compile(&wrapping, GuestAddress(u32::MAX), limits(1, 2)).unwrap();
        assert_eq!(code.entrypoints(), &[GuestAddress(u32::MAX)]);
        assert_eq!(code.instruction_count(), 1);
    }

    #[test]
    fn source_changes_and_replaced_mappings_invalidate_compiled_code() {
        let mut memory = memory(&[0x1000]);
        let range = PageRange::new(GuestAddress(0x1000), 1).unwrap();
        memory
            .write(GuestAddress(0x1000), &[0x90, 0x0f, 0x0b])
            .unwrap();
        let original = compile(&memory, GuestAddress(0x1000), limits(8, 1)).unwrap();
        assert!(original.is_current(&memory));
        memory.write(GuestAddress(0x1000), &[0x40]).unwrap();
        assert!(!original.is_current(&memory));

        let before_remap = compile(&memory, GuestAddress(0x1000), limits(8, 1)).unwrap();
        assert!(before_remap.is_current(&memory));
        memory.unmap(range).unwrap();
        assert!(!before_remap.is_current(&memory));
        memory.map_zeroed(range, Permissions::ALL).unwrap();
        memory
            .write(GuestAddress(0x1000), &[0x40, 0x0f, 0x0b])
            .unwrap();
        assert!(!before_remap.is_current(&memory));
        assert!(
            compile(&memory, GuestAddress(0x1000), limits(8, 1))
                .unwrap()
                .is_current(&memory)
        );
    }
}
