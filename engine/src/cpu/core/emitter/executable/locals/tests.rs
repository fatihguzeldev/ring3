use super::*;
use crate::cpu::{
    core::{
        program,
        region::{Region, values::RegionPlan},
    },
    x86::{
        Register32, X87Format, X87Operation,
        ir::{EffectiveAddress, Location32, Operation, UnaryKind, Value32},
    },
};

const BASE: u32 = 179;

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

fn program(blocks: &[program::Block]) -> ExecutableProgram {
    let region = Region::from_blocks(blocks, blocks[0].entry).unwrap();
    let values = RegionPlan::analyze(&region);
    ExecutableProgram::analyze(&region, &values)
}

fn mov(register: Register32, source: Value32) -> Operation {
    Operation::Move {
        destination: Location32::Register(register),
        source,
    }
}

fn load(register: Register32, offset: u32) -> Operation {
    mov(
        register,
        Value32::Memory(EffectiveAddress {
            base: Some(Register32::Ebx),
            displacement: offset,
            ..Default::default()
        }),
    )
}

fn lea(register: Register32, source: Register32, displacement: u32) -> Operation {
    Operation::Lea {
        destination: register,
        address: EffectiveAddress {
            base: Some(source),
            displacement,
            ..Default::default()
        },
    }
}

fn fp(operation: X87Operation, memory: bool) -> Operation {
    Operation::X87 {
        operation,
        address: memory.then_some(EffectiveAddress {
            base: Some(Register32::Ebx),
            ..Default::default()
        }),
        opcode: 0x103,
        waiting: true,
    }
}

fn declared_type(layout: &Layout, local: u32) -> ValType {
    let mut start = BASE;
    for &(count, ty) in &layout.declarations {
        if (start..start + count).contains(&local) {
            return ty;
        }
        start += count;
    }
    panic!("allocated local must be declared")
}

#[test]
fn integer_program_allocates_no_latent_fp_inputs_or_dead_pure_history() {
    let mut operations = vec![lea(Register32::Eax, Register32::Eax, 1); 128];
    operations.push(mov(Register32::Eax, Value32::Immediate(7)));
    let program = program(&[block(0x1000, &operations)]);
    let layout = Layout::build(&program, 1, false, BASE);
    assert!(
        program.value_count() > 300,
        "fixture contains historical and latent FP nodes"
    );
    for id in 0..program.value_count() {
        if matches!(program.value(ExecValueId(id)).node, ValueNode::Fp(_)) {
            assert!(
                layout.values[id].is_none(),
                "integer lowering must not declare latent FP lanes"
            );
        }
    }
    let allocated = layout.values.iter().flatten().count();
    assert_eq!(
        allocated, 9,
        "eight actual entry parameters plus the final constant"
    );
    assert!(
        layout.next <= BASE + 16,
        "dead pure history does not enlarge the Wasm frame"
    );
    assert!(
        layout
            .declarations
            .iter()
            .all(|&(_, ty)| ty == ValType::I32)
    );
}

#[test]
fn a_fault_recovery_keeps_old_and_new_values_distinct_when_both_survive_the_read() {
    let program = program(&[block(
        0x1000,
        &[
            mov(Register32::Edx, Value32::Register(Register32::Eax)),
            load(Register32::Eax, 0),
            lea(Register32::Ecx, Register32::Edx, 1),
        ],
    )]);
    let layout = Layout::build(&program, 1, false, BASE);
    let effect = program.block(BlockId(0)).effects().next().unwrap();
    let old = effect.before.gpr[0];
    let loaded = effect.after.gpr[0];
    assert_eq!(effect.after.gpr[2], old);
    assert_ne!(
        layout.values[old.0], layout.values[loaded.0],
        "the successful after recovery needs both the preserved old value and the loaded value"
    );
    let (values, _) = memberships(&program, 1, false);
    assert_eq!(
        values[loaded.0][0].interval.first,
        effect.point.prefix * 2 + 1
    );
    assert_eq!(
        values[old.0][0].interval.first, 0,
        "incoming values are pinned before physical clobbers"
    );
}

