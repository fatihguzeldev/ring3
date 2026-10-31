use super::*;
use crate::cpu::x86::{
    X87Binary, X87Format, X87Operation,
    simd::{FloatKind, SimdKind},
};

const PENDING: u32 = 72;

fn numeric_preserves_flags(operation: Operation) -> bool {
    match operation {
        Operation::X87 {
            operation: numeric, ..
        } => {
            let preserving = matches!(
                numeric,
                X87Operation::Wait
                    | X87Operation::Load(X87Format::Float32 | X87Format::Float64)
                    | X87Operation::Store {
                        format: X87Format::Float32 | X87Format::Float64,
                        truncate: false,
                        ..
                    }
                    | X87Operation::LoadConstant { index: 0 | 6 }
                    | X87Operation::LoadRegister { .. }
                    | X87Operation::StoreRegister { .. }
                    | X87Operation::Exchange { .. }
                    | X87Operation::ChangeSign
                    | X87Operation::LoadControl
                    | X87Operation::StoreControl
                    | X87Operation::StoreStatus
                    | X87Operation::StatusToAx
                    | X87Operation::Compare {
                        integer_flags: false,
                        ..
                    }
                    | X87Operation::CompareMemory {
                        format: X87Format::Float32 | X87Format::Float64,
                        ..
                    }
                    | X87Operation::Binary {
                        kind: X87Binary::Add
                            | X87Binary::Subtract
                            | X87Binary::ReverseSubtract
                            | X87Binary::Multiply
                            | X87Binary::Divide,
                        ..
                    }
                    | X87Operation::BinaryMemory {
                        kind: X87Binary::Add
                            | X87Binary::Subtract
                            | X87Binary::ReverseSubtract
                            | X87Binary::Multiply
                            | X87Binary::Divide,
                        format: X87Format::Float32 | X87Format::Float64,
                    }
            );
            preserving
                && (x87::supports(&operation)
                    || x87_compare::supports(&operation)
                    || x87_arithmetic::supports(&operation)
                    || x87_multiply::supports(&operation))
        }
        Operation::X87StatusToAx
        | Operation::X87StatusToMemory { .. }
        | Operation::X87ControlToMemory { .. } => true,
        Operation::Simd(numeric) => {
            matches!(
                numeric.kind,
                SimdKind::Move | SimdKind::Float(FloatKind::Add)
            ) && simd::supports(&operation)
        }
        _ => false,
    }
}

