use super::*;
use crate::cpu::core::program;
use crate::cpu::x86::{
    Register32, X87Binary, X87Format,
    ir::{BinaryKind, BranchTarget, Location32, Operation, Value32},
};
use crate::memory::GuestAddress;

fn block(pc: u32, operations: &[Operation]) -> program::Block {
    program::Block {
        entry: pc,
        instructions: operations
            .iter()
            .copied()
            .enumerate()
            .map(|(i, operation)| program::Instruction {
                pc: pc + i as u32,
                next: pc + i as u32 + 1,
                operation: Some(operation),
                gate: None,
            })
            .collect(),
    }
}
fn mov(register: Register32, value: u32) -> Operation {
    Operation::Move {
        destination: Location32::Register(register),
        source: Value32::Immediate(value),
    }
}
fn jump(pc: u32) -> Operation {
    Operation::Jump {
        target: BranchTarget::Direct(GuestAddress(pc)),
    }
}
fn fp(operation: X87Operation) -> Operation {
    Operation::X87 {
        operation,
        address: None,
        waiting: false,
        opcode: 0x123,
    }
}
fn single(operations: &[Operation]) -> RegionPlan {
    RegionPlan::analyze(&Region::from_blocks(&[block(0x1000, operations)], 0x1000).unwrap())
}
fn binary() -> Operation {
    fp(X87Operation::Binary {
        kind: X87Binary::Add,
        destination: 0,
        source: 1,
        pop: false,
    })
}

#[test]
fn arithmetic_keeps_mode_correlated_precision_without_narrowing_pc53() {
    let plan = single(&[binary(), binary()]);
    let after = plan.instruction(BlockId(0), 0).after;
    let payload = plan.frame(after).fp.payload[0];
    let facts: Vec<_> = plan
        .proofs_at(after)
        .iter()
        .map(|&id| plan.proof(id))
        .filter(|proof| proof.value.is_some_and(|value| payload.contains(value)))
        .map(|proof| proof.kind)
        .collect();
    assert!(facts.contains(&ProofKind::Native24InSingle));
    assert!(facts.contains(&ProofKind::BoundedF64));
    assert!(!facts.contains(&ProofKind::Native24));

    let blocks = [
        block(0x1000, &[binary(), jump(0x2000)]),
        block(0x2000, &[binary()]),
    ];
    let plan = RegionPlan::analyze(&Region::from_blocks(&blocks, 0x1000).unwrap());
    let contract = &plan.block(BlockId(1)).contract;
    let payload = plan.frame(contract.frame).fp.payload[0];
    assert!(contract.guards.iter().any(|&id| {
        let proof = plan.proof(id);
        proof.kind == ProofKind::Native24InSingle
            && proof.value.is_some_and(|value| payload.contains(value))
    }));
    assert!(!contract.guards.iter().any(|&id| {
        let proof = plan.proof(id);
        proof.kind == ProofKind::Native24
            && proof.value.is_some_and(|value| payload.contains(value))
    }));
}

#[test]
fn scalar_copies_constants_and_wrapped_status_output_have_distinct_versions() {
    let plan = single(&[
        mov(Register32::Eax, 7),
        Operation::Move {
            destination: Location32::Register(Register32::Ebx),
            source: Value32::Register(Register32::Eax),
        },
        fp(X87Operation::StatusToAx),
        Operation::Binary {
            kind: BinaryKind::Add,
            destination: Location32::Register(Register32::Ebx),
            source: Value32::Immediate(2),
        },
    ]);
    let after_copy = plan.frame(plan.instruction(BlockId(0), 1).after);
    assert_eq!(after_copy.gpr[0], after_copy.gpr[3]);
    let after_status = plan.frame(plan.instruction(BlockId(0), 2).after);
    assert_eq!(plan.constant(after_status.gpr[0]), None);
    assert_eq!(plan.constant(after_status.gpr[3]), Some(7));
    assert_eq!(
        plan.constant(plan.frame(plan.block(BlockId(0)).exit).gpr[3]),
        Some(9)
    );
}

