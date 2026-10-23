use super::*;
use crate::{
    cpu::{core::trace::PendingColdObservation, x86::ir::Condition},
    memory::{PageRange, Permissions},
};

fn memory(bytes: &[u8]) -> AddressSpace {
    let mut memory = AddressSpace::new(2).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(0x1000), bytes).unwrap();
    memory
}

fn completed(memory: &AddressSpace, pc: u32, post: u32) -> CompletedColdObservation {
    let (instruction, _) = decoded_node(memory, pc).unwrap();
    PendingColdObservation::capture(
        memory,
        &NativeInstruction {
            pc,
            next: instruction.next,
            operation: instruction.operation.unwrap(),
        },
    )
    .unwrap()
    .complete(memory, post)
    .unwrap()
}

fn compile_cold(
    memory: &AddressSpace,
    observations: &[CompletedColdObservation],
    previous: Option<&CompiledCode>,
) -> Option<CompiledCode> {
    let events: Vec<_> = observations
        .iter()
        .enumerate()
        .map(|(index, observation)| observation.event(index).unwrap())
        .collect();
    compile_observed_with_cold(
        memory,
        GuestAddress(observations[0].pc()),
        CompileOptions::default(),
        &events,
        &[],
        memory.code_source_version(),
        observations,
        previous,
    )
    .unwrap()
}

#[test]
fn one_completed_cold_event_is_authoritative_without_a_future_witness() {
    let memory = memory(&[0x90, 0x0f, 0x0b]);
    let code = compile_cold(&memory, &[completed(&memory, 0x1000, 0x1001)], None).unwrap();
    assert!(code.is_observed_family());
    assert_eq!(code.instruction_pcs().collect::<Vec<_>>(), [0x1000]);
    assert_eq!(
        code.observed.as_ref().unwrap().edges,
        BTreeSet::from([(0x1000, 0x1001)])
    );
    assert_eq!(code.entrypoints(), &[GuestAddress(0x1000)]);
}

#[test]
fn zero_owner_never_grants_legacy_block_or_numeric_authority() {
    let memory = memory(&[0x90, 0x90]);
    let observations = [completed(&memory, 0x1000, 0x1001)];
    for kind in [BLOCK, NUMERIC] {
        let event = TraceEvent {
            kind,
            pc: 0x1000,
            end: 0x1001,
            owner: 0,
            offset: 0,
        };
        assert!(
            compile_observed_with_cold(
                &memory,
                GuestAddress(0x1000),
                CompileOptions::default(),
                &[event],
                &[],
                memory.code_source_version(),
                &observations,
                None
            )
            .unwrap()
            .is_none()
        );
    }
    let mut event = observations[0].event(0).unwrap();
    event.owner = 17;
    assert!(
        compile_observed_with_cold(
            &memory,
            GuestAddress(0x1000),
            CompileOptions::default(),
            &[event],
            &[],
            memory.code_source_version(),
            &observations,
            None
        )
        .unwrap()
        .is_none()
    );
}

#[test]
fn captured_source_must_survive_completion_and_later_compilation() {
    let mut memory = memory(&[0x90]);
    let instruction = NativeInstruction {
        pc: 0x1000,
        next: 0x1001,
        operation: Operation::Nop,
    };
    let pending = PendingColdObservation::capture(&memory, &instruction).unwrap();
    memory.write(GuestAddress(0x1000), &[0x90]).unwrap();
    assert!(
        pending.complete(&memory, 0x1001).is_none(),
        "same bytes do not revive old source authority"
    );
    let observation = completed(&memory, 0x1000, 0x1001);
    memory.write(GuestAddress(0x1000), &[0x90]).unwrap();
    assert!(compile_cold(&memory, &[observation], None).is_none());
}

