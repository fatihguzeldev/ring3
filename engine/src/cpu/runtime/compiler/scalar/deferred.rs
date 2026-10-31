use super::*;

const PENDING: u32 = 72;
const LEFT: u32 = 73;
const RIGHT: u32 = 74;
const RESULT: u32 = 75;
const INPUT_CARRY: u32 = 76;

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
        if narrow::supports(&operation)
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
            self.materialize_flags();
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
        for (source, destination) in [(A, LEFT), (B, RIGHT), (R, RESULT), (CARRY, INPUT_CARRY)] {
            self.c().local_get(source).local_set(destination);
        }
        self.c()
            .i32_const((width | (mode << 6)) as i32)
            .local_set(PENDING);
    }

    pub(super) fn deferred_bit(&mut self, bit: u32) {
        if matches!(bit, 2 | 6 | 7) {
            self.c()
                .local_get(PENDING)
                .if_(BlockType::Result(ValType::I32))
                .local_get(RESULT);
            match bit {
                2 => {
                    self.c()
                        .i32_const(255)
                        .i32_and()
                        .i32_popcnt()
                        .i32_const(1)
                        .i32_and()
                        .i32_eqz();
                }
                6 => {
                    self.c().i32_eqz();
                }
                7 => {
                    self.c()
                        .local_get(PENDING)
                        .i32_const(63)
                        .i32_and()
                        .i32_const(1)
                        .i32_sub()
                        .i32_shr_u()
                        .i32_const(1)
                        .i32_and();
                }
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