#[test]
fn internal_backedge_facts_keep_a_distinct_guarded_external_input() {
    let blocks = [
        block(0x1000, &[mov(Register32::Eax, 19), jump(0x2000)]),
        block(0x2000, &[Operation::Nop, jump(0x2000)]),
    ];
    let region = Region::from_blocks(&blocks, 0x1000).unwrap();
    let plan = RegionPlan::analyze(&region);
    let entry = &plan.block(BlockId(1)).contract;
    assert!(entry.external);
    assert_eq!(entry.gpr_constants[0], Some(19));
    let input = plan.frame(entry.frame).gpr[0];
    assert_eq!(
        plan.value(input).kind,
        ValueKind::Input {
            block: BlockId(1),
            field: InputField::Gpr(0)
        }
    );
    // The backedge reuses the same parameter/version rather than re-lowering
    // unbounded symbolic loop iterations or bypassing the import guard.
    assert_eq!(input, plan.frame(plan.block(BlockId(1)).exit).gpr[0]);
    let rerun = RegionPlan::analyze(&region);
    assert_eq!(plan.frames(), rerun.frames());
}

#[test]
fn secondary_constant_contract_requires_imported_register_equality() {
    let blocks = [
        block(0x1000, &[mov(Register32::Eax, 19), jump(0x2000)]),
        block(0x2000, &[Operation::Nop]),
    ];
    let plan = RegionPlan::analyze(&Region::from_blocks(&blocks, 0x1000).unwrap());
    let contract = &plan.block(BlockId(1)).contract;
    assert!(contract.external);
    assert_eq!(contract.gpr_constants[0], Some(19));
    assert_eq!(plan.constant(plan.frame(contract.frame).gpr[0]), Some(19));
}

#[test]
fn loop_admissions_and_occupancy_are_guarded_at_public_entry() {
    let blocks = [
        block(0x1000, &[binary(), jump(0x2000)]),
        block(0x2000, &[binary(), jump(0x2000)]),
    ];
    let plan = RegionPlan::analyze(&Region::from_blocks(&blocks, 0x1000).unwrap());
    let contract = &plan.block(BlockId(1)).contract;
    assert!(contract.external);
    assert!(
        contract
            .guards
            .iter()
            .any(|&id| plan.proof(id).kind == ProofKind::Control)
    );
    for index in 0..2 {
        let values = plan.frame(contract.frame).fp.payload[index].values();
        assert!(
            contract
                .guards
                .iter()
                .any(|&id| plan.proof(id).kind == ProofKind::Occupied
                    && plan.proof(id).value.is_some_and(|v| values.contains(&v)))
        );
        assert!(matches!(
            plan.frame(contract.frame).fp.payload[index],
            FpValue::ExactF64(..)
        ));
        assert!(contract.guards.iter().any(|&id| {
            let proof = plan.proof(id);
            proof.kind == ProofKind::Native24InSingle
                && proof.conditional.is_none()
                && proof.value.is_some_and(|v| values.contains(&v))
        }));
    }
}

#[test]
fn modifying_backedge_kills_a_preheader_constant_at_the_join() {
    let blocks = [
        block(0x1000, &[mov(Register32::Eax, 19), jump(0x2000)]),
        block(
            0x2000,
            &[
                Operation::Binary {
                    kind: BinaryKind::Add,
                    destination: Location32::Register(Register32::Eax),
                    source: Value32::Immediate(1),
                },
                jump(0x2000),
            ],
        ),
    ];
    let plan = RegionPlan::analyze(&Region::from_blocks(&blocks, 0x1000).unwrap());
    assert_eq!(plan.block(BlockId(1)).contract.gpr_constants[0], None);
}

#[test]
fn stack_renaming_retains_all_ghosts_and_uses_at_most_eight_cells() {
    let plan = single(&[
        fp(X87Operation::LoadRegister { index: 1 }),
        fp(X87Operation::Exchange { index: 2 }),
        fp(X87Operation::StoreRegister {
            index: 1,
            pop: true,
        }),
    ]);
    let entry = plan.frame(plan.block(BlockId(0)).entry);
    let pushed = plan.frame(plan.instruction(BlockId(0), 0).after);
    assert_eq!(pushed.fp.payload[0], entry.fp.payload[1]);
    assert_eq!(pushed.fp.payload[1], entry.fp.payload[0]);
    // Only the architecturally overwritten push destination disappears.
    for old in &entry.fp.payload[..7] {
        assert!(pushed.fp.payload.contains(old));
    }
    let final_frame = plan.frame(plan.block(BlockId(0)).exit);
    assert_eq!(final_frame.fp.tags[7], TagFact::Empty);
    assert_eq!(final_frame.fp.payload[7], final_frame.fp.payload[0]);
    for frame in plan.frames() {
        assert_eq!(frame.fp.payload.len(), 8);
        for payload in frame.fp.payload {
            for value in payload.values() {
                assert!(plan.storage(value).unwrap().cell.0 < 8);
            }
        }
    }
}