#[test]
fn foreign_memory_and_forged_decode_or_successor_do_not_form_code() {
    let first = memory(&[0x90, 0x90]);
    let second = memory(&[0x90, 0x90]);
    let observation = completed(&first, 0x1000, 0x1001);
    assert!(compile_cold(&second, &[observation], None).is_none());
    let pending = PendingColdObservation::capture(
        &first,
        &NativeInstruction {
            pc: 0x1000,
            next: 0x1001,
            operation: Operation::ConditionalJump {
                condition: Condition::Equal,
                target: GuestAddress(0x1001),
            },
        },
    )
    .unwrap();
    assert!(compile_cold(&first, &[pending.complete(&first, 0x1001).unwrap()], None).is_none());
    assert!(compile_cold(&first, &[completed(&first, 0x1000, 0x1007)], None).is_none());
    assert!(
        compile_cold(
            &first,
            &[
                completed(&first, 0x1000, 0x1001),
                completed(&first, 0x1000, 0x1001)
            ],
            None
        )
        .is_none()
    );
}

#[test]
fn historical_observed_sets_union_both_arms_and_preserve_every_port() {
    // jne1004; nop; nop; nop
    let memory = memory(&[0x75, 0x02, 0x90, 0x90, 0x90]);
    let first = compile_cold(
        &memory,
        &[
            completed(&memory, 0x1000, 0x1002),
            completed(&memory, 0x1002, 0x1003),
        ],
        None,
    )
    .unwrap();
    let next = compile_cold(
        &memory,
        &[
            completed(&memory, 0x1000, 0x1004),
            completed(&memory, 0x1004, 0x1005),
        ],
        Some(&first),
    )
    .unwrap();
    assert!(next.observed_grows(&first));
    assert_eq!(
        next.instruction_pcs().collect::<Vec<_>>(),
        [0x1000, 0x1002, 0x1004]
    );
    assert_eq!(
        next.entrypoints(),
        &[
            GuestAddress(0x1000),
            GuestAddress(0x1002),
            GuestAddress(0x1004)
        ]
    );
    assert!(next.supports_entry(0x1002));
    assert!(!next.supports_entry(0x1001));
    assert!(
        first
            .observed
            .as_ref()
            .unwrap()
            .edges
            .is_subset(&next.observed.as_ref().unwrap().edges)
    );
    assert!(
        compile_cold(
            &memory,
            &[
                completed(&memory, 0x1000, 0x1002),
                completed(&memory, 0x1002, 0x1003),
            ],
            Some(&next)
        )
        .is_none()
    );
    assert!(
        compile_cold(&memory, &[completed(&memory, 0x1002, 0x1003)], Some(&next)).is_none(),
        "a rotated root is not graph growth"
    );
}

#[test]
fn instruction_limit_decline_keeps_the_old_exact_graph_intact() {
    let memory = memory(&[0x90, 0x90, 0x90]);
    let first = compile_cold(
        &memory,
        &[
            completed(&memory, 0x1000, 0x1001),
            completed(&memory, 0x1001, 0x1002),
        ],
        None,
    )
    .unwrap();
    let observations = [completed(&memory, 0x1002, 0x1003)];
    let event = observations[0].event(0).unwrap();
    let result = compile_observed_with_cold(
        &memory,
        GuestAddress(0x1002),
        CompileOptions {
            instructions: 1,
            ..CompileOptions::default()
        },
        &[event],
        &[],
        memory.code_source_version(),
        &observations,
        Some(&first),
    )
    .unwrap();
    assert!(result.is_none());
    assert_eq!(
        first.instruction_pcs().collect::<Vec<_>>(),
        [0x1000, 0x1001]
    );
    assert!(first.is_current(&memory));
}

#[test]
fn contiguous_cold_nodes_share_a_block_without_advertising_unobserved_bytes() {
    let memory = memory(&[0x90, 0x90, 0x90, 0x0f, 0x0b]);
    let code = compile_cold(
        &memory,
        &[
            completed(&memory, 0x1000, 0x1001),
            completed(&memory, 0x1001, 0x1002),
            completed(&memory, 0x1002, 0x1003),
        ],
        None,
    )
    .unwrap();
    assert_eq!(code.blocks.len(), 1);
    assert_eq!(code.entrypoints().len(), 3);
    assert!(!code.supports_entry(0x1003));
}

