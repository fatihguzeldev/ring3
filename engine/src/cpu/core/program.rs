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
    memory_identity: u64,
    validated_source_version: AtomicU64,
    wasm: Vec<u8>,
    sources: Vec<CodeSnapshot>,
    entries: Vec<GuestAddress>,
    instructions: usize,
    native: Vec<NativeInstruction>,
    roots: [GuestAddress; 8],
    root_count: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct NativeInstruction {
    pub pc: u32,
    pub next: u32,
    pub operation: Operation,
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
    pub fn native_instructions(&self) -> &[NativeInstruction] {
        &self.native
    }
    pub(crate) fn formation_roots(&self) -> &[GuestAddress] {
        &self.roots[..self.root_count]
    }
    pub(crate) fn compile_transition(
        memory: &AddressSpace,
        entry: GuestAddress,
        options: CompileOptions,
        gates: &[(GuestAddress, u32)],
        frontier: &[GuestAddress],
        roots: &[GuestAddress],
    ) -> Result<Option<Self>, CompileError> {
        let mut retained = [entry; 8];
        let mut count = 1;
        for root in roots {
            if retained[..count].contains(root) {
                continue;
            }
            if count == retained.len() {
                return Ok(None);
            }
            retained[count] = *root;
            count += 1;
        }
        if count < 2
            || count > options.blocks
            || retained[..count]
                .iter()
                .any(|root| root.0 != entry.0 && frontier.contains(root))
        {
            return Ok(None);
        }
        retained[..count].sort_unstable_by_key(|root| root.0);
        let mut code =
            compile_formation(memory, entry, options, gates, frontier, &retained[..count])?;
        // shortening may keep the requested entry while dropping another owner's root.
        if !retained[..count].iter().all(|root| {
            code.entries
                .binary_search_by_key(&root.0, |pc| pc.0)
                .is_ok()
        }) {
            return Ok(None);
        }
        code.roots = retained;
        code.root_count = count;
        Ok(Some(code))
    }
    pub fn is_current(&self, memory: &AddressSpace) -> bool {
        if memory.identity() != self.memory_identity {
            return false;
        }
        let version = memory.code_source_version();
        if self.validated_source_version.load(Ordering::Relaxed) == version {
            return true;
        }
        let current = self
            .sources
            .iter()
            .all(|source| memory.is_code_current(source));
        if current {
            self.validated_source_version
                .store(version, Ordering::Relaxed);
        }
        current
    }
}

#[derive(Clone, Copy)]
pub(super) struct Instruction {
    pub pc: u32,
    pub next: u32,
    pub operation: Option<Operation>,
    pub gate: Option<u32>,
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
    compile_with_gates(memory, entry, options, &[])
}

pub fn compile_with_gates(
    memory: &AddressSpace,
    entry: GuestAddress,
    options: CompileOptions,
    gates: &[(GuestAddress, u32)],
) -> Result<CompiledCode, CompileError> {
    compile_with_frontier(memory, entry, options, gates, &[])
}

pub fn compile_with_frontier(
    memory: &AddressSpace,
    entry: GuestAddress,
    options: CompileOptions,
    gates: &[(GuestAddress, u32)],
    frontier: &[GuestAddress],
) -> Result<CompiledCode, CompileError> {
    compile_formation(memory, entry, options, gates, frontier, &[])
}