#[test]
fn register_transfers_classify_written_tags_without_changing_untouched_sources() {
    let load = single(&[fp(X87Operation::LoadRegister { index: 0 })]);
    let before = load.frame(load.block(BlockId(0)).entry);
    let after = load.frame(load.block(BlockId(0)).exit);
    assert_eq!(after.fp.tags[1], before.fp.tags[0]);
    assert_eq!(
        load.shape(load.block(BlockId(0)).exit).tags[0],
        TagRecipe::Value(StorageCell(0))
    );
    let store = single(&[fp(X87Operation::StoreRegister {
        index: 2,
        pop: false,
    })]);
    let before = store.frame(store.block(BlockId(0)).entry);
    let after = store.frame(store.block(BlockId(0)).exit);
    assert_eq!(after.fp.tags[0], before.fp.tags[0]);
    assert_eq!(
        store.shape(store.block(BlockId(0)).exit).tags[2],
        TagRecipe::Value(StorageCell(0))
    );
    let exchange = single(&[fp(X87Operation::Exchange { index: 1 })]);
    let shape = exchange.shape(exchange.block(BlockId(0)).exit);
    assert_eq!(shape.tags[0], TagRecipe::Value(StorageCell(1)));
    assert_eq!(shape.tags[1], TagRecipe::Value(StorageCell(0)));
}

#[test]
fn explicit_fp_output_cell_survives_aliases_and_a_nonzero_destination_pop() {
    let plan = single(&[
        fp(X87Operation::LoadRegister { index: 1 }),
        fp(X87Operation::Binary {
            kind: X87Binary::Add,
            destination: 1,
            source: 0,
            pop: true,
        }),
    ]);
    let instruction = plan.instruction(BlockId(0), 1);
    let after = plan.frame(instruction.after);
    let result = after.fp.payload[0].values()[0];
    assert_eq!(
        plan.fp_output_cell(instruction.after),
        Some(plan.storage(result).unwrap().cell)
    );
    assert_ne!(
        after.fp.payload[7], after.fp.payload[0],
        "the popped source ghost stays distinct from the arithmetic result"
    );
}

#[test]
fn helper_and_control_change_kill_all_previous_admission_proofs() {
    let plan = single(&[binary(), fp(X87Operation::LoadControl), binary()]);
    let first = plan.instruction(BlockId(0), 0);
    let helper = plan.instruction(BlockId(0), 1);
    let resumed = plan.instruction(BlockId(0), 2);
    assert!(plan.proofs_at(first.before).is_empty());
    assert!(
        plan.proofs_at(first.after)
            .iter()
            .any(|&id| plan.proof(id).kind == ProofKind::BoundedF64)
    );
    assert_ne!(
        plan.frame(first.after).fp.control_epoch,
        plan.frame(helper.after).fp.control_epoch
    );
    assert!(plan.proofs_at(resumed.before).is_empty());
    assert!(
        plan.frame(helper.after)
            .fp
            .payload
            .iter()
            .all(|payload| matches!(payload, FpValue::Tagged { .. }))
    );
    assert!(plan.frame(helper.after).fp.metadata.canonical);
}

#[test]
fn success_proofs_do_not_appear_at_the_faulting_prefix() {
    let plan = single(&[
        fp(X87Operation::Load(X87Format::Float32)),
        binary(),
        binary(),
    ]);
    let load = plan.instruction(BlockId(0), 0);
    let first = plan.instruction(BlockId(0), 1);
    let second = plan.instruction(BlockId(0), 2);
    assert!(plan.proofs_at(load.before).is_empty());
    assert!(
        plan.proofs_at(first.before)
            .iter()
            .any(|&id| plan.proof(id).kind == ProofKind::Native24)
    );
    let source = plan.frame(first.after).fp.payload[1].values()[0];
    assert!(
        !plan
            .proofs_at(first.before)
            .iter()
            .any(|&id| plan.proof(id).value == Some(source)
                && plan.proof(id).kind == ProofKind::BoundedF64)
    );
    assert!(
        plan.proofs_at(second.before)
            .iter()
            .any(|&id| plan.proof(id).value == Some(source)
                && plan.proof(id).kind == ProofKind::BoundedF64)
    );
    assert_eq!(plan.frame(first.before).pc, ResumePc::Exact(0x1001));
    assert_eq!(plan.frame(first.before).retired, 1);
    assert_eq!(plan.frame(first.after).pc, ResumePc::Exact(0x1002));
    assert_eq!(plan.frame(first.after).retired, 2);
}

