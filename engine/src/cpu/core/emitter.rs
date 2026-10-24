mod access;
mod code;
mod control;
mod executable;
mod exits;
mod integer;
pub(super) mod link;
mod numeric;
mod recovery;
mod region;
mod scalar;
mod strings;
pub(in crate::cpu::core) mod support;
#[cfg(test)]
mod tests;
mod trace;

use super::{
    StopReason, flags,
    program::{Block, NativeInstruction},
};
use crate::cpu::x86::{
    ControlOperation,
    ir::{BranchTarget, Condition, Location32, Operation},
};
use wasm_encoder::{
    BlockType, CodeSection, CustomSection, EntityType, ExportKind, ExportSection, Function,
    FunctionSection, GlobalType, ImportSection, MemArg, MemoryType, Module, NameMap, NameSection,
    TypeSection, ValType,
};

pub(super) const CTX: u32 = 0;
pub(super) const EIP: u32 = 10;
pub(super) const FLAGS: u32 = 11;
pub(super) const REMAIN: u32 = 12;
pub(super) const RETIRED: u32 = 13;
pub(super) const A: u32 = 14;
pub(super) const B: u32 = 15;
pub(super) const RESULT: u32 = 16;
pub(super) const ADDRESS: u32 = 17;
pub(super) const CARRY: u32 = 18;
pub(super) const WRITE_STATUS: u32 = 19;
pub(super) const KIND: u32 = 20;
pub(super) const LEFT: u32 = 21;
pub(super) const RIGHT: u32 = 22;
pub(super) const VALUE: u32 = 23;
pub(super) const CARRY_IN: u32 = 24;
pub(super) const WIDTH: u32 = 25;
pub(super) const INDEX: u32 = 26;
pub(super) const WIDE: u32 = 27;
pub(super) const LOGICAL_ADDRESS: u32 = 28;
pub(super) const GP_FACTS_VALID: u32 = 129;
pub(super) const ENTRY_OFFSET: u32 = 130;
pub(super) const PREFIX_BASE: u32 = 131;
pub(super) const FULL_BLOCK: u32 = 151;
pub(super) const SERVICE_RESUME: u32 = 157;
pub(super) const SERVICE_ORDINAL: u32 = 172;
pub(super) const SERVICE_PC: u32 = 173;
pub(super) const SERVICE_NEXT: u32 = 174;
pub(super) const SERVICE_TRANSFER: u32 = 175;
pub(super) const FRESH_OWNER_ENTRY: u32 = 176;
pub(super) const FP_PAYLOAD_TOKENS: u32 = 177;
pub(super) const FP_TAG_TOKENS: u32 = 178;
pub(super) const SCALAR_CACHE_BASE: u32 = 179;

const BLOCK_BITS: u32 = 14;

pub(super) fn mem(offset: u64, align: u32) -> MemArg {
    MemArg {
        offset,
        align,
        memory_index: 0,
    }
}

pub(super) fn supports(operation: &Operation) -> bool {
    integer::supports(operation)
        || control::supports(operation)
        || strings::supports(operation)
        || is_native(operation)
        || matches!(
            operation,
            Operation::Nop
                | Operation::Jump { .. }
                | Operation::ConditionalJump { .. }
                | Operation::CountBranch { .. }
                | Operation::Call { .. }
                | Operation::Push { .. }
                | Operation::Pop { .. }
                | Operation::PushFlags
                | Operation::Return { .. }
                | Operation::Leave
                | Operation::Control(
                    ControlOperation::InvalidOpcode
                        | ControlOperation::Breakpoint
                        | ControlOperation::Sysenter
                )
        )
}

pub(in crate::cpu::core) fn scalar_effect(
    operation: Operation,
) -> super::region::values::scalar::ScalarEffect {
    integer::scalar_effect(operation)
}

pub(super) fn is_native(operation: &Operation) -> bool {
    numeric::supports(operation)
}

pub(super) fn preserves_fp_success(operation: &Operation) -> bool {
    numeric::preserves_fp_success(operation)
}

pub(in crate::cpu::core) fn variable_work(operation: Operation) -> bool {
    matches!(operation, Operation::RepeatString { .. })
}

pub(super) fn emit(
    blocks: &[Block],
    entry: u32,
    identity: u64,
    native: &[NativeInstruction],
) -> Vec<u8> {
    let spans = super::program::spans::partition(blocks);
    let blocks: Vec<_> = spans.iter().map(|span| span.block.clone()).collect();
    let graph = super::region::Region::from_blocks(&blocks, entry)
        .expect("decoded executable spans form a region");
    emit_graph(&spans, entry, identity, native, graph, false)
}

pub(super) fn emit_observed(
    blocks: &[Block],
    entry: u32,
    identity: u64,
    native: &[NativeInstruction],
    observed_edges: &[(u32, u32)],
) -> Vec<u8> {
    let spans = super::program::spans::partition(blocks);
    let blocks: Vec<_> = spans.iter().map(|span| span.block.clone()).collect();
    let graph = super::region::Region::from_observed(&blocks, entry, observed_edges)
        .expect("recorded blocks and edges form an observed region");
    emit_graph(&spans, entry, identity, native, graph, false)
}

pub(super) fn emit_observed_family(
    blocks: &[Block],
    entry: u32,
    identity: u64,
    native: &[NativeInstruction],
    observed_edges: &[(u32, u32)],
) -> Vec<u8> {
    let spans = super::program::spans::partition(blocks);
    let blocks: Vec<_> = spans.iter().map(|span| span.block.clone()).collect();
    let graph = super::region::Region::from_observed(&blocks, entry, observed_edges)
        .expect("recorded family forms an observed region");
    emit_graph(&spans, entry, identity, native, graph, true)
}

