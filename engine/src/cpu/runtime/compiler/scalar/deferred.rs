use super::*;
use crate::cpu::x86::{
    X87Binary, X87Format, X87Operation,
    simd::{FloatKind, SimdKind},
};

const PENDING: u32 = 72;
const LEFT: u32 = 73;
const RIGHT: u32 = 74;
const RESULT: u32 = 75;
const INPUT_CARRY: u32 = 76;

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

impl Output {
    pub(super) fn materialize_flags(&mut self) {
        self.c().local_get(PENDING).if_(BlockType::Empty);
        for local in [FLAGS, LEFT, RIGHT, RESULT, INPUT_CARRY, PENDING] {
            self.c().local_get(local);
        }
        self.c()
            .call(3)
            .local_set(FLAGS)
            .i32_const(0)
            .local_set(PENDING)
            .end();
    }

    pub(super) fn prepare_flags(&mut self, operation: Operation) {
        if numeric_preserves_flags(operation)
            || narrow::supports(&operation)
            || matches!(
                operation,
                Operation::Nop
                    | Operation::Move { .. }
                    | Operation::MoveByte { .. }
                    | Operation::MoveWord { .. }
                    | Operation::LoadByte { .. }
                    | Operation::LoadWord { .. }
                    | Operation::StoreByte { .. }
                    | Operation::StoreWord { .. }
                    | Operation::Extend { .. }
                    | Operation::ExtendByteToWord { .. }
                    | Operation::ReadExtendByteToWord { .. }
                    | Operation::Lea { .. }
                    | Operation::LeaWord { .. }
                    | Operation::Exchange { .. }
                    | Operation::SignExtendAccumulatorWord
                    | Operation::SignExtendHighWord
                    | Operation::SignExtendHigh
                    | Operation::Push { .. }
                    | Operation::Pop { .. }
                    | Operation::Call { .. }
                    | Operation::Return { .. }
                    | Operation::Leave
                    | Operation::Jump { .. }
                    | Operation::ConditionalJump { .. }
                    | Operation::CountBranch { .. }
                    | Operation::SetByte { .. }
                    | Operation::MemorySetByte { .. }
                    | Operation::ConditionalMove { .. }
                    | Operation::ConditionalMoveWord { .. }
                    | Operation::ReadConditionalMove { .. }
                    | Operation::ReadConditionalMoveWord { .. }
                    | Operation::Binary { .. }
                    | Operation::Unary { .. }
                    | Operation::Direction { .. }
            )
        {
            return;
        }
        self.materialize_flags();
    }

    pub(super) fn flags(&mut self, kind: BinaryKind, width: u32, preserve_carry: bool) {
        if preserve_carry {
            self.c().local_get(FLAGS).i32_const(!1).i32_and();
            self.bit(0);
            self.c().i32_or().local_set(FLAGS);
        }
        let mode = u32::from(matches!(
            kind,
            BinaryKind::And | BinaryKind::Or | BinaryKind::Xor | BinaryKind::Test
        )) | (u32::from(matches!(
            kind,
            BinaryKind::Sub | BinaryKind::Sbb | BinaryKind::Cmp
        )) << 1)
            | (u32::from(matches!(kind, BinaryKind::Adc | BinaryKind::Sbb)) << 2)
            | (u32::from(preserve_carry) << 3);
        self.c().local_get(R).local_set(RESULT);
        if mode & 1 == 0 {
            self.c()
                .local_get(A)
                .local_set(LEFT)
                .local_get(B)
                .local_set(RIGHT);
        }
        if mode & 4 != 0 {
            self.c().local_get(CARRY).local_set(INPUT_CARRY);
        }
        self.c()
            .i32_const((width | (mode << 6)) as i32)
            .local_set(PENDING);
    }

    fn pending_carry_input(&mut self) {
        self.c()
            .local_get(PENDING)
            .i32_const(256)
            .i32_and()
            .if_(BlockType::Result(ValType::I64))
            .local_get(INPUT_CARRY)
            .i64_extend_i32_u()
            .else_()
            .i64_const(0)
            .end();
    }

    fn pending_carry(&mut self) {
        self.c()
            .local_get(PENDING)
            .i32_const(64)
            .i32_and()
            .if_(BlockType::Result(ValType::I32))
            .i32_const(0)
            .else_()
            .local_get(PENDING)
            .i32_const(512)
            .i32_and()
            .if_(BlockType::Result(ValType::I32))
            .local_get(FLAGS)
            .i32_const(1)
            .i32_and()
            .else_()
            .local_get(PENDING)
            .i32_const(128)
            .i32_and()
            .if_(BlockType::Result(ValType::I32))
            .local_get(LEFT)
            .i64_extend_i32_u()
            .local_get(RIGHT)
            .i64_extend_i32_u();
        self.pending_carry_input();
        self.c()
            .i64_add()
            .i64_lt_u()
            .else_()
            .local_get(LEFT)
            .i64_extend_i32_u()
            .local_get(RIGHT)
            .i64_extend_i32_u()
            .i64_add();
        self.pending_carry_input();
        self.c()
            .i64_add()
            .local_get(PENDING)
            .i32_const(63)
            .i32_and()
            .i64_extend_i32_u()
            .i64_shr_u()
            .i32_wrap_i64()
            .end()
            .end()
            .end();
    }

    fn pending_overflow(&mut self) {
        self.c()
            .local_get(PENDING)
            .i32_const(64)
            .i32_and()
            .if_(BlockType::Result(ValType::I32))
            .i32_const(0)
            .else_()
            .local_get(PENDING)
            .i32_const(128)
            .i32_and()
            .if_(BlockType::Result(ValType::I32))
            .local_get(LEFT)
            .local_get(RIGHT)
            .i32_xor()
            .local_get(LEFT)
            .local_get(RESULT)
            .i32_xor()
            .i32_and()
            .else_()
            .local_get(LEFT)
            .local_get(RESULT)
            .i32_xor()
            .local_get(RIGHT)
            .local_get(RESULT)
            .i32_xor()
            .i32_and()
            .end()
            .local_get(PENDING)
            .i32_const(63)
            .i32_and()
            .i32_const(1)
            .i32_sub()
            .i32_shr_u()
            .i32_const(1)
            .i32_and()
            .end();
    }

    pub(super) fn deferred_bit(&mut self, bit: u32) {
        if matches!(bit, 0 | 2 | 6 | 7 | 11) {
            self.c()
                .local_get(PENDING)
                .if_(BlockType::Result(ValType::I32));
            match bit {
                0 => self.pending_carry(),
                2 => {
                    self.c()
                        .local_get(RESULT)
                        .i32_const(255)
                        .i32_and()
                        .i32_popcnt()
                        .i32_const(1)
                        .i32_and()
                        .i32_eqz();
                }
                6 => {
                    self.c().local_get(RESULT).i32_eqz();
                }
                7 => {
                    self.c()
                        .local_get(RESULT)
                        .local_get(PENDING)
                        .i32_const(63)
                        .i32_and()
                        .i32_const(1)
                        .i32_sub()
                        .i32_shr_u()
                        .i32_const(1)
                        .i32_and();
                }
                11 => self.pending_overflow(),
                _ => unreachable!(),
            }
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
