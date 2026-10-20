use super::flags::{AF, ARITHMETIC, CF, OF, PF, SF, ZF};
use crate::cpu::x86::ir::{
    BinaryKind, BranchTarget, CarryKind, Condition, CountBranchKind, Location32, Operation,
    UnaryKind, Value32,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FlagEffects {
    pub(crate) read: u32,
    pub(crate) write: u32,
    pub(crate) undefined: u32,
}

pub(crate) fn condition_mask(condition: Condition) -> u32 {
    match condition {
        Condition::Overflow | Condition::NotOverflow => OF,
        Condition::Below | Condition::AboveOrEqual => CF,
        Condition::Equal | Condition::NotEqual => ZF,
        Condition::BelowOrEqual | Condition::Above => CF | ZF,
        Condition::Sign | Condition::NotSign => SF,
        Condition::Parity | Condition::NotParity => PF,
        Condition::Less | Condition::GreaterOrEqual => SF | OF,
        Condition::LessOrEqual | Condition::Greater => ZF | SF | OF,
    }
}

pub(crate) fn analyze(operation: Operation) -> Option<FlagEffects> {
    let (read, write, undefined) = match operation {
        Operation::Binary { kind, .. } => match kind {
            BinaryKind::Add | BinaryKind::Sub | BinaryKind::Cmp => (0, ARITHMETIC, 0),
            BinaryKind::Adc | BinaryKind::Sbb => (CF, ARITHMETIC, 0),
            BinaryKind::And | BinaryKind::Or | BinaryKind::Xor | BinaryKind::Test => {
                (0, ARITHMETIC, AF)
            }
        },
        Operation::Unary { kind, .. }
        | Operation::UnaryByte { kind, .. }
        | Operation::UnaryWord { kind, .. }
        | Operation::MemoryUnaryByte { kind, .. }
        | Operation::MemoryUnaryWord { kind, .. } => match kind {
            UnaryKind::Inc | UnaryKind::Dec => (0, ARITHMETIC & !CF, 0),
            UnaryKind::Neg => (0, ARITHMETIC, 0),
            UnaryKind::Not => (0, 0, 0),
        },
        Operation::ConditionalJump { condition, .. }
        | Operation::SetByte { condition, .. }
        | Operation::MemorySetByte { condition, .. }
        | Operation::ConditionalMove { condition, .. }
        | Operation::ConditionalMoveWord { condition, .. }
        | Operation::ReadConditionalMove { condition, .. }
        | Operation::ReadConditionalMoveWord { condition, .. } => (condition_mask(condition), 0, 0),
        Operation::CountBranch { kind, .. } | Operation::CountBranchWord { kind, .. } => (
            match kind {
                CountBranchKind::LoopEqual | CountBranchKind::LoopNotEqual => ZF,
                _ => 0,
            },
            0,
            0,
        ),
        Operation::Carry { kind } => (u32::from(kind == CarryKind::Complement) * CF, CF, 0),
        Operation::FlagsToAh => (CF | PF | AF | ZF | SF, 0, 0),
        Operation::AhToFlags => (0, CF | PF | AF | ZF | SF, 0),
        Operation::SetAlFromCarry => (CF, 0, 0),
        Operation::PushFlags | Operation::PushFlagsWord => (ARITHMETIC, 0, 0),
        Operation::Nop
        | Operation::Move { .. }
        | Operation::MoveByte { .. }
        | Operation::MoveWord { .. }
        | Operation::LoadByte { .. }
        | Operation::LoadWord { .. }
        | Operation::StoreByte { .. }
        | Operation::StoreWord { .. }
        | Operation::Lea { .. }
        | Operation::LeaWord { .. }
        | Operation::Exchange { .. }
        | Operation::ExchangeByte { .. }
        | Operation::ExchangeWord { .. }
        | Operation::Jump { .. }
        | Operation::JumpWord { .. }
        | Operation::ReadJumpWord { .. }
        | Operation::Call { .. }
        | Operation::CallWord { .. }
        | Operation::CallRegisterWord { .. }
        | Operation::ReadCallWord { .. }
        | Operation::Push { .. }
        | Operation::PushWord { .. }
        | Operation::PushMemoryWord { .. }
        | Operation::Pop { .. }
        | Operation::PopWord { .. }
        | Operation::PopMemoryWord { .. }
        | Operation::Return { .. }
        | Operation::ReturnWord { .. }
        | Operation::ByteSwap { .. }
        | Operation::Extend { .. }
        | Operation::Direction { .. }
        | Operation::EnterZero
        | Operation::EnterZeroWord
        | Operation::Leave
        | Operation::LeaveWord => (0, 0, 0),
        _ => return None,
    };
    Some(FlagEffects {
        read,
        write,
        undefined,
    })
}

// demands describe each instruction's successful output. faults still expose its input flags.
pub(crate) fn live_flags(operations: &[Operation]) -> Vec<u32> {
    let mut demands = vec![0; operations.len()];
    let mut live = ARITHMETIC;
    for (index, operation) in operations.iter().copied().enumerate().rev() {
        let Some(effect) = analyze(operation) else {
            demands[index] = ARITHMETIC;
            live = ARITHMETIC;
            continue;
        };
        let (can_fault, can_write) = memory_barrier(operation);
        if can_write {
            // an executable alias can stop immediately after the committed instruction.
            live = ARITHMETIC;
        }
        demands[index] = live;
        live = (live & !effect.write) | effect.read;
        if can_fault {
            live = ARITHMETIC;
        }
    }
    demands
}

pub(super) fn memory_barrier(operation: Operation) -> (bool, bool) {
    let location = |value| matches!(value, Location32::Memory(_));
    let source = |value| matches!(value, Value32::Memory(_));
    match operation {
        Operation::Move {
            destination,
            source: value,
        } => {
            let write = location(destination);
            (write || source(value), write)
        }
        Operation::Binary {
            kind,
            destination,
            source: value,
        } => {
            let memory = location(destination);
            (
                memory || source(value),
                memory && !matches!(kind, BinaryKind::Cmp | BinaryKind::Test),
            )
        }
        Operation::Unary { destination, .. } => (location(destination), location(destination)),
        Operation::MemoryUnaryByte { .. }
        | Operation::MemoryUnaryWord { .. }
        | Operation::MemoryArithmeticByte { .. }
        | Operation::MemoryArithmeticWord { .. }
        | Operation::MemoryLogicalByte { .. }
        | Operation::MemoryLogicalWord { .. }
        | Operation::MemoryExchange { .. }
        | Operation::MemoryExchangeByte { .. }
        | Operation::MemoryExchangeWord { .. }
        | Operation::MemoryExchangeAdd { .. }
        | Operation::MemoryExchangeAddByte { .. }
        | Operation::MemoryExchangeAddWord { .. }
        | Operation::MemoryCompareExchange { .. }
        | Operation::MemoryCompareExchangeByte { .. }
        | Operation::MemoryCompareExchangeWord { .. }
        | Operation::StoreByte { .. }
        | Operation::StoreWord { .. }
        | Operation::MemorySetByte { .. }
        | Operation::PopMemoryWord { .. }
        | Operation::Push { .. }
        | Operation::PushWord { .. }
        | Operation::PushMemoryWord { .. }
        | Operation::PushFlags
        | Operation::PushFlagsWord
        | Operation::Call { .. }
        | Operation::CallWord { .. }
        | Operation::CallRegisterWord { .. }
        | Operation::ReadCallWord { .. }
        | Operation::EnterZero
        | Operation::EnterZeroWord => (true, true),
        Operation::LoadByte { .. }
        | Operation::LoadWord { .. }
        | Operation::ReadArithmeticByte { .. }
        | Operation::ReadArithmeticWord { .. }
        | Operation::ReadLogicalByte { .. }
        | Operation::ReadLogicalWord { .. }
        | Operation::ReadCompareByte { .. }
        | Operation::ReadCompareWord { .. }
        | Operation::MemoryCompareWord { .. }
        | Operation::MemoryTestWord { .. }
        | Operation::MemoryPredicateByte { .. }
        | Operation::ReadExtendByteToWord { .. }
        | Operation::ReadSignedMultiplyWord { .. }
        | Operation::ReadSignedMultiplyImmediateWord { .. }
        | Operation::ReadMultiplyAccumulator { .. }
        | Operation::ReadByteMultiplyAccumulator { .. }
        | Operation::ReadWordMultiplyAccumulator { .. }
        | Operation::ReadDivideAccumulator { .. }
        | Operation::ReadByteDivideAccumulator { .. }
        | Operation::ReadWordDivideAccumulator { .. }
        | Operation::ReadBitTest { .. }
        | Operation::ReadBitTestWord { .. }
        | Operation::ReadTrailingZeroCount32 { .. }
        | Operation::ReadBitScan { .. }
        | Operation::ReadBitScanWord { .. }
        | Operation::ReadConditionalMove { .. }
        | Operation::ReadConditionalMoveWord { .. }
        | Operation::ReadJumpWord { .. }
        | Operation::PopWord { .. }
        | Operation::Return { .. }
        | Operation::ReturnWord { .. }
        | Operation::Leave
        | Operation::LeaveWord => (true, false),
        Operation::Pop { destination } => (true, location(destination)),
        Operation::Jump {
            target: BranchTarget::Indirect(value),
        } => (location(value), false),
        Operation::Nop
        | Operation::MoveByte { .. }
        | Operation::MoveWord { .. }
        | Operation::ArithmeticByte { .. }
        | Operation::ArithmeticWord { .. }
        | Operation::LogicalByte { .. }
        | Operation::LogicalWord { .. }
        | Operation::CompareByte { .. }
        | Operation::CompareWord { .. }
        | Operation::TestByte { .. }
        | Operation::TestWord { .. }
        | Operation::UnaryByte { .. }
        | Operation::UnaryWord { .. }
        | Operation::Lea { .. }
        | Operation::LeaWord { .. }
        | Operation::Exchange { .. }
        | Operation::ExchangeByte { .. }
        | Operation::ExchangeWord { .. }
        | Operation::ConditionalMove { .. }
        | Operation::ConditionalMoveWord { .. }
        | Operation::SetByte { .. }
        | Operation::Carry { .. }
        | Operation::Direction { .. }
        | Operation::FlagsToAh
        | Operation::AhToFlags
        | Operation::SetAlFromCarry
        | Operation::ByteSwap { .. }
        | Operation::Jump {
            target: BranchTarget::Direct(_),
        }
        | Operation::JumpWord { .. }
        | Operation::ConditionalJump { .. }
        | Operation::CountBranch { .. }
        | Operation::CountBranchWord { .. } => (false, false),
        _ => (true, true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cpu::x86::{
        Register32,
        ir::{Location32, ShiftCount, ShiftKind, Value32},
    };

    #[test]
    fn all_conditions_request_only_the_flags_their_relation_uses() {
        let expected = [
            (Condition::Overflow, OF),
            (Condition::NotOverflow, OF),
            (Condition::Below, CF),
            (Condition::AboveOrEqual, CF),
            (Condition::Equal, ZF),
            (Condition::NotEqual, ZF),
            (Condition::BelowOrEqual, CF | ZF),
            (Condition::Above, CF | ZF),
            (Condition::Sign, SF),
            (Condition::NotSign, SF),
            (Condition::Parity, PF),
            (Condition::NotParity, PF),
            (Condition::Less, SF | OF),
            (Condition::GreaterOrEqual, SF | OF),
            (Condition::LessOrEqual, ZF | SF | OF),
            (Condition::Greater, ZF | SF | OF),
        ];
        for (condition, mask) in expected {
            assert_eq!(condition_mask(condition), mask);
        }
    }

    #[test]
    fn carry_consumption_preservation_and_undefined_bits_are_explicit() {
        let binary = |kind| {
            analyze(Operation::Binary {
                kind,
                destination: Location32::Register(Register32::Eax),
                source: Value32::Immediate(1),
            })
            .unwrap()
        };
        assert_eq!(
            binary(BinaryKind::Adc),
            FlagEffects {
                read: CF,
                write: ARITHMETIC,
                undefined: 0
            }
        );
        assert_eq!(
            binary(BinaryKind::Sbb),
            FlagEffects {
                read: CF,
                write: ARITHMETIC,
                undefined: 0
            }
        );
        assert_eq!(
            binary(BinaryKind::Xor),
            FlagEffects {
                read: 0,
                write: ARITHMETIC,
                undefined: AF
            }
        );
        let increment = analyze(Operation::Unary {
            kind: UnaryKind::Inc,
            destination: Location32::Register(Register32::Eax),
        })
        .unwrap();
        assert_eq!(
            increment,
            FlagEffects {
                read: 0,
                write: ARITHMETIC & !CF,
                undefined: 0
            }
        );
        assert_eq!((CF | ZF) & !increment.write | increment.read, CF);
        assert_eq!(
            ZF & !binary(BinaryKind::Adc).write | binary(BinaryKind::Adc).read,
            CF
        );
    }

    #[test]
    fn unreviewed_dynamic_operations_are_not_silently_classified() {
        assert_eq!(
            analyze(Operation::Shift {
                kind: ShiftKind::Shl,
                destination: Location32::Register(Register32::Eax),
                count: ShiftCount::Cl
            }),
            None
        );
        assert_eq!(
            analyze(Operation::Nop),
            Some(FlagEffects {
                read: 0,
                write: 0,
                undefined: 0
            })
        );
    }

    fn arithmetic(kind: BinaryKind) -> Operation {
        Operation::Binary {
            kind,
            destination: Location32::Register(Register32::Eax),
            source: Value32::Immediate(1),
        }
    }

    #[test]
    fn overwritten_flag_recipes_are_dead_but_carry_dependencies_survive() {
        let add = arithmetic(BinaryKind::Add);
        let xor = arithmetic(BinaryKind::Xor);
        assert_eq!(live_flags(&[add, xor]), [0, ARITHMETIC]);
        let increment = Operation::Unary {
            kind: UnaryKind::Inc,
            destination: Location32::Register(Register32::Eax),
        };
        assert_eq!(live_flags(&[add, increment]), [CF, ARITHMETIC]);
        assert_eq!(
            live_flags(&[add, arithmetic(BinaryKind::Adc)]),
            [CF, ARITHMETIC]
        );
        assert!(live_flags(&[]).is_empty());
    }

    #[test]
    fn a_read_fault_demands_the_previous_recipe_and_a_store_stop_demands_its_own() {
        let add = arithmetic(BinaryKind::Add);
        let read = Operation::Move {
            destination: Location32::Register(Register32::Ebx),
            source: Value32::Memory(Default::default()),
        };
        let xor = arithmetic(BinaryKind::Xor);
        assert_eq!(live_flags(&[add, read, xor]), [ARITHMETIC, 0, ARITHMETIC]);
        let modify = Operation::Binary {
            kind: BinaryKind::Add,
            destination: Location32::Memory(Default::default()),
            source: Value32::Immediate(1),
        };
        assert_eq!(
            live_flags(&[add, modify, xor]),
            [ARITHMETIC, ARITHMETIC, ARITHMETIC]
        );
        let unknown = Operation::Shift {
            kind: ShiftKind::Shl,
            destination: Location32::Register(Register32::Eax),
            count: ShiftCount::Cl,
        };
        assert_eq!(
            live_flags(&[add, unknown, xor]),
            [ARITHMETIC, ARITHMETIC, ARITHMETIC]
        );
    }

    #[test]
    fn narrow_memory_forms_keep_post_store_exits_without_burdening_register_forms() {
        use crate::cpu::x86::ir::{
            ByteArithmeticKind, ByteRegister, ByteValue, MemoryByteArithmeticKind,
        };
        assert_eq!(
            memory_barrier(Operation::MemoryArithmeticByte {
                kind: MemoryByteArithmeticKind::Adc,
                address: Default::default(),
                source: ByteValue::Immediate(1),
            }),
            (true, true)
        );
        assert_eq!(
            memory_barrier(Operation::MemoryCompareWord {
                address: Default::default(),
                right: crate::cpu::x86::ir::WordValue::Immediate(1),
            }),
            (true, false)
        );
        assert_eq!(
            memory_barrier(Operation::ArithmeticByte {
                kind: ByteArithmeticKind::Adc,
                destination: ByteRegister::Al,
                source: ByteValue::Immediate(1),
            }),
            (false, false)
        );
    }
}