#[test]
fn sequential_discarded_read_results_reuse_storage_after_exact_before_recovery() {
    let program = program(&[block(
        0x1000,
        &[
            load(Register32::Eax, 0),
            load(Register32::Eax, 4),
            load(Register32::Eax, 8),
        ],
    )]);
    let layout = Layout::build(&program, 1, false, BASE);
    let effects: Vec<_> = program.block(BlockId(0)).effects().collect();
    let first = effects[0].after.gpr[0];
    let second = effects[1].after.gpr[0];
    assert_ne!(
        first, second,
        "the two faulting reads remain distinct effects"
    );
    assert_eq!(effects[1].before.gpr[0], first);
    let (values, _) = memberships(&program, 1, false);
    assert_eq!(values[first.0][0].interval.last, 3);
    assert_eq!(values[second.0][0].interval.first, 3);
    assert_ne!(
        layout.values[first.0], layout.values[second.0],
        "a partial lowering retains both its before recovery and new definitions"
    );
    // Address expressions have their own overlapping lifetimes. Which free
    // color the third result receives is not part of the allocator contract.
    let reads: Vec<_> = (0..128)
        .map(|index| load(Register32::Eax, index * 4))
        .collect();
    let long = self::program(&[block(0x2000, &reads)]);
    let long_layout = Layout::build(&long, 1, false, BASE);
    let outputs: BTreeSet<_> = long
        .block(BlockId(0))
        .effects()
        .map(|effect| long_layout.values[effect.after.gpr[0].0].unwrap())
        .collect();
    assert_eq!(
        long.block(BlockId(0)).effects().count(),
        128,
        "every unused memory read remains a real ordered effect"
    );
    assert!(
        outputs.len() <= 8,
        "successive read definitions reuse bounded storage despite distinct addresses"
    );
    assert!(
        long_layout.next <= layout.next + 4,
        "the frame does not reserve one local per historical read or address"
    );
}

#[test]
fn a_dead_entry_parameter_must_not_be_artificially_pinned_at_a_later_kernel_write() {
    let mut operations = vec![mov(Register32::Eax, Value32::Immediate(7))];
    operations.extend([Operation::Nop; 100]);
    operations.push(load(Register32::Eax, 0));
    let program = program(&[block(0x1000, &operations)]);
    let layout = Layout::build(&program, 1, false, BASE);
    let body = program.block(BlockId(0));
    let input = body.entry.gpr[0];
    let effect = body.effects().next().unwrap();
    let live = effect.before.gpr[0];
    assert!(layout.live_at(input, BlockId(0), 0));
    assert!(!layout.live_at(input, BlockId(0), effect.point.prefix * 2));
    assert!(layout.live_at(live, BlockId(0), effect.point.prefix * 2));
    assert_eq!(
        layout.values[input.0], layout.values[live.0],
        "the dead input's slot is legitimately reused by the value needed at the fault"
    );
}

#[test]
fn independent_blocks_reuse_locals_and_shared_constants_keep_one_mapping() {
    let operations = [
        mov(Register32::Ecx, Value32::Immediate(7)),
        load(Register32::Eax, 0),
        lea(Register32::Edx, Register32::Eax, 3),
    ];
    let single = program(&[block(0x1000, &operations)]);
    let single_layout = Layout::build(&single, 1, false, BASE);
    let blocks: Vec<_> = (0..64)
        .map(|i| block(0x1000 + i * 0x100, &operations))
        .collect();
    let program = program(&blocks);
    let layout = Layout::build(&program, blocks.len(), false, BASE);
    let shared = program.block(BlockId(0)).exit.gpr[1];
    for index in 0..blocks.len() {
        assert_eq!(program.block(BlockId(index)).exit.gpr[1], shared);
        assert_eq!(layout.flags_entry[index], layout.flags_entry[0]);
        let effect = program.block(BlockId(index)).effects().next().unwrap();
        for recovery in [
            &effect.before,
            &effect.after,
            &program.block(BlockId(index)).exit,
        ] {
            let live = program.dependency_order(&recovery.scalar_roots());
            for (i, &left) in live.iter().enumerate() {
                for &right in &live[i + 1..] {
                    assert_ne!(
                        layout.values[left.0], layout.values[right.0],
                        "simultaneous roots and dependencies cannot share a local"
                    );
                }
            }
        }
    }
    assert!(layout.values[shared.0].is_some());
    assert!(
        layout.next <= single_layout.next + 2,
        "the declaration count depends on simultaneous block values, not owner block count"
    );
}