pub(super) fn materializer() -> Function {
    let mut function = Function::new([(3, ValType::I32), (1, ValType::I64)]);
    let mut c = function.instructions();
    c.local_get(5).i32_const(63).i32_and().local_set(7);
    c.local_get(5).i32_const(6).i32_shr_u().local_set(8);
    c.local_get(0)
        .local_get(8)
        .i32_const(8)
        .i32_and()
        .if_(BlockType::Result(ValType::I32))
        .i32_const(!0x8d4)
        .else_()
        .i32_const(!0x8d5)
        .end()
        .i32_and()
        .local_get(3)
        .i32_eqz()
        .i32_const(6)
        .i32_shl()
        .i32_or()
        .local_get(3)
        .local_get(7)
        .i32_const(1)
        .i32_sub()
        .i32_shr_u()
        .i32_const(7)
        .i32_shl()
        .i32_or()
        .local_get(3)
        .i32_const(255)
        .i32_and()
        .i32_popcnt()
        .i32_const(1)
        .i32_and()
        .i32_eqz()
        .i32_const(2)
        .i32_shl()
        .i32_or()
        .local_set(6);
    c.local_get(8)
        .i32_const(1)
        .i32_and()
        .i32_eqz()
        .if_(BlockType::Empty);
    c.local_get(6)
        .local_get(1)
        .local_get(2)
        .i32_xor()
        .local_get(3)
        .i32_xor()
        .i32_const(16)
        .i32_and()
        .i32_or();
    c.local_get(8)
        .i32_const(2)
        .i32_and()
        .if_(BlockType::Result(ValType::I32));
    c.local_get(1)
        .local_get(2)
        .i32_xor()
        .local_get(1)
        .local_get(3)
        .i32_xor()
        .i32_and();
    c.else_()
        .local_get(1)
        .local_get(3)
        .i32_xor()
        .local_get(2)
        .local_get(3)
        .i32_xor()
        .i32_and()
        .end();
    c.local_get(7)
        .i32_const(1)
        .i32_sub()
        .i32_shr_u()
        .i32_const(11)
        .i32_shl()
        .i32_or()
        .local_set(6);
    c.local_get(8)
        .i32_const(8)
        .i32_and()
        .i32_eqz()
        .if_(BlockType::Empty);
    c.local_get(2).i64_extend_i32_u();
    c.local_get(8)
        .i32_const(4)
        .i32_and()
        .if_(BlockType::Result(ValType::I64))
        .local_get(4)
        .i64_extend_i32_u()
        .else_()
        .i64_const(0)
        .end()
        .i64_add()
        .local_set(9);
    c.local_get(6)
        .local_get(8)
        .i32_const(2)
        .i32_and()
        .if_(BlockType::Result(ValType::I32));
    c.local_get(1).i64_extend_i32_u().local_get(9).i64_lt_u();
    c.else_()
        .local_get(1)
        .i64_extend_i32_u()
        .local_get(9)
        .i64_add()
        .local_get(7)
        .i64_extend_i32_u()
        .i64_shr_u()
        .i32_wrap_i64()
        .end()
        .i32_or()
        .local_set(6);
    c.end().end().local_get(6).i32_const(2).i32_or().end();
    function
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Operand {
    Immediate(u32),
    Register(Register32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Recipe {
    Logical,
    Arithmetic { kind: BinaryKind, right: Operand },
    Negate,
    Increment { decrement: bool },
}

fn descriptor(kind: BinaryKind, width: u32, preserve: bool) -> i32 {
    let mode = u32::from(matches!(
        kind,
        BinaryKind::And | BinaryKind::Or | BinaryKind::Xor | BinaryKind::Test
    )) | (u32::from(matches!(
        kind,
        BinaryKind::Sub | BinaryKind::Sbb | BinaryKind::Cmp
    )) << 1)
        | (u32::from(matches!(kind, BinaryKind::Adc | BinaryKind::Sbb)) << 2)
        | (u32::from(preserve) << 3);
    (width | mode << 6) as i32
}

impl Recipe {
    pub(super) fn register(self) -> Option<Register32> {
        match self {
            Self::Arithmetic {
                right: Operand::Register(register),
                ..
            } => Some(register),
            _ => None,
        }
    }

    fn carry_input(self) -> bool {
        matches!(
            self,
            Self::Arithmetic {
                kind: BinaryKind::Adc | BinaryKind::Sbb,
                ..
            }
        )
    }

    fn subtract(self) -> bool {
        matches!(
            self,
            Self::Negate
                | Self::Increment { decrement: true }
                | Self::Arithmetic {
                    kind: BinaryKind::Sub | BinaryKind::Sbb | BinaryKind::Cmp,
                    ..
                }
        )
    }

    fn descriptor(self) -> i32 {
        match self {
            Self::Logical => descriptor(BinaryKind::And, 32, false),
            Self::Arithmetic { kind, .. } => descriptor(kind, 32, false),
            Self::Negate => descriptor(BinaryKind::Sub, 32, false),
            Self::Increment { decrement } => descriptor(
                if decrement {
                    BinaryKind::Sub
                } else {
                    BinaryKind::Add
                },
                32,
                true,
            ),
        }
    }

    fn right(self, out: &mut Output) {
        match self {
            Self::Logical => {
                out.c().i32_const(0);
            }
            Self::Arithmetic {
                right: Operand::Immediate(value),
                ..
            } => {
                out.c().i32_const(value as i32);
            }
            Self::Arithmetic {
                right: Operand::Register(register),
                ..
            } => {
                out.c().local_get(reg(register));
            }
            Self::Negate => {
                out.c()
                    .i32_const(0)
                    .local_get(PENDING)
                    .i32_wrap_i64()
                    .i32_sub();
            }
            Self::Increment { .. } => {
                out.c().i32_const(1);
            }
        }
    }

    fn input(self, out: &mut Output) {
        if self.carry_input() {
            out.c()
                .local_get(PENDING)
                .i64_const(33)
                .i64_shr_u()
                .i32_wrap_i64()
                .i32_const(1)
                .i32_and();
        } else {
            out.c().i32_const(0);
        }
    }

    fn left(self, out: &mut Output) {
        if matches!(self, Self::Logical | Self::Negate) {
            out.c().i32_const(0);
            return;
        }
        out.c().local_get(PENDING).i32_wrap_i64();
        self.right(out);
        if self.subtract() {
            out.c().i32_add();
        } else {
            out.c().i32_sub();
        }
        if self.carry_input() {
            self.input(out);
            if self.subtract() {
                out.c().i32_add();
            } else {
                out.c().i32_sub();
            }
        }
    }

    fn materialize(self, out: &mut Output) {
        out.c().local_get(FLAGS);
        self.left(out);
        self.right(out);
        out.c().local_get(PENDING).i32_wrap_i64();
        self.input(out);
        out.c()
            .i32_const(self.descriptor())
            .call(3)
            .local_set(FLAGS);
    }

    fn materialize_inline(self, out: &mut Output) {
        let preserve = matches!(self, Self::Increment { .. });
        out.c()
            .local_get(FLAGS)
            .i32_const(if preserve { !0x8d4 } else { !0x8d5 })
            .i32_and();
        for bit in [6, 7, 2] {
            self.bit(out, bit);
            out.c().i32_const(bit as i32).i32_shl().i32_or();
        }
        if !matches!(self, Self::Logical) {
            self.left(out);
            self.right(out);
            out.c()
                .i32_xor()
                .local_get(PENDING)
                .i32_wrap_i64()
                .i32_xor()
                .i32_const(16)
                .i32_and()
                .i32_or();
            self.bit(out, 11);
            out.c().i32_const(11).i32_shl().i32_or();
            if !preserve {
                self.bit(out, 0);
                out.c().i32_or();
            }
        }
        out.c().i32_const(2).i32_or().local_set(FLAGS);
    }

    fn bit(self, out: &mut Output, bit: u32) {
        match bit {
            0 => match self {
                Self::Logical => {
                    out.c().i32_const(0);
                }
                Self::Increment { .. } => {
                    out.c().local_get(FLAGS).i32_const(1).i32_and();
                }
                Self::Negate => {
                    out.c()
                        .local_get(PENDING)
                        .i32_wrap_i64()
                        .i32_eqz()
                        .i32_eqz();
                }
                Self::Arithmetic { .. } => {
                    out.c().local_get(PENDING).i32_wrap_i64().i64_extend_i32_u();
                    self.right(out);
                    out.c().i64_extend_i32_u();
                    if self.carry_input() {
                        self.input(out);
                        out.c().i64_extend_i32_u().i64_add();
                    }
                    if self.subtract() {
                        out.c().i64_add().i64_const(32).i64_shr_u().i32_wrap_i64();
                    } else {
                        out.c().i64_lt_u();
                    }
                }
            },
            2 => {
                out.c()
                    .local_get(PENDING)
                    .i32_wrap_i64()
                    .i32_const(255)
                    .i32_and()
                    .i32_popcnt()
                    .i32_const(1)
                    .i32_and()
                    .i32_eqz();
            }
            6 => {
                out.c().local_get(PENDING).i32_wrap_i64().i32_eqz();
            }
            7 => {
                out.c()
                    .local_get(PENDING)
                    .i32_wrap_i64()
                    .i32_const(31)
                    .i32_shr_u();
            }
            11 => {
                if matches!(self, Self::Logical) {
                    out.c().i32_const(0);
                } else {
                    self.left(out);
                    if self.subtract() {
                        self.right(out);
                    } else {
                        out.c().local_get(PENDING).i32_wrap_i64();
                    }
                    out.c().i32_xor();
                    if self.subtract() {
                        self.left(out);
                    } else {
                        self.right(out);
                    }
                    out.c()
                        .local_get(PENDING)
                        .i32_wrap_i64()
                        .i32_xor()
                        .i32_and()
                        .i32_const(31)
                        .i32_shr_u();
                }
            }
            _ => unreachable!(),
        }
    }
}

pub(super) fn full_producer(operation: Operation) -> bool {
    matches!(
        operation,
        Operation::Binary { .. }
            | Operation::ArithmeticWord { .. }
            | Operation::ReadArithmeticWord { .. }
            | Operation::MemoryArithmeticWord { .. }
            | Operation::LogicalWord { .. }
            | Operation::ReadLogicalWord { .. }
            | Operation::MemoryLogicalWord { .. }
            | Operation::CompareWord { .. }
            | Operation::TestWord { .. }
            | Operation::MemoryCompareWord { .. }
            | Operation::ReadCompareWord { .. }
            | Operation::MemoryTestWord { .. }
            | Operation::ArithmeticByte { .. }
            | Operation::ReadArithmeticByte { .. }
            | Operation::MemoryArithmeticByte { .. }
            | Operation::LogicalByte { .. }
            | Operation::ReadLogicalByte { .. }
            | Operation::MemoryLogicalByte { .. }
            | Operation::CompareByte { .. }
            | Operation::ReadCompareByte { .. }
            | Operation::TestByte { .. }
            | Operation::MemoryPredicateByte { .. }
            | Operation::UnaryByte {
                kind: UnaryKind::Neg,
                ..
            }
            | Operation::MemoryUnaryByte {
                kind: UnaryKind::Neg,
                ..
            }
            | Operation::UnaryWord {
                kind: UnaryKind::Neg,
                ..
            }
            | Operation::MemoryUnaryWord {
                kind: UnaryKind::Neg,
                ..
            }
            | Operation::Unary {
                kind: UnaryKind::Neg,
                ..
            }
    )
}

#[derive(Clone, Copy)]
pub(super) enum Preparation {
    Keep,
    Canonical,
    CarryOnly,
    OverflowOnly,
}

pub(super) fn preparation(operation: Operation, recipe: Option<Recipe>) -> Preparation {
    match operation {
        _ if full_producer(operation) => Preparation::Keep,
        Operation::Unary {
            kind: UnaryKind::Inc | UnaryKind::Dec,
            destination: Location32::Register(_),
        } => Preparation::CarryOnly,
        Operation::AhToFlags => Preparation::OverflowOnly,
        _ if flags_flow::preserves(
            operation,
            recipe.and_then(Recipe::register),
            numeric_preserves_flags(operation),
        ) =>
        {
            Preparation::Keep
        }
        _ => Preparation::Canonical,
    }
}

pub(super) fn recipe_for_flags(
    operation: Operation,
    kind: BinaryKind,
    width: u32,
) -> Option<Recipe> {
    if width == 32 {
        match operation {
            Operation::Binary {
                kind: BinaryKind::And | BinaryKind::Or | BinaryKind::Xor | BinaryKind::Test,
                ..
            } => Some(Recipe::Logical),
            Operation::Binary {
                destination,
                source,
                ..
            } => {
                let right = match source {
                    Value32::Immediate(value) => Some(Operand::Immediate(value)),
                    Value32::Register(register) => Some(Operand::Register(register)),
                    _ => None,
                };
                right.filter(|right| !matches!((destination, right), (Location32::Register(left), Operand::Register(right)) if left == *right && kind != BinaryKind::Cmp))
                        .map(|right| Recipe::Arithmetic { kind, right })
            }
            Operation::Unary {
                kind: UnaryKind::Neg,
                ..
            } => Some(Recipe::Negate),
            Operation::Unary {
                kind: UnaryKind::Inc | UnaryKind::Dec,
                ..
            } => Some(Recipe::Increment {
                decrement: kind == BinaryKind::Sub,
            }),
            _ => None,
        }
    } else {
        None
    }
}

pub(super) fn transfer(operation: Operation, incoming: Option<Recipe>) -> Option<Recipe> {
    let prepared = match preparation(operation, incoming) {
        Preparation::Keep => incoming,
        _ => None,
    };
    match operation {
        Operation::Binary { kind, .. } => recipe_for_flags(operation, kind, 32),
        Operation::Unary {
            kind: UnaryKind::Neg,
            ..
        } => recipe_for_flags(operation, BinaryKind::Sub, 32),
        Operation::Unary {
            kind: UnaryKind::Inc,
            ..
        } => recipe_for_flags(operation, BinaryKind::Add, 32),
        Operation::Unary {
            kind: UnaryKind::Dec,
            ..
        } => recipe_for_flags(operation, BinaryKind::Sub, 32),
        _ if full_producer(operation) => None,
        _ => prepared,
    }
}

impl Output {
    pub(super) fn materialize_flags(&mut self) {
        if let Some(recipe) = self.recipe {
            self.c()
                .local_get(PENDING)
                .i64_const(0)
                .i64_ne()
                .if_(BlockType::Empty);
            recipe.materialize(self);
            self.c().i64_const(0).local_set(PENDING).end();
        }
    }

    pub(super) fn materialize_flags_inline(&mut self) {
        if let Some(recipe) = self.recipe {
            self.c()
                .local_get(PENDING)
                .i64_const(0)
                .i64_ne()
                .if_(BlockType::Empty);
            recipe.materialize_inline(self);
            self.c().i64_const(0).local_set(PENDING).end();
        }
    }

    pub(super) fn canonicalize_flags(&mut self) {
        self.materialize_flags_inline();
        self.recipe = None;
    }

    pub(super) fn capture_carry(&mut self) {
        self.c().local_get(FLAGS).i32_const(!1).i32_and();
        self.bit(0);
        self.c().i32_or().local_set(FLAGS);
    }

    pub(super) fn prepare_flags(&mut self, operation: Operation) {
        self.operation = operation;
        match preparation(operation, self.recipe) {
            Preparation::Keep => {}
            Preparation::Canonical => self.canonicalize_flags(),
            Preparation::CarryOnly => {
                self.capture_carry();
                self.c().i64_const(0).local_set(PENDING);
                self.recipe = None;
            }
            Preparation::OverflowOnly => {
                if self.recipe.is_some() {
                    self.c().local_get(FLAGS).i32_const(!0x800).i32_and();
                    self.bit(11);
                    self.c()
                        .i32_const(11)
                        .i32_shl()
                        .i32_or()
                        .local_set(FLAGS)
                        .i64_const(0)
                        .local_set(PENDING);
                    self.recipe = None;
                }
            }
        }
    }

    pub(super) fn flags(&mut self, kind: BinaryKind, width: u32, preserve: bool) {
        let recipe = recipe_for_flags(self.operation, kind, width);
        if let Some(recipe) = recipe {
            self.c().local_get(R).i64_extend_i32_u();
            if recipe.carry_input() {
                self.c()
                    .local_get(CARRY)
                    .i64_extend_i32_u()
                    .i64_const(33)
                    .i64_shl()
                    .i64_const(1_i64 << 32)
                    .i64_or();
            } else {
                self.c().i64_const(1_i64 << 32);
            }
            self.c().i64_or().local_set(PENDING);
            self.recipe = Some(recipe);
        } else {
            for local in [FLAGS, A, B, R, CARRY] {
                self.c().local_get(local);
            }
            self.c()
                .i32_const(descriptor(kind, width, preserve))
                .call(3)
                .local_set(FLAGS)
                .i64_const(0)
                .local_set(PENDING);
            self.recipe = None;
        }
    }

    pub(super) fn deferred_bit(&mut self, bit: u32) {
        if let Some(recipe) = self.recipe.filter(|_| matches!(bit, 0 | 2 | 6 | 7 | 11)) {
            self.c()
                .local_get(PENDING)
                .i64_const(0)
                .i64_ne()
                .if_(BlockType::Result(ValType::I32));
            recipe.bit(self, bit);
            self.c()
                .else_()
                .local_get(FLAGS)
                .i32_const(bit as i32)
                .i32_shr_u()
                .i32_const(1)
                .i32_and()
                .end();
        } else {
            self.materialize_flags();
            self.c()
                .local_get(FLAGS)
                .i32_const(bit as i32)
                .i32_shr_u()
                .i32_const(1)
                .i32_and();
        }
    }
}