#[test]
fn retained_numeric_prefix_and_final_completed_cold_record_share_one_dictionary() {
    let memory = memory(&[0xd8, 0xc1, 0x90, 0x0f, 0x0b]);
    let owner = compile(
        &memory,
        GuestAddress(0x1000),
        CompileOptions {
            instructions: 2,
            blocks: 1,
            ..CompileOptions::default()
        },
    )
    .unwrap();
    let block = &owner.blocks[0];
    let cold = [completed(&memory, 0x1002, 0x1003)];
    let events = [
        TraceEvent {
            kind: BLOCK,
            pc: 0x1000,
            end: block.end,
            offset: 0,
            owner: owner.identity(),
        },
        TraceEvent {
            kind: NUMERIC,
            pc: 0x1000,
            end: 0x1002,
            offset: 0,
            owner: owner.identity(),
        },
        cold[0].event(0).unwrap(),
    ];
    let code = compile_observed_with_cold(
        &memory,
        GuestAddress(0x1000),
        CompileOptions::default(),
        &events,
        &[&owner],
        memory.code_source_version(),
        &cold,
        None,
    )
    .unwrap()
    .unwrap();
    assert_eq!(code.instruction_pcs().collect::<Vec<_>>(), [0x1000, 0x1002]);
    assert_eq!(code.native.len(), 1);
    assert_eq!(code.native[0].pc, 0x1000);
    assert_eq!(code.native[0].operation, owner.native[0].operation);
    assert_eq!(
        code.observed.as_ref().unwrap().edges,
        BTreeSet::from([(0x1000, 0x1002), (0x1002, 0x1003)])
    );
    assert_eq!(owner.entrypoints(), &[GuestAddress(0x1000)]);
}

#[test]
fn final_retained_event_still_cannot_claim_its_own_unexecuted_prefix() {
    let memory = memory(&[0x90, 0x0f, 0x0b]);
    let owner = compile(
        &memory,
        GuestAddress(0x1000),
        CompileOptions {
            instructions: 1,
            blocks: 1,
            ..CompileOptions::default()
        },
    )
    .unwrap();
    let event = TraceEvent {
        kind: BLOCK,
        pc: 0x1000,
        end: 0x1001,
        offset: 0,
        owner: owner.identity(),
    };
    assert!(
        compile_observed_with_cold(
            &memory,
            GuestAddress(0x1000),
            CompileOptions::default(),
            &[event],
            &[&owner],
            memory.code_source_version(),
            &[],
            None
        )
        .unwrap()
        .is_none()
    );
}

#[test]
fn fresh_rw_authority_changes_do_not_masquerade_as_code_source_changes() {
    let mut memory = memory(&[0x90]);
    let instruction = NativeInstruction {
        pc: 0x1000,
        next: 0x1001,
        operation: Operation::Nop,
    };
    let pending = PendingColdObservation::capture(&memory, &instruction).unwrap();
    let code_stamp = memory.code_source_version();
    let validation = memory.code_validation_version();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x2000), 1).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    assert_ne!(memory.code_validation_version(), validation);
    assert_eq!(memory.code_source_version(), code_stamp);
    let observation = pending.complete(&memory, 0x1001).unwrap();
    assert!(compile_cold(&memory, &[observation], None).is_some());
}