fn emit_graph(
    spans: &[super::program::spans::ExecutableSpan],
    entry: u32,
    identity: u64,
    native: &[NativeInstruction],
    graph: super::region::Region,
    family: bool,
) -> Vec<u8> {
    let plan = super::region::values::RegionPlan::analyze(&graph);
    let numeric = !native.is_empty();
    let (body, recovery_data, arithmetic_kernels) =
        run(&graph, &plan, identity, native, family, spans);
    let has_arithmetic = !arithmetic_kernels.is_empty();
    let status_resolver = support::owner_defined(10 + arithmetic_kernels.len() as u32, numeric);
    let recovery_blob = recovery::metadata(&recovery_data, identity);
    let recovery_base = link::GLOBAL_COUNT + 1;
    let recovery_getter = recovery::getter(&recovery_data, recovery_base);
    let mut module = Module::new();
    let mut types = TypeSection::new();
    types.ty().function([ValType::I32; 2], [ValType::I32]);
    types.ty().function([ValType::I32; 3], [ValType::I64]);
    types.ty().function([ValType::I32; 4], [ValType::I32]);
    types.ty().function([ValType::I32; 8], [ValType::I32]);
    types.ty().function([ValType::I32], [ValType::I32]);
    types.ty().function(
        [ValType::I32]
            .into_iter()
            .chain(link::boundary_fields(!native.is_empty()).map(|(_, kind)| kind))
            .collect::<Vec<_>>(),
        [],
    );
    types.ty().function([ValType::I32], []);
    types.ty().function(
        [ValType::I32; 6]
            .into_iter()
            .chain([ValType::I64])
            .collect::<Vec<_>>(),
        [ValType::I32, ValType::I64],
    );
    types.ty().function([ValType::I32], []);
    types.ty().function(
        [
            ValType::I32,
            ValType::I32,
            ValType::F64,
            ValType::F64,
            ValType::F64,
        ],
        [ValType::I32],
    );
    types.ty().function([ValType::I32; 3], [ValType::I32; 3]);
    types.ty().function(
        numeric::normalizer_parameters(),
        numeric::normalizer_results(),
    );
    types.ty().function([ValType::I32; 9], [ValType::I32; 3]);
    types.ty().function(
        numeric::normalizer_kernel_parameters(),
        numeric::normalizer_kernel_results(),
    );
    types.ty().function(
        [
            ValType::I32,
            ValType::I32,
            ValType::F64,
            ValType::I32,
            ValType::I32,
        ],
        [ValType::I32],
    );
    types.ty().function(
        [ValType::I32, ValType::I32, ValType::I64, ValType::I32],
        [ValType::I32],
    );
    types.ty().function(
        numeric::descriptor_parameters(),
        numeric::descriptor_results(),
    );
    types.ty().function(
        [
            ValType::I32,
            ValType::I64,
            ValType::I32,
            ValType::I32,
            ValType::I32,
            ValType::I32,
        ],
        [],
    );
    types.ty().function([ValType::I32; 3], [ValType::I32]);
    types.ty().function([ValType::I32], [ValType::I64]);
    types.ty().function(
        [ValType::F64, ValType::F64, ValType::I32, ValType::I32],
        [ValType::F64, ValType::I32, ValType::I32, ValType::I32],
    );
    module.section(&types);
    let mut imports = ImportSection::new();
    imports.import(
        "ring3",
        "memory",
        EntityType::Memory(MemoryType {
            minimum: 1,
            maximum: None,
            memory64: false,
            shared: false,
            page_size_log2: None,
        }),
    );
    imports.import("ring3", "read", EntityType::Function(1));
    imports.import("ring3", "write", EntityType::Function(2));
    if !native.is_empty() {
        imports.import("ring3", "numeric", EntityType::Function(18));
    }
    imports.import("ring3", "precise", EntityType::Function(4));
    link::imports(&mut imports);
    support::imports(&mut imports, [3, 6, 9, 10, 12, 7, 7, 7, 13, 14, 15, 17]);
    if !recovery_blob.is_empty() {
        imports.import(
            "ring3",
            "recovery.base",
            EntityType::Global(GlobalType {
                val_type: ValType::I32,
                mutable: false,
                shared: false,
            }),
        );
    }
    module.section(&imports);
    let mut functions = FunctionSection::new();
    for kind in [0, 4, 5, 11, 16, 0, 0, 8, 0] {
        functions.function(kind);
    }
    functions.function(19);
    for _ in &arithmetic_kernels {
        functions.function(20);
    }
    if has_arithmetic {
        functions.function(9);
    }
    module.section(&functions);
    let mut exports = ExportSection::new();
    let wrappers = support::owner_defined(5, numeric);
    exports.export("run", ExportKind::Func, wrappers);
    exports.export("link", ExportKind::Func, wrappers + 1);
    exports.export("publish", ExportKind::Func, wrappers + 2);
    module.section(&exports);
    let mut code = CodeSection::new();
    code.raw(&body);
    code.function(&lookup(spans));
    code.function(&link::emit_boundary(
        numeric,
        support::owner_internal(2, numeric),
        support::owner_internal(7, numeric),
        support::owner_internal(8, numeric),
    ));
    code.function(&numeric::emit_normalizer_adapter(
        &plan,
        numeric,
        support::owner_kernel(numeric),
        support::owner_internal(14, numeric),
    ));
    code.function(&numeric::emit_descriptor(&plan, numeric));
    code.function(&link::emit_entry(
        support::owner_internal(3, numeric),
        false,
        numeric,
    ));
    code.function(&link::emit_entry(
        support::owner_internal(3, numeric),
        true,
        numeric,
    ));
    code.function(&link::emit_publisher(
        support::owner_internal(2, numeric),
        support::owner_internal(7, numeric),
        support::owner_internal(8, numeric),
    ));
    code.function(&hot_entry(
        spans,
        (graph.is_observed && !family).then_some(entry),
    ));
    code.function(&recovery_getter);
    for kernel in &arithmetic_kernels {
        code.function(&numeric::executable::primitive::emit(
            kernel,
            status_resolver,
        ));
    }
    if has_arithmetic {
        code.function(&numeric::executable::primitive::emit_status());
    }
    module.section(&code);
    let mut names = NameSection::new();
    names.module(&format!("ring3.guest.{entry:08x}.{identity}"));
    let mut functions = NameMap::new();
    functions.append(0, "guest.read_slow");
    functions.append(1, "guest.write_slow");
    if numeric {
        functions.append(2, "guest.numeric");
    }
    functions.append(2 + u32::from(numeric), "guest.precise");
    support::names(&mut functions, 3 + u32::from(numeric));
    for (index, name) in [
        "guest.run",
        "guest.lookup",
        "guest.publish",
        "guest.commit_frame_cold",
        "guest.select_frame",
    ]
    .into_iter()
    .enumerate()
    {
        functions.append(support::owner_defined(index as u32, numeric), name);
    }
    for (offset, name) in ["guest.entry", "guest.link", "guest.publish_link"]
        .into_iter()
        .enumerate()
    {
        functions.append(wrappers + offset as u32, name);
    }
    functions.append(support::owner_defined(8, numeric), "guest.hot_entry");
    functions.append(support::owner_defined(9, numeric), "guest.recovery_data");
    for (index, kernel) in arithmetic_kernels.iter().enumerate() {
        functions.append(
            support::owner_defined(10 + index as u32, numeric),
            kernel.name(),
        );
    }
    if has_arithmetic {
        functions.append(status_resolver, "guest.fp.status");
    }
    names.functions(&functions);
    module.section(&names);
    if !recovery_blob.is_empty() {
        module.section(&CustomSection {
            name: "ring3.recovery".into(),
            data: recovery_blob.into(),
        });
    }
    module.finish()
}

