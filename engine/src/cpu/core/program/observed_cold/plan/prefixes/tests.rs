use super::*;
use crate::cpu::core::trace::PendingColdObservation;
use crate::memory::{PageRange, Permissions};

fn memory() -> AddressSpace {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    for pc in (0x1000..0x1800).step_by(0x100) {
        // nop; jmp esi. the recording supplies the actual next owner.
        memory.write(GuestAddress(pc), &[0x90, 0xff, 0xe6]).unwrap();
    }
    memory
}
fn cold(memory: &AddressSpace, pc: u32, post: u32) -> CompletedColdObservation {
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
fn owner(memory: &AddressSpace, pc: u32) -> CompiledCode {
    let cold = vec![cold(memory, pc, pc + 1), cold(memory, pc + 1, 0x1000)];
    let events: Vec<_> = cold
        .iter()
        .enumerate()
        .map(|(i, c)| c.event(i).unwrap())
        .collect();
    let plan = prepare_observed_plan(
        memory,
        GuestAddress(pc),
        CompileOptions::default(),
        &events,
        &[],
        memory.code_source_version(),
        &cold,
        &[],
    )
    .unwrap()
    .unwrap();
    emit_observed_plan(memory, plan).unwrap().unwrap()
}
fn block(owner: &CompiledCode) -> TraceEvent {
    let block = &owner.blocks[0];
    TraceEvent {
        kind: BLOCK,
        pc: block.pc,
        end: block.end,
        offset: 0,
        owner: owner.identity(),
    }
}
fn candidates(
    memory: &AddressSpace,
    events: &[TraceEvent],
    owners: &[&CompiledCode],
    ingresses: &[RecordedIngress],
    cold: &[CompletedColdObservation],
) -> Vec<ExecutionCandidate> {
    let root = if events[0].kind == COLD_COMPLETE {
        events[0].pc
    } else {
        let owner = owners
            .iter()
            .find(|owner| owner.identity() == events[0].owner)
            .unwrap();
        let block = owner.blocks.iter().find(|b| b.pc == events[0].pc).unwrap();
        owner.block_pcs[(block.first + events[0].offset) as usize]
    };
    let mut result = Vec::new();
    for_each_execution_candidate(
        memory,
        GuestAddress(root),
        CompileOptions::default(),
        events,
        owners,
        memory.code_source_version(),
        cold,
        ingresses,
        &mut |c| result.push(c),
    )
    .unwrap();
    result
}

#[test]
fn geometric_complete_span_prefixes_ignore_variable_unexecuted_tail() {
    let memory = memory();
    let owners: Vec<_> = (0x1000..0x1700)
        .step_by(0x100)
        .map(|pc| owner(&memory, pc))
        .collect();
    let refs: Vec<_> = owners.iter().collect();
    let mut events: Vec<_> = owners.iter().map(block).collect();
    events.push(block(&owners[0]));
    let ingress = [RecordedIngress {
        event_index: 0,
        origin: Some(0),
        target: 0x1000,
        owner: owners[0].identity(),
    }];
    let original = candidates(&memory, &events, &refs, &ingress, &[]);
    assert_eq!(
        original
            .iter()
            .map(|c| c.completed_work())
            .collect::<Vec<_>>(),
        [14, 8, 4]
    );
    assert!(
        original
            .iter()
            .all(|c| !c.is_closed_cycle() && c.origin() == Some(0))
    );
    assert_eq!(
        original.first().unwrap().plan().instruction_pcs().count(),
        14
    );
    assert!(
        !original
            .first()
            .unwrap()
            .plan()
            .fingerprint
            .graph
            .edges
            .contains(&(0x1601, 0x1000))
    );
    events[7] = block(&owners[1]);
    let changed = candidates(&memory, &events, &refs, &ingress, &[]);
    for (a, b) in original.iter().zip(&changed) {
        assert_eq!(a.plan().fingerprint(), b.plan().fingerprint());
        assert!(a.plan().has_executable_gain());
    }
    assert_eq!(
        owners.iter().map(|o| o.instruction_count()).sum::<usize>(),
        14
    );
}

#[test]
fn actual_two_cycle_certificate_and_open_evidence_remain_distinct() {
    let memory = memory();
    let a = owner(&memory, 0x1000);
    let b = owner(&memory, 0x1100);
    let events = [block(&a), block(&b), block(&a), block(&b), block(&a)];
    let ingress = [RecordedIngress {
        event_index: 0,
        origin: None,
        target: 0x1000,
        owner: a.identity(),
    }];
    let found = candidates(&memory, &events, &[&a, &b], &ingress, &[]);
    let closed = found.iter().find(|c| c.is_closed_cycle()).unwrap();
    assert_eq!(closed.plan().completed_cycle_count(), 2);
    assert_eq!(closed.plan().cycle_instruction_count(), 4);
    assert_eq!(closed.plan().cross_owner_edge_count(), 2);
    assert_eq!(closed.completed_work(), 8);
    assert!(
        found
            .iter()
            .any(|c| !c.is_closed_cycle() && c.plan().completed_cycle_count() == 0)
    );
}

#[test]
fn cold_and_unconfirmed_spans_cannot_become_prefix_evidence() {
    let memory = memory();
    let a = owner(&memory, 0x1000);
    let b = owner(&memory, 0x1100);
    let c = owner(&memory, 0x1200);
    let d = owner(&memory, 0x1300);
    let observation = cold(&memory, 0x1100, 0x1101);
    let observation2 = cold(&memory, 0x1101, 0x1200);
    let cold = [observation, observation2];
    let events = [
        block(&a),
        block(&b),
        cold[0].event(0).unwrap(),
        cold[1].event(1).unwrap(),
        block(&c),
        block(&d),
        block(&a),
    ];
    let ingress = [
        RecordedIngress {
            event_index: 0,
            origin: None,
            target: 0x1000,
            owner: a.identity(),
        },
        RecordedIngress {
            event_index: 4,
            origin: Some(0x1100),
            target: 0x1200,
            owner: c.identity(),
        },
    ];
    let found = candidates(&memory, &events, &[&a, &b, &c, &d], &ingress, &cold);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].origin(), Some(0x1100));
    assert_eq!(
        found[0].plan().instruction_pcs().collect::<Vec<_>>(),
        [0x1200, 0x1201, 0x1300, 0x1301]
    );
    assert!(
        !found[0]
            .plan()
            .instruction_pcs()
            .any(|pc| pc == 0x1000 || pc == 0x1100)
    );
    let short = [block(&a), block(&b)];
    assert!(candidates(&memory, &short, &[&a, &b], &ingress[..1], &[]).is_empty());
}

