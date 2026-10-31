use super::{Block, EntryPoint};
use crate::cpu::runtime::memory_cache::NativeMemoryBinding;
use crate::cpu::x86::{Register32, SegmentRegister, ir::*};
use wasm_encoder::{
    BlockType, CodeSection, EntityType, ExportKind, ExportSection, Function, FunctionSection,
    ImportSection, InstructionSink, MemArg, MemoryType, Module, TypeSection, ValType,
};

mod bit;
mod deferred;
mod exchange;
mod flags;
mod memory;
mod memory_helpers;
mod multiply;
mod narrow;
mod rotate;
mod routing;
mod shift;
mod simd;
mod string;
mod x87;
mod x87_arithmetic;
mod x87_compare;
mod x87_divide;
mod x87_multiply;

const FLAGS: u32 = 9;
const PC: u32 = 10;
const REMAIN: u32 = 11;
const RETIRED: u32 = 12;
const A: u32 = 13;
const B: u32 = 14;
const R: u32 = 15;
const ADDRESS: u32 = 16;
const WRITE_STATUS: u32 = 17;
const CARRY: u32 = 18;
const TARGET: u32 = 19;
const TEMP: u32 = 20;
const DESTINATION: u32 = 21;
const SEGMENT: u32 = 22;
const OFFSET: u32 = 23;

fn mem(offset: u64) -> MemArg {
    MemArg {
        offset,
        align: 2,
        memory_index: 0,
    }
}
fn reg(register: Register32) -> u32 {
    register.index() as u32 + 1
}

pub(super) fn supports(operation: &Operation) -> bool {
    narrow::supports(operation)
        || multiply::supports(operation)
        || string::supports(operation)
        || bit::supports(operation)
        || exchange::supports(operation)
        || flags::supports(operation)
        || shift::supports(operation)
        || rotate::supports(operation)
        || simd::supports(operation)
        || x87::supports(operation)
        || x87_compare::supports(operation)
        || x87_arithmetic::supports(operation)
        || x87_multiply::supports(operation)
        || matches!(
            operation,
            Operation::Nop
                | Operation::Move { .. }
                | Operation::Binary { .. }
                | Operation::Unary { .. }
                | Operation::Lea { .. }
                | Operation::Exchange { .. }
                | Operation::Push { .. }
                | Operation::Pop { .. }
                | Operation::PushFlags
                | Operation::Call { .. }
                | Operation::Return { .. }
                | Operation::Leave
                | Operation::Jump { .. }
                | Operation::ConditionalJump { .. }
                | Operation::CountBranch { .. }
                | Operation::Carry { .. }
                | Operation::Direction { .. }
                | Operation::MoveByte { .. }
                | Operation::MoveWord { .. }
                | Operation::LoadByte { .. }
                | Operation::LoadWord { .. }
                | Operation::StoreByte { .. }
                | Operation::StoreWord { .. }
                | Operation::Extend { .. }
                | Operation::ExtendByteToWord { .. }
                | Operation::ReadExtendByteToWord { .. }
                | Operation::LeaWord { .. }
                | Operation::SignExtendAccumulatorWord
                | Operation::SignExtendHighWord
                | Operation::SignExtendHigh
                | Operation::Shift { .. }
                | Operation::SetByte { .. }
                | Operation::MemorySetByte { .. }
                | Operation::ConditionalMove { .. }
                | Operation::ConditionalMoveWord { .. }
                | Operation::ReadConditionalMove { .. }
                | Operation::ReadConditionalMoveWord { .. }
        )
}

fn entry_route(out: &mut Output, entries: &[EntryPoint]) {
    if entries.is_empty() {
        out.c().i32_const(-1);
    } else if entries.len() == 1 {
        let entry = &entries[0];
        out.c()
            .local_get(PC)
            .i32_const(entry.pc as i32)
            .i32_eq()
            .if_(BlockType::Result(ValType::I32))
            .i32_const(((entry.block << 6) | entry.instruction) as i32)
            .else_()
            .i32_const(-1)
            .end();
    } else {
        let middle = entries.len() / 2;
        out.c()
            .local_get(PC)
            .i32_const(entries[middle].pc as i32)
            .i32_lt_u()
            .if_(BlockType::Result(ValType::I32));
        entry_route(out, &entries[..middle]);
        out.c().else_();
        entry_route(out, &entries[middle..]);
        out.c().end();
    }
}