fn lookup(spans: &[super::program::spans::ExecutableSpan]) -> Function {
    fn tree(code: &mut wasm_encoder::InstructionSink<'_>, ports: &[(u32, u32)]) {
        if ports.is_empty() {
            code.i32_const(-1);
            return;
        }
        let middle = ports.len() / 2;
        let (pc, port) = ports[middle];
        code.local_get(0)
            .i32_const(pc as i32)
            .i32_eq()
            .if_(BlockType::Result(ValType::I32));
        code.i32_const(port as i32);
        code.else_()
            .local_get(0)
            .i32_const(pc as i32)
            .i32_lt_u()
            .if_(BlockType::Result(ValType::I32));
        tree(code, &ports[..middle]);
        code.else_();
        tree(code, &ports[middle + 1..]);
        code.end().end();
    }
    let mut ports: Vec<_> = spans
        .iter()
        .enumerate()
        .flat_map(|(block, span)| {
            span.block
                .instructions
                .iter()
                .enumerate()
                .map(move |(prefix, instruction)| {
                    (instruction.pc, (prefix as u32) << BLOCK_BITS | block as u32)
                })
        })
        .collect();
    // Separate decode paths may interleave their instruction starts even though
    // both the blocks and each individual block are ordered.
    ports.sort_unstable_by_key(|&(pc, _)| pc);
    let mut function = Function::new([]);
    tree(&mut function.instructions(), &ports);
    function.instructions().end();
    function
}

fn hot_entry(spans: &[super::program::spans::ExecutableSpan], root: Option<u32>) -> Function {
    fn count(c: &mut wasm_encoder::InstructionSink<'_>, entries: &[(u32, u32)]) {
        if entries.is_empty() {
            c.i32_const(0);
            return;
        }
        let mid = entries.len() / 2;
        let (pc, work) = entries[mid];
        c.local_get(0)
            .i32_const(pc as i32)
            .i32_eq()
            .if_(BlockType::Result(ValType::I32))
            .i32_const(work as i32)
            .else_()
            .local_get(0)
            .i32_const(pc as i32)
            .i32_lt_u()
            .if_(BlockType::Result(ValType::I32));
        count(c, &entries[..mid]);
        c.else_();
        count(c, &entries[mid + 1..]);
        c.end().end();
    }
    let mut entries: Vec<_> = spans
        .iter()
        .filter(|span| root.is_none_or(|pc| span.block.entry == pc))
        .map(|span| (span.block.entry, span.work().minimum_work()))
        .collect();
    entries.sort_unstable();
    let mut function = Function::new([(1, ValType::I32)]);
    let mut c = function.instructions();
    count(&mut c, &entries);
    c.local_tee(2)
        .i32_const(0)
        .i32_ne()
        .local_get(1)
        .local_get(2)
        .i32_ge_u()
        .i32_and()
        .end();
    function
}

