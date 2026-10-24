use super::*;
use crate::{
    cpu::core::trace::PendingColdObservation,
    memory::{PageRange, Permissions},
};

fn memory(bytes: &[u8]) -> AddressSpace {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(0x1000), bytes).unwrap();
    memory
}

fn owner(memory: &AddressSpace, instructions: usize) -> CompiledCode {
    compile(
        memory,
        GuestAddress(0x1000),
        CompileOptions {
            instructions,
            blocks: 1,
            ..CompileOptions::default()
        },
    )
    .unwrap()
}

fn block(owner: &CompiledCode, offset: u32) -> TraceEvent {
    assert_eq!(owner.blocks.len(), 1);
    let source = &owner.blocks[0];
    TraceEvent {
        kind: BLOCK,
        pc: source.pc,
        end: source.end,
        offset,
        owner: owner.identity(),
    }
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

fn capture(
    memory: &AddressSpace,
    owner: &CompiledCode,
    events: &[TraceEvent],
    cold: &[CompletedColdObservation],
) -> Option<CapturedGraph> {
    capture_graph(
        memory,
        GuestAddress(0x1000),
        CompileOptions::default(),
        events,
        &[owner],
        memory.code_source_version(),
        cold,
    )
    .unwrap()
}

fn assert_capture(captured: &CapturedGraph, pcs: &[u32], edges: &[(u32, u32)]) {
    assert_eq!(
        captured.graph.nodes.keys().copied().collect::<Vec<_>>(),
        pcs
    );
    assert_eq!(
        captured.graph.ports.iter().copied().collect::<Vec<_>>(),
        pcs
    );
    assert_eq!(
        captured.graph.edges.iter().copied().collect::<Vec<_>>(),
        edges
    );
}

#[test]
fn forward_span_marker_confirms_only_the_prefix_before_an_unexecuted_terminal() {
    let memory = memory(&[0x90, 0x90, 0x0f, 0x0b]);
    let owner = owner(&memory, 3);
    assert_eq!(owner.block_pcs.as_ref(), [0x1000, 0x1001, 0x1002]);
    let events = [block(&owner, 0), block(&owner, 2)];
    let bytes = owner.wasm().to_vec();
    let mut ram = [0; 4096];
    memory.read(GuestAddress(0x1000), &mut ram).unwrap();
    let revision = memory.transaction_version();

    let captured = capture(&memory, &owner, &events, &[]).unwrap();
    assert_capture(
        &captured,
        &[0x1000, 0x1001],
        &[(0x1000, 0x1001), (0x1001, 0x1002)],
    );
    let legacy = compile_observed(
        &memory,
        GuestAddress(0x1000),
        CompileOptions::default(),
        &events,
        &[&owner],
        memory.code_source_version(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        legacy.instruction_pcs().collect::<Vec<_>>(),
        [0x1000, 0x1001]
    );
    let mut after = [0; 4096];
    memory.read(GuestAddress(0x1000), &mut after).unwrap();
    assert_eq!(after, ram);
    assert_eq!(memory.transaction_version(), revision);
    assert_eq!(owner.wasm(), bytes);
    assert!(owner.is_current(&memory));
}

#[test]
fn forward_span_marker_after_a_planned_callback_does_not_claim_the_terminal_suffix() {
    // NOP; CPUID; NOP; NOP; UD2 remain one original source block.
    let memory = memory(&[0x90, 0x0f, 0xa2, 0x90, 0x90, 0x0f, 0x0b]);
    let owner = owner(&memory, 5);
    assert_eq!(
        owner.block_pcs.as_ref(),
        [0x1000, 0x1001, 0x1003, 0x1004, 0x1005]
    );
    assert_eq!(owner.native.len(), 1);
    let events = [
        block(&owner, 0),
        TraceEvent {
            kind: NUMERIC,
            pc: 0x1001,
            end: 0x1003,
            offset: 0,
            owner: owner.identity(),
        },
        block(&owner, 2),
        block(&owner, 4),
    ];
    let captured = capture(&memory, &owner, &events, &[]).unwrap();
    let pcs = [0x1000, 0x1001, 0x1003, 0x1004];
    assert_capture(
        &captured,
        &pcs,
        &[
            (0x1000, 0x1001),
            (0x1001, 0x1003),
            (0x1003, 0x1004),
            (0x1004, 0x1005),
        ],
    );
    let legacy = compile_observed(
        &memory,
        GuestAddress(0x1000),
        CompileOptions::default(),
        &events,
        &[&owner],
        memory.code_source_version(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(legacy.instruction_pcs().collect::<Vec<_>>(), pcs);
    assert_eq!(legacy.native.len(), 1);
    assert_eq!(legacy.native[0].pc, 0x1001);
    assert_eq!(legacy.native[0].operation, owner.native[0].operation);
}

#[test]
fn completed_precise_instruction_cuts_an_empty_or_nonempty_retained_prefix() {
    let memory = memory(&[0x90, 0x90, 0x0f, 0x0b]);
    let owner = owner(&memory, 3);
    for pc in [0x1000, 0x1001] {
        let cold = [completed(&memory, pc, pc + 1)];
        let events = [block(&owner, 0), cold[0].event(0).unwrap()];
        let captured = capture(&memory, &owner, &events, &cold).unwrap();
        if pc == 0x1000 {
            assert_capture(&captured, &[0x1000], &[(0x1000, 0x1001)]);
        } else {
            assert_capture(
                &captured,
                &[0x1000, 0x1001],
                &[(0x1000, 0x1001), (0x1001, 0x1002)],
            );
        }
        assert!(
            compile_observed(
                &memory,
                GuestAddress(0x1000),
                CompileOptions::default(),
                &events,
                &[&owner],
                memory.code_source_version(),
            )
            .unwrap()
            .is_none(),
            "legacy capture still cannot authenticate cold observations"
        );
    }
}

#[test]
fn unconfirmed_fault_site_never_fabricates_a_completed_precise_instruction() {
    // The final retained marker is before a potentially faulting MOV, not proof
    // that it executed. Only the preceding completed NOPs enter the graph.
    let memory = memory(&[0x90, 0x90, 0x8b, 0x03, 0x90, 0x0f, 0x0b]);
    let owner = owner(&memory, 5);
    let cold = [completed(&memory, 0x1001, 0x1002)];
    let events = [
        block(&owner, 0),
        cold[0].event(0).unwrap(),
        block(&owner, 2),
    ];
    let captured = capture(&memory, &owner, &events, &cold).unwrap();
    assert_capture(
        &captured,
        &[0x1000, 0x1001],
        &[(0x1000, 0x1001), (0x1001, 0x1002)],
    );
    assert!(capture(&memory, &owner, &events[..1], &[]).is_none());
}

#[test]
fn cold_cut_requires_current_source_and_an_authenticated_completed_record() {
    let mut memory = memory(&[0x90, 0x90, 0x0f, 0x0b]);
    let owner = owner(&memory, 3);
    let cold = [completed(&memory, 0x1000, 0x1001)];
    let mut events = [block(&owner, 0), cold[0].event(0).unwrap()];
    assert!(capture(&memory, &owner, &events, &[]).is_none());
    events[1].end = 0x1002;
    assert!(capture(&memory, &owner, &events, &cold).is_none());
    events[1] = cold[0].event(0).unwrap();
    let source_version = memory.code_source_version();
    memory.write(GuestAddress(0x1000), &[0x90]).unwrap();
    assert!(!owner.is_current(&memory));
    for expected in [source_version, memory.code_source_version()] {
        assert!(
            capture_graph(
                &memory,
                GuestAddress(0x1000),
                CompileOptions::default(),
                &events,
                &[&owner],
                expected,
                &cold,
            )
            .unwrap()
            .is_none()
        );
    }
}

#[test]
fn equal_offset_backedges_and_foreign_cold_targets_keep_the_whole_source_suffix() {
    let loop_memory = memory(&[0x90, 0xeb, 0xfd]);
    let loop_owner = owner(&loop_memory, 2);
    let events = [block(&loop_owner, 0), block(&loop_owner, 0)];
    assert_capture(
        &capture(&loop_memory, &loop_owner, &events, &[]).unwrap(),
        &[0x1000, 0x1001],
        &[(0x1000, 0x1001), (0x1001, 0x1000)],
    );

    let mut memory = memory(&[0x90, 0xeb, 0x0d, 0x90, 0x0f, 0x0b]);
    memory.write(GuestAddress(0x1010), &[0x90]).unwrap();
    let owner = owner(&memory, 2);
    let cold = [completed(&memory, 0x1010, 0x1011)];
    let events = [block(&owner, 0), cold[0].event(0).unwrap()];
    assert_capture(
        &capture(&memory, &owner, &events, &cold).unwrap(),
        &[0x1000, 0x1001, 0x1010],
        &[(0x1000, 0x1001), (0x1001, 0x1010), (0x1010, 0x1011)],
    );
}
