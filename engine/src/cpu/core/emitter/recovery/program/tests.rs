use super::*;
use crate::cpu::{
    core::{
        program as decoded,
        region::{BlockId, Region, values::RegionPlan},
    },
    x86::{
        Register32,
        ir::{BinaryKind, CarryKind, EffectiveAddress, Location32, Operation, Value32},
    },
};

fn executable(operations: &[Operation]) -> ExecutableProgram {
    let block = decoded::Block {
        entry: 0x1000,
        instructions: operations
            .iter()
            .copied()
            .enumerate()
            .map(|(index, operation)| decoded::Instruction {
                pc: 0x1000 + index as u32,
                next: 0x1001 + index as u32,
                operation: Some(operation),
                gate: None,
            })
            .collect(),
    };
    let region = Region::from_blocks(&[block], 0x1000).unwrap();
    let values = RegionPlan::analyze(&region);
    ExecutableProgram::analyze(&region, &values)
}

fn mov(destination: Register32, source: Value32) -> Operation {
    Operation::Move {
        destination: Location32::Register(destination),
        source,
    }
}

fn leaf(program: &mut Program, value: usize, local: u32, op: SourceOp) -> RefId {
    program
        .reference(
            ValueKey::Executable(ExecValueId(value)),
            Color {
                local,
                ty: ValueType::I32,
            },
            op,
        )
        .unwrap()
}

#[test]
fn a_color_is_reusable_between_sites_but_never_inside_one_request_closure() {
    let mut program = Program::new(8);
    let old = leaf(&mut program, 0, 200, SourceOp::Available);
    let newer = leaf(&mut program, 1, 200, SourceOp::Constant { bits: 9 });
    assert_eq!(program.validate_roots(&[old]).unwrap(), vec![old]);
    assert_eq!(program.validate_roots(&[newer]).unwrap(), vec![newer]);
    assert!(matches!(
        program.validate_roots(&[old, newer]),
        Err(Error::ColorConflict {
            color: Color { local: 200, .. },
            ..
        })
    ));

    let dependent = leaf(
        &mut program,
        2,
        201,
        SourceOp::ShiftLeft32 {
            value: old,
            amount: 1,
        },
    );
    assert!(
        matches!(
            program.validate_roots(&[dependent, newer]),
            Err(Error::ColorConflict { .. })
        ),
        "an available dependency cannot be overwritten before its lazy user reads it"
    );
}

#[test]
fn exact_source_mode_is_interned_and_reference_limit_refusal_is_neutral() {
    let mut program = Program::new(2);
    let captured = leaf(&mut program, 0, 200, SourceOp::Available);
    let lazy = leaf(&mut program, 0, 200, SourceOp::PhysicalInput { local: 2 });
    assert_ne!(
        captured, lazy,
        "same value/local is not the same initialization proof"
    );
    assert_eq!(leaf(&mut program, 0, 200, SourceOp::Available), captured);
    let before = program.refs().to_vec();
    let colors: Vec<_> = program.colors().collect();
    assert_eq!(
        program.reference(
            ValueKey::Executable(ExecValueId(1)),
            Color {
                local: 201,
                ty: ValueType::I32
            },
            SourceOp::Constant { bits: 1 },
        ),
        Err(Error::ReferenceLimit)
    );
    assert_eq!(program.refs(), before);
    assert_eq!(program.colors().collect::<Vec<_>>(), colors);
}

#[test]
fn literal_sources_share_only_exact_bits_type_and_color_without_merging_effect_identity() {
    let mut program = Program::new(16);
    let first = leaf(&mut program, 0, 200, SourceOp::Constant { bits: 7 });
    let same = leaf(&mut program, 999, 200, SourceOp::Constant { bits: 7 });
    assert_eq!(
        first, same,
        "a literal has no defining effect or input snapshot"
    );
    assert_eq!(program.validate_roots(&[first, same]).unwrap(), vec![first]);
    let other_bits = leaf(&mut program, 0, 200, SourceOp::Constant { bits: 8 });
    let other_color = leaf(&mut program, 0, 201, SourceOp::Constant { bits: 7 });
    assert_ne!(first, other_bits);
    assert_ne!(first, other_color);
    assert_eq!(
        program.reference(
            ValueKey::Executable(ExecValueId(0)),
            Color {
                local: 200,
                ty: ValueType::I64
            },
            SourceOp::Constant { bits: 7 },
        ),
        Err(Error::TypeMismatch)
    );
    let wide = program
        .reference(
            ValueKey::Executable(ExecValueId(0)),
            Color {
                local: 202,
                ty: ValueType::I64,
            },
            SourceOp::Constant { bits: 7 },
        )
        .unwrap();
    assert_ne!(first, wide);
    let old = leaf(&mut program, 1, 203, SourceOp::Available);
    let new = leaf(&mut program, 2, 203, SourceOp::Available);
    let physical = leaf(&mut program, 1, 203, SourceOp::PhysicalInput { local: 2 });
    assert_ne!(
        old, new,
        "available effect values retain their immutable identity"
    );
    assert_ne!(
        old, physical,
        "availability and physical-input proofs stay distinct"
    );
    assert!(matches!(
        program.validate_roots(&[old, new]),
        Err(Error::ColorConflict { .. })
    ));
}

