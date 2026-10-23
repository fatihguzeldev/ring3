mod control;
mod integer;
mod numeric;
mod strings;
#[cfg(test)]
mod tests;

use super::{
    StopReason, flags,
    program::{Block, NativeInstruction},
};
use crate::cpu::x86::{
    ControlOperation,
    ir::{BranchTarget, Condition, CountBranchKind, Location32, Operation},
};
use wasm_encoder::{
    BlockType, CodeSection, EntityType, ExportKind, ExportSection, Function, FunctionSection,
    ImportSection, MemArg, MemoryType, Module, NameMap, NameSection, TypeSection, ValType,
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

pub(super) fn is_native(operation: &Operation) -> bool {
    numeric::supports(operation)
}

pub(super) fn emit(
    blocks: &[Block],
    entry: u32,
    identity: u64,
    native: &[NativeInstruction],
) -> Vec<u8> {
    let shift = u32::from(!native.is_empty());
    let (fast, store_widths) = run(blocks, identity, native, false, Vec::new());
    let (precise, store_widths) = run(blocks, identity, native, true, store_widths);
    let mut module = Module::new();
    let mut types = TypeSection::new();
    types.ty().function([ValType::I32; 2], [ValType::I32]);
    types.ty().function([ValType::I32; 3], [ValType::I64]);
    types.ty().function([ValType::I32; 4], [ValType::I32]);
    types.ty().function([ValType::I32; 8], [ValType::I32]);
    types.ty().function([ValType::I32], [ValType::I32]);
    types.ty().function([ValType::I32; 13], []);
    if !store_widths.is_empty() {
        types.ty().function([ValType::I32; 5], [ValType::I32]);
    }
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
        imports.import("ring3", "numeric", EntityType::Function(0));
    }
    module.section(&imports);
    let mut functions = FunctionSection::new();
    for kind in [3, 0, 4, 5, 0] {
        functions.function(kind);
    }
    for _ in &store_widths {
        functions.function(6);
    }
    module.section(&functions);
    let mut exports = ExportSection::new();
    exports.export("run", ExportKind::Func, 3 + shift);
    module.section(&exports);
    let mut code = CodeSection::new();
    code.function(&flags::emit_function());
    code.function(&fast);
    code.function(&lookup(blocks));
    code.function(&publisher());
    code.function(&precise);
    for &width in &store_widths {
        code.function(&super::memory::emit_store(width));
    }
    module.section(&code);
    let mut names = NameSection::new();
    names.module(&format!("ring3.guest.{entry:08x}.{identity}"));
    let mut functions = NameMap::new();
    functions.append(0, "guest.read_slow");
    functions.append(1, "guest.write_slow");
    if shift != 0 {
        functions.append(2, "guest.numeric");
    }
    for (index, name) in [
        "guest.flags",
        "guest.run",
        "guest.lookup",
        "guest.publish",
        "guest.precise",
    ]
    .into_iter()
    .enumerate()
    {
        functions.append(2 + shift + index as u32, name);
    }
    for (index, width) in store_widths.iter().enumerate() {
        functions.append(
            7 + shift + index as u32,
            &format!("guest.store{}", width * 8),
        );
    }
    names.functions(&functions);
    module.section(&names);
    module.finish()
}

fn publisher() -> Function {
    let mut function = Function::new([]);
    for field in 0..12 {
        function
            .instructions()
            .local_get(0)
            .local_get(field + 1)
            .i32_store(mem(u64::from(field * 4), 2));
    }
    function.instructions().end();
    function
}

fn lookup(blocks: &[Block]) -> Function {
    fn tree(code: &mut wasm_encoder::InstructionSink<'_>, blocks: &[Block], offset: usize) {
        if blocks.is_empty() {
            code.i32_const(-1);
            return;
        }
        let middle = blocks.len() / 2;
        let pc = blocks[middle].entry;
        code.local_get(0)
            .i32_const(pc as i32)
            .i32_eq()
            .if_(BlockType::Result(ValType::I32));
        code.i32_const((offset + middle) as i32);
        code.else_()
            .local_get(0)
            .i32_const(pc as i32)
            .i32_lt_u()
            .if_(BlockType::Result(ValType::I32));
        tree(code, &blocks[..middle], offset);
        code.else_();
        tree(code, &blocks[middle + 1..], offset + middle + 1);
        code.end().end();
    }
    let mut function = Function::new([]);
    tree(&mut function.instructions(), blocks, 0);
    function.instructions().end();
    function
}