#[test]
fn missing_or_mismatched_ingress_never_infers_a_context_from_owner_changes() {
    let memory = memory();
    let a = owner(&memory, 0x1000);
    let b = owner(&memory, 0x1100);
    let events = [block(&a), block(&b), block(&a)];
    assert!(candidates(&memory, &events, &[&a, &b], &[], &[]).is_empty());
    let forged = [RecordedIngress {
        event_index: 0,
        origin: Some(0),
        target: 0x1001,
        owner: a.identity(),
    }];
    assert!(candidates(&memory, &events, &[&a, &b], &forged, &[]).is_empty());
}

#[test]
fn a_foreign_completed_cold_target_confirms_the_full_terminal_span_only() {
    let memory = memory();
    let a = owner(&memory, 0x1000);
    let b = owner(&memory, 0x1100);
    let cold = [cold(&memory, 0x1400, 0x1401)];
    let events = [block(&a), block(&b), cold[0].event(0).unwrap()];
    let ingress = [RecordedIngress {
        event_index: 0,
        origin: Some(0x1700),
        target: 0x1000,
        owner: a.identity(),
    }];
    let found = candidates(&memory, &events, &[&a, &b], &ingress, &cold);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].completed_work(), 4);
    assert_eq!(
        found[0].plan().instruction_pcs().collect::<Vec<_>>(),
        [0x1000, 0x1001, 0x1100, 0x1101]
    );
    assert!(
        !found[0]
            .plan()
            .fingerprint
            .graph
            .edges
            .iter()
            .any(|&(source, target)| source == 0x1400 || target == 0x1400)
    );
    assert_eq!(found[0].plan().prefix_spans, 2);
}