pub(super) fn emit(
    blocks: &[Block],
    entries: &[EntryPoint],
    binding: Option<NativeMemoryBinding>,
) -> Vec<u8> {
    let mut module = Module::new();
    let mut types = TypeSection::new();
    types.ty().function([ValType::I32; 3], [ValType::I32]);
    types.ty().function([ValType::I32; 4], [ValType::I32]);
    types.ty().function([ValType::I32], [ValType::I32]);
    types.ty().function([ValType::I32; 6], [ValType::I32]);
    types.ty().function([ValType::I32; 4], [ValType::I32; 3]);
    types.ty().function([ValType::I32; 3], [ValType::I32; 3]);
    module.section(&types);
    let mut imports = ImportSection::new();
    imports.import(
        "env",
        "memory",
        EntityType::Memory(MemoryType {
            minimum: 1,
            maximum: None,
            memory64: false,
            shared: false,
            page_size_log2: None,
        }),
    );
    imports.import("cpu", "read", EntityType::Function(0));
    imports.import("cpu", "write", EntityType::Function(1));
    module.section(&imports);
    let mut functions = FunctionSection::new();
    functions.function(2);
    functions.function(3);
    functions.function(4);
    functions.function(5);
    module.section(&functions);
    let mut exports = ExportSection::new();
    exports.export("run", ExportKind::Func, 2);
    module.section(&exports);
    let mut out = Output {
        function: Function::new([
            (24, ValType::I32),
            (1, ValType::I64),
            (18, ValType::I32),
            (2, ValType::I64),
            (3, ValType::F32),
            (9, ValType::F64),
            (5, ValType::I32),
            (6, ValType::I64),
            (14, ValType::I32),
            (1, ValType::I64),
            (2, ValType::I32),
        ]),
        emitted_guest_write: false,
    };
    if let Some(binding) = binding {
        out.c()
            .local_get(0)
            .i32_const(binding.context() as i32)
            .i32_ne()
            .if_(BlockType::Empty)
            .i32_const(8)
            .return_()
            .end();
    }
    out.c()
        .local_get(0)
        .i32_load(mem(0))
        .i32_const(1)
        .i32_ne()
        .local_get(0)
        .i32_load(mem(4))
        .i32_const(160)
        .i32_ne()
        .i32_or()
        .if_(BlockType::Empty)
        .i32_const(8)
        .return_()
        .end();
    for (offset, local) in
        (8..40)
            .step_by(4)
            .zip(1..9)
            .chain([(40, PC), (44, FLAGS), (48, REMAIN), (56, RETIRED)])
    {
        out.c().local_get(0).i32_load(mem(offset)).local_set(local);
    }
    if let Some(binding) = binding {
        memory::initialize_bound(&mut out, binding);
    } else {
        memory::initialize(&mut out);
    }
    out.c().loop_(BlockType::Empty);
    entry_route(&mut out, entries);
    out.c().local_set(A);
    // the two tables bound label nesting by blocks plus instructions per block.
    let block_count = blocks.len() as u32;
    out.c().loop_(BlockType::Empty).block(BlockType::Empty);
    for _ in blocks {
        out.c().block(BlockType::Empty);
    }
    out.c()
        .local_get(A)
        .i32_const(6)
        .i32_shr_u()
        .br_table(0..block_count, block_count);
    for (block_index, block) in blocks.iter().enumerate() {
        out.c().end();
        let remaining_blocks = block_count - block_index as u32;
        let self_loop = routing::self_loop(blocks, entries, block_index);
        let local_loop = u32::from(self_loop);
        if self_loop {
            out.c().loop_(BlockType::Empty);
        }
        let landing_count = (block.instructions.len() + usize::from(block.gate.is_some())) as u32;
        for _ in 0..landing_count {
            out.c().block(BlockType::Empty);
        }
        out.c().local_get(A).i32_const(63).i32_and().br_table(
            0..landing_count,
            landing_count + remaining_blocks - 1 + local_loop,
        );
        for instruction in &block.instructions {
            out.c().end();
            out.c().local_get(REMAIN).i32_eqz().if_(BlockType::Empty);
            out.exit(2);
            out.c().end().i32_const(0).local_set(WRITE_STATUS);
            out.emitted_guest_write = false;
            if !out.instruction(*instruction.operation(), instruction.next_pc().0) {
                out.exit(7);
            } else {
                out.c()
                    .local_get(REMAIN)
                    .i32_const(1)
                    .i32_sub()
                    .local_set(REMAIN)
                    .local_get(RETIRED)
                    .i32_const(1)
                    .i32_add()
                    .local_set(RETIRED);
                if out.emitted_guest_write {
                    out.c()
                        .local_get(WRITE_STATUS)
                        .i32_const(6)
                        .i32_eq()
                        .if_(BlockType::Empty);
                    out.exit(6);
                    out.c().end();
                }
            }
        }
        if let Some(gate) = block.gate {
            out.c().end();
            out.c().local_get(REMAIN).i32_eqz().if_(BlockType::Empty);
            out.exit(2);
            out.c()
                .end()
                .local_get(0)
                .i32_const(gate as i32)
                .i32_store(mem(68));
            out.exit(5);
        }
        if routing::next(&mut out, blocks, entries, block_index) {
            out.c()
                .local_get(A)
                .i32_const(-2)
                .i32_eq()
                .if_(BlockType::Empty)
                .br(remaining_blocks + 2 + local_loop)
                .end();
        }
        routing::branch_known(&mut out, blocks, entries, block_index, self_loop);
        out.c().br(remaining_blocks + local_loop);
        if self_loop {
            out.c().end();
        }
    }
    out.c().end();
    out.exit(0);
    out.c().end().unreachable().end().unreachable().end();
    let mut code = CodeSection::new();
    code.function(&out.function);
    code.function(&deferred::materializer());
    code.function(&memory_helpers::translation());
    code.function(&memory_helpers::admission());
    module.section(&code);
    module.finish()
}