#[test]
fn compare_then_replace_keeps_old_flags_dependencies_before_parallel_gpr_projection() {
    let executable = executable(&[
        Operation::Binary {
            kind: BinaryKind::Cmp,
            destination: Location32::Register(Register32::Eax),
            source: Value32::Immediate(1),
        },
        mov(Register32::Eax, Value32::Immediate(0)),
    ]);
    let layout = Layout::build(&executable, 1, false, 179);
    let block = executable.block(BlockId(0));
    let available = BTreeSet::new();
    let context = SourceContext {
        executable: &executable,
        layout: &layout,
        available: &available,
        physical_gpr: Some(block.entry.gpr),
        physical_flags: Some(block.entry.flags),
        active_effect: None,
        captured_effect: None,
    };
    let mut program = Program::new(64);
    let projection = program
        .scalar(
            &block.exit,
            KernelMasks {
                gpr: 0,
                flags: false,
            },
            &context,
        )
        .unwrap();
    let result = projection.gpr[0].unwrap();
    let flags = projection
        .flags
        .as_ref()
        .unwrap()
        .arithmetic
        .as_ref()
        .unwrap();
    assert_eq!(flags.kind, FlagKind::Sub);
    assert!(matches!(
        program.get(result).op,
        SourceOp::Constant { bits: 0 }
    ));
    assert!(matches!(
        program.get(flags.left).op,
        SourceOp::PhysicalInput { local: 2 }
    ));
    assert!(
        program
            .validate_roots(&projection.roots())
            .unwrap()
            .contains(&flags.left)
    );
    assert_ne!(program.get(result).color, program.get(flags.left).color);
    assert_eq!(
        program.colors().filter(|c| c.ty == ValueType::I32).count(),
        program.colors().count()
    );
    assert_eq!(projection.flags.as_ref().unwrap().origin.len(), 7);
    assert!(
        program.reverse_refs().all(|(id, source)| source
            .op
            .dependencies()
            .iter()
            .all(|&dep| dep < id))
    );
}

#[test]
fn an_ordered_read_is_available_or_current_kernel_output_and_can_never_be_replayed() {
    let executable = executable(&[mov(
        Register32::Eax,
        Value32::Memory(EffectiveAddress {
            base: Some(Register32::Ebx),
            ..Default::default()
        }),
    )]);
    let layout = Layout::build(&executable, 1, false, 179);
    let block = executable.block(BlockId(0));
    let effect = block.effects().next().unwrap();
    let loaded = effect.after.gpr[0];
    let empty = BTreeSet::new();
    let mut context = SourceContext {
        executable: &executable,
        layout: &layout,
        available: &empty,
        physical_gpr: Some(effect.after.gpr),
        physical_flags: Some(effect.after.flags),
        active_effect: None,
        captured_effect: None,
    };
    let mut program = Program::new(32);
    assert_eq!(
        program.value(loaded, &context),
        Err(Error::UncapturedEffect(loaded))
    );
    assert!(
        program.refs().is_empty(),
        "refusal never invents a read recipe"
    );
    context.active_effect = Some(effect.id);
    let current = program.value(loaded, &context).unwrap();
    assert!(matches!(
        program.get(current).op,
        SourceOp::PhysicalInput { local: 2 }
    ));
    let captured = BTreeSet::from([loaded]);
    context.available = &captured;
    context.active_effect = None;
    let earlier = program.value(loaded, &context).unwrap();
    assert!(matches!(program.get(earlier).op, SourceOp::Available));
    assert_ne!(current, earlier);
}