#[test]
fn numeric_empty_prefix_and_unconfirmed_final_numeric_do_not_add_spans() {
    let mut memory = memory();
    // cpuid; nop; jmp esi share original geometry but three trace markers
    // around CPUID license only one completed instruction at that point.
    memory
        .write(GuestAddress(0x1000), &[0x0f, 0xa2, 0x90, 0xff, 0xe6])
        .unwrap();
    let a = compile(
        &memory,
        GuestAddress(0x1000),
        CompileOptions {
            instructions: 3,
            blocks: 1,
            ..CompileOptions::default()
        },
    )
    .unwrap();
    let b = owner(&memory, 0x1100);
    assert_eq!(a.blocks.len(), 1);
    let numeric = TraceEvent {
        kind: NUMERIC,
        pc: 0x1000,
        end: 0x1002,
        offset: 0,
        owner: a.identity(),
    };
    let mut suffix = block(&a);
    suffix.offset = 1;
    let events = [block(&a), numeric, suffix, block(&b), block(&a), numeric];
    let ingress = [RecordedIngress {
        event_index: 0,
        origin: None,
        target: 0x1000,
        owner: a.identity(),
    }];
    let found = candidates(&memory, &events, &[&a, &b], &ingress, &[]);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].completed_work(), 5);
    assert_eq!(found[0].plan().prefix_spans, 3);
    assert_eq!(found[0].plan().completed_cycle_count(), 0);
    assert_eq!(
        found[0].plan().instruction_pcs().collect::<Vec<_>>(),
        [0x1000, 0x1002, 0x1003, 0x1100, 0x1101]
    );
}

#[test]
fn retained_source_geometry_uses_the_actual_entry_offset_as_root() {
    let mut memory = memory();
    memory
        .write(GuestAddress(0x1000), &[0x90, 0x0f, 0xa2, 0x90, 0xff, 0xe6])
        .unwrap();
    let a = compile(
        &memory,
        GuestAddress(0x1000),
        CompileOptions {
            instructions: 4,
            blocks: 1,
            ..CompileOptions::default()
        },
    )
    .unwrap();
    let b = owner(&memory, 0x1100);
    assert_eq!(a.blocks.len(), 1);
    assert!(a.hot_span(0x1003).is_some());
    let mut entry = block(&a);
    entry.offset = 2;
    let events = [entry, block(&b), entry];
    let mut ingress = [RecordedIngress {
        event_index: 0,
        origin: Some(0),
        target: 0x1003,
        owner: a.identity(),
    }];
    let found = candidates(&memory, &events, &[&a, &b], &ingress, &[]);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].plan().anchor(), 0x1003);
    assert_eq!(
        found[0].plan().instruction_pcs().collect::<Vec<_>>(),
        [0x1003, 0x1004, 0x1100, 0x1101]
    );
    ingress[0].target = 0x1000;
    assert!(candidates(&memory, &events, &[&a, &b], &ingress, &[]).is_empty());
}

#[test]
fn source_epoch_or_owner_source_change_declines_before_any_candidate() {
    let mut memory = memory();
    let a = owner(&memory, 0x1000);
    let b = owner(&memory, 0x1100);
    let events = [block(&a), block(&b), block(&a)];
    let ingress = [RecordedIngress {
        event_index: 0,
        origin: None,
        target: 0x1000,
        owner: a.identity(),
    }];
    let before = memory.code_source_version();
    assert_eq!(
        candidates(&memory, &events, &[&a, &b], &ingress, &[]).len(),
        1
    );
    memory.write(GuestAddress(0x1000), &[0x90]).unwrap();
    for version in [before, memory.code_source_version()] {
        let mut count = 0;
        for_each_execution_candidate(
            &memory,
            GuestAddress(0x1000),
            CompileOptions::default(),
            &events,
            &[&a, &b],
            version,
            &[],
            &ingress,
            &mut |_| count += 1,
        )
        .unwrap();
        assert_eq!(count, 0);
    }
}