fn run(
    graph: &super::region::Region,
    plan: &super::region::values::RegionPlan,
    identity: u64,
    native: &[NativeInstruction],
    family: bool,
    spans: &[super::program::spans::ExecutableSpan],
) -> (
    Vec<u8>,
    Vec<i64>,
    Vec<numeric::executable::primitive::Kernel>,
) {
    let execution = super::region::values::ExecutionPlan::analyze(graph, plan);
    let expression_base = SCALAR_CACHE_BASE + execution.scalar.cache_count();
    let local_layout = executable::locals::Layout::build(
        &execution.executable,
        graph.blocks.len(),
        !native.is_empty(),
        expression_base,
    );
    let mut locals = vec![
        (25, ValType::I32),
        (1, ValType::I64),
        (18, ValType::I32),
        (2, ValType::I64),
        (4, ValType::F64),
        (8, ValType::I32),
        (6, ValType::V128),
        (10, ValType::F64),
        (8, ValType::F64),
        (13, ValType::I32),
        (1, ValType::F64),
        (1, ValType::I32),
        (1, ValType::I64),
        (1, ValType::I32),
        (3, ValType::F64),
        (4, ValType::I32),
        (1, ValType::I32),
        (1, ValType::I32),
        (8, ValType::I64),
        (8, ValType::I32),
        (1, ValType::I32),
        (3, ValType::I32),
        (2, ValType::I32),
        (23, ValType::I32),
        (2, ValType::I64),
        (15, ValType::I32),
        (4, ValType::I32),
        (1, ValType::I32),
        (1, ValType::I64),
        (1, ValType::I32),
        (execution.scalar.cache_count() as u32, ValType::I32),
    ];
    locals.extend(local_layout.declarations.iter().copied());
    let fp_entry_marker = local_layout.next;
    locals.push((1, ValType::I32));
    let recovery_locals = recovery::Locals {
        base: fp_entry_marker + 1,
        colors: local_layout.next - expression_base,
        expression_base,
    };
    locals.push((16 + recovery_locals.colors, ValType::I32));
    locals.push((3, ValType::I64));
    locals.push((1, ValType::F64));
    let points = graph
        .blocks
        .iter()
        .map(|block| block.instructions.len() + 1)
        .sum::<usize>();
    let recovery_limit = execution
        .executable
        .value_count()
        .saturating_mul(points + 8)
        .saturating_add(recovery_locals.colors as usize);
    let mut function = Function::new(locals);
    let mut out = Output {
        function: &mut function,
        depth: 0,
        return_target: None,
        pc: 0,
        prefix: 0,
        record_flags: true,
        tail_flags: false,
        plan,
        scalar_plan: &execution.scalar,
        fp_schedule: &execution.fp,
        executable: &execution.executable,
        local_layout: &local_layout,
        fp_entry_marker,
        recovery: recovery::State::new(recovery_limit, recovery_locals),
        arithmetic_kernels: Vec::new(),
        available: std::collections::BTreeSet::new(),
        physical_gpr: None,
        physical_flags: None,
        active_recovery: None,
        active_fp_recovery: None,
        active_effect: None,
        captured_effect: None,
        frame: None,
        before_frame: None,
        after_frame: None,
        address_fact: None,
        stack_access: None,
        guarded_gpr_facts: false,
        current_block: None,
        grouped_accesses: true,
        is_trace: graph.is_observed,
        family,
        guarded_control: false,
        canonical_service: false,
        exact_service: false,
        native,
        spans,
    };
    out.c()
        .block(BlockType::Empty)
        .local_get(CTX)
        .i32_load(mem(112, 2))
        .i32_const(1)
        .i32_ne()
        .local_get(CTX)
        .i64_load(mem(120, 3))
        .i64_const(identity as i64)
        .i64_ne()
        .i32_or()
        .if_(BlockType::Empty)
        .i32_const(StopReason::Infrastructure as i32)
        .return_()
        .end();
    out.return_target = Some(1);
    link::import_canonical(&mut out);
    link::has_state(out.c().raw());
    out.c().if_(BlockType::Empty);
    link::restore(&mut out);
    out.c().else_();
    super::memory::authority::load(out.c().raw());
    for register in 0..8 {
        out.c()
            .local_get(CTX)
            .i32_load(mem(register * 4, 2))
            .local_set(register as u32 + 2);
    }
    for (offset, local) in [(32, EIP), (36, FLAGS), (40, RETIRED)] {
        out.c()
            .local_get(CTX)
            .i32_load(mem(offset, 2))
            .local_set(local);
    }
    out.c().local_get(1).local_set(REMAIN);
    out.c()
        .i32_const(flags::FlagKind::None as i32)
        .local_set(KIND);
    out.c().end();
    if family {
        out.c().i32_const(1).local_set(FRESH_OWNER_ENTRY);
    }
    if graph.is_observed && !family {
        out.c()
            .local_get(EIP)
            .i32_const(graph.blocks[graph.entry.0].entry as i32)
            .i32_ne()
            .if_(BlockType::Empty);
        out.exit(StopReason::NeedCode, None, 0);
        out.c().end();
    }
    out.c()
        .local_get(FLAGS)
        .i32_const(0x50100)
        .i32_and()
        .if_(BlockType::Empty);
    out.exit(StopReason::Debug, None, 0);
    out.c().end();
    out.c()
        .local_get(CTX)
        .i32_load(mem(super::memory::CODE_VERSION, 2))
        .local_tee(A)
        .if_(BlockType::Empty);
    out.c()
        .local_get(A)
        .i64_load(mem(0, 3))
        .local_get(CTX)
        .i64_load(mem(super::memory::EXPECTED_CODE_VERSION, 3))
        .i64_ne()
        .if_(BlockType::Empty);
    out.exit(StopReason::CodeInvalidated, None, 0);
    out.c().end().end();
    super::memory::load_view(out.c().raw());
    out.c()
        .global_get(link::PORT)
        .if_(BlockType::Empty)
        .global_get(link::PORT)
        .i32_const(1)
        .i32_sub()
        .local_tee(INDEX)
        .i32_const(BLOCK_BITS as i32)
        .i32_shr_u()
        .local_set(ENTRY_OFFSET)
        .local_get(INDEX)
        .i32_const((1 << BLOCK_BITS) - 1)
        .i32_and()
        .local_set(INDEX)
        .i32_const(0)
        .global_set(link::PORT)
        .else_();
    out.lookup_port(EIP);
    out.c().end();
    // Bootstrap runs once. Every exact prefix exits the inner block to the
    // single selective commit; computed targets and canonical helper requests
    // resume the retained body here without repeating register bootstrap.
    out.c().loop_(BlockType::Empty).block(BlockType::Empty);
    out.return_target = Some(out.depth);
    region::emit(&mut out, graph);
    // Guarded control owns only the instruction's actual target. Canonical
    // service continuation always resolves its next private instruction port.
    out.guarded_control = false;
    out.c().end();
    out.return_target = Some(1);
    recovery::emit(&mut out);
    numeric::emit_inline_commit(&mut out);
    numeric::prepare_service(&mut out);
    out.c().local_get(SERVICE_RESUME).if_(BlockType::Empty);
    // Ordinary exits retain the existing single mode test. The helper spine is
    // reached only for a service request; computed restarts simply pass it.
    numeric::emit_service(&mut out);
    out.c()
        .i32_const(0)
        .local_set(SERVICE_RESUME)
        .br(1)
        .end()
        .local_get(exits::REASON)
        .return_()
        .end()
        .end();
    assert_eq!(out.depth, 0, "the region exit wrapper is closed");
    let recovery_data = out.recovery.finish_data();
    let arithmetic_kernels = std::mem::take(&mut out.arithmetic_kernels);
    (
        exits::finish(
            function,
            !native.is_empty(),
            graph.is_observed.then_some(identity),
        ),
        recovery_data,
        arithmetic_kernels,
    )
}

