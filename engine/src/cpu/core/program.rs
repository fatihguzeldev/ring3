use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

mod observed_cold;
#[cfg(test)]
mod reform;
pub(crate) mod spans;
pub(crate) use observed_cold::{
    ObservedPlan, ObservedPlanFingerprint, WindowFingerprint, WindowPlan,
    compile_observed_with_cold, emit_hot_window, emit_observed_plan, prepare_cold_prefix_plan,
    prepare_hot_window, prepare_observed_plan,
};

static NEXT_IDENTITY: AtomicU64 = AtomicU64::new(1);

use crate::{
    cpu::x86::{
        decode::{BoundDecode, DecodeAuthority, DecodeError, DecodePolicy, decode_for_compilation},
        ir::{BranchTarget, Operation},
    },
    memory::{AddressSpace, CodeSnapshot, GuestAddress, MemoryError},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TraceScore {
    pub instructions: usize,
    pub internal_edges: usize,
}

impl TraceScore {
    /// Replacing a retained trace must add executable work or connectivity
    /// without reducing either component. Equal recordings cannot recompile it.
    pub(crate) fn strictly_grows(self, previous: Self) -> bool {
        self.instructions >= previous.instructions
            && self.internal_edges >= previous.internal_edges
            && self != previous
    }

    fn emitted(blocks: &[Block], observed_edges: &BTreeSet<(u32, u32)>) -> Self {
        let pcs: BTreeSet<_> = blocks
            .iter()
            .flat_map(|block| block.instructions.iter().map(|instruction| instruction.pc))
            .collect();
        Self {
            instructions: blocks.iter().map(|block| block.instructions.len()).sum(),
            internal_edges: observed_edges
                .iter()
                .filter(|(source, target)| pcs.contains(source) && pcs.contains(target))
                .count(),
        }
    }
}

#[derive(Debug)]
pub struct CompiledCode {
    identity: u64,
    memory_identity: u64,
    validated_source_version: AtomicU64,
    wasm: Vec<u8>,
    sources: Vec<CodeSnapshot>,
    entries: Vec<GuestAddress>,
    instruction_pcs: Box<[u32]>,
    block_pcs: Box<[u32]>,
    blocks: Box<[CodeBlock]>,
    hot_spans: Box<[HotSpan]>,
    native: Vec<NativeInstruction>,
    trace_root: Option<u32>,
    trace_score: Option<TraceScore>,
    observed: Option<Arc<observed_cold::ObservedHistory>>,
    observed_fingerprint: Option<ObservedPlanFingerprint>,
    window_fingerprint: Option<WindowFingerprint>,
    #[cfg(test)]
    roots: Box<[GuestAddress]>,
}

// PC geometry only: recording validation never retains decoded Operations/IR or
// decodes an unexecuted prefix to recover an interior entry's instruction PC.
#[derive(Debug)]
struct CodeBlock {
    pc: u32,
    end: u32,
    first: u32,
    count: u32,
}

/// an executable entry with a certified minimum fuel requirement. fixed spans
/// consume that exact work; isolated budgeted kernels meter their own progress.
/// source coverage and arbitrary precise ports remain separate authorities.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct HotSpan {
    pub entry: u32,
    pub end: u32,
    /// minimum work before entry, including one fuel for a budgeted string.
    pub instructions: u32,
    pub port: u32,
}

#[cfg(test)]
mod hot_span_tests {
    use super::*;
    use crate::memory::{PageRange, Permissions};

