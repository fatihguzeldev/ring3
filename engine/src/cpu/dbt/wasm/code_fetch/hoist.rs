use crate::cpu::{
    dbt::region::CompiledBlock,
    x86::ir::{BranchTarget, Location32, Operation, Value32},
};
pub(in crate::cpu::dbt::wasm) fn helper_free_block(block: &CompiledBlock) -> bool {
    !block.instructions.is_empty()
        && block
            .instructions
            .iter()
            .all(|instruction| helper_free(instruction.operation()))
}

fn helper_free(operation: &Operation) -> bool {
    // all unreviewed families retain instruction-boundary checks, including pure
    // families whose future lowering could gain an external helper.
    matches!(
        operation,
        Operation::Nop
            | Operation::Move {
                destination: Location32::Register(_),
                source: Value32::Register(_) | Value32::Immediate(_),
            }
            | Operation::Binary {
                destination: Location32::Register(_),
                source: Value32::Register(_) | Value32::Immediate(_),
                ..
            }
            | Operation::Unary {
                destination: Location32::Register(_),
                ..
            }
            | Operation::MoveByte { .. }
            | Operation::MoveWord { .. }
            | Operation::ArithmeticWord { .. }
            | Operation::LogicalWord { .. }
            | Operation::CompareWord { .. }
            | Operation::TestWord { .. }
            | Operation::UnaryWord { .. }
            | Operation::UnaryByte { .. }
            | Operation::Carry { .. }
            | Operation::Direction { .. }
            | Operation::Lea { .. }
            | Operation::LeaWord { .. }
            | Operation::Jump {
                target: BranchTarget::Direct(_)
            }
            | Operation::Jump {
                target: BranchTarget::Indirect(Location32::Register(_))
            }
            | Operation::JumpWord { .. }
            | Operation::ConditionalJump { .. }
            | Operation::CountBranch { .. }
            | Operation::CountBranchWord { .. }
    )
}