pub(super) struct Output<'a> {
    function: &'a mut Function,
    depth: u32,
    return_target: Option<u32>,
    pc: u32,
    prefix: u32,
    record_flags: bool,
    tail_flags: bool,
    plan: &'a super::region::values::RegionPlan,
    scalar_plan: &'a super::region::values::scalar::ScalarPlan,
    fp_schedule: &'a super::region::values::Guards,
    executable: &'a super::region::values::executable::ExecutablePlan,
    local_layout: &'a executable::locals::Layout,
    fp_entry_marker: u32,
    recovery: recovery::State,
    arithmetic_kernels: Vec<numeric::executable::primitive::Kernel>,
    available: std::collections::BTreeSet<super::region::values::executable::ExecValueId>,
    physical_gpr: Option<[super::region::values::executable::ExecValueId; 8]>,
    physical_flags: Option<super::region::values::executable::FlagsState>,
    active_recovery: Option<super::region::values::executable::Recovery>,
    active_fp_recovery: Option<super::region::values::executable::fp::FpRecovery>,
    active_effect: Option<super::region::values::executable::ExecEffectId>,
    captured_effect: Option<super::region::values::executable::ExecEffectId>,
    frame: Option<super::region::values::FrameStateId>,
    before_frame: Option<super::region::values::FrameStateId>,
    after_frame: Option<super::region::values::FrameStateId>,
    address_fact: Option<integer::AddressFact>,
    stack_access: Option<crate::cpu::x86::ir::EffectiveAddress>,
    guarded_gpr_facts: bool,
    current_block: Option<super::region::BlockId>,
    grouped_accesses: bool,
    is_trace: bool,
    family: bool,
    guarded_control: bool,
    canonical_service: bool,
    exact_service: bool,
    native: &'a [NativeInstruction],
    spans: &'a [super::program::spans::ExecutableSpan],
}