#[test]
fn shape_interning_excludes_instruction_metadata() {
    let plan = single(&[fp(X87Operation::ChangeSign), fp(X87Operation::Absolute)]);
    let first = plan.instruction(BlockId(0), 0).after;
    let second = plan.instruction(BlockId(0), 1).after;
    assert_eq!(plan.shape_id(first), plan.shape_id(second));
    assert_ne!(
        plan.metadata(first).instruction,
        plan.metadata(second).instruction
    );
}

#[test]
fn identity_layout_with_changed_dirty_or_tags_still_rebases_at_edge() {
    let blocks = [
        block(0x1000, &[fp(X87Operation::ChangeSign), jump(0x2000)]),
        block(0x2000, &[Operation::Nop]),
    ];
    let plan = RegionPlan::analyze(&Region::from_blocks(&blocks, 0x1000).unwrap());
    let edge = &plan.block(BlockId(0)).edges[0];
    assert_eq!(
        plan.shape(edge.frame).cells,
        plan.shape(plan.block(edge.target).entry).cells
    );
    assert!(edge.fp_rebase);
    assert_eq!(edge.frame, plan.block(BlockId(0)).exit);
}

#[test]
fn consumed_shape_keeps_popped_ghosts_and_independent_tag_destinations() {
    let plan = single(&[
        fp(X87Operation::LoadRegister { index: 0 }),
        fp(X87Operation::StoreRegister {
            index: 0,
            pop: true,
        }),
        fp(X87Operation::Exchange { index: 1 }),
    ]);
    let pushed = plan.instruction(BlockId(0), 0).after;
    let popped = plan.instruction(BlockId(0), 1).after;
    assert_eq!(plan.fp_input_origin(pushed, 0), Some(0));
    let ghost = plan.shape(popped);
    assert_eq!(ghost.top_delta, 0);
    assert_eq!(ghost.cells[7], StorageCell(0));
    assert_eq!(ghost.written, 1 << 7);
    assert_eq!(ghost.tag_written, 1 << 7);
    assert_eq!(ghost.tags[7], TagRecipe::Empty);
    let exchanged = plan.shape(plan.instruction(BlockId(0), 2).after);
    assert_eq!(exchanged.written, 0x83);
    assert_eq!(exchanged.cells[7], ghost.cells[7]);
    assert_eq!(exchanged.tag_written, 0x83);
}

#[test]
fn memory_pop_changes_only_occupancy_and_top_at_its_successful_prefix() {
    let plan = single(&[fp(X87Operation::Store {
        format: X87Format::Float64,
        pop: true,
        truncate: false,
    })]);
    let step = plan.instruction(BlockId(0), 0);
    assert_eq!(plan.shape(step.before).tag_written, 0);
    let completed = plan.shape(step.after);
    assert_eq!(completed.written, 0);
    // The consumed descriptor rotates this final logical bit by TOPdelta=1:
    // logical7 is the popped physical slot0, whose payload is unchanged.
    assert_eq!(completed.tag_written, 1 << 7);
    assert_eq!(completed.tags[7], TagRecipe::Empty);
    assert_eq!(completed.top_delta, 1);
}

#[test]
fn binary_pop_result_and_popped_ghost_keep_separate_physical_destinations() {
    let plan = single(&[
        fp(X87Operation::Load(X87Format::Float32)),
        fp(X87Operation::Binary {
            kind: X87Binary::Add,
            destination: 1,
            source: 0,
            pop: true,
        }),
    ]);
    let after = plan.instruction(BlockId(0), 1).after;
    let shape = plan.shape(after);
    assert_eq!(shape.top_delta, 0);
    assert_eq!(shape.written, 0x81);
    assert_eq!(shape.tag_written, 0x81);
    assert_eq!(shape.tags[7], TagRecipe::Empty);
}