fn run(
    blocks: &[Block],
    identity: u64,
    native: &[NativeInstruction],
    precise: bool,
    store_widths: Vec<u32>,
) -> (Function, Vec<u32>) {
    let mut function = Function::new([
        (25, ValType::I32),
        (1, ValType::I64),
        (18, ValType::I32),
        (2, ValType::I64),
        (4, ValType::F64),
        (8, ValType::I32),
        (6, ValType::V128),
        (10, ValType::F64),
    ]);
    let mut out = Output {
        function: &mut function,
        blocks,
        pc: 0,
        prefix: 0,
        record_flags: true,
        native,
        store_widths,
    };
    out.c()
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
    super::memory::load_view(&mut out.c());
    let lookup = out.internal(4);
    out.c().local_get(EIP).call(lookup).local_set(INDEX);
    out.c().loop_(BlockType::Empty);
    out.c().local_get(REMAIN).i32_eqz().if_(BlockType::Empty);
    out.exit(StopReason::Budget, None, 0);
    out.c().end().block(BlockType::Empty);
    for _ in blocks {
        out.c().block(BlockType::Empty);
    }
    out.c()
        .local_get(INDEX)
        .br_table(0..blocks.len() as u32, blocks.len() as u32);
    for (index, block) in blocks.iter().enumerate() {
        out.c().end();
        if precise {
            out.block(block, true);
        } else {
            out.c()
                .local_get(REMAIN)
                .i32_const(block.instructions.len() as i32)
                .i32_lt_u()
                .if_(BlockType::Empty);
            out.publish_exit(Some(block.entry), 0);
            let exact = out.internal(6);
            out.c()
                .local_get(CTX)
                .local_get(REMAIN)
                .call(exact)
                .return_()
                .end();
            out.block(block, false);
        }
        out.c().br((blocks.len() - index) as u32);
    }
    out.c().end();
    out.exit(StopReason::NeedCode, None, 0);
    out.c().end().unreachable().end();
    let Output { store_widths, .. } = out;
    (function, store_widths)
}

pub(super) struct Output<'a> {
    function: &'a mut Function,
    blocks: &'a [Block],
    pc: u32,
    prefix: u32,
    record_flags: bool,
    native: &'a [NativeInstruction],
    store_widths: Vec<u32>,
}