impl Output<'_> {
    pub(super) fn c(&mut self) -> code::Sink<'_> {
        code::Sink::new(
            self.function.instructions(),
            &mut self.depth,
            self.return_target,
        )
    }

    fn internal(&self, index: u32) -> u32 {
        support::owner_internal(index, !self.native.is_empty())
    }

    fn block(&mut self, id: super::region::BlockId, block: &super::region::RegionBlock) {
        self.current_block = Some(id);
        if self.grouped_accesses && self.plan.has_access_groups() {
            super::memory::groups::invalidate(self.c().raw());
        }
        self.begin_executable_block(id);
        let fp = self.executable.block(id).entry.fp.clone();
        numeric::executable::emit_ingress(self, &fp);
        let mut counted = false;
        let steps = self.executable.block(id).steps.clone();
        for step in &steps {
            match step {
                super::region::values::executable::ExecutableStep::Pure(run) => self.pure_run(run),
                super::region::values::executable::ExecutableStep::Effect(effect) => {
                    self.prepare_effect(effect);
                    counted |= self.block_instruction(
                        id,
                        block,
                        &block.instructions[effect.point.prefix as usize],
                    );
                    self.finish_effect(effect);
                }
            }
        }
        let recovery = self.executable.block(id).exit.clone();
        self.recover_scalar(&recovery, 0, false);
        self.active_fp_recovery = Some(recovery.fp.clone());
        self.active_recovery = None;
        self.active_effect = None;
        if !counted {
            self.count(block.instructions.len() as u32);
        }
    }

    fn block_instruction(
        &mut self,
        id: super::region::BlockId,
        block: &super::region::RegionBlock,
        instruction: &super::region::RegionInstruction,
    ) -> bool {
        let demands = &block.flags.after;
        let analyzed = self.plan.instruction(id, instruction.prefix);
        let before_id = analyzed.before;
        let after_id = analyzed.after;
        self.before_frame = Some(before_id);
        self.after_frame = Some(after_id);
        self.frame = self.before_frame;
        let before = self.plan.frame(before_id);
        let prefix = before.retired;
        self.pc = match before.pc {
            super::region::ResumePc::Exact(pc) => pc,
            super::region::ResumePc::Computed => unreachable!("instruction entry is exact"),
        };
        self.prefix = prefix;
        self.stack_access = None;
        self.record_flags = true;
        self.tail_flags = instruction
            .operation
            .and_then(super::effects::analyze)
            .is_some_and(|effect| effect.write & demands[instruction.prefix as usize] == 0);
        // Direct region control is lowered from its typed terminator. Its flags
        // and counter effects are emitted once where the Wasm edge is selected.
        if instruction.prefix as usize + 1 == block.instructions.len()
            && matches!(
                block.terminator,
                super::region::Terminator::Branch { .. }
                    | super::region::Terminator::CountBranch { .. }
            )
        {
            self.frame = self.after_frame;
            return false;
        }
        if let Some(operation) = instruction.operation.filter(strings::supports) {
            self.address_fact = None;
            // Runtime string iterations advance their address registers inside
            // one guest instruction; entry constants are not loop invariants.
            let before = self.before_frame.take();
            strings::emit(self, operation, instruction.next);
            self.before_frame = before;
            self.frame = self.after_frame;
            return true;
        }
        let writes_memory = instruction
            .operation
            .is_some_and(|operation| super::effects::memory_barrier(operation).1);
        if writes_memory {
            self.c().i32_const(0).local_set(WRITE_STATUS);
        }
        if let Some(gate) = instruction.gate {
            self.c()
                .local_get(CTX)
                .i32_const(gate as i32)
                .i32_store(mem(116, 2));
            self.exit(StopReason::Host, Some(self.pc), prefix);
        } else {
            match instruction.operation {
                None => self.exit(StopReason::Unsupported, Some(self.pc), prefix),
                Some(operation) => {
                    let kind = self
                        .active_effect
                        .map(|id| self.executable.effect(id).kind.clone());
                    match kind {
                        Some(super::region::values::executable::EffectKind::PlainRead32 {
                            address,
                            result,
                        }) => self.emit_plain_read(&address, result),
                        Some(super::region::values::executable::EffectKind::Fp(effect)) => {
                            numeric::executable::emit_effect(self, &effect)
                        }
                        _ => self.instruction(operation, instruction.next),
                    }
                }
            }
        }
        self.frame = self.after_frame;
        if let Some(effect) = self.active_effect {
            let effect = self.executable.effect(effect).clone();
            self.capture_effect(&effect);
        }
        if writes_memory && instruction.exits.after.is_some() {
            let completed = self.plan.frame(after_id);
            let completed_pc = completed.pc;
            let completed_retired = completed.retired;
            self.c()
                .local_get(WRITE_STATUS)
                .i32_const(2)
                .i32_eq()
                .if_(BlockType::Empty);
            let pc = match completed_pc {
                super::region::ResumePc::Exact(pc) => Some(pc),
                super::region::ResumePc::Computed => None,
            };
            self.exit(StopReason::CodeInvalidated, pc, completed_retired);
            self.c().end();
        }
        false
    }

    pub(super) fn count(&mut self, retired: u32) {
        if retired != 0 {
            self.c()
                .local_get(REMAIN)
                .i32_const(retired as i32)
                .local_get(PREFIX_BASE)
                .i32_sub()
                .i32_sub()
                .local_set(REMAIN);
            self.c()
                .local_get(RETIRED)
                .i32_const(retired as i32)
                .local_get(PREFIX_BASE)
                .i32_sub()
                .i32_add()
                .local_set(RETIRED)
                .i32_const(0)
                .local_set(PREFIX_BASE);
        }
    }

    pub(super) fn materialize(&mut self, mask: u32) {
        if !scalar::try_materialize(self, mask) {
            self.materialize_fallback(mask);
        }
    }

    fn materialize_fallback(&mut self, mask: u32) {
        for local in [FLAGS, KIND, WIDTH, LEFT, RIGHT, VALUE, CARRY_IN] {
            self.c().local_get(local);
        }
        let function = self.internal(2);
        self.c()
            .i32_const(mask as i32)
            .call(function)
            .local_set(FLAGS);
    }

    pub(super) fn exit(&mut self, reason: StopReason, pc: Option<u32>, retired: u32) {
        if reason == StopReason::ExactPrefix && !self.native.is_empty() {
            numeric::request_exact_suffix(self, pc, retired);
            return;
        }
        self.publish_exit(pc, retired);
        if reason == StopReason::NeedCode {
            self.c()
                .i32_const(StopReason::Budget as i32)
                .i32_const(reason as i32)
                .local_get(REMAIN);
            if retired != 0 {
                self.c()
                    .i32_const(retired as i32)
                    .local_get(PREFIX_BASE)
                    .i32_sub()
                    .i32_sub();
            }
            self.c().i32_eqz().select().return_();
        } else {
            self.c().i32_const(reason as i32).return_();
        }
    }

    fn publish_exit(&mut self, pc: Option<u32>, retired: u32) {
        let frame = if retired != 0 && retired == self.prefix + 1 {
            self.after_frame
        } else {
            self.frame
        };
        let action = frame.map_or(recovery::FrameAction::None, recovery::FrameAction::Select);
        let exit = recovery::ExitAction { pc, retired };
        if let Some(mut recovery) = self.active_recovery.clone() {
            let after = retired != 0 && retired == self.prefix + 1;
            let mut kernel_gpr = 0;
            let mut kernel_flags = false;
            let mut branch_capture = None;
            if let Some(id) = self.active_effect {
                let effect = self.executable.effect(id).clone();
                if after {
                    if self.captured_effect != Some(id) {
                        // An exact kernel can report a completed stop before
                        // its ordinary fallthrough capture (POPF's Debug arm).
                        // Capture the real successful definitions on this arm,
                        // without granting them to the separately emitted arm.
                        branch_capture = Some((
                            self.available.clone(),
                            self.physical_gpr,
                            self.physical_flags,
                            self.captured_effect,
                        ));
                        self.capture_effect(&effect);
                    }
                    recovery = effect.after;
                } else if matches!(
                    effect.contract.fault_recovery,
                    super::region::values::executable::FaultRecovery::Kernel
                ) {
                    kernel_gpr = effect.contract.gpr_writes;
                    kernel_flags = effect.contract.flags_write != 0;
                }
            }
            recovery::request(
                self,
                Some(&recovery),
                Some(&recovery.fp),
                action,
                recovery::program::KernelMasks {
                    gpr: kernel_gpr,
                    flags: kernel_flags,
                },
                exit,
            );
            if let Some((available, physical_gpr, physical_flags, captured_effect)) = branch_capture
            {
                self.available = available;
                self.physical_gpr = physical_gpr;
                self.physical_flags = physical_flags;
                self.captured_effect = captured_effect;
            }
        } else if let Some(recovery) = self.active_fp_recovery.clone() {
            recovery::request(
                self,
                None,
                Some(&recovery),
                action,
                recovery::program::KernelMasks {
                    gpr: 0,
                    flags: false,
                },
                exit,
            );
        } else {
            recovery::request(
                self,
                None,
                None,
                action,
                recovery::program::KernelMasks {
                    gpr: 0,
                    flags: false,
                },
                exit,
            );
        }
    }

    fn publish_operands(&mut self) {
        let pc = self.pc;
        self.c().i32_const(pc as i32).local_set(EIP);
        for register in 0..8 {
            self.c()
                .local_get(CTX)
                .local_get(register + 2)
                .i32_store(mem(u64::from(register * 4), 2));
        }
        for (offset, local) in [(32, EIP), (36, FLAGS)] {
            self.c()
                .local_get(CTX)
                .local_get(local)
                .i32_store(mem(offset, 2));
        }
    }

    fn known_gpr(&self, local: u32) -> Option<u32> {
        let register = local.checked_sub(2)? as usize;
        if register >= 8 {
            return None;
        }
        let frame = self.plan.frame(self.before_frame?);
        self.plan.constant(frame.gpr[register])
    }

    fn dynamic_pc(&mut self, local: u32) {
        self.c().local_get(local).local_set(EIP);
        if !self.guarded_indirect() {
            self.lookup_port(EIP);
        }
    }

    fn guarded_indirect(&self) -> bool {
        self.guarded_control
    }

    fn lookup_port(&mut self, local: u32) {
        let lookup = self.internal(4);
        self.c()
            .local_get(local)
            .call(lookup)
            .local_tee(INDEX)
            .i32_const(BLOCK_BITS as i32)
            .i32_shr_u()
            .local_set(ENTRY_OFFSET);
        self.c()
            .local_get(INDEX)
            .i32_const((1 << BLOCK_BITS) - 1)
            .i32_and()
            .i32_const(-1)
            .local_get(INDEX)
            .i32_const(-1)
            .i32_ne()
            .select()
            .local_set(INDEX);
    }

    pub(super) fn read(&mut self, width: u32, destination: u32) {
        self.read_memory(width, destination, false);
    }

    fn read_prepared(&mut self, width: u32, destination: u32) {
        self.read_memory(width | 0x100, destination, true);
    }

    fn read_memory(&mut self, width: u32, destination: u32, prepare: bool) {
        if let Some(cached) = scalar::reusable_read(self, width, prepare) {
            self.c()
                .local_get(FULL_BLOCK)
                .if_(BlockType::Empty)
                .local_get(cached)
                .local_set(destination)
                .else_();
            self.read_memory_fallback(width, destination, prepare);
            self.c().end();
        } else {
            self.read_memory_fallback(width, destination, prepare);
        }
    }

    fn read_memory_fallback(&mut self, width: u32, destination: u32, prepare: bool) {
        let grouped = self.current_group_geometry(width & 0xff, false);
        if grouped {
            self.c()
                .local_tee(super::memory::groups::ADMITTED)
                .i32_eqz()
                .if_(BlockType::Empty);
            self.check_span(width & 0xff);
            self.c().end();
        } else {
            self.c()
                .i32_const(0)
                .local_set(super::memory::groups::ADMITTED);
            self.check_span(width & 0xff);
        }
        let translate = self.internal(9);
        super::memory::read(self.c().raw(), width, 0, translate, prepare, grouped);
        self.c()
            .local_tee(WIDE)
            .i64_const(32)
            .i64_shr_u()
            .i32_wrap_i64()
            .local_tee(WRITE_STATUS)
            .if_(BlockType::Empty);
        self.memory_exit();
        self.c()
            .end()
            .local_get(WIDE)
            .i32_wrap_i64()
            .local_set(destination);
        scalar::capture_read(self, width, destination, prepare);
    }

    pub(super) fn write(&mut self, width: u32, source: u32) {
        self.write_memory(width, source, false);
    }

    fn write_prepared(&mut self, width: u32, source: u32) {
        self.write_memory(width, source, true);
    }

    fn write_memory(&mut self, width: u32, source: u32, prepared: bool) {
        let mut grouped = false;
        if !prepared {
            // Only a successful nonfaulting group admission bypasses the usual
            // current-access checks. Failed speculation keeps the existing path.
            self.c().i32_const(0).local_set(35);
            grouped = self.current_group_geometry(width, true);
            if grouped {
                self.c()
                    .local_tee(super::memory::groups::ADMITTED)
                    .if_(BlockType::Empty)
                    .local_get(33)
                    .i32_const(1)
                    .i32_add()
                    .local_set(35)
                    .local_get(34)
                    .local_set(36)
                    .else_();
                self.check_span(width);
                self.c().end();
            } else {
                self.c()
                    .i32_const(0)
                    .local_set(super::memory::groups::ADMITTED);
                self.check_span(width);
            }
        }
        let commit = support::owner_store(width, !self.native.is_empty());
        let translate = self.internal(9);
        super::memory::write(
            self.c().raw(),
            width,
            source,
            commit,
            1,
            translate,
            prepared || grouped,
            grouped,
        );
        self.c()
            .local_tee(WRITE_STATUS)
            .i32_const(2)
            .i32_gt_u()
            .local_get(WRITE_STATUS)
            .i32_const(1)
            .i32_eq()
            .i32_or()
            .if_(BlockType::Empty);
        self.memory_exit();
        self.c().end();
    }

    fn memory_exit(&mut self) {
        strings::normalize_memory_fault(self);
        self.publish_exit(Some(self.pc), self.prefix);
        self.c()
            .i32_const(StopReason::MemoryFault as i32)
            .i32_const(StopReason::Infrastructure as i32)
            .local_get(WRITE_STATUS)
            .i32_const(1)
            .i32_eq()
            .select()
            .return_();
    }

    fn check_span(&mut self, width: u32) {
        self.c()
            .local_get(LOGICAL_ADDRESS)
            .i32_const((u32::MAX - (width - 1)) as i32)
            .i32_gt_u()
            .if_(BlockType::Empty);
        self.exit(StopReason::Unsupported, Some(self.pc), self.prefix);
        self.c().end();
    }

    fn jump_condition(&mut self, condition: Condition) {
        if scalar::try_condition(self, condition) {
            return;
        }
        if !matches!(condition, Condition::Equal | Condition::NotEqual) {
            self.condition(condition);
            return;
        }
        self.c()
            .local_get(KIND)
            .i32_const(1)
            .i32_sub()
            .i32_const(7)
            .i32_le_u()
            .local_get(WIDTH)
            .i32_const(8)
            .i32_eq()
            .local_get(WIDTH)
            .i32_const(16)
            .i32_eq()
            .i32_or()
            .local_get(WIDTH)
            .i32_const(32)
            .i32_eq()
            .i32_or()
            .i32_and()
            .if_(BlockType::Result(ValType::I32));
        // shifting discards bits outside the pending operand width.
        self.c()
            .local_get(VALUE)
            .i32_const(32)
            .local_get(WIDTH)
            .i32_sub()
            .i32_shl()
            .i32_eqz();
        if condition == Condition::NotEqual {
            self.c().i32_eqz();
        }
        self.c().else_();
        self.condition(condition);
        self.c().end();
    }

    pub(super) fn condition(&mut self, condition: Condition) {
        if !scalar::try_condition(self, condition) {
            self.condition_fallback(condition);
        }
    }

    fn condition_fallback(&mut self, condition: Condition) {
        self.materialize_fallback(super::effects::condition_mask(condition));
        match condition {
            Condition::Overflow | Condition::NotOverflow => {
                self.c()
                    .local_get(FLAGS)
                    .i32_const(11)
                    .i32_shr_u()
                    .i32_const(1)
                    .i32_and();
            }
            Condition::Below | Condition::AboveOrEqual => {
                self.c().local_get(FLAGS).i32_const(1).i32_and();
            }
            Condition::Equal | Condition::NotEqual => {
                self.c()
                    .local_get(FLAGS)
                    .i32_const(6)
                    .i32_shr_u()
                    .i32_const(1)
                    .i32_and();
            }
            Condition::Sign | Condition::NotSign => {
                self.c()
                    .local_get(FLAGS)
                    .i32_const(7)
                    .i32_shr_u()
                    .i32_const(1)
                    .i32_and();
            }
            Condition::Parity | Condition::NotParity => {
                self.c()
                    .local_get(FLAGS)
                    .i32_const(2)
                    .i32_shr_u()
                    .i32_const(1)
                    .i32_and();
            }
            Condition::BelowOrEqual | Condition::Above => {
                self.c()
                    .local_get(FLAGS)
                    .i32_const(65)
                    .i32_and()
                    .i32_eqz()
                    .i32_eqz();
            }
            Condition::Less
            | Condition::GreaterOrEqual
            | Condition::LessOrEqual
            | Condition::Greater => {
                self.c()
                    .local_get(FLAGS)
                    .i32_const(7)
                    .i32_shr_u()
                    .local_get(FLAGS)
                    .i32_const(11)
                    .i32_shr_u()
                    .i32_xor()
                    .i32_const(1)
                    .i32_and();
                if matches!(condition, Condition::LessOrEqual | Condition::Greater) {
                    self.c()
                        .local_get(FLAGS)
                        .i32_const(6)
                        .i32_shr_u()
                        .i32_const(1)
                        .i32_and()
                        .i32_or();
                }
            }
        }
        if matches!(
            condition,
            Condition::NotOverflow
                | Condition::AboveOrEqual
                | Condition::NotEqual
                | Condition::Above
                | Condition::NotSign
                | Condition::NotParity
                | Condition::GreaterOrEqual
                | Condition::Greater
        ) {
            self.c().i32_eqz();
        }
    }

    fn target(&mut self, target: BranchTarget, destination: u32) {
        match target {
            BranchTarget::Direct(pc) => {
                self.c().i32_const(pc.0 as i32).local_set(destination);
            }
            BranchTarget::Indirect(location) => self.load_location(location, 4, destination),
        }
    }

    fn instruction(&mut self, operation: Operation, next: u32) {
        if numeric::supports(&operation) {
            self.address_fact = None;
            numeric::emit(self, next);
            self.address_fact = None;
            return;
        }
        if integer::emit(self, operation) {
            return;
        }
        if control::emit(self, operation, next) {
            return;
        }
        match operation {
            Operation::Nop => {}
            Operation::Jump {
                target: BranchTarget::Direct(_),
            } => {}
            Operation::Jump { target } => {
                self.target(target, RESULT);
                self.dynamic_pc(RESULT);
            }
            Operation::ConditionalJump { .. } | Operation::CountBranch { .. } => {
                unreachable!("direct control is owned by the region terminator")
            }
            Operation::Push { source } => {
                self.load_value(source, 4, RESULT);
                self.stack_address(crate::cpu::x86::Register32::Esp, -4i32 as u32);
                self.write(4, RESULT);
                self.c().local_get(ADDRESS).local_set(6);
            }
            Operation::PushFlags => {
                self.materialize(flags::ARITHMETIC);
                self.c()
                    .local_get(FLAGS)
                    .i32_const(!0x30000)
                    .i32_and()
                    .local_set(RESULT);
                self.stack_address(crate::cpu::x86::Register32::Esp, -4i32 as u32);
                self.write(4, RESULT);
                self.c().local_get(ADDRESS).local_set(6);
            }
            Operation::Pop { destination } => {
                self.stack_address(crate::cpu::x86::Register32::Esp, 0);
                self.read(4, RESULT);
                match destination {
                    Location32::Register(register) => {
                        self.c().local_get(6).i32_const(4).i32_add().local_set(6);
                        self.c()
                            .local_get(RESULT)
                            .local_set(register.index() as u32 + 2);
                    }
                    Location32::Memory(mut address) => {
                        if matches!(address.base, Some(crate::cpu::x86::Register32::Esp)) {
                            address.displacement = address.displacement.wrapping_add(4);
                        }
                        self.address(address);
                        self.write(4, RESULT);
                        self.c().local_get(6).i32_const(4).i32_add().local_set(6);
                    }
                }
            }
            Operation::Call { target } => {
                if matches!(target, BranchTarget::Indirect(_)) {
                    self.target(target, B);
                }
                self.c().i32_const(next as i32).local_set(RESULT);
                self.stack_address(crate::cpu::x86::Register32::Esp, -4i32 as u32);
                self.write(4, RESULT);
                self.c().local_get(ADDRESS).local_set(6);
                match target {
                    BranchTarget::Direct(_) => {}
                    BranchTarget::Indirect(_) => self.dynamic_pc(B),
                }
            }
            Operation::Return { stack_adjust } => {
                self.stack_address(crate::cpu::x86::Register32::Esp, 0);
                self.read(4, RESULT);
                self.c()
                    .local_get(6)
                    .i32_const(4 + i32::from(stack_adjust))
                    .i32_add()
                    .local_set(6);
                self.dynamic_pc(RESULT);
            }
            Operation::Leave => {
                self.stack_address(crate::cpu::x86::Register32::Ebp, 0);
                self.read(4, RESULT);
                self.c()
                    .local_get(7)
                    .i32_const(4)
                    .i32_add()
                    .local_set(6)
                    .local_get(RESULT)
                    .local_set(7);
            }
            Operation::Control(ControlOperation::Sysenter) => {
                self.exit(StopReason::Host, Some(self.pc), self.prefix)
            }
            Operation::Control(ControlOperation::Breakpoint) => {
                self.exit(StopReason::Debug, Some(next), self.prefix + 1)
            }
            _ => self.exit(StopReason::Unsupported, Some(self.pc), self.prefix),
        }
    }
}
