use super::*;
use crate::{cpu::x86::Register32, memory::GuestAddress};

fn block(pc: u32, operations: &[Operation]) -> program::Block {
    program::Block {
        entry: pc,
        instructions: operations
            .iter()
            .copied()
            .enumerate()
            .map(|(index, operation)| program::Instruction {
                pc: pc + index as u32,
                next: pc + index as u32 + 1,
                operation: Some(operation),
                gate: None,
            })
            .collect(),
    }
}

fn jump(pc: u32) -> Operation {
    Operation::Jump {
        target: BranchTarget::Direct(GuestAddress(pc)),
    }
}

fn branch(pc: u32) -> Operation {
    Operation::ConditionalJump {
        condition: Condition::NotEqual,
        target: GuestAddress(pc),
    }
}

#[test]
fn diamond_has_direct_forward_edges_and_one_copy_of_its_join() {
    let blocks = [
        block(0x1000, &[branch(0x2000)]),
        block(0x1001, &[jump(0x3000)]),
        block(0x2000, &[jump(0x3000)]),
        block(0x3000, &[jump(0x4000)]),
    ];
    let region = Region::from_blocks(&blocks, 0x1000).unwrap();
    let plan = region.structure();
    assert_eq!(
        plan.shapes,
        (0..4)
            .map(|id| Shape::Block(BlockId(id)))
            .collect::<Vec<_>>()
    );
    assert!(!plan.requires_dispatch());
    assert_eq!(region.blocks[3].predecessors, [BlockId(1), BlockId(2)]);
    assert_eq!(
        region.blocks[3].terminator,
        Terminator::Next {
            target: Target::External(0x4000),
            kind: DirectKind::Jump,
        }
    );
}

#[test]
fn loop_backedge_targets_a_loop_and_the_exit_stays_outside_it() {
    let blocks = [
        block(0x1000, &[jump(0x2000)]),
        block(0x2000, &[branch(0x1000)]),
        block(0x2001, &[jump(0x3000)]),
    ];
    let region = Region::from_blocks(&blocks, 0x1000).unwrap();
    assert_eq!(
        region.structure().shapes,
        [
            Shape::Loop {
                header: BlockId(0),
                body: vec![Shape::Block(BlockId(0)), Shape::Block(BlockId(1))],
            },
            Shape::Block(BlockId(2)),
        ]
    );
}

#[test]
fn only_the_multiple_entry_cycle_uses_dispatch() {
    let blocks = [
        block(0x1000, &[branch(0x2000)]),
        block(0x1001, &[jump(0x2000)]),
        block(0x2000, &[branch(0x1001)]),
        block(0x2001, &[jump(0x3000)]),
    ];
    let region = Region::from_blocks(&blocks, 0x1000).unwrap();
    assert_eq!(
        region.structure().shapes,
        [
            Shape::Block(BlockId(0)),
            Shape::Dispatch {
                entries: vec![BlockId(1), BlockId(2)],
                blocks: vec![BlockId(1), BlockId(2)],
            },
            Shape::Block(BlockId(3)),
        ]
    );
}

#[test]
fn nested_single_entry_cycles_get_distinct_loop_scopes() {
    let blocks = [
        block(0x1000, &[jump(0x2000)]),
        block(0x2000, &[branch(0x2000)]),
        block(0x2001, &[branch(0x1000)]),
        block(0x2002, &[jump(0x4000)]),
    ];
    let region = Region::from_blocks(&blocks, 0x1000).unwrap();
    assert_eq!(
        region.structure().shapes,
        [
            Shape::Loop {
                header: BlockId(0),
                body: vec![
                    Shape::Block(BlockId(0)),
                    Shape::Loop {
                        header: BlockId(1),
                        body: vec![Shape::Block(BlockId(1))]
                    },
                    Shape::Block(BlockId(2)),
                ],
            },
            Shape::Block(BlockId(3)),
        ]
    );
}

#[test]
fn secondary_entry_restructures_the_same_cycle_around_the_entered_block() {
    let blocks = [
        block(0x1000, &[jump(0x2000)]),
        block(0x2000, &[branch(0x2002)]),
        block(0x2001, &[jump(0x2000)]),
        block(0x2002, &[jump(0x3000)]),
    ];
    let region = Region::from_blocks(&blocks, 0x1000).unwrap();
    let secondary = region.structure_from(BlockId(2));
    assert_eq!(secondary.reachable, [false, true, true, true]);
    assert_eq!(
        secondary.shapes,
        [
            Shape::Loop {
                header: BlockId(2),
                body: vec![Shape::Block(BlockId(2)), Shape::Block(BlockId(1))],
            },
            Shape::Block(BlockId(3)),
        ]
    );
}

#[test]
fn all_entry_plan_includes_disconnected_cycles_and_indirect_return_continuations() {
    let blocks = [
        block(0x1000, &[jump(0x9000)]),
        block(0x2000, &[jump(0x2001)]),
        block(0x2001, &[jump(0x2000)]),
        block(0x4000, &[Operation::Return { stack_adjust: 0 }]),
    ];
    let region = Region::from_blocks(&blocks, 0x1000).unwrap();
    assert_eq!(region.structure().reachable, [true, false, false, false]);
    let all = region.structure_all();
    assert_eq!(all.reachable, [true; 4]);
    assert_eq!(
        all.shapes,
        [
            Shape::Block(BlockId(0)),
            Shape::Loop {
                header: BlockId(1),
                body: vec![Shape::Block(BlockId(1)), Shape::Block(BlockId(2))],
            },
            Shape::Block(BlockId(3)),
        ]
    );
    assert!(!all.requires_dispatch());
}