#[test]
fn kernel_owned_partial_definitions_and_flags_are_not_recovered_from_before_state() {
    let executable = executable(&[mov(Register32::Eax, Value32::Immediate(7))]);
    let layout = Layout::build(&executable, 1, false, 179);
    let block = executable.block(BlockId(0));
    let available = BTreeSet::new();
    let context = SourceContext {
        executable: &executable,
        layout: &layout,
        available: &available,
        physical_gpr: Some(block.entry.gpr),
        physical_flags: None,
        active_effect: None,
        captured_effect: None,
    };
    let mut program = Program::new(16);
    let projection = program
        .scalar(
            &block.exit,
            KernelMasks {
                gpr: 1,
                flags: true,
            },
            &context,
        )
        .unwrap();
    assert_eq!(projection.gpr, [None; 8]);
    assert!(projection.flags.is_none());
    assert!(program.refs().is_empty());
}

#[test]
fn an_allocated_current_effect_flags_tuple_is_not_an_initialization_proof() {
    let executable = executable(&[Operation::Carry {
        kind: CarryKind::Set,
    }]);
    let layout = Layout::build(&executable, 1, false, 179);
    let block = executable.block(BlockId(0));
    let effect = block.effects().next().unwrap();
    let available = BTreeSet::new();
    let mut context = SourceContext {
        executable: &executable,
        layout: &layout,
        available: &available,
        physical_gpr: Some(effect.after.gpr),
        physical_flags: None,
        active_effect: Some(effect.id),
        captured_effect: None,
    };
    let mut program = Program::new(16);
    assert_eq!(
        program.scalar(
            &effect.after,
            KernelMasks {
                gpr: 0,
                flags: false
            },
            &context
        ),
        Err(Error::UncapturedFlags(FlagsOrigin::Effect(effect.id)))
    );
    assert!(program.refs().is_empty());
    context.captured_effect = Some(effect.id);
    let projection = program
        .scalar(
            &effect.after,
            KernelMasks {
                gpr: 0,
                flags: false,
            },
            &context,
        )
        .unwrap();
    assert_eq!(projection.flags.unwrap().origin.len(), 7);
}

#[test]
fn distinct_ordered_read_prefixes_share_the_input_and_do_not_clone_lazy_expression_graphs() {
    for count in [1usize, 8, 32, 128] {
        let mut operations = Vec::new();
        for index in 0..count {
            operations.push(Operation::Lea {
                destination: Register32::Eax,
                address: EffectiveAddress {
                    base: Some(Register32::Eax),
                    displacement: 1,
                    ..Default::default()
                },
            });
            operations.push(mov(
                Register32::Edx,
                Value32::Memory(EffectiveAddress {
                    base: Some(Register32::Ebx),
                    displacement: index as u32 * 4,
                    ..Default::default()
                }),
            ));
        }
        let executable = executable(&operations);
        let layout = Layout::build(&executable, 1, false, 179);
        let block = executable.block(BlockId(0));
        let available = BTreeSet::new();
        let mut program = Program::new(3 * count + 8);
        for effect in block.effects() {
            let mut physical = effect.before.gpr;
            physical[0] = block.entry.gpr[0];
            let context = SourceContext {
                executable: &executable,
                layout: &layout,
                available: &available,
                physical_gpr: Some(physical),
                physical_flags: Some(effect.before.flags),
                active_effect: Some(effect.id),
                captured_effect: None,
            };
            let projection = program
                .scalar(
                    &effect.before,
                    KernelMasks {
                        gpr: 0,
                        flags: false,
                    },
                    &context,
                )
                .unwrap();
            assert_eq!(projection.gpr.iter().flatten().count(), 1);
            assert_eq!(
                program.validate_roots(&projection.roots()).unwrap().len(),
                3,
                "each request needs only the input, its literal increment, and one add"
            );
            let length = program.refs().len();
            assert_eq!(
                program
                    .scalar(
                        &effect.before,
                        KernelMasks {
                            gpr: 0,
                            flags: false
                        },
                        &context
                    )
                    .unwrap(),
                projection
            );
            assert_eq!(
                program.refs().len(),
                length,
                "repeated guards reuse exact sources"
            );
        }
        assert_eq!(
            program
                .refs()
                .iter()
                .filter(|r| matches!(r.op, SourceOp::PhysicalInput { local: 2 }))
                .count(),
            1
        );
        assert!(
            program.refs().len() <= 2 * count + 1,
            "only one shared input plus one constant/add pair per distinct prefix"
        );
        // Constants are also shared with the load's address expressions. Their
        // allocator intervals can cross later arithmetic prefixes, so total
        // owner color count is not the same as a request's three live refs.
    }
}