#[test]
fn remapping_or_writing_an_executable_alias_revokes_held_cold_source() {
    let mut ordinary = memory(&[0x90]);
    let instruction = NativeInstruction {
        pc: 0x1000,
        next: 0x1001,
        operation: Operation::Nop,
    };
    let pending = PendingColdObservation::capture(&ordinary, &instruction).unwrap();
    let range = PageRange::new(GuestAddress(0x1000), 1).unwrap();
    ordinary.unmap(range).unwrap();
    ordinary.map_zeroed(range, Permissions::ALL).unwrap();
    ordinary.write(GuestAddress(0x1000), &[0x90]).unwrap();
    assert!(pending.complete(&ordinary, 0x1001).is_none());

    let mut aliases = AddressSpace::new(2).unwrap();
    let shared = aliases.allocate_shared_pages(1, &[0x90]).unwrap();
    aliases
        .map_shared_pages(range, &shared, 0, Permissions::ALL, false)
        .unwrap();
    aliases
        .map_shared_pages(
            PageRange::new(GuestAddress(0x2000), 1).unwrap(),
            &shared,
            0,
            Permissions::READ_WRITE,
            false,
        )
        .unwrap();
    let pending = PendingColdObservation::capture(&aliases, &instruction).unwrap();
    aliases.write(GuestAddress(0x2000), &[0x90]).unwrap();
    assert!(pending.complete(&aliases, 0x1001).is_none());
}

#[test]
fn changing_only_an_indirect_external_target_is_not_executable_growth() {
    let memory = memory(&[0xc3]);
    let first = compile_cold(&memory, &[completed(&memory, 0x1000, 0x2000)], None).unwrap();
    let id = first.identity();
    let bytes = first.wasm().to_vec();
    for target in [0x2001, 0x3000, 0x4000, 0xffff_ffff] {
        assert!(
            compile_cold(&memory, &[completed(&memory, 0x1000, target)], Some(&first)).is_none()
        );
    }
    assert_eq!(first.identity(), id);
    assert_eq!(first.wasm(), bytes);
    assert_eq!(
        first.observed.as_ref().unwrap().edges,
        BTreeSet::from([(0x1000, 0x2000)])
    );
}

#[test]
fn total_observed_edges_use_a_fixed_twice_instruction_budget_before_emission() {
    let memory = memory(&[0xc3, 0xc3, 0xc3]);
    let sequence = [
        0x1000, 0x1000, 0x1001, 0x1000, 0x1002, 0x1001, 0x1001, 0x1002, 0x1002, 0x1000,
    ];
    let observations: Vec<_> = sequence
        .iter()
        .enumerate()
        .map(|(index, &pc)| {
            completed(
                &memory,
                pc,
                sequence.get(index + 1).copied().unwrap_or(0x1000),
            )
        })
        .collect();
    let events: Vec<_> = observations
        .iter()
        .enumerate()
        .map(|(index, observation)| observation.event(index).unwrap())
        .collect();
    let options = CompileOptions {
        instructions: 3,
        blocks: 3,
        ..CompileOptions::default()
    };
    assert!(
        compile_observed_with_cold(
            &memory,
            GuestAddress(0x1000),
            options,
            &events,
            &[],
            memory.code_source_version(),
            &observations,
            None
        )
        .unwrap()
        .is_none(),
        "nine actually observed RET edges exceed the six-edge budget despite only three decoded nodes"
    );

    let prefix = [
        completed(&memory, 0x1000, 0x1000),
        completed(&memory, 0x1000, 0x1001),
        completed(&memory, 0x1001, 0x1001),
        completed(&memory, 0x1001, 0x1000),
    ];
    let old = compile_cold(&memory, &prefix, None).unwrap();
    let old_bytes = old.wasm().to_vec();
    assert!(
        compile_observed_with_cold(
            &memory,
            GuestAddress(0x1000),
            options,
            &events,
            &[],
            memory.code_source_version(),
            &observations,
            Some(&old)
        )
        .unwrap()
        .is_none()
    );
    assert_eq!(old.wasm(), old_bytes);
    assert_eq!(old.observed.as_ref().unwrap().edges.len(), 4);
    assert_eq!(old.instruction_pcs().collect::<Vec<_>>(), [0x1000, 0x1001]);
}