impl Output<'_> {
    pub(super) fn c(&mut self) -> wasm_encoder::InstructionSink<'_> {
        self.function.instructions()
    }

    fn internal(&self, index: u32) -> u32 {
        index + u32::from(!self.native.is_empty())
    }

    fn block(&mut self, block: &Block, precise: bool) {
        let operations: Vec<_> = block
            .instructions
            .iter()
            .map(|instruction| {
                instruction
                    .operation
                    .unwrap_or(Operation::Control(ControlOperation::InvalidOpcode))
            })
            .collect();
        let demands = super::effects::live_flags(&operations);
        for (prefix, instruction) in block.instructions.iter().enumerate() {
            self.pc = instruction.pc;
            self.prefix = prefix as u32;
            if precise {
                self.c()
                    .local_get(REMAIN)
                    .i32_const(prefix as i32)
                    .i32_le_u()
                    .if_(BlockType::Empty);
                self.exit(StopReason::Budget, Some(instruction.pc), prefix as u32);
                self.c().end();
            }
            self.record_flags = precise
                || instruction
                    .operation
                    .and_then(super::effects::analyze)
                    .is_none_or(|effect| effect.write & demands[prefix] != 0);
            if let Some(operation) = instruction.operation.filter(strings::supports) {
                strings::emit(self, operation, instruction.next);
                return;
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
                self.exit(StopReason::Host, Some(instruction.pc), prefix as u32);
            } else {
                match instruction.operation {
                    None => self.exit(StopReason::Unsupported, Some(instruction.pc), prefix as u32),
                    Some(operation) => self.instruction(operation, instruction.next),
                }
            }
            if writes_memory {
                self.c()
                    .local_get(WRITE_STATUS)
                    .i32_const(2)
                    .i32_eq()
                    .if_(BlockType::Empty);
                self.exit(StopReason::CodeInvalidated, None, prefix as u32 + 1);
                self.c().end();
            }
        }
        self.count(block.instructions.len() as u32);
    }

    pub(super) fn count(&mut self, retired: u32) {
        if retired != 0 {
            self.c()
                .local_get(REMAIN)
                .i32_const(retired as i32)
                .i32_sub()
                .local_set(REMAIN);
            self.c()
                .local_get(RETIRED)
                .i32_const(retired as i32)
                .i32_add()
                .local_set(RETIRED);
        }
    }

    pub(super) fn materialize(&mut self, mask: u32) {
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
        self.publish_exit(pc, retired);
        self.c().i32_const(reason as i32).return_();
    }

    fn publish_exit(&mut self, pc: Option<u32>, retired: u32) {
        if let Some(pc) = pc {
            self.c().i32_const(pc as i32).local_set(EIP);
        }
        self.count(retired);
        self.materialize(flags::ARITHMETIC);
        for local in [CTX, 2, 3, 4, 5, 6, 7, 8, 9, EIP, FLAGS, RETIRED, REMAIN] {
            self.c().local_get(local);
        }
        let function = self.internal(5);
        self.c().call(function);
    }

    fn publish_operands(&mut self) {
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

    fn index(&self, pc: u32) -> i32 {
        self.blocks
            .binary_search_by_key(&pc, |block| block.entry)
            .map_or(-1, |index| index as i32)
    }

    pub(super) fn set_pc(&mut self, pc: u32) {
        let index = self.index(pc);
        self.c()
            .i32_const(pc as i32)
            .local_set(EIP)
            .i32_const(index)
            .local_set(INDEX);
    }

    fn dynamic_pc(&mut self, local: u32) {
        let lookup = self.internal(4);
        self.c()
            .local_get(local)
            .local_tee(EIP)
            .call(lookup)
            .local_set(INDEX);
    }

    pub(super) fn read(&mut self, width: u32, destination: u32) {
        self.read_memory(width, destination, false);
    }

    fn read_prepared(&mut self, width: u32, destination: u32) {
        self.read_memory(width | 0x100, destination, true);
    }

    fn read_memory(&mut self, width: u32, destination: u32, prepare: bool) {
        self.check_span(width & 0xff);
        super::memory::read(&mut self.c(), width, 0, prepare);
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
    }

    pub(super) fn write(&mut self, width: u32, source: u32) {
        self.write_memory(width, source, false);
    }

    fn write_prepared(&mut self, width: u32, source: u32) {
        self.write_memory(width, source, true);
    }

    fn write_memory(&mut self, width: u32, source: u32, prepared: bool) {
        if !prepared {
            self.check_span(width);
        }
        let index = match self.store_widths.iter().position(|&stored| stored == width) {
            Some(index) => index,
            None => {
                let index = self.store_widths.len();
                self.store_widths.push(width);
                index
            }
        };
        let commit = self.internal(7 + index as u32);
        super::memory::write(&mut self.c(), width, source, commit, 1, prepared);
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
        self.materialize(super::effects::condition_mask(condition));
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
            numeric::emit(self, next);
            return;
        }
        if integer::emit(self, operation) {
            self.set_pc(next);
            return;
        }
        if control::emit(self, operation, next) {
            return;
        }
        match operation {
            Operation::Nop => self.set_pc(next),
            Operation::Jump {
                target: BranchTarget::Direct(pc),
            } => self.set_pc(pc.0),
            Operation::Jump { target } => {
                self.target(target, RESULT);
                self.dynamic_pc(RESULT);
            }
            Operation::ConditionalJump { condition, target } => {
                let taken = self.index(target.0);
                let otherwise = self.index(next);
                self.jump_condition(condition);
                self.c().local_set(A);
                self.c()
                    .i32_const(target.0 as i32)
                    .i32_const(next as i32)
                    .local_get(A)
                    .select()
                    .local_set(EIP);
                self.c()
                    .i32_const(taken)
                    .i32_const(otherwise)
                    .local_get(A)
                    .select()
                    .local_set(INDEX);
            }
            Operation::CountBranch { kind, target } => {
                if !matches!(kind, CountBranchKind::EcxZero) {
                    self.c().local_get(3).i32_const(1).i32_sub().local_set(3);
                }
                self.c().local_get(3).i32_eqz();
                if !matches!(kind, CountBranchKind::EcxZero) {
                    self.c().i32_eqz();
                }
                self.c().local_set(A);
                if matches!(
                    kind,
                    CountBranchKind::LoopEqual | CountBranchKind::LoopNotEqual
                ) {
                    self.condition(if matches!(kind, CountBranchKind::LoopEqual) {
                        Condition::Equal
                    } else {
                        Condition::NotEqual
                    });
                    self.c().local_get(A).i32_and().local_set(A);
                }
                let taken = self.index(target.0);
                let otherwise = self.index(next);
                self.c()
                    .i32_const(target.0 as i32)
                    .i32_const(next as i32)
                    .local_get(A)
                    .select()
                    .local_set(EIP);
                self.c()
                    .i32_const(taken)
                    .i32_const(otherwise)
                    .local_get(A)
                    .select()
                    .local_set(INDEX);
            }
            Operation::Push { source } => {
                self.load_value(source, 4, RESULT);
                self.c()
                    .local_get(6)
                    .i32_const(4)
                    .i32_sub()
                    .local_tee(ADDRESS)
                    .local_set(LOGICAL_ADDRESS);
                self.write(4, RESULT);
                self.c().local_get(ADDRESS).local_set(6);
                self.set_pc(next);
            }
            Operation::PushFlags => {
                self.materialize(flags::ARITHMETIC);
                self.c()
                    .local_get(FLAGS)
                    .i32_const(!0x30000)
                    .i32_and()
                    .local_set(RESULT);
                self.c()
                    .local_get(6)
                    .i32_const(4)
                    .i32_sub()
                    .local_tee(ADDRESS)
                    .local_set(LOGICAL_ADDRESS);
                self.write(4, RESULT);
                self.c().local_get(ADDRESS).local_set(6);
                self.set_pc(next);
            }
            Operation::Pop { destination } => {
                self.c()
                    .local_get(6)
                    .local_tee(ADDRESS)
                    .local_set(LOGICAL_ADDRESS);
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
                self.set_pc(next);
            }
            Operation::Call { target } => {
                self.target(target, B);
                self.c()
                    .i32_const(next as i32)
                    .local_set(RESULT)
                    .local_get(6)
                    .i32_const(4)
                    .i32_sub()
                    .local_tee(ADDRESS)
                    .local_set(LOGICAL_ADDRESS);
                self.write(4, RESULT);
                self.c().local_get(ADDRESS).local_set(6);
                match target {
                    BranchTarget::Direct(pc) => self.set_pc(pc.0),
                    BranchTarget::Indirect(_) => self.dynamic_pc(B),
                }
            }
            Operation::Return { stack_adjust } => {
                self.c()
                    .local_get(6)
                    .local_tee(ADDRESS)
                    .local_set(LOGICAL_ADDRESS);
                self.read(4, RESULT);
                self.c()
                    .local_get(6)
                    .i32_const(4 + i32::from(stack_adjust))
                    .i32_add()
                    .local_set(6);
                self.dynamic_pc(RESULT);
            }
            Operation::Leave => {
                self.c()
                    .local_get(7)
                    .local_tee(ADDRESS)
                    .local_set(LOGICAL_ADDRESS);
                self.read(4, RESULT);
                self.c()
                    .local_get(7)
                    .i32_const(4)
                    .i32_add()
                    .local_set(6)
                    .local_get(RESULT)
                    .local_set(7);
                self.set_pc(next);
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