#[test]
fn direct_call_reaches_the_callee_without_inventing_its_return_edge() {
    let blocks = [
        block(0x1000, &[jump(0x2000)]),
        block(
            0x2000,
            &[Operation::Call {
                target: BranchTarget::Direct(GuestAddress(0x3000)),
            }],
        ),
        block(0x3000, &[jump(0x4000)]),
        block(0x9000, &[jump(0x2000)]),
    ];
    let region = Region::from_blocks(&blocks, 0x1000).unwrap();
    assert_eq!(region.blocks[1].successors, [BlockId(2)]);
    assert_eq!(
        region.blocks[1].terminator,
        Terminator::Next {
            target: Target::Block(BlockId(2)),
            kind: DirectKind::Call,
        }
    );
    assert!(!region.structure().reachable[3]);
}

#[test]
fn indirect_calls_have_no_invented_fallthrough_successor() {
    let blocks = [
        block(
            0x1000,
            &[Operation::Call {
                target: BranchTarget::Indirect(crate::cpu::x86::ir::Location32::Register(
                    Register32::Eax,
                )),
            }],
        ),
        block(0x1001, &[Operation::Nop]),
    ];
    let region = Region::from_blocks(&blocks, 0x1000).unwrap();
    assert!(region.blocks[0].successors.is_empty());
    assert_eq!(region.blocks[0].terminator, Terminator::Indirect);
    assert_eq!(region.structure().shapes, [Shape::Block(BlockId(0))]);
}

#[test]
fn exit_points_preserve_before_fault_and_after_commit_instruction_counts() {
    let blocks = [block(
        0x1000,
        &[Operation::Nop, Operation::Nop, jump(0x2000)],
    )];
    let region = Region::from_blocks(&blocks, 0x1000).unwrap();
    let instruction = &region.blocks[0].instructions[1];
    assert_eq!(
        instruction.exits.before,
        ExitPoint {
            pc: ResumePc::Exact(0x1001),
            retired: 1
        }
    );
    assert_eq!(
        instruction.exits.after,
        Some(ExitPoint {
            pc: ResumePc::Exact(0x1002),
            retired: 2
        })
    );
    assert_eq!(
        region.blocks[0].instructions[2].exits.after,
        Some(ExitPoint {
            pc: ResumePc::Exact(0x2000),
            retired: 3
        })
    );
}

#[test]
fn flags_remain_observable_at_a_successor_budget_boundary() {
    use crate::cpu::x86::ir::{BinaryKind, Location32, Value32};
    let add = Operation::Binary {
        kind: BinaryKind::Add,
        destination: Location32::Register(Register32::Eax),
        source: Value32::Immediate(1),
    };
    let blocks = [block(0x1000, &[add, jump(0x2000)]), block(0x2000, &[add])];
    let region = Region::from_blocks(&blocks, 0x1000).unwrap();
    assert_eq!(
        region.blocks[0].flags.after,
        [super::super::flags::ARITHMETIC; 2]
    );
    assert_eq!(region.blocks[1].flags.before[0], 0);
}

#[test]
fn a_later_fault_keeps_flags_that_an_unfaulting_overwrite_can_discard() {
    use crate::cpu::x86::ir::{BinaryKind, Location32, Value32};
    let add = Operation::Binary {
        kind: BinaryKind::Add,
        destination: Location32::Register(Register32::Eax),
        source: Value32::Immediate(1),
    };
    let read = Operation::Move {
        destination: Location32::Register(Register32::Ecx),
        source: Value32::Memory(Default::default()),
    };
    let safe = Region::from_blocks(&[block(0x1000, &[add, add])], 0x1000).unwrap();
    let faulting = Region::from_blocks(&[block(0x1000, &[add, read, add])], 0x1000).unwrap();
    assert_eq!(safe.blocks[0].flags.after[0], 0);
    assert_eq!(
        faulting.blocks[0].flags.after[0],
        super::super::flags::ARITHMETIC
    );
}

#[test]
fn gates_publish_the_unretired_instruction_and_have_no_success_edge() {
    let mut gated = block(0x1000, &[Operation::Nop]);
    gated.instructions[0].operation = None;
    gated.instructions[0].gate = Some(17);
    let region = Region::from_blocks(&[gated], 0x1000).unwrap();
    assert_eq!(
        region.blocks[0].terminator,
        Terminator::Exit {
            reason: StopReason::Host
        }
    );
    assert_eq!(
        region.blocks[0].instructions[0].exits.before,
        ExitPoint {
            pc: ResumePc::Exact(0x1000),
            retired: 0
        }
    );
    assert_eq!(region.blocks[0].instructions[0].exits.after, None);
}

#[test]
fn graph_rejects_missing_roots_duplicate_entries_and_empty_blocks() {
    assert!(Region::from_blocks(&[block(0x1000, &[Operation::Nop])], 0x2000).is_none());
    assert!(
        Region::from_blocks(
            &[
                block(0x1000, &[Operation::Nop]),
                block(0x1000, &[Operation::Nop])
            ],
            0x1000
        )
        .is_none()
    );
    assert!(Region::from_blocks(&[block(0x1000, &[])], 0x1000).is_none());
}