#[test]
fn native24_entry_fact_is_consumed_only_behind_an_actual_import_guard() {
    let blocks = [
        block(
            0x1000,
            &[fp(X87Operation::Load(X87Format::Float32)), jump(0x2000)],
        ),
        block(
            0x2000,
            &[fp(X87Operation::Store {
                format: X87Format::Float32,
                pop: true,
                truncate: false,
            })],
        ),
    ];
    let plan = RegionPlan::analyze(&Region::from_blocks(&blocks, 0x1000).unwrap());
    let target = plan.block(BlockId(1));
    let FpValue::ExactF64(value, _) = plan.frame(target.entry).fp.payload[0] else {
        panic!("successful imported contract has an exact native value");
    };
    assert!(target.contract.guards.iter().any(|&id| {
        let proof = plan.proof(id);
        proof.kind == ProofKind::Native24 && proof.value == Some(value)
    }));
    for kind in [
        ProofKind::ExactF64,
        ProofKind::Native24,
        ProofKind::BoundedF64,
    ] {
        assert!(plan.proofs_at(target.entry).iter().any(|&id| {
            let proof = plan.proof(id);
            proof.kind == kind && proof.value == Some(value) && proof.conditional.is_none()
        }));
    }
    let edge = &plan.block(BlockId(0)).edges[0];
    assert_eq!(edge.fp_transfer.top_delta, 7);
    assert_eq!(edge.fp_transfer.sources, plan.shape(edge.frame).cells);
}

#[test]
fn actual_simd_fast_success_preserves_the_renamed_fp_frame() {
    use crate::cpu::x86::simd::{FloatKind, SimdKind, SimdOperand, SimdOperation, SimdRegister};
    let move_op = SimdOperation {
        kind: SimdKind::Move,
        destination: SimdOperand::Register(SimdRegister::Xmm(0)),
        source: SimdOperand::Register(SimdRegister::Xmm(1)),
        vector_bytes: 16,
        lane_bytes: 16,
        memory_bytes: 0,
        immediate: None,
        aligned: false,
        scalar: false,
        implicit_address: None,
    };
    let add_op = SimdOperation {
        kind: SimdKind::Float(FloatKind::Add),
        lane_bytes: 4,
        ..move_op
    };
    for operation in [move_op, add_op] {
        let plan = single(&[
            fp(X87Operation::LoadRegister { index: 1 }),
            Operation::Simd(operation),
            binary(),
        ]);
        let instruction = plan.instruction(BlockId(0), 1);
        assert_eq!(
            plan.frame(instruction.before).fp,
            plan.frame(instruction.after).fp,
            "the actual direct SIMD success keeps every x87 value, tag, epoch and metadata recipe"
        );
        assert_eq!(
            plan.shape(instruction.before),
            plan.shape(instruction.after)
        );
        assert_ne!(plan.shape(instruction.after).top_delta, 0);
        assert_eq!(plan.instruction(BlockId(0), 2).before, instruction.after);
    }
    for operation in [
        SimdOperation {
            scalar: true,
            ..add_op
        },
        SimdOperation {
            destination: SimdOperand::Register(SimdRegister::Mmx(0)),
            source: SimdOperand::Register(SimdRegister::Mmx(1)),
            vector_bytes: 8,
            lane_bytes: 8,
            ..move_op
        },
    ] {
        let plan = single(&[
            fp(X87Operation::LoadRegister { index: 1 }),
            Operation::Simd(operation),
        ]);
        let instruction = plan.instruction(BlockId(0), 1);
        assert_ne!(
            plan.frame(instruction.before).fp.control_epoch,
            plan.frame(instruction.after).fp.control_epoch
        );
        assert_eq!(plan.shape(instruction.after).top_delta, 0);
    }
}

#[test]
fn actual_simd_fast_success_carries_fp_admission_facts_across_cfg_edges() {
    use crate::cpu::x86::simd::{SimdKind, SimdOperand, SimdOperation, SimdRegister};
    let operation = Operation::Simd(SimdOperation {
        kind: SimdKind::Move,
        destination: SimdOperand::Register(SimdRegister::Xmm(0)),
        source: SimdOperand::Register(SimdRegister::Xmm(1)),
        vector_bytes: 16,
        lane_bytes: 16,
        memory_bytes: 0,
        immediate: None,
        aligned: false,
        scalar: false,
        implicit_address: None,
    });
    let blocks = [
        block(0x1000, &[binary(), operation, jump(0x2000)]),
        block(0x2000, &[Operation::Nop]),
    ];
    let plan = RegionPlan::analyze(&Region::from_blocks(&blocks, 0x1000).unwrap());
    let contract = &plan.block(BlockId(1)).contract;
    for kind in [
        ProofKind::Control,
        ProofKind::Occupied,
        ProofKind::Native24InSingle,
    ] {
        assert!(
            contract
                .guards
                .iter()
                .any(|&id| plan.proof(id).kind == kind)
        );
    }
    assert!(
        plan.proofs_at(contract.frame)
            .iter()
            .any(|&id| { plan.proof(id).kind == ProofKind::ExactF64 })
    );
}