fn compile_formation(
    memory: &AddressSpace,
    entry: GuestAddress,
    options: CompileOptions,
    gates: &[(GuestAddress, u32)],
    frontier: &[GuestAddress],
    additional_roots: &[GuestAddress],
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
    let mut sorted_frontier = Vec::with_capacity(frontier.len());
    sorted_frontier.extend(
        frontier
            .iter()
            .map(|address| address.0)
            .filter(|&pc| pc != entry.0),
    );
    sorted_frontier.sort_unstable();
    sorted_frontier.dedup();
    let mut pending = VecDeque::from([entry.0]);
    let mut nodes = BTreeMap::new();
    let mut starts = BTreeSet::from([entry.0]);
    for root in additional_roots {
        if starts.contains(&root.0) {
            continue;
        }
        if starts.len() == options.blocks || sorted_frontier.binary_search(&root.0).is_ok() {
            return Err(CompileError::InvalidLimits);
        }
        starts.insert(root.0);
        pending.push_back(root.0);
    }
    let mut sources = Vec::new();
    while let Some(mut pc) = pending.pop_front() {
        if nodes.len() >= options.instructions || starts.len() > options.blocks {
            break;
        }
        while !nodes.contains_key(&pc) && nodes.len() < options.instructions {
            if sorted_frontier.binary_search(&pc).is_ok() {
                break;
            }
            let mut matching_gates = gates.iter().filter(|(address, _)| address.0 == pc);
            if let Some(&(_, id)) = matching_gates.next() {
                if id == 0 || matching_gates.next().is_some() {
                    return Err(CompileError::InvalidLimits);
                }
                let mut stub = [0; 2];
                memory
                    .fetch(GuestAddress(pc), &mut stub)
                    .map_err(CompileError::Memory)?;
                if stub != [0x0f, 0x0b] {
                    return Err(CompileError::InvalidLimits);
                }
                sources.push(
                    memory
                        .snapshot_code(GuestAddress(pc), stub.len())
                        .map_err(CompileError::Memory)?,
                );
                nodes.insert(
                    pc,
                    Instruction {
                        pc,
                        next: pc.wrapping_add(2),
                        operation: None,
                        gate: Some(id),
                    },
                );
                break;
            }
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
                    gate: None,
                },
            );
            let mut enqueue = |target| {
                if sorted_frontier.binary_search(&target).is_err() && starts.len() < options.blocks
                {
                    starts.insert(target);
                    pending.push_back(target);
                }
            };
            match operation {
                Some(
                    Operation::ConditionalJump { target, .. }
                    | Operation::CountBranch { target, .. }
                    | Operation::CountBranchWord { target, .. },
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
                Some(
                    Operation::Call { .. }
                    | Operation::CallWord { .. }
                    | Operation::CallRegisterWord { .. }
                    | Operation::ReadCallWord { .. }
                    | Operation::String { .. }
                    | Operation::RepeatString { .. }
                    | Operation::Control(crate::cpu::x86::ControlOperation::PopFlags { .. }),
                ) => {
                    enqueue(next);
                    break;
                }
                Some(
                    Operation::Jump { .. }
                    | Operation::JumpWord { .. }
                    | Operation::ReadJumpWord { .. }
                    | Operation::Return { .. }
                    | Operation::ReturnWord { .. }
                    | Operation::Control(_),
                )
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
    let (wasm, native) = loop {
        let native: Vec<_> = blocks
            .iter()
            .flat_map(|block| &block.instructions)
            .filter_map(|instruction| {
                instruction
                    .operation
                    .filter(super::emitter::is_native)
                    .map(|operation| NativeInstruction {
                        pc: instruction.pc,
                        next: instruction.next,
                        operation,
                    })
            })
            .collect();
        let wasm = super::emitter::emit(&blocks, entry.0, identity, &native);
        if wasm.len() <= options.wasm_bytes {
            break (wasm, native);
        }
        let instructions: usize = blocks.iter().map(|block| block.instructions.len()).sum();
        if instructions == 1 {
            return Err(CompileError::WasmLimit);
        }
        retain_entry_prefix(&mut blocks, entry.0, instructions / 2);
    };
    Ok(CompiledCode {
        identity,
        memory_identity: memory.identity(),
        validated_source_version: AtomicU64::new(memory.code_source_version()),
        entries: blocks
            .iter()
            .map(|block| GuestAddress(block.entry))
            .collect(),
        instructions: blocks.iter().map(|block| block.instructions.len()).sum(),
        wasm,
        sources,
        native,
        roots: [entry; 8],
        root_count: 1,
    })
}

#[cfg(test)]
#[path = "program/call_tests.rs"]
mod call_tests;

#[cfg(test)]
#[path = "program/transition_tests.rs"]
mod transition_tests;

fn retain_entry_prefix(blocks: &mut Vec<Block>, entry: u32, limit: usize) {
    let requested = blocks
        .binary_search_by_key(&entry, |block| block.entry)
        .expect("requested entry was decoded");
    let retained = blocks[requested].instructions.len().min(limit);
    blocks[requested].instructions.truncate(retained);
    let mut remaining = limit - retained;
    for (index, block) in blocks.iter_mut().enumerate() {
        if index != requested {
            let retained = block.instructions.len().min(remaining);
            block.instructions.truncate(retained);
            remaining -= retained;
        }
    }
    blocks.retain(|block| !block.instructions.is_empty());
}

fn terminates(operation: Option<Operation>) -> bool {
    matches!(
        operation,
        None | Some(
            Operation::Jump { .. }
                | Operation::JumpWord { .. }
                | Operation::ReadJumpWord { .. }
                | Operation::ConditionalJump { .. }
                | Operation::CountBranch { .. }
                | Operation::CountBranchWord { .. }
                | Operation::Call { .. }
                | Operation::CallWord { .. }
                | Operation::CallRegisterWord { .. }
                | Operation::ReadCallWord { .. }
                | Operation::Return { .. }
                | Operation::ReturnWord { .. }
                | Operation::String { .. }
                | Operation::RepeatString { .. }
                | Operation::Control(_)
        )
    )
}

#[cfg(test)]
#[path = "program/frontier_tests.rs"]
mod frontier_tests;

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
    fn direct_calls_keep_the_caller_continuation_without_absorbing_the_callee() {
        for (caller, callee, continuation) in [
            (
                &[0xe8, 0xfb, 0, 0, 0, 0x83, 0xc0, 1, 0x0f, 0x0b][..],
                &[0x83, 0xc0, 3, 0xc3][..],
                0x1005,
            ),
            (
                &[0x66, 0xe8, 0xfc, 0, 0x66, 0x40, 0x0f, 0x0b][..],
                &[0x66, 0x40, 0x66, 0xc3][..],
                0x1004,
            ),
        ] {
            let mut memory = memory(&[0x1000]);
            memory.write(GuestAddress(0x1000), caller).unwrap();
            memory.write(GuestAddress(0x1100), callee).unwrap();
            let caller = compile(&memory, GuestAddress(0x1000), limits(64, 8)).unwrap();
            assert_eq!(
                caller.entrypoints(),
                &[GuestAddress(0x1000), GuestAddress(continuation)]
            );
            assert_eq!(caller.instruction_count(), 3);
            let callee = compile(&memory, GuestAddress(0x1100), limits(64, 8)).unwrap();
            assert_eq!(callee.entrypoints(), &[GuestAddress(0x1100)]);
            assert_eq!(callee.instruction_count(), 2);
        }
    }

    fn lowering_mix() -> AddressSpace {
        let mut memory = memory(&[0x1000]);
        let mut bytes = [0x83, 0x03, 1, 0xd3, 0x23, 0x66, 0x83, 0x03, 1].repeat(24);
        bytes.extend_from_slice(&[0x0f, 0x0b]);
        memory.write(GuestAddress(0x1000), &bytes).unwrap();
        memory
    }

    #[test]
    fn an_oversized_lowering_mix_emits_a_fitting_entry_prefix() {
        let memory = lowering_mix();
        let minimum = compile(&memory, GuestAddress(0x1000), limits(1, 8)).unwrap();
        let full = compile(&memory, GuestAddress(0x1000), limits(128, 8)).unwrap();
        let quota = minimum.wasm().len() + 1024;
        assert!(full.wasm().len() > quota);
        let code = compile(
            &memory,
            GuestAddress(0x1000),
            CompileOptions {
                wasm_bytes: quota,
                ..limits(128, 8)
            },
        )
        .unwrap();
        assert!(code.wasm().len() <= quota);
        assert!(code.entrypoints().contains(&GuestAddress(0x1000)));
        assert!(
            code.instruction_count() >= 1 && code.instruction_count() < full.instruction_count()
        );
        assert!(code.is_current(&memory));
    }

    #[test]
    fn emission_size_reduction_retains_a_higher_requested_entry() {
        let mut memory = lowering_mix();
        memory
            .write(GuestAddress(0x1300), &[0xe9, 0xfb, 0xfc, 0xff, 0xff])
            .unwrap();
        let minimum = compile(&memory, GuestAddress(0x1300), limits(1, 8)).unwrap();
        let full = compile(&memory, GuestAddress(0x1300), limits(128, 8)).unwrap();
        let quota = minimum.wasm().len() + 1024;
        assert!(full.wasm().len() > quota);
        let code = compile(
            &memory,
            GuestAddress(0x1300),
            CompileOptions {
                wasm_bytes: quota,
                ..limits(128, 8)
            },
        )
        .unwrap();
        assert!(code.wasm().len() <= quota);
        assert!(code.entrypoints().contains(&GuestAddress(0x1300)));
        assert!(
            code.instruction_count() >= 1 && code.instruction_count() < full.instruction_count()
        );
        assert!(code.is_current(&memory));
    }

    #[test]
    fn an_unrepresentable_single_instruction_still_reports_the_byte_limit() {
        let memory = lowering_mix();
        assert!(matches!(
            compile(
                &memory,
                GuestAddress(0x1000),
                CompileOptions {
                    wasm_bytes: 128,
                    ..limits(128, 8)
                }
            ),
            Err(CompileError::WasmLimit)
        ));
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

    fn source_memo_memory() -> AddressSpace {
        let mut memory = AddressSpace::new(4).unwrap();
        for (page, permissions) in [
            (0x1000, Permissions::ALL),
            (0x2000, Permissions::ALL),
            (0x3000, Permissions::READ_WRITE),
        ] {
            memory
                .map_zeroed(PageRange::new(GuestAddress(page), 1).unwrap(), permissions)
                .unwrap();
        }
        memory
            .write(GuestAddress(0x1000), &[0x90, 0x0f, 0x0b])
            .unwrap();
        memory
            .write(GuestAddress(0x2000), &[0x90, 0x0f, 0x0b])
            .unwrap();
        memory
    }

    #[test]
    fn source_memo_rejects_foreign_memory_with_the_same_stamp() {
        let memory = source_memo_memory();
        let foreign = source_memo_memory();
        let code = compile(&memory, GuestAddress(0x1000), limits(8, 1)).unwrap();
        let stamp = memory.code_source_version();
        assert_eq!(stamp, foreign.code_source_version());
        assert_ne!(memory.identity(), foreign.identity());
        assert_eq!(code.memory_identity, memory.identity());
        assert_eq!(code.validated_source_version.load(Ordering::Relaxed), stamp);
        assert!(code.is_current(&memory));
        assert!(!code.is_current(&foreign));
        assert_eq!(code.validated_source_version.load(Ordering::Relaxed), stamp);
    }

    #[test]
    fn source_memo_stays_valid_through_private_data_writes_and_new_rw_pages() {
        let mut memory = source_memo_memory();
        let code = compile(&memory, GuestAddress(0x1000), limits(8, 1)).unwrap();
        let stamp = memory.code_source_version();
        memory.core_write(0x3000, 4, 0x1234_5678).unwrap();
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0x4000), 1).unwrap(),
                Permissions::READ_WRITE,
            )
            .unwrap();
        memory.write(GuestAddress(0x4000), &[1, 2, 3, 4]).unwrap();
        assert_eq!(memory.code_source_version(), stamp);
        assert!(code.is_current(&memory));
        assert_eq!(code.validated_source_version.load(Ordering::Relaxed), stamp);
    }

    #[test]
    fn source_memo_advances_only_after_successful_full_source_revalidation() {
        let mut memory = source_memo_memory();
        let code = compile(&memory, GuestAddress(0x1000), limits(8, 1)).unwrap();
        let old = memory.code_source_version();
        memory.write(GuestAddress(0x2000), &[0x40]).unwrap();
        let current = memory.code_source_version();
        assert_ne!(old, current);
        assert_eq!(code.validated_source_version.load(Ordering::Relaxed), old);
        assert!(code.is_current(&memory));
        assert_eq!(
            code.validated_source_version.load(Ordering::Relaxed),
            current
        );
        memory.core_write(0x1000, 1, 0x40).unwrap();
        assert_ne!(memory.code_source_version(), current);
        assert!(!code.is_current(&memory));
        assert_eq!(
            code.validated_source_version.load(Ordering::Relaxed),
            current
        );
        assert!(!code.is_current(&memory));
    }

    #[test]
    fn source_memo_does_not_revive_after_execute_permission_changes() {
        let mut memory = source_memo_memory();
        let code = compile(&memory, GuestAddress(0x1000), limits(8, 1)).unwrap();
        let stamp = code.validated_source_version.load(Ordering::Relaxed);
        let range = PageRange::new(GuestAddress(0x1000), 1).unwrap();
        memory.protect(range, Permissions::READ_WRITE).unwrap();
        assert!(!code.is_current(&memory));
        memory.protect(range, Permissions::ALL).unwrap();
        assert!(!code.is_current(&memory));
        assert_eq!(code.validated_source_version.load(Ordering::Relaxed), stamp);
    }

    #[test]
    fn source_memo_rejects_identical_bytes_after_unmap_and_remap() {
        let mut memory = source_memo_memory();
        let code = compile(&memory, GuestAddress(0x1000), limits(8, 1)).unwrap();
        let stamp = code.validated_source_version.load(Ordering::Relaxed);
        let range = PageRange::new(GuestAddress(0x1000), 1).unwrap();
        memory.unmap(range).unwrap();
        memory.map_zeroed(range, Permissions::ALL).unwrap();
        memory
            .write(GuestAddress(0x1000), &[0x90, 0x0f, 0x0b])
            .unwrap();
        assert!(!code.is_current(&memory));
        assert_eq!(code.validated_source_version.load(Ordering::Relaxed), stamp);
    }

    #[test]
    fn source_memo_rejects_a_write_through_a_nonexecutable_alias() {
        let mut memory = AddressSpace::new(1).unwrap();
        let pages = memory
            .allocate_shared_pages(1, &[0x90, 0x0f, 0x0b])
            .unwrap();
        memory
            .map_shared_pages(
                PageRange::new(GuestAddress(0x1000), 1).unwrap(),
                &pages,
                0,
                Permissions::READ_EXECUTE,
                false,
            )
            .unwrap();
        memory
            .map_shared_pages(
                PageRange::new(GuestAddress(0x3000), 1).unwrap(),
                &pages,
                0,
                Permissions::READ_WRITE,
                false,
            )
            .unwrap();
        let code = compile(&memory, GuestAddress(0x1000), limits(8, 1)).unwrap();
        let stamp = code.validated_source_version.load(Ordering::Relaxed);
        memory.core_write(0x3000, 1, 0x40).unwrap();
        assert_ne!(memory.code_source_version(), stamp);
        assert!(!code.is_current(&memory));
        assert_eq!(code.validated_source_version.load(Ordering::Relaxed), stamp);
    }

    #[test]
    fn native_metadata_retains_typed_operations_and_decoded_addresses() {
        let mut memory = memory(&[0x1000]);
        memory
            .write(
                GuestAddress(0x1000),
                &[0xd9, 0x03, 0xd8, 0x43, 0x04, 0xd9, 0x5b, 0x08, 0x0f, 0x0b],
            )
            .unwrap();
        let code = compile(&memory, GuestAddress(0x1000), limits(8, 1)).unwrap();
        let native = code.native_instructions();
        assert_eq!(code.instruction_count(), 4);
        assert_eq!(
            native
                .iter()
                .map(|item| (item.pc, item.next))
                .collect::<Vec<_>>(),
            [(0x1000, 0x1002), (0x1002, 0x1005), (0x1005, 0x1008)]
        );
        let Operation::X87 {
            operation,
            address: Some(address),
            waiting,
            opcode,
        } = native[1].operation
        else {
            panic!("expected typed x87 memory operation");
        };
        assert_eq!(operation.read_size(), 4);
        assert_eq!(operation.write_size(), 0);
        assert_eq!(address.base, Some(crate::cpu::x86::Register32::Ebx));
        assert_eq!(address.displacement, 4);
        assert!(waiting);
        assert_eq!(opcode, 0x43);
    }

    #[test]
    fn integer_code_does_not_require_native_operations() {
        let mut memory = memory(&[0x1000]);
        memory
            .write(GuestAddress(0x1000), &[0x40, 0x0f, 0x0b])
            .unwrap();
        let code = compile(&memory, GuestAddress(0x1000), limits(8, 1)).unwrap();
        assert!(code.native_instructions().is_empty());
    }

    #[test]
    fn stack_frames_retain_width_allocation_and_nesting_for_transactional_execution() {
        use crate::cpu::x86::StackFrameKind;

        let mut memory = memory(&[0x1000]);
        memory
            .write(
                GuestAddress(0x1000),
                &[
                    0x66, 0x60, 0x60, 0xc8, 0x20, 0, 2, 0x66, 0xc8, 0x10, 0, 0, 0x0f, 0x0b,
                ],
            )
            .unwrap();
        let code = compile(&memory, GuestAddress(0x1000), limits(8, 1)).unwrap();
        let native = code.native_instructions();
        assert_eq!(native.len(), 4);
        for (instruction, expected) in native.iter().zip([
            (0x1000, 0x1002, StackFrameKind::PushAll16, 0, 0),
            (0x1002, 0x1003, StackFrameKind::PushAll32, 0, 0),
            (0x1003, 0x1007, StackFrameKind::Enter32, 32, 2),
            (0x1007, 0x100c, StackFrameKind::Enter16, 16, 0),
        ]) {
            assert_eq!((instruction.pc, instruction.next), (expected.0, expected.1));
            let Operation::StackFrame {
                kind,
                allocation,
                nesting,
            } = instruction.operation
            else {
                panic!("expected typed stack frame");
            };
            assert_eq!(
                (kind, allocation, nesting),
                (expected.2, expected.3, expected.4)
            );
        }
    }

    #[test]
    fn declared_gates_require_exact_executable_stubs_and_current_sources() {
        let mut memory = memory(&[0x1000]);
        memory.write(GuestAddress(0x1000), &[0x0f, 0x0b]).unwrap();
        let code = compile_with_gates(
            &memory,
            GuestAddress(0x1000),
            limits(8, 1),
            &[(GuestAddress(0x1000), 17)],
        )
        .unwrap();
        assert_eq!(code.entrypoints(), &[GuestAddress(0x1000)]);
        assert_eq!(code.instruction_count(), 1);
        assert!(code.native_instructions().is_empty());
        assert!(code.is_current(&memory));
        memory.write(GuestAddress(0x1001), &[0x90]).unwrap();
        assert!(!code.is_current(&memory));
        assert!(matches!(
            compile_with_gates(
                &memory,
                GuestAddress(0x1000),
                limits(8, 1),
                &[(GuestAddress(0x1000), 17)],
            ),
            Err(CompileError::InvalidLimits)
        ));
        memory.write(GuestAddress(0x1000), &[0x0f, 0x0b]).unwrap();
        memory
            .protect(
                PageRange::new(GuestAddress(0x1000), 1).unwrap(),
                Permissions::READ,
            )
            .unwrap();
        assert!(matches!(
            compile_with_gates(
                &memory,
                GuestAddress(0x1000),
                limits(8, 1),
                &[(GuestAddress(0x1000), 17)],
            ),
            Err(CompileError::Memory(_))
        ));
    }

    #[test]
    fn only_reached_gate_metadata_is_validated() {
        let mut memory = memory(&[0x1000]);
        memory.write(GuestAddress(0x1000), &[0x0f, 0x0b]).unwrap();
        for gates in [
            vec![(GuestAddress(0x1000), 0)],
            vec![(GuestAddress(0x1000), 17), (GuestAddress(0x1000), 18)],
        ] {
            assert!(matches!(
                compile_with_gates(&memory, GuestAddress(0x1000), limits(8, 1), &gates),
                Err(CompileError::InvalidLimits)
            ));
        }
        let code = compile_with_gates(
            &memory,
            GuestAddress(0x1000),
            limits(8, 1),
            &[(GuestAddress(0x2000), 0)],
        )
        .unwrap();
        assert_eq!(code.instruction_count(), 1);
        assert!(code.native_instructions().is_empty());
    }
}