struct Output {
    function: Function,
    emitted_guest_write: bool,
}

impl Output {
    fn c(&mut self) -> InstructionSink<'_> {
        self.function.instructions()
    }
    fn publish(&mut self) {
        self.materialize_flags();
        for (offset, local) in
            (8..40)
                .step_by(4)
                .zip(1..9)
                .chain([(40, PC), (44, FLAGS), (48, REMAIN), (56, RETIRED)])
        {
            self.c()
                .local_get(0)
                .local_get(local)
                .i32_store(mem(offset));
        }
    }
    fn exit(&mut self, status: i32) {
        self.publish();
        self.c()
            .local_get(0)
            .i32_const(status)
            .i32_store(mem(52))
            .i32_const(status)
            .return_();
    }
    fn failed(&mut self, local: u32, completed: bool) {
        self.c().local_get(local);
        if completed {
            self.c()
                .i32_eqz()
                .i32_eqz()
                .local_get(local)
                .i32_const(6)
                .i32_ne()
                .i32_and();
        }
        self.c().if_(BlockType::Empty);
        self.publish();
        self.c().local_get(local).return_().end();
    }
    fn address(&mut self, address: EffectiveAddress) {
        self.offset(address);
        self.c().local_tee(OFFSET);
        match address.resolved_segment() {
            SegmentRegister::Fs => {
                self.c().local_get(0).i32_load(mem(116)).i32_add();
            }
            SegmentRegister::Gs => {
                self.c().local_get(0).i32_load(mem(120)).i32_add();
            }
            _ => {}
        }
        self.c()
            .local_set(ADDRESS)
            .i32_const(address.resolved_segment().index() as i32)
            .local_set(SEGMENT);
    }
    fn offset(&mut self, address: EffectiveAddress) {
        self.c().i32_const(address.displacement as i32);
        if let Some(base) = address.base {
            self.c().local_get(reg(base)).i32_add();
        }
        if let Some(index) = address.index {
            self.c()
                .local_get(reg(index))
                .i32_const(i32::from(address.scale))
                .i32_mul()
                .i32_add();
        }
        if address.address_size == AddressSize::Bits16 {
            self.c().i32_const(0xffff).i32_and();
        }
    }
    fn read(&mut self, width: u32, destination: u32) {
        memory::read(self, width, destination);
    }
    fn write(&mut self, width: u32, value: u32) {
        memory::write(self, width, value);
    }
    fn value(&mut self, value: Value32, destination: u32) {
        match value {
            Value32::Register(register) => {
                self.c().local_get(reg(register)).local_set(destination);
            }
            Value32::Immediate(value) => {
                self.c().i32_const(value as i32).local_set(destination);
            }
            Value32::Memory(address) => {
                self.address(address);
                self.read(4, destination);
            }
        }
    }
    fn byte(&mut self, value: ByteValue, destination: u32) {
        match value {
            ByteValue::Register(register) => {
                let (local, shift) = byte_register(register);
                self.c()
                    .local_get(local)
                    .i32_const(shift)
                    .i32_shr_u()
                    .i32_const(255)
                    .i32_and()
                    .local_set(destination);
            }
            ByteValue::Immediate(value) => {
                self.c().i32_const(i32::from(value)).local_set(destination);
            }
        }
    }
    fn store_byte(&mut self, register: ByteRegister, source: u32) {
        let (local, shift) = byte_register(register);
        self.c()
            .local_get(local)
            .i32_const(!(255 << shift))
            .i32_and()
            .local_get(source)
            .i32_const(255)
            .i32_and()
            .i32_const(shift)
            .i32_shl()
            .i32_or()
            .local_set(local);
    }
    fn word(&mut self, value: WordValue, destination: u32) {
        match value {
            WordValue::Register(register) => {
                self.c()
                    .local_get(reg(register))
                    .i32_const(0xffff)
                    .i32_and()
                    .local_set(destination);
            }
            WordValue::Immediate(value) => {
                self.c().i32_const(i32::from(value)).local_set(destination);
            }
        }
    }
    fn store_word(&mut self, register: Register32, source: u32) {
        self.c()
            .local_get(reg(register))
            .i32_const(!0xffff)
            .i32_and()
            .local_get(source)
            .i32_const(0xffff)
            .i32_and()
            .i32_or()
            .local_set(reg(register));
    }
    fn extend(&mut self, kind: ExtensionKind, width: SmallWidth) {
        if kind == ExtensionKind::Sign {
            let shift = if width == SmallWidth::Byte { 24 } else { 16 };
            self.c()
                .local_get(R)
                .i32_const(shift)
                .i32_shl()
                .i32_const(shift)
                .i32_shr_s()
                .local_set(R);
        }
    }
    fn location(&mut self, location: Location32, destination: u32, modify: bool) {
        match location {
            Location32::Register(register) => {
                self.c().local_get(reg(register)).local_set(destination);
            }
            Location32::Memory(address) => {
                self.address(address);
                self.c().local_get(ADDRESS).local_set(DESTINATION);
                self.read(if modify { 0x104 } else { 4 }, destination);
            }
        }
    }
    fn store(&mut self, location: Location32, source: u32) {
        match location {
            Location32::Register(register) => {
                self.c().local_get(source).local_set(reg(register));
            }
            Location32::Memory(_) => {
                self.c().local_get(DESTINATION).local_set(ADDRESS);
                self.write(4, source);
            }
        }
    }
    fn target(&mut self, target: BranchTarget) {
        match target {
            BranchTarget::Direct(target) => {
                self.c().i32_const(target.0 as i32).local_set(TARGET);
            }
            BranchTarget::Indirect(location) => self.location(location, TARGET, false),
        }
    }
    fn stack(&mut self, displacement: i32) {
        self.c()
            .local_get(reg(Register32::Esp))
            .i32_const(displacement)
            .i32_add()
            .local_tee(OFFSET)
            .local_set(ADDRESS)
            .i32_const(SegmentRegister::Ss.index() as i32)
            .local_set(SEGMENT);
    }
    fn segment(&mut self, width: u32, write: bool) {
        self.c()
            .local_get(0)
            .i32_load(mem(152))
            .i32_const(1)
            .local_get(SEGMENT)
            .i32_shl()
            .i32_and()
            .i32_eqz()
            .local_get(OFFSET)
            .i32_const((u32::MAX - (width - 1)) as i32)
            .i32_gt_u()
            .i32_or();
        if write {
            self.c()
                .local_get(SEGMENT)
                .i32_const(SegmentRegister::Cs.index() as i32)
                .i32_eq()
                .i32_or();
        }
        self.c()
            .if_(BlockType::Empty)
            .local_get(0)
            .i32_const(0)
            .i32_store(mem(60))
            .local_get(0)
            .i32_const(0)
            .i32_store(mem(64))
            .local_get(0)
            .local_get(SEGMENT)
            .i32_const(SegmentRegister::Ss.index() as i32)
            .i32_eq()
            .if_(BlockType::Result(ValType::I32))
            .i32_const(12)
            .else_()
            .i32_const(13)
            .end()
            .i32_store(mem(68));
        self.exit(9);
        self.c().end();
    }
    fn push(&mut self, source: u32) {
        self.stack(-4);
        self.write(4, source);
        self.c()
            .local_get(reg(Register32::Esp))
            .i32_const(4)
            .i32_sub()
            .local_set(reg(Register32::Esp));
    }
    fn instruction(&mut self, operation: Operation, next: u32) -> bool {
        self.prepare_flags(operation);
        let mut dynamic = false;
        match operation {
            Operation::Nop => {}
            Operation::Move {
                destination,
                source,
            } => {
                self.value(source, R);
                if let Location32::Memory(address) = destination {
                    self.address(address);
                    self.c().local_get(ADDRESS).local_set(DESTINATION);
                }
                self.store(destination, R);
            }
            Operation::MoveByte {
                destination,
                source,
            } => {
                self.byte(source, R);
                self.store_byte(destination, R);
            }
            Operation::MoveWord {
                destination,
                source,
            } => {
                self.word(source, R);
                self.store_word(destination, R);
            }
            Operation::LoadByte {
                destination,
                address,
            } => {
                self.address(address);
                self.read(1, R);
                self.store_byte(destination, R);
            }
            Operation::LoadWord {
                destination,
                address,
            } => {
                self.address(address);
                self.read(2, R);
                self.store_word(destination, R);
            }
            Operation::StoreByte { address, source } => {
                self.byte(source, R);
                self.address(address);
                self.write(1, R);
            }
            Operation::StoreWord { address, source } => {
                self.word(source, R);
                self.address(address);
                self.write(2, R);
            }
            Operation::SetByte {
                condition,
                destination,
            } => {
                self.condition(condition);
                self.c().local_set(R);
                self.store_byte(destination, R);
            }
            Operation::MemorySetByte { condition, address } => {
                self.condition(condition);
                self.c().local_set(R);
                self.address(address);
                self.write(1, R);
            }
            Operation::ConditionalMove {
                condition,
                destination,
                source,
            } => {
                self.condition(condition);
                self.c()
                    .if_(BlockType::Empty)
                    .local_get(reg(source))
                    .local_set(reg(destination))
                    .end();
            }
            Operation::ConditionalMoveWord {
                condition,
                destination,
                source,
            } => {
                self.condition(condition);
                self.c().if_(BlockType::Empty);
                self.word(WordValue::Register(source), R);
                self.store_word(destination, R);
                self.c().end();
            }
            Operation::ReadConditionalMove {
                condition,
                destination,
                address,
            } => {
                self.address(address);
                self.read(4, R);
                self.condition(condition);
                self.c()
                    .if_(BlockType::Empty)
                    .local_get(R)
                    .local_set(reg(destination))
                    .end();
            }
            Operation::ReadConditionalMoveWord {
                condition,
                destination,
                address,
            } => {
                self.address(address);
                self.read(2, R);
                self.condition(condition);
                self.c().if_(BlockType::Empty);
                self.store_word(destination, R);
                self.c().end();
            }
            Operation::Extend {
                kind,
                destination,
                source,
            } => {
                let width = match source {
                    SmallSource::Register {
                        register,
                        width,
                        high_byte,
                    } => {
                        self.c().local_get(reg(register));
                        if high_byte {
                            self.c().i32_const(8).i32_shr_u();
                        }
                        self.c()
                            .i32_const(if width == SmallWidth::Byte {
                                255
                            } else {
                                0xffff
                            })
                            .i32_and()
                            .local_set(R);
                        width
                    }
                    SmallSource::Memory { address, width } => {
                        self.address(address);
                        self.read(if width == SmallWidth::Byte { 1 } else { 2 }, R);
                        width
                    }
                };
                self.extend(kind, width);
                self.c().local_get(R).local_set(reg(destination));
            }
            Operation::ExtendByteToWord {
                kind,
                destination,
                source,
            } => {
                self.byte(ByteValue::Register(source), R);
                self.extend(kind, SmallWidth::Byte);
                self.store_word(destination, R);
            }
            Operation::ReadExtendByteToWord {
                kind,
                destination,
                address,
            } => {
                self.address(address);
                self.read(1, R);
                self.extend(kind, SmallWidth::Byte);
                self.store_word(destination, R);
            }
            Operation::SignExtendAccumulatorWord => {
                self.c()
                    .local_get(reg(Register32::Eax))
                    .i32_const(24)
                    .i32_shl()
                    .i32_const(24)
                    .i32_shr_s()
                    .local_set(R);
                self.store_word(Register32::Eax, R);
            }
            Operation::SignExtendHighWord => {
                self.c()
                    .local_get(reg(Register32::Eax))
                    .i32_const(16)
                    .i32_shl()
                    .i32_const(31)
                    .i32_shr_s()
                    .local_set(R);
                self.store_word(Register32::Edx, R);
            }
            Operation::SignExtendHigh => {
                self.c()
                    .local_get(reg(Register32::Eax))
                    .i32_const(31)
                    .i32_shr_s()
                    .local_set(reg(Register32::Edx));
            }
            Operation::Lea {
                destination,
                address,
            } => {
                self.offset(address);
                self.c().local_set(reg(destination));
            }
            Operation::LeaWord {
                destination,
                address,
            } => {
                self.offset(address);
                self.c().local_set(R);
                self.store_word(destination, R);
            }
            Operation::Exchange { left, right } => {
                self.c()
                    .local_get(reg(left))
                    .local_set(A)
                    .local_get(reg(right))
                    .local_set(reg(left))
                    .local_get(A)
                    .local_set(reg(right));
            }
            Operation::Binary {
                kind,
                destination,
                source,
            } => {
                self.location(
                    destination,
                    A,
                    !matches!(kind, BinaryKind::Cmp | BinaryKind::Test),
                );
                self.value(source, B);
                self.binary(kind);
                if !matches!(kind, BinaryKind::Cmp | BinaryKind::Test) {
                    if matches!(destination, Location32::Memory(_))
                        && matches!(source, Value32::Register(_) | Value32::Immediate(_))
                    {
                        memory::write_after_modify(self, 4, R);
                    } else {
                        self.store(destination, R);
                    }
                }
                self.flags(kind, 32, false);
            }
            Operation::Unary { kind, destination } => {
                self.location(destination, A, true);
                match kind {
                    UnaryKind::Not => {
                        self.c().local_get(A).i32_const(-1).i32_xor().local_set(R);
                        self.store(destination, R);
                    }
                    UnaryKind::Inc | UnaryKind::Dec => {
                        self.c().i32_const(1).local_set(B);
                        let binary = if kind == UnaryKind::Inc {
                            BinaryKind::Add
                        } else {
                            BinaryKind::Sub
                        };
                        self.binary(binary);
                        self.store(destination, R);
                        self.flags(binary, 32, true);
                    }
                    UnaryKind::Neg => {
                        self.c().local_get(A).local_set(B).i32_const(0).local_set(A);
                        self.binary(BinaryKind::Sub);
                        self.store(destination, R);
                        self.flags(BinaryKind::Sub, 32, false);
                    }
                }
            }
            Operation::Shift {
                kind,
                destination,
                count,
            } => {
                self.location(destination, A, true);
                self.count(count);
                self.c().local_get(A).local_get(B);
                match kind {
                    ShiftKind::Shl => {
                        self.c().i32_shl();
                    }
                    ShiftKind::Shr => {
                        self.c().i32_shr_u();
                    }
                    ShiftKind::Sar => {
                        self.c().i32_shr_s();
                    }
                }
                self.c().local_set(R);
                self.store(destination, R);
                self.c().local_get(B).if_(BlockType::Empty);
                self.shift_flags(kind);
                self.c().end();
            }
            Operation::Push { source } => {
                self.value(source, R);
                self.push(R);
            }
            Operation::PushFlags => {
                self.c()
                    .local_get(FLAGS)
                    .i32_const(!0x30000)
                    .i32_and()
                    .local_set(R);
                self.push(R);
            }
            Operation::Pop { destination } => {
                self.stack(0);
                self.read(4, R);
                if let Location32::Memory(mut address) = destination {
                    if address.base == Some(Register32::Esp) {
                        address.displacement = address.displacement.wrapping_add(4);
                    }
                    self.address(address);
                    self.write(4, R);
                }
                self.c()
                    .local_get(reg(Register32::Esp))
                    .i32_const(4)
                    .i32_add()
                    .local_set(reg(Register32::Esp));
                if let Location32::Register(register) = destination {
                    self.c().local_get(R).local_set(reg(register));
                }
            }
            Operation::Call { target } => {
                self.target(target);
                self.c().i32_const(next as i32).local_set(R);
                self.push(R);
                self.c().local_get(TARGET).local_set(PC);
                dynamic = true;
            }
            Operation::Return { stack_adjust } => {
                self.stack(0);
                self.read(4, TARGET);
                self.c()
                    .local_get(reg(Register32::Esp))
                    .i32_const(4 + i32::from(stack_adjust))
                    .i32_add()
                    .local_set(reg(Register32::Esp))
                    .local_get(TARGET)
                    .local_set(PC);
                dynamic = true;
            }
            Operation::Leave => {
                self.c()
                    .local_get(reg(Register32::Ebp))
                    .local_tee(OFFSET)
                    .local_set(ADDRESS)
                    .i32_const(SegmentRegister::Ss.index() as i32)
                    .local_set(SEGMENT);
                self.read(4, R);
                self.c()
                    .local_get(reg(Register32::Ebp))
                    .i32_const(4)
                    .i32_add()
                    .local_set(reg(Register32::Esp))
                    .local_get(R)
                    .local_set(reg(Register32::Ebp));
            }
            Operation::Jump { target } => {
                self.target(target);
                self.c().local_get(TARGET).local_set(PC);
                dynamic = true;
            }
            Operation::ConditionalJump { condition, target } => {
                self.condition(condition);
                self.c()
                    .if_(BlockType::Result(ValType::I32))
                    .i32_const(target.0 as i32)
                    .else_()
                    .i32_const(next as i32)
                    .end()
                    .local_set(PC);
                dynamic = true;
            }
            Operation::CountBranch { kind, target } => {
                if kind != CountBranchKind::EcxZero {
                    self.c()
                        .local_get(reg(Register32::Ecx))
                        .i32_const(1)
                        .i32_sub()
                        .local_set(reg(Register32::Ecx));
                }
                self.c().local_get(reg(Register32::Ecx)).i32_eqz();
                if kind != CountBranchKind::EcxZero {
                    self.c().i32_eqz();
                }
                if kind == CountBranchKind::LoopEqual {
                    self.bit(6);
                    self.c().i32_and();
                }
                if kind == CountBranchKind::LoopNotEqual {
                    self.bit(6);
                    self.c().i32_eqz().i32_and();
                }
                self.c()
                    .if_(BlockType::Result(ValType::I32))
                    .i32_const(target.0 as i32)
                    .else_()
                    .i32_const(next as i32)
                    .end()
                    .local_set(PC);
                dynamic = true;
            }
            Operation::Carry { kind } => {
                self.c().local_get(FLAGS);
                match kind {
                    CarryKind::Clear => {
                        self.c().i32_const(!1).i32_and();
                    }
                    CarryKind::Set => {
                        self.c().i32_const(1).i32_or();
                    }
                    CarryKind::Complement => {
                        self.c().i32_const(1).i32_xor();
                    }
                }
                self.c().local_set(FLAGS);
            }
            Operation::Direction { set } => {
                self.c()
                    .local_get(FLAGS)
                    .i32_const(if set { 0x400 } else { !0x400 });
                if set {
                    self.c().i32_or();
                } else {
                    self.c().i32_and();
                }
                self.c().local_set(FLAGS);
            }
            _ => {
                if !narrow::emit(self, operation)
                    && !multiply::emit(self, operation)
                    && !string::emit(self, operation)
                    && !bit::emit(self, operation)
                    && !exchange::emit(self, operation)
                    && !flags::emit(self, operation)
                    && !shift::emit(self, operation)
                    && !rotate::emit(self, operation)
                    && !simd::emit(self, operation)
                    && !x87::emit(self, operation)
                    && !x87_compare::emit(self, operation)
                    && !x87_multiply::emit(self, operation)
                    && !x87_arithmetic::emit(self, operation)
                {
                    return false;
                }
            }
        }
        if !dynamic {
            self.c().i32_const(next as i32).local_set(PC);
        }
        true
    }
    fn binary(&mut self, kind: BinaryKind) {
        if matches!(kind, BinaryKind::Adc | BinaryKind::Sbb) {
            self.bit(0);
            self.c().local_set(CARRY);
        }
        self.c().local_get(A).local_get(B);
        match kind {
            BinaryKind::Add | BinaryKind::Adc => {
                self.c().i32_add();
                if kind == BinaryKind::Adc {
                    self.c().local_get(CARRY).i32_add();
                }
            }
            BinaryKind::Sub | BinaryKind::Sbb | BinaryKind::Cmp => {
                self.c().i32_sub();
                if kind == BinaryKind::Sbb {
                    self.c().local_get(CARRY).i32_sub();
                }
            }
            BinaryKind::And | BinaryKind::Test => {
                self.c().i32_and();
            }
            BinaryKind::Or => {
                self.c().i32_or();
            }
            BinaryKind::Xor => {
                self.c().i32_xor();
            }
        }
        self.c().local_set(R);
    }
    fn count(&mut self, count: ShiftCount) {
        match count {
            ShiftCount::Immediate(count) => {
                self.c().i32_const(i32::from(count & 31)).local_set(B);
            }
            ShiftCount::Cl => {
                self.c()
                    .local_get(reg(Register32::Ecx))
                    .i32_const(31)
                    .i32_and()
                    .local_set(B);
            }
        }
    }
    fn shift_flags(&mut self, kind: ShiftKind) {
        self.c().local_get(A);
        if kind == ShiftKind::Shl {
            self.c().i32_const(32).local_get(B).i32_sub();
        } else {
            self.c().local_get(B).i32_const(1).i32_sub();
        }
        self.c()
            .i32_shr_u()
            .i32_const(1)
            .i32_and()
            .local_set(CARRY)
            .local_get(FLAGS)
            .i32_const(!0x8d5)
            .i32_and()
            .local_get(CARRY)
            .i32_or()
            .local_get(R)
            .i32_eqz()
            .i32_const(6)
            .i32_shl()
            .i32_or()
            .local_get(R)
            .i32_const(31)
            .i32_shr_u()
            .i32_const(7)
            .i32_shl()
            .i32_or()
            .local_get(R)
            .i32_const(255)
            .i32_and()
            .i32_popcnt()
            .i32_const(1)
            .i32_and()
            .i32_eqz()
            .i32_const(2)
            .i32_shl()
            .i32_or()
            .local_set(FLAGS)
            .local_get(B)
            .i32_const(1)
            .i32_eq()
            .if_(BlockType::Empty)
            .local_get(FLAGS)
            .i32_const(!0x800)
            .i32_and();
        match kind {
            ShiftKind::Shl => {
                self.c()
                    .local_get(R)
                    .i32_const(31)
                    .i32_shr_u()
                    .local_get(CARRY)
                    .i32_xor();
            }
            ShiftKind::Shr => {
                self.c().local_get(A).i32_const(31).i32_shr_u();
            }
            ShiftKind::Sar => {
                self.c().i32_const(0);
            }
        }
        self.c()
            .i32_const(11)
            .i32_shl()
            .i32_or()
            .local_set(FLAGS)
            .end();
    }
    fn bit(&mut self, bit: u32) {
        self.deferred_bit(bit);
    }
    fn condition(&mut self, condition: Condition) {
        match condition {
            Condition::Overflow | Condition::NotOverflow => self.bit(11),
            Condition::Below | Condition::AboveOrEqual => self.bit(0),
            Condition::Equal | Condition::NotEqual => self.bit(6),
            Condition::BelowOrEqual | Condition::Above => {
                self.bit(0);
                self.bit(6);
                self.c().i32_or();
            }
            Condition::Sign | Condition::NotSign => self.bit(7),
            Condition::Parity | Condition::NotParity => self.bit(2),
            Condition::Less | Condition::GreaterOrEqual => {
                self.bit(7);
                self.bit(11);
                self.c().i32_xor();
            }
            Condition::LessOrEqual | Condition::Greater => {
                self.bit(7);
                self.bit(11);
                self.c().i32_xor();
                self.bit(6);
                self.c().i32_or();
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
}

fn byte_register(register: ByteRegister) -> (u32, i32) {
    match register {
        ByteRegister::Al => (1, 0),
        ByteRegister::Cl => (2, 0),
        ByteRegister::Dl => (3, 0),
        ByteRegister::Bl => (4, 0),
        ByteRegister::Ah => (1, 8),
        ByteRegister::Ch => (2, 8),
        ByteRegister::Dh => (3, 8),
        ByteRegister::Bh => (4, 8),
    }
}