#[test]
fn numeric_values_use_exact_types_and_every_emitted_definition_has_storage() {
    let operations = [
        fp(X87Operation::ChangeSign, false),
        fp(X87Operation::Load(X87Format::Float32), true),
        fp(
            X87Operation::StoreRegister {
                index: 0,
                pop: true,
            },
            false,
        ),
    ];
    let program = program(&[block(0x1000, &operations), block(0x1100, &operations)]);
    let layout = Layout::build(&program, 2, true, BASE);
    let mut seen = [false; 3];
    for (id, local) in layout.values.iter().enumerate() {
        let Some(local) = *local else {
            continue;
        };
        let ty = program.value(ExecValueId(id)).ty;
        seen[kind(ty)] = true;
        assert_eq!(
            declared_type(&layout, local),
            match ty {
                ValueType::I32 => ValType::I32,
                ValueType::I64 => ValType::I64,
                ValueType::F64 => ValType::F64,
            }
        );
    }
    assert_eq!(
        seen, [true; 3],
        "raw limbs, tags/status and native lanes are all present"
    );
    for block in [BlockId(0), BlockId(1)] {
        for effect in program.block(block).effects() {
            for definition in effect.definitions() {
                assert!(
                    layout.values[definition.0].is_some(),
                    "even a dead emitted definition needs typed storage"
                );
            }
        }
    }
    assert_eq!(layout.flags_entry[0], layout.flags_entry[1]);
    assert!(
        layout.next < BASE + program.value_count() as u32,
        "independent numeric blocks do not each reserve a whole-owner value arena"
    );
}

#[test]
fn flag_origin_tuples_reuse_only_after_the_last_recovery_use() {
    let increment = Operation::Unary {
        kind: UnaryKind::Inc,
        destination: Location32::Register(Register32::Eax),
    };
    let program = program(&[block(0x1000, &[increment, increment, increment])]);
    let layout = Layout::build(&program, 1, false, BASE);
    assert_eq!(layout.flags_effect.len(), 3);
    let entry = layout.flags_entry[0].unwrap();
    assert_ne!(layout.flags_effect[0], Some(entry));
    assert_eq!(layout.flags_effect[1], Some(entry));
    assert_eq!(layout.flags_effect[2], layout.flags_effect[0]);
    for origin in &layout.flags_effect {
        for field in 0..7 {
            assert_eq!(
                declared_type(&layout, origin.unwrap() + field),
                ValType::I32
            );
        }
    }
    let (_, flags) = memberships(&program, 1, false);
    assert_eq!(flags[0][0].interval, Interval { first: 0, last: 1 });
    assert_eq!(flags[1][0].interval, Interval { first: 1, last: 3 });
    assert_eq!(flags[2][0].interval, Interval { first: 3, last: 5 });
}

#[test]
fn tagged_sign_outputs_cannot_overwrite_old_inputs_or_before_fault_recovery() {
    let program = program(&[block(
        0x1000,
        &[
            fp(X87Operation::ChangeSign, false),
            fp(X87Operation::LoadRegister { index: 0 }, false),
            fp(X87Operation::ChangeSign, false),
        ],
    )]);
    let layout = Layout::build(&program, 1, true, BASE);
    let effect = program.block(BlockId(0)).effects().next().unwrap();
    assert!(matches!(
        effect.before.fp.payload[0],
        crate::cpu::core::region::values::executable::fp::FpPayload::Tagged { .. }
    ));
    let mut roots = effect.before.roots();
    roots.extend(effect.inputs());
    let old = program.dependency_order(&roots);
    for definition in effect.definitions() {
        for &input in &old {
            if definition != input {
                assert_ne!(
                    layout.values[definition.0], layout.values[input.0],
                    "a tagged sign may define validity before reading old raw limbs and can still fail"
                );
            }
        }
    }
}

#[test]
fn same_time_intervals_interfere_but_adjacent_times_can_share_typed_storage() {
    let mut colors = Colors::default();
    let first = colors.allocate(&[Membership {
        block: 0,
        interval: Interval { first: 0, last: 2 },
    }]);
    let simultaneous = colors.allocate(&[Membership {
        block: 0,
        interval: Interval { first: 2, last: 3 },
    }]);
    let later = colors.allocate(&[Membership {
        block: 0,
        interval: Interval { first: 4, last: 5 },
    }]);
    let other_block = colors.allocate(&[Membership {
        block: 1,
        interval: Interval { first: 0, last: 5 },
    }]);
    assert_ne!(first, simultaneous);
    assert_eq!(first, later);
    assert_eq!(first, other_block);
}