    #[test]
    fn executable_spans_keep_source_ports_and_budgeted_kernel_minimum_fuel() {
        let mut memory = AddressSpace::new(2).unwrap();
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0x1000), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
        // NOP; CPUID; NOP; REP MOVSB; NOP.
        memory
            .write(
                GuestAddress(0x1000),
                &[0x90, 0x0f, 0xa2, 0x90, 0xf3, 0xa4, 0x90],
            )
            .unwrap();
        let code = compile(
            &memory,
            GuestAddress(0x1000),
            CompileOptions {
                instructions: 5,
                blocks: 8,
                ..CompileOptions::default()
            },
        )
        .unwrap();
        assert!(code.contains_instruction(0x1001));
        assert!(code.supports_entry(0x1001));
        assert_eq!(code.hot_span(0x1001), None);
        assert_eq!(
            code.hot_span(0x1000),
            Some(HotSpan {
                entry: 0x1000,
                end: 0x1003,
                instructions: 2,
                port: 0,
            })
        );
        assert_eq!(code.eligible_hot_span(0x1000, 1), None);
        assert_eq!(code.eligible_hot_span(0x1000, 2), code.hot_span(0x1000));
        assert_eq!(
            code.hot_span(0x1003),
            Some(HotSpan {
                entry: 0x1003,
                end: 0x1004,
                instructions: 1,
                port: 1,
            })
        );
        assert_eq!(
            code.hot_span(0x1004),
            Some(HotSpan {
                entry: 0x1004,
                end: 0x1006,
                instructions: 1,
                port: 2,
            })
        );
        assert_eq!(code.eligible_hot_span(0x1004, 0), None);
        assert_eq!(code.eligible_hot_span(0x1004, 1), code.hot_span(0x1004));
        assert_eq!(
            code.hot_span(0x1006),
            Some(HotSpan {
                entry: 0x1006,
                end: 0x1007,
                instructions: 1,
                port: 3,
            })
        );
    }
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
    #[cfg(test)]
    pub(crate) fn reserve_wasm_capacity_for_test(&mut self, additional: usize) {
        self.wasm.reserve(additional);
    }
    #[cfg(test)]
    pub(crate) fn set_wasm_capacity_for_test(&mut self, capacity: usize) -> usize {
        assert!(capacity >= self.wasm.len());
        let mut bytes = Vec::with_capacity(capacity);
        bytes.extend_from_slice(&self.wasm);
        self.wasm = bytes;
        assert!(self.wasm.capacity() >= capacity);
        self.wasm.capacity()
    }
    /// Declared owned storage, including reserved capacities. Cache directories,
    /// Rc control words, allocator overhead and opaque browser JIT storage are
    /// accounted separately or remain outside this metric.
    pub(crate) fn resident_charge(&self) -> usize {
        let mut charge = std::mem::size_of::<Self>()
            + self.wasm.capacity()
            + self.sources.capacity() * std::mem::size_of::<CodeSnapshot>()
            + self
                .sources
                .iter()
                .map(|source| std::mem::size_of_val(source.versions.as_ref()))
                .sum::<usize>()
            + self.entries.capacity() * std::mem::size_of::<GuestAddress>()
            + std::mem::size_of_val(self.instruction_pcs.as_ref())
            + std::mem::size_of_val(self.block_pcs.as_ref())
            + std::mem::size_of_val(self.blocks.as_ref())
            + std::mem::size_of_val(self.hot_spans.as_ref())
            + self.native.capacity() * std::mem::size_of::<NativeInstruction>();
        if let Some(history) = &self.observed {
            charge += history.resident_charge();
        }
        if let Some(fingerprint) = &self.observed_fingerprint {
            charge += fingerprint.metadata_charge();
        }
        if let Some(fingerprint) = &self.window_fingerprint {
            charge += fingerprint.metadata_charge() + fingerprint.graph_charge();
        }
        #[cfg(test)]
        {
            charge += std::mem::size_of_val(self.roots.as_ref());
        }
        charge
    }
    pub fn entrypoints(&self) -> &[GuestAddress] {
        &self.entries
    }
    pub fn instruction_count(&self) -> usize {
        self.instruction_pcs.len()
    }
    pub(crate) fn supports_entry(&self, pc: u32) -> bool {
        if self.observed.is_some() {
            return self.instruction_pcs.binary_search(&pc).is_ok();
        }
        self.trace_root.map_or_else(
            || self.instruction_pcs.binary_search(&pc).is_ok(),
            |root| root == pc,
        )
    }
    pub(crate) fn contains_instruction(&self, pc: u32) -> bool {
        self.instruction_pcs.binary_search(&pc).is_ok()
    }
    pub(crate) fn hot_span(&self, pc: u32) -> Option<HotSpan> {
        if !self.supports_entry(pc) {
            return None;
        }
        self.hot_spans
            .binary_search_by_key(&pc, |span| span.entry)
            .ok()
            .map(|index| self.hot_spans[index])
    }
    pub(crate) fn eligible_hot_span(&self, pc: u32, remaining: u32) -> Option<HotSpan> {
        self.hot_span(pc)
            .filter(|span| span.instructions <= remaining)
    }
    pub(crate) fn hot_spans(&self) -> impl Iterator<Item = HotSpan> + '_ {
        self.hot_spans
            .iter()
            .copied()
            .filter(|span| self.supports_entry(span.entry))
    }
    pub(crate) fn trace_root(&self) -> Option<u32> {
        self.trace_root
    }
    pub(crate) fn trace_score(&self) -> Option<TraceScore> {
        self.trace_score
    }
    pub(crate) fn is_observed_family(&self) -> bool {
        self.observed.is_some()
    }
    pub(crate) fn observed_fingerprint(&self) -> Option<&ObservedPlanFingerprint> {
        self.observed_fingerprint.as_ref()
    }
    pub(crate) fn is_execution_window(&self) -> bool {
        self.window_fingerprint.is_some()
    }
    pub(crate) fn window_fingerprint(&self) -> Option<&WindowFingerprint> {
        self.window_fingerprint.as_ref()
    }
    pub(crate) fn observed_grows(&self, previous: &Self) -> bool {
        self.memory_identity == previous.memory_identity
            && self
                .observed
                .as_ref()
                .zip(previous.observed.as_ref())
                .is_some_and(|(next, old)| next.strictly_grows(old))
    }
    pub(crate) fn instruction_pcs(&self) -> impl Iterator<Item = u32> + '_ {
        self.instruction_pcs.iter().copied()
    }
    pub fn native_instructions(&self) -> &[NativeInstruction] {
        &self.native
    }
    #[cfg(test)]
    pub(crate) fn formation_roots(&self) -> &[GuestAddress] {
        &self.roots
    }
    #[cfg(test)]
    pub(crate) fn compile_transition(
        memory: &AddressSpace,
        entry: GuestAddress,
        options: CompileOptions,
        gates: &[(GuestAddress, u32)],
        frontier: &[GuestAddress],
        roots: &[GuestAddress],
    ) -> Result<Option<Self>, CompileError> {
        let mut retained = roots.to_vec();
        retained.push(entry);
        retained.sort_unstable_by_key(|root| root.0);
        retained.dedup();
        if retained.len() < 2
            || retained.len() > options.blocks
            || retained
                .iter()
                .any(|root| root.0 != entry.0 && frontier.contains(root))
        {
            return Ok(None);
        }
        let mut code = compile_formation(memory, entry, options, gates, frontier, &retained)?;
        // shortening may keep the requested entry while dropping another owner's root.
        if !retained.iter().all(|root| {
            code.entries
                .binary_search_by_key(&root.0, |pc| pc.0)
                .is_ok()
        }) {
            return Ok(None);
        }
        code.roots = retained.into_boxed_slice();
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

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Instruction {
    pub pc: u32,
    pub next: u32,
    pub operation: Option<Operation>,
    pub gate: Option<u32>,
}

#[derive(Clone, Debug)]
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

pub fn compile_with_frontier_query(
    memory: &AddressSpace,
    entry: GuestAddress,
    options: CompileOptions,
    gates: &[(GuestAddress, u32)],
    frontier: &dyn Fn(u32) -> bool,
) -> Result<CompiledCode, CompileError> {
    compile_formation_query(memory, entry, options, gates, frontier, &[])
}

/// Compile only confirmed recorded spans. The final event is a witness for its
/// predecessor, never evidence that its own instruction(s) completed.
pub(crate) fn compile_observed(
    memory: &AddressSpace,
    root: GuestAddress,
    options: CompileOptions,
    events: &[super::trace::TraceEvent],
    owners: &[&CompiledCode],
    expected_source_version: u64,
) -> Result<Option<CompiledCode>, CompileError> {
    compile_observed_with_growth(
        memory,
        root,
        options,
        events,
        owners,
        expected_source_version,
        None,
    )
}

/// An existing executor requires strict growth before allocating an identity or
/// emitting Wasm. The score covers this capture, never an unobserved owner union.
pub(crate) fn compile_observed_with_growth(
    memory: &AddressSpace,
    root: GuestAddress,
    options: CompileOptions,
    events: &[super::trace::TraceEvent],
    owners: &[&CompiledCode],
    expected_source_version: u64,
    previous: Option<TraceScore>,
) -> Result<Option<CompiledCode>, CompileError> {
    use super::trace::{BLOCK, CAPACITY, NUMERIC};
    if validate_options(options).is_err()
        || !(2..=CAPACITY).contains(&events.len())
        || memory.code_source_version() != expected_source_version
        || owners.is_empty()
        || owners.iter().any(|owner| !owner.is_current(memory))
    {
        return Ok(None);
    }
    let mut retained = BTreeMap::new();
    for &owner in owners {
        retained.entry(owner.identity()).or_insert(owner);
    }
    let mut sites = Vec::with_capacity(events.len());
    for event in events {
        let Some(owner) = retained.get(&event.owner).copied() else {
            return Ok(None);
        };
        let pcs = match event.kind {
            BLOCK => {
                let Ok(index) = owner
                    .blocks
                    .binary_search_by_key(&event.pc, |block| block.pc)
                else {
                    return Ok(None);
                };
                let block = &owner.blocks[index];
                if event.end != block.end || event.offset >= block.count {
                    return Ok(None);
                }
                &owner.block_pcs
                    [(block.first + event.offset) as usize..(block.first + block.count) as usize]
            }
            NUMERIC => {
                if event.offset != 0
                    || !owner.native.iter().any(|instruction| {
                        instruction.pc == event.pc && instruction.next == event.end
                    })
                {
                    return Ok(None);
                }
                let Ok(index) = owner.instruction_pcs.binary_search(&event.pc) else {
                    return Ok(None);
                };
                &owner.instruction_pcs[index..index + 1]
            }
            _ => return Ok(None),
        };
        sites.push((owner, pcs));
    }
    if sites[0].1[0] != root.0 {
        return Ok(None);
    }

    let mut nodes = BTreeMap::new();
    let mut sources = Vec::new();
    let mut starts = BTreeSet::from([root.0]);
    let mut edges = BTreeSet::new();
    for index in 0..events.len() - 1 {
        let event = &events[index];
        let next = &events[index + 1];
        let (owner, suffix) = sites[index];
        let target = sites[index + 1].1[0];
        let pcs = if event.kind == BLOCK && next.kind == NUMERIC {
            if event.owner != next.owner {
                return Ok(None);
            }
            let Some(end) = suffix.iter().position(|&pc| pc == next.pc) else {
                return Ok(None);
            };
            &suffix[..end]
        } else if event.kind == BLOCK
            && next.kind == BLOCK
            && event.owner == next.owner
            && event.pc == next.pc
            && event.end == next.end
            && event.offset < next.offset
        {
            // Multiple executable spans can share one original source block.
            // Its next span witnesses only the completed prefix before it.
            let Some(end) = suffix.iter().position(|&pc| pc == target) else {
                return Ok(None);
            };
            &suffix[..end]
        } else {
            suffix
        };
        starts.insert(suffix[0]);
        starts.insert(target);
        for (position, &pc) in pcs.iter().enumerate() {
            if !nodes.contains_key(&pc) {
                if nodes.len() == options.instructions {
                    return Ok(None);
                }
                let Ok((instruction, source)) = decode_instruction(memory, pc, &[]) else {
                    return Ok(None);
                };
                if !observed_operation(instruction.operation) {
                    return Ok(None);
                }
                sources.push(source);
                nodes.insert(pc, instruction);
            }
            let instruction = nodes[&pc];
            let successor = pcs.get(position + 1).copied().unwrap_or(target);
            // Prefixes are straight-line slices of the recorded source block.
            // Only the final instruction may select a control successor.
            if position + 1 < pcs.len() {
                if instruction.next != successor || terminates(instruction.operation) {
                    return Ok(None);
                }
            } else if !observed_successor(instruction, successor) {
                return Ok(None);
            }
            if event.kind == NUMERIC
                && (instruction.next != event.end
                    || !owner.native.iter().any(|native| {
                        native.pc == pc
                            && native.next == instruction.next
                            && Some(native.operation) == instruction.operation
                    }))
            {
                return Ok(None);
            }
            edges.insert((pc, successor));
        }
    }
    if nodes.is_empty() || !nodes.contains_key(&root.0) {
        return Ok(None);
    }
    let blocks = partition_blocks(nodes, &starts);
    if blocks.len() > options.blocks {
        return Ok(None);
    }
    let native = collect_native(&blocks);
    let score = TraceScore::emitted(&blocks, &edges);
    if previous.is_some_and(|previous| !score.strictly_grows(previous)) {
        return Ok(None);
    }
    let identity = next_identity()?;
    let observed_edges: Vec<_> = edges.into_iter().collect();
    let wasm = super::emitter::emit_observed(&blocks, root.0, identity, &native, &observed_edges);
    if wasm.len() > options.wasm_bytes
        || memory.code_source_version() != expected_source_version
        || owners.iter().any(|owner| !owner.is_current(memory))
    {
        return Ok(None);
    }
    Ok(Some(finish_code(
        memory,
        blocks,
        identity,
        wasm,
        native,
        sources,
        &[root],
        Some((root.0, score)),
    )))
}

fn observed_operation(operation: Option<Operation>) -> bool {
    use crate::cpu::x86::ControlOperation;
    !matches!(
        operation,
        None | Some(Operation::SoftwareInterrupt { .. })
            | Some(Operation::Control(
                ControlOperation::Sysenter
                    | ControlOperation::Breakpoint
                    | ControlOperation::InvalidOpcode
                    | ControlOperation::GeneralProtectionZero
                    | ControlOperation::DebugTrap
            ))
    )
}

fn observed_successor(instruction: Instruction, successor: u32) -> bool {
    match instruction.operation {
        Some(
            Operation::Jump {
                target: BranchTarget::Direct(target),
            }
            | Operation::Call {
                target: BranchTarget::Direct(target),
            },
        ) => successor == target.0,
        Some(Operation::CallWord { target }) => successor == (target.0 & 0xffff),
        Some(
            Operation::ConditionalJump { target, .. }
            | Operation::CountBranch { target, .. }
            | Operation::CountBranchWord { target, .. },
        ) => successor == target.0 || successor == instruction.next,
        Some(
            Operation::JumpWord { .. }
            | Operation::ReadJumpWord { .. }
            | Operation::CallRegisterWord { .. }
            | Operation::ReadCallWord { .. }
            | Operation::ReturnWord { .. },
        ) => successor <= u32::from(u16::MAX),
        Some(Operation::Jump { .. } | Operation::Call { .. } | Operation::Return { .. }) => true,
        _ => successor == instruction.next,
    }
}

fn compile_formation(
    memory: &AddressSpace,
    entry: GuestAddress,
    options: CompileOptions,
    gates: &[(GuestAddress, u32)],
    frontier: &[GuestAddress],
    additional_roots: &[GuestAddress],
) -> Result<CompiledCode, CompileError> {
    validate_options(options)?;
    let mut sorted_frontier: Vec<_> = frontier.iter().map(|address| address.0).collect();
    sorted_frontier.sort_unstable();
    sorted_frontier.dedup();
    compile_formation_query(
        memory,
        entry,
        options,
        gates,
        &|pc| sorted_frontier.binary_search(&pc).is_ok(),
        additional_roots,
    )
}

fn validate_options(options: CompileOptions) -> Result<(), CompileError> {
    if options.instructions == 0
        || options.instructions > 16384
        || options.blocks == 0
        || options.blocks > 1024
        || options.wasm_bytes < 128
        || options.wasm_bytes > 16 << 20
    {
        return Err(CompileError::InvalidLimits);
    }
    Ok(())
}

fn compile_formation_query(
    memory: &AddressSpace,
    entry: GuestAddress,
    options: CompileOptions,
    gates: &[(GuestAddress, u32)],
    frontier: &dyn Fn(u32) -> bool,
    additional_roots: &[GuestAddress],
) -> Result<CompiledCode, CompileError> {
    validate_options(options)?;
    let is_frontier = |pc| pc != entry.0 && frontier(pc);
    let mut pending = VecDeque::from([entry.0]);
    let mut nodes = BTreeMap::new();
    let mut starts = BTreeSet::from([entry.0]);
    for root in additional_roots {
        if starts.contains(&root.0) {
            continue;
        }
        if starts.len() == options.blocks || is_frontier(root.0) {
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
            if is_frontier(pc) {
                break;
            }
            let (instruction, source) = match decode_instruction(memory, pc, gates) {
                Ok(decoded) => decoded,
                Err(CompileError::Decode(_)) if !nodes.is_empty() => break,
                Err(error) => return Err(error),
            };
            let next = instruction.next;
            let operation = instruction.operation;
            sources.push(source);
            nodes.insert(pc, instruction);
            let mut enqueue = |target| {
                if !is_frontier(target) && starts.len() < options.blocks {
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
    let mut blocks = partition_blocks(nodes, &starts);
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
    encode_blocks(
        memory,
        entry,
        options,
        blocks,
        sources,
        &[entry],
        EmissionLimit::Shorten,
    )
    .map(|code| code.expect("prefix emission does not decline"))
}

fn decode_instruction(
    memory: &AddressSpace,
    pc: u32,
    gates: &[(GuestAddress, u32)],
) -> Result<(Instruction, CodeSnapshot), CompileError> {
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
        let source = memory
            .snapshot_code(GuestAddress(pc), stub.len())
            .map_err(CompileError::Memory)?;
        return Ok((
            Instruction {
                pc,
                next: pc.wrapping_add(2),
                operation: None,
                gate: Some(id),
            },
            source,
        ));
    }
    let decoded = match decode_for_compilation(
        memory,
        GuestAddress(pc),
        DecodePolicy::Execution,
        None,
        DecodeAuthority::default(),
    )
    .map_err(CompileError::Decode)?
    {
        BoundDecode::Instruction(decoded) => decoded,
        BoundDecode::CodeFault(_) => unreachable!("flat fetch has no segment authority"),
    };
    let source = memory
        .snapshot_code(decoded.pc(), usize::from(decoded.length()))
        .map_err(CompileError::Memory)?;
    let operation = if decoded.segment_accesses() & (1 << 5) == 0
        && !decoded.is_privileged_instruction()
        && super::emitter::supports(decoded.operation())
    {
        Some(*decoded.operation())
    } else {
        None
    };
    Ok((
        Instruction {
            pc,
            next: decoded.next_pc().0,
            operation,
            gate: None,
        },
        source,
    ))
}

fn partition_blocks(nodes: BTreeMap<u32, Instruction>, starts: &BTreeSet<u32>) -> Vec<Block> {
    // An advertised entry may enter the middle of an already decoded linear span.
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
    blocks
}

enum EmissionLimit {
    Shorten,
    #[cfg(test)]
    Decline,
}

fn encode_blocks(
    memory: &AddressSpace,
    entry: GuestAddress,
    options: CompileOptions,
    mut blocks: Vec<Block>,
    sources: Vec<CodeSnapshot>,
    _roots: &[GuestAddress],
    _limit: EmissionLimit,
) -> Result<Option<CompiledCode>, CompileError> {
    let identity = next_identity()?;
    let (wasm, native) = loop {
        let native = collect_native(&blocks);
        let wasm = super::emitter::emit(&blocks, entry.0, identity, &native);
        if wasm.len() <= options.wasm_bytes {
            break (wasm, native);
        }
        #[cfg(test)]
        if matches!(_limit, EmissionLimit::Decline) {
            return Ok(None);
        }
        let instructions: usize = blocks.iter().map(|block| block.instructions.len()).sum();
        if instructions == 1 {
            return Err(CompileError::WasmLimit);
        }
        retain_entry_prefix(&mut blocks, entry.0, instructions / 2);
    };
    Ok(Some(finish_code(
        memory, blocks, identity, wasm, native, sources, _roots, None,
    )))
}

fn next_identity() -> Result<u64, CompileError> {
    NEXT_IDENTITY
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
            value.checked_add(1)
        })
        .map_err(|_| CompileError::Memory(MemoryError::VersionExhausted))
}

fn collect_native(blocks: &[Block]) -> Vec<NativeInstruction> {
    blocks
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
        .collect()
}

fn executable_hot_spans(blocks: &[Block]) -> Vec<HotSpan> {
    let mut hot_spans: Vec<_> = spans::partition(blocks)
        .into_iter()
        .enumerate()
        .map(|(index, span)| HotSpan {
            entry: span.block.entry,
            end: span
                .block
                .instructions
                .last()
                .expect("nonempty executable span")
                .next,
            instructions: span.work().minimum_work(),
            port: index as u32,
        })
        .collect();
    hot_spans.sort_unstable_by_key(|span| span.entry);
    hot_spans
}

fn finish_code(
    memory: &AddressSpace,
    blocks: Vec<Block>,
    identity: u64,
    wasm: Vec<u8>,
    native: Vec<NativeInstruction>,
    mut sources: Vec<CodeSnapshot>,
    _roots: &[GuestAddress],
    trace: Option<(u32, TraceScore)>,
) -> CompiledCode {
    // instruction snapshots on the same pages share one mapping/content authority.
    sources.sort_unstable_by_key(|source| (source.first, source.versions.len()));
    sources.dedup_by(|source, retained| {
        source.first == retained.first && source.versions == retained.versions
    });
    let mut block_pcs = Vec::new();
    let geometry = blocks
        .iter()
        .map(|block| {
            let first = block_pcs.len() as u32;
            block_pcs.extend(block.instructions.iter().map(|instruction| instruction.pc));
            CodeBlock {
                pc: block.entry,
                end: block.instructions.last().expect("nonempty code block").next,
                first,
                count: block.instructions.len() as u32,
            }
        })
        .collect();
    let mut instruction_pcs = block_pcs.clone();
    instruction_pcs.sort_unstable();
    instruction_pcs.dedup();
    let hot_spans = executable_hot_spans(&blocks);
    CompiledCode {
        identity,
        memory_identity: memory.identity(),
        validated_source_version: AtomicU64::new(memory.code_source_version()),
        entries: trace.map_or_else(
            || {
                blocks
                    .iter()
                    .map(|block| GuestAddress(block.entry))
                    .collect()
            },
            |(root, _)| vec![GuestAddress(root)],
        ),
        instruction_pcs: instruction_pcs.into_boxed_slice(),
        block_pcs: block_pcs.into_boxed_slice(),
        blocks: geometry,
        hot_spans: hot_spans.into_boxed_slice(),
        wasm,
        sources,
        native,
        trace_root: trace.map(|(root, _)| root),
        trace_score: trace.map(|(_, score)| score),
        observed: None,
        observed_fingerprint: None,
        window_fingerprint: None,
        #[cfg(test)]
        roots: _roots.into(),
    }
}

#[cfg(test)]
#[path = "program/call_tests.rs"]
mod call_tests;

#[cfg(test)]
#[path = "program/transition_tests.rs"]
mod transition_tests;

#[cfg(test)]
#[path = "program/coverage_tests.rs"]
mod coverage_tests;

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
mod observed_tests {
    use super::*;
    use crate::{
        cpu::core::trace::{BLOCK, NUMERIC, TraceEvent},
        memory::{PageRange, Permissions},
    };

    fn memory() -> AddressSpace {
        let mut memory = AddressSpace::new(1).unwrap();
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0x1000), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
        memory
    }

    fn block(owner: &CompiledCode, pc: u32, offset: u32) -> TraceEvent {
        let block = owner.blocks.iter().find(|block| block.pc == pc).unwrap();
        TraceEvent {
            kind: BLOCK,
            pc,
            end: block.end,
            offset,
            owner: owner.identity(),
        }
    }

    fn numeric(owner: &CompiledCode, pc: u32) -> TraceEvent {
        let instruction = owner
            .native
            .iter()
            .find(|instruction| instruction.pc == pc)
            .unwrap();
        TraceEvent {
            kind: NUMERIC,
            pc,
            end: instruction.next,
            offset: 0,
            owner: owner.identity(),
        }
    }

    fn call_loop(memory: &mut AddressSpace) -> (CompiledCode, CompiledCode) {
        memory
            .write(
                GuestAddress(0x1000),
                &[0xe8, 0xfb, 0, 0, 0, 0x83, 0xc0, 1, 0xeb, 0xf6],
            )
            .unwrap();
        memory
            .write(GuestAddress(0x1100), &[0x83, 0xc0, 3, 0xc3])
            .unwrap();
        (
            compile(memory, GuestAddress(0x1000), CompileOptions::default()).unwrap(),
            compile(memory, GuestAddress(0x1100), CompileOptions::default()).unwrap(),
        )
    }

    #[test]
    fn confirmed_call_return_path_has_one_independent_root_only_owner() {
        let mut memory = memory();
        let (caller, callee) = call_loop(&mut memory);
        let before = caller.wasm().to_vec();
        let events = [
            block(&caller, 0x1000, 0),
            block(&callee, 0x1100, 0),
            block(&caller, 0x1005, 0),
            block(&caller, 0x1000, 0),
        ];
        let code = compile_observed(
            &memory,
            GuestAddress(0x1000),
            CompileOptions::default(),
            &events,
            &[&caller, &callee],
            memory.code_source_version(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(code.trace_root(), Some(0x1000));
        assert_eq!(code.entrypoints(), &[GuestAddress(0x1000)]);
        assert_eq!(
            code.instruction_pcs().collect::<Vec<_>>(),
            [0x1000, 0x1005, 0x1008, 0x1100, 0x1103]
        );
        assert!(code.supports_entry(0x1000));
        assert!(!code.supports_entry(0x1100));
        assert!(code.is_current(&memory));
        assert_ne!(code.identity(), caller.identity());
        assert_eq!(caller.wasm(), before);
        assert!(caller.is_current(&memory) && callee.is_current(&memory));
    }

    #[test]
    fn completed_trace_owner_spans_extend_into_the_actual_baseline_callee_and_continuation() {
        let mut memory = memory();
        let (caller, callee) = call_loop(&mut memory);
        assert_eq!(caller.trace_score(), None);
        assert_eq!(callee.trace_score(), None);
        let prefix = compile_observed(
            &memory,
            GuestAddress(0x1000),
            CompileOptions::default(),
            &[block(&caller, 0x1000, 0), block(&callee, 0x1100, 0)],
            &[&caller, &callee],
            memory.code_source_version(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(prefix.instruction_pcs().collect::<Vec<_>>(), [0x1000]);
        assert_eq!(
            prefix.trace_score(),
            Some(TraceScore {
                instructions: 1,
                internal_edges: 0
            })
        );
        let prefix_bytes = prefix.wasm().to_vec();
        let events = [
            block(&prefix, 0x1000, 0),
            block(&callee, 0x1100, 0),
            block(&caller, 0x1005, 0),
            block(&prefix, 0x1000, 0),
        ];
        let extended = compile_observed_with_growth(
            &memory,
            GuestAddress(0x1000),
            CompileOptions::default(),
            &events,
            &[&prefix, &caller, &callee],
            memory.code_source_version(),
            prefix.trace_score(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            extended.trace_score(),
            Some(TraceScore {
                instructions: 5,
                internal_edges: 5
            })
        );
        assert!(
            extended
                .trace_score()
                .unwrap()
                .strictly_grows(prefix.trace_score().unwrap())
        );
        assert_eq!(extended.entrypoints(), &[GuestAddress(0x1000)]);
        assert!(!extended.supports_entry(0x1100));
        assert_eq!(prefix.wasm(), prefix_bytes);
        assert!(prefix.is_current(&memory));
        assert!(caller.is_current(&memory) && callee.is_current(&memory));
    }

    #[test]
    fn a_recorded_trace_owner_has_the_same_geometry_and_source_obligations_as_a_baseline() {
        let mut memory = memory();
        let (caller, callee) = call_loop(&mut memory);
        let prefix = compile_observed(
            &memory,
            GuestAddress(0x1000),
            CompileOptions::default(),
            &[block(&caller, 0x1000, 0), block(&callee, 0x1100, 0)],
            &[&caller, &callee],
            memory.code_source_version(),
        )
        .unwrap()
        .unwrap();
        let valid = [block(&prefix, 0x1000, 0), block(&callee, 0x1100, 0)];
        for field in 0..3 {
            let mut events = valid;
            match field {
                0 => events[0].end += 1,
                1 => events[0].offset = 1,
                _ => events[0].owner = 0,
            }
            assert!(
                compile_observed(
                    &memory,
                    GuestAddress(0x1000),
                    CompileOptions::default(),
                    &events,
                    &[&prefix, &callee],
                    memory.code_source_version(),
                )
                .unwrap()
                .is_none()
            );
        }
        let old_source = memory.code_source_version();
        memory.write(GuestAddress(0x1000), &[0x90]).unwrap();
        assert!(!prefix.is_current(&memory));
        for expected_source in [old_source, memory.code_source_version()] {
            assert!(
                compile_observed(
                    &memory,
                    GuestAddress(0x1000),
                    CompileOptions::default(),
                    &valid,
                    &[&prefix, &callee],
                    expected_source,
                )
                .unwrap()
                .is_none()
            );
        }
    }

    #[test]
    fn replaying_equal_completed_coverage_does_not_qualify_as_trace_growth() {
        let mut memory = memory();
        let (caller, callee) = call_loop(&mut memory);
        let first = compile_observed(
            &memory,
            GuestAddress(0x1000),
            CompileOptions::default(),
            &[
                block(&caller, 0x1000, 0),
                block(&callee, 0x1100, 0),
                block(&caller, 0x1005, 0),
                block(&caller, 0x1000, 0),
            ],
            &[&caller, &callee],
            memory.code_source_version(),
        )
        .unwrap()
        .unwrap();
        let replay = compile_observed(
            &memory,
            GuestAddress(0x1000),
            CompileOptions::default(),
            &[
                block(&first, 0x1000, 0),
                block(&first, 0x1100, 0),
                block(&first, 0x1005, 0),
                block(&first, 0x1000, 0),
            ],
            &[&first],
            memory.code_source_version(),
        )
        .unwrap()
        .unwrap();
        assert_ne!(first.identity(), replay.identity());
        assert_eq!(replay.trace_score(), first.trace_score());
        assert!(
            !replay
                .trace_score()
                .unwrap()
                .strictly_grows(first.trace_score().unwrap())
        );
        assert!(
            !first
                .trace_score()
                .unwrap()
                .strictly_grows(replay.trace_score().unwrap())
        );
        assert!(
            compile_observed_with_growth(
                &memory,
                GuestAddress(0x1000),
                CompileOptions::default(),
                &[
                    block(&first, 0x1000, 0),
                    block(&first, 0x1100, 0),
                    block(&first, 0x1005, 0),
                    block(&first, 0x1000, 0),
                ],
                &[&first],
                memory.code_source_version(),
                first.trace_score(),
            )
            .unwrap()
            .is_none()
        );

        // A source owner's other blocks are not evidence that this capture
        // executed them. The callee entry is only a final witness here.
        let prefix = compile_observed(
            &memory,
            GuestAddress(0x1000),
            CompileOptions::default(),
            &[block(&first, 0x1000, 0), block(&first, 0x1100, 0)],
            &[&first],
            memory.code_source_version(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(prefix.instruction_pcs().collect::<Vec<_>>(), [0x1000]);
        assert_eq!(
            prefix.trace_score(),
            Some(TraceScore {
                instructions: 1,
                internal_edges: 0
            })
        );
        assert!(
            !prefix
                .trace_score()
                .unwrap()
                .strictly_grows(first.trace_score().unwrap())
        );
        assert!(
            compile_observed_with_growth(
                &memory,
                GuestAddress(0x1000),
                CompileOptions::default(),
                &[block(&first, 0x1000, 0), block(&first, 0x1100, 0)],
                &[&first],
                memory.code_source_version(),
                first.trace_score(),
            )
            .unwrap()
            .is_none()
        );
    }

    #[test]
    fn a_completed_loop_closing_edge_grows_a_trace_without_adding_instruction_pcs() {
        let mut memory = memory();
        memory.write(GuestAddress(0x1000), &[0xeb, 3]).unwrap();
        // JNZ 1000; NOP; RET. The initial recording observes only fallthrough.
        memory
            .write(GuestAddress(0x1005), &[0x75, 0xf9, 0x90, 0xc3])
            .unwrap();
        let owner = compile(&memory, GuestAddress(0x1000), CompileOptions::default()).unwrap();
        let first = compile_observed(
            &memory,
            GuestAddress(0x1000),
            CompileOptions::default(),
            &[
                block(&owner, 0x1000, 0),
                block(&owner, 0x1005, 0),
                block(&owner, 0x1007, 0),
            ],
            &[&owner],
            memory.code_source_version(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            first.trace_score(),
            Some(TraceScore {
                instructions: 2,
                internal_edges: 1
            })
        );
        let closed = compile_observed_with_growth(
            &memory,
            GuestAddress(0x1000),
            CompileOptions::default(),
            &[
                block(&first, 0x1000, 0),
                block(&first, 0x1005, 0),
                block(&first, 0x1000, 0),
                block(&first, 0x1005, 0),
                block(&owner, 0x1007, 0),
            ],
            &[&first, &owner],
            memory.code_source_version(),
            first.trace_score(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            closed.trace_score(),
            Some(TraceScore {
                instructions: 2,
                internal_edges: 2
            })
        );
        assert_eq!(
            closed.instruction_pcs().collect::<Vec<_>>(),
            [0x1000, 0x1005]
        );
        assert_eq!(closed.instruction_count(), first.instruction_count());
        assert!(
            closed
                .trace_score()
                .unwrap()
                .strictly_grows(first.trace_score().unwrap())
        );
        assert!(!closed.instruction_pcs().any(|pc| pc == 0x1007));
    }

    #[test]
    fn trace_growth_requires_componentwise_dominance_and_one_strict_component() {
        let previous = TraceScore {
            instructions: 3,
            internal_edges: 2,
        };
        for (instructions, internal_edges, grows) in [
            (3, 2, false),
            (4, 2, true),
            (3, 3, true),
            (4, 3, true),
            (2, 3, false),
            (4, 1, false),
            (2, 1, false),
        ] {
            let candidate = TraceScore {
                instructions,
                internal_edges,
            };
            assert_eq!(candidate.strictly_grows(previous), grows, "{candidate:?}");
        }
    }

    #[test]
    fn canonical_witness_confirms_only_the_prior_prefix_and_actual_native_operation() {
        let mut memory = memory();
        memory
            .write(GuestAddress(0x1000), &[0x90, 0xd8, 0x03, 0x90, 0xc3])
            .unwrap();
        let owner = compile(&memory, GuestAddress(0x1000), CompileOptions::default()).unwrap();
        let events = [
            block(&owner, 0x1000, 0),
            numeric(&owner, 0x1001),
            block(&owner, 0x1000, 2),
        ];
        let code = compile_observed(
            &memory,
            GuestAddress(0x1000),
            CompileOptions::default(),
            &events,
            &[&owner],
            memory.code_source_version(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(code.instruction_pcs().collect::<Vec<_>>(), [0x1000, 0x1001]);
        assert_eq!(code.native_instructions().len(), 1);
        assert_eq!(code.native_instructions()[0].pc, 0x1001);
        assert!(
            !code
                .instruction_pcs()
                .any(|pc| pc == 0x1003 || pc == 0x1004)
        );

        let interior = [
            block(&owner, 0x1000, 1),
            numeric(&owner, 0x1001),
            block(&owner, 0x1000, 2),
        ];
        assert!(
            compile_observed(
                &memory,
                GuestAddress(0x1000),
                CompileOptions::default(),
                &interior,
                &[&owner],
                memory.code_source_version()
            )
            .unwrap()
            .is_none()
        );
        let code = compile_observed(
            &memory,
            GuestAddress(0x1001),
            CompileOptions::default(),
            &interior,
            &[&owner],
            memory.code_source_version(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(code.instruction_pcs().collect::<Vec<_>>(), [0x1001]);
    }

    #[test]
    fn interior_offsets_use_the_original_block_path_not_sorted_coverage() {
        let mut memory = memory();
        memory
            .write(
                GuestAddress(0x1000),
                &[0xb8, 0, 0xb8, 0x11, 0x22, 0x90, 0xc3],
            )
            .unwrap();
        let mut sources = Vec::new();
        let mut decode = |pc| {
            let (instruction, source) = decode_instruction(&memory, pc, &[]).unwrap();
            sources.push(source);
            instruction
        };
        let blocks = vec![
            Block {
                entry: 0x1000,
                instructions: vec![decode(0x1000), decode(0x1005)],
            },
            Block {
                entry: 0x1002,
                instructions: vec![decode(0x1002)],
            },
            Block {
                entry: 0x1006,
                instructions: vec![decode(0x1006)],
            },
        ];
        let owner = encode_blocks(
            &memory,
            GuestAddress(0x1000),
            CompileOptions::default(),
            blocks,
            sources,
            &[GuestAddress(0x1000)],
            EmissionLimit::Decline,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            owner.instruction_pcs().collect::<Vec<_>>(),
            [0x1000, 0x1002, 0x1005, 0x1006]
        );
        let events = [block(&owner, 0x1000, 1), block(&owner, 0x1006, 0)];
        let code = compile_observed(
            &memory,
            GuestAddress(0x1005),
            CompileOptions::default(),
            &events,
            &[&owner],
            memory.code_source_version(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(code.instruction_pcs().collect::<Vec<_>>(), [0x1005]);
    }

    #[test]
    fn forged_geometry_control_and_owner_identity_decline_neutrally() {
        let mut memory = memory();
        let (caller, callee) = call_loop(&mut memory);
        let valid = [block(&caller, 0x1000, 0), block(&callee, 0x1100, 0)];
        for (field, value) in [(0, 0), (1, 0x1006), (2, u32::MAX), (3, 7)] {
            let mut events = valid;
            match field {
                0 => events[0].owner = u64::from(value),
                1 => events[0].end = value,
                2 => events[0].offset = value,
                _ => events[0].kind = value,
            }
            assert!(
                compile_observed(
                    &memory,
                    GuestAddress(0x1000),
                    CompileOptions::default(),
                    &events,
                    &[&caller, &callee],
                    memory.code_source_version()
                )
                .unwrap()
                .is_none()
            );
        }
        let wrong_target = [block(&caller, 0x1000, 0), block(&caller, 0x1005, 0)];
        assert!(
            compile_observed(
                &memory,
                GuestAddress(0x1000),
                CompileOptions::default(),
                &wrong_target,
                &[&caller],
                memory.code_source_version()
            )
            .unwrap()
            .is_none()
        );
        assert!(
            compile_observed(
                &memory,
                GuestAddress(0x1000),
                CompileOptions::default(),
                &valid[..1],
                &[&caller, &callee],
                memory.code_source_version()
            )
            .unwrap()
            .is_none()
        );
    }

    #[test]
    fn observed_word_control_cannot_claim_a_wide_actual_target() {
        let mut instruction = Instruction {
            pc: 0x1000,
            next: 0x1002,
            operation: Some(Operation::ReturnWord { stack_adjust: 0 }),
            gate: None,
        };
        assert!(observed_successor(instruction, 0xffff));
        assert!(!observed_successor(instruction, 0x1_0000));
        instruction.operation = Some(Operation::CallWord {
            target: GuestAddress(0x1_2345),
        });
        assert!(observed_successor(instruction, 0x2345));
        assert!(!observed_successor(instruction, 0x1_2345));
    }

    #[test]
    fn trace_bounds_and_source_changes_decline_without_shortening() {
        let mut memory = memory();
        let (caller, callee) = call_loop(&mut memory);
        let events = [
            block(&caller, 0x1000, 0),
            block(&callee, 0x1100, 0),
            block(&caller, 0x1005, 0),
            block(&caller, 0x1000, 0),
        ];
        for options in [
            CompileOptions {
                instructions: 4,
                ..CompileOptions::default()
            },
            CompileOptions {
                blocks: 2,
                ..CompileOptions::default()
            },
            CompileOptions {
                wasm_bytes: 128,
                ..CompileOptions::default()
            },
            CompileOptions {
                instructions: 0,
                ..CompileOptions::default()
            },
        ] {
            assert!(
                compile_observed(
                    &memory,
                    GuestAddress(0x1000),
                    options,
                    &events,
                    &[&caller, &callee],
                    memory.code_source_version()
                )
                .unwrap()
                .is_none()
            );
        }
        let source_version = memory.code_source_version();
        memory.write(GuestAddress(0x1100), &[0x90]).unwrap();
        assert!(
            compile_observed(
                &memory,
                GuestAddress(0x1000),
                CompileOptions::default(),
                &events,
                &[&caller, &callee],
                source_version
            )
            .unwrap()
            .is_none()
        );
        assert!(
            compile_observed(
                &memory,
                GuestAddress(0x1000),
                CompileOptions::default(),
                &events,
                &[&caller, &callee],
                memory.code_source_version()
            )
            .unwrap()
            .is_none()
        );
    }

    #[test]
    fn an_unconfirmed_host_instruction_is_not_compiled() {
        let mut memory = memory();
        memory
            .write(GuestAddress(0x1000), &[0x90, 0xeb, 0x0d])
            .unwrap();
        memory.write(GuestAddress(0x1010), &[0x0f, 0x0b]).unwrap();
        let owner = compile(&memory, GuestAddress(0x1000), CompileOptions::default()).unwrap();
        let events = [block(&owner, 0x1000, 0), block(&owner, 0x1010, 0)];
        let code = compile_observed(
            &memory,
            GuestAddress(0x1000),
            CompileOptions::default(),
            &events,
            &[&owner],
            memory.code_source_version(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(code.instruction_pcs().collect::<Vec<_>>(), [0x1000, 0x1001]);
        assert!(!code.instruction_pcs().any(|pc| pc == 0x1010));
    }
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

    #[test]
    fn repeated_instruction_sources_share_one_page_snapshot() {
        let mut memory = memory(&[0x1000, 0x2000]);
        let mut instructions = vec![0x40; 63];
        instructions.extend([0x0f, 0x0b]);
        memory.write(GuestAddress(0x1000), &instructions).unwrap();
        let code = compile(&memory, GuestAddress(0x1000), limits(64, 1)).unwrap();
        assert_eq!(code.instruction_count(), 64);
        assert_eq!(code.sources.len(), 1);
        memory.write(GuestAddress(0x2000), &[0x40]).unwrap();
        assert!(code.is_current(&memory));
        memory.write(GuestAddress(0x103e), &[0x48]).unwrap();
        assert!(!code.is_current(&memory));
    }

    #[test]
    fn compacted_cross_page_sources_still_validate_both_pages() {
        for changed in [0x1ffc, 0x2001] {
            let mut memory = memory(&[0x1000, 0x2000, 0x3000]);
            let mut instructions = vec![0xb8, 0x12, 0x34, 0x56, 0x78];
            instructions.extend([0x40; 31]);
            instructions.extend([0x0f, 0x0b]);
            memory.write(GuestAddress(0x1ffc), &instructions).unwrap();
            let code = compile(&memory, GuestAddress(0x1ffc), limits(40, 1)).unwrap();
            assert_eq!(code.instruction_count(), 33);
            assert_eq!(code.sources.len(), 2);
            memory.write(GuestAddress(0x3000), &[0x40]).unwrap();
            assert!(code.is_current(&memory));
            memory.write(GuestAddress(changed), &[0x48]).unwrap();
            assert!(!code.is_current(&memory));
        }
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
