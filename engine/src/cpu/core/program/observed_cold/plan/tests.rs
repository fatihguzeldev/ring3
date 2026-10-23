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
    // A: nop; jmp B. B: nop; jmp A. C: inc eax; jmp A.
    memory
        .write(GuestAddress(0x1000), &[0x90, 0xeb, 0x0d])
        .unwrap();
    memory
        .write(GuestAddress(0x1010), &[0x90, 0xeb, 0xed])
        .unwrap();
    memory
        .write(GuestAddress(0x1020), &[0x40, 0xeb, 0xdd])
        .unwrap();
    memory.write(GuestAddress(0x1030), &[0x90, 0x90]).unwrap();
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

fn prepare(
    memory: &AddressSpace,
    path: &[(u32, u32)],
    parents: &[&CompiledCode],
    options: CompileOptions,
) -> Option<ObservedPlan> {
    let cold: Vec<_> = path
        .iter()
        .map(|&(pc, post)| cold(memory, pc, post))
        .collect();
    let events: Vec<_> = cold
        .iter()
        .enumerate()
        .map(|(index, item)| item.event(index).unwrap())
        .collect();
    prepare_observed_plan(
        memory,
        GuestAddress(path[0].0),
        options,
        &events,
        &[],
        memory.code_source_version(),
        &cold,
        parents,
    )
    .unwrap()
}

fn family(memory: &AddressSpace, path: &[(u32, u32)]) -> CompiledCode {
    emit_observed_plan(
        memory,
        prepare(memory, path, &[], CompileOptions::default()).unwrap(),
    )
    .unwrap()
    .unwrap()
}

#[test]
fn actual_cross_family_history_is_planned_without_losing_either_anchor_or_ports() {
    let memory = memory();
    let a = family(&memory, &[(0x1000, 0x1001), (0x1001, 0x1010)]);
    let b = family(&memory, &[(0x1010, 0x1011), (0x1011, 0x1000)]);
    let unrelated = family(&memory, &[(0x1030, 0x1031)]);
    let a_bytes = a.wasm().to_vec();
    let b_bytes = b.wasm().to_vec();
    let plan = prepare(
        &memory,
        &[
            (0x1000, 0x1001),
            (0x1001, 0x1010),
            (0x1010, 0x1011),
            (0x1011, 0x1000),
        ],
        &[&unrelated, &b, &a],
        CompileOptions::default(),
    )
    .unwrap();
    assert_eq!(plan.parent_identities(), [a.identity(), b.identity()]);
    assert_eq!(plan.anchor(), 0x1000);
    assert!(plan.preserves(&a) && plan.preserves(&b));
    assert!(!plan.preserves(&unrelated));
    assert!(plan.has_executable_gain());
    assert_eq!(
        plan.instruction_pcs().collect::<Vec<_>>(),
        [0x1000, 0x1001, 0x1010, 0x1011]
    );
    let merged = emit_observed_plan(&memory, plan).unwrap().unwrap();
    assert!(merged.observed_grows(&a) && merged.observed_grows(&b));
    assert_eq!(merged.trace_root(), Some(0x1000));
    assert_eq!(
        merged.entrypoints(),
        [
            GuestAddress(0x1000),
            GuestAddress(0x1001),
            GuestAddress(0x1010),
            GuestAddress(0x1011)
        ]
    );
    assert_eq!(a.wasm(), a_bytes);
    assert_eq!(b.wasm(), b_bytes);
}

#[test]
fn a_completed_edge_can_absorb_a_final_unexecuted_but_already_observed_parent() {
    let memory = memory();
    let a = family(&memory, &[(0x1000, 0x1001), (0x1001, 0x1010)]);
    let b = family(&memory, &[(0x1010, 0x1011), (0x1011, 0x1000)]);
    let plan = prepare(
        &memory,
        &[(0x1000, 0x1001), (0x1001, 0x1010)],
        &[&a, &b],
        CompileOptions::default(),
    )
    .unwrap();
    assert!(plan.preserves(&a) && plan.preserves(&b));
    assert_eq!(plan.parent_identities().len(), 2);
    assert_eq!(plan.instruction_pcs().count(), 4);
}

#[test]
fn historical_actual_exit_can_connect_a_freshly_completed_successor() {
    let memory = memory();
    let a = family(&memory, &[(0x1000, 0x1001), (0x1001, 0x1010)]);
    let plan = prepare(
        &memory,
        &[(0x1010, 0x1011)],
        &[&a],
        CompileOptions::default(),
    )
    .unwrap();
    assert!(plan.preserves(&a));
    assert_eq!(plan.anchor(), 0x1000);
    assert_eq!(
        plan.instruction_pcs().collect::<Vec<_>>(),
        [0x1000, 0x1001, 0x1010]
    );
}

#[test]
fn equal_or_rotated_recapture_has_no_emission_plan() {
    let memory = memory();
    let a = family(&memory, &[(0x1000, 0x1001), (0x1001, 0x1010)]);
    assert!(
        prepare(
            &memory,
            &[(0x1000, 0x1001), (0x1001, 0x1010)],
            &[&a],
            CompileOptions::default()
        )
        .is_none()
    );
    assert!(
        prepare(
            &memory,
            &[(0x1001, 0x1010)],
            &[&a],
            CompileOptions::default()
        )
        .is_none()
    );
}

#[test]
fn exact_fingerprint_is_independent_of_parent_order_but_keeps_limits_and_sources() {
    let mut memory = memory();
    let a = family(&memory, &[(0x1000, 0x1001), (0x1001, 0x1010)]);
    let b = family(&memory, &[(0x1010, 0x1011), (0x1011, 0x1000)]);
    let path = [(0x1000, 0x1001), (0x1001, 0x1010), (0x1010, 0x1011)];
    let first = prepare(&memory, &path, &[&a, &b], CompileOptions::default()).unwrap();
    let repeated = prepare(&memory, &path, &[&b, &a, &b], CompileOptions::default()).unwrap();
    assert_eq!(first.fingerprint(), repeated.fingerprint());
    let tighter = prepare(
        &memory,
        &path,
        &[&a, &b],
        CompileOptions {
            wasm_bytes: 128,
            ..CompileOptions::default()
        },
    )
    .unwrap();
    assert_ne!(first.fingerprint(), tighter.fingerprint());
    memory.write(GuestAddress(0x1000), &[0x90]).unwrap();
    assert!(!first.is_current(&memory));
    assert!(emit_observed_plan(&memory, first).unwrap().is_none());
}

#[test]
fn over_bound_union_falls_back_to_only_completed_unowned_prefix() {
    let memory = memory();
    let a = family(&memory, &[(0x1000, 0x1001), (0x1001, 0x1010)]);
    let b = family(&memory, &[(0x1010, 0x1011), (0x1011, 0x1000)]);
    let path = [(0x1020, 0x1021), (0x1021, 0x1000), (0x1000, 0x1001)];
    let observations: Vec<_> = path
        .iter()
        .map(|&(pc, post)| cold(&memory, pc, post))
        .collect();
    let events: Vec<_> = observations
        .iter()
        .enumerate()
        .map(|(i, item)| item.event(i).unwrap())
        .collect();
    let options = CompileOptions {
        instructions: 2,
        ..CompileOptions::default()
    };
    assert!(
        prepare_observed_plan(
            &memory,
            GuestAddress(0x1020),
            options,
            &events,
            &[],
            memory.code_source_version(),
            &observations,
            &[&a, &b]
        )
        .unwrap()
        .is_none()
    );
    let plan = prepare_cold_prefix_plan(&memory, options, &events, &observations, &|pc| {
        a.supports_entry(pc) || b.supports_entry(pc)
    })
    .unwrap()
    .unwrap();
    assert_eq!(plan.anchor(), 0x1020);
    assert!(plan.parent_identities().is_empty());
    assert_eq!(plan.instruction_pcs().collect::<Vec<_>>(), [0x1020, 0x1021]);
    assert_eq!(
        plan.fingerprint
            .graph
            .edges
            .iter()
            .copied()
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([(0x1020, 0x1021), (0x1021, 0x1000)])
    );
    let prefix = emit_observed_plan(&memory, plan).unwrap().unwrap();
    assert!(!prefix.supports_entry(0x1000));
    assert!(a.is_current(&memory) && b.is_current(&memory));
}

#[test]
fn prefix_resource_boundary_keeps_longest_real_prefix_and_non_cold_boundary_stops_it() {
    let memory = memory();
    let path = [(0x1000, 0x1001), (0x1001, 0x1010), (0x1010, 0x1011)];
    let observations: Vec<_> = path
        .iter()
        .map(|&(pc, post)| cold(&memory, pc, post))
        .collect();
    let mut events: Vec<_> = observations
        .iter()
        .enumerate()
        .map(|(i, item)| item.event(i).unwrap())
        .collect();
    let plan = prepare_cold_prefix_plan(
        &memory,
        CompileOptions {
            instructions: 1,
            ..CompileOptions::default()
        },
        &events,
        &observations,
        &|_| false,
    )
    .unwrap()
    .unwrap();
    assert_eq!(plan.instruction_pcs().collect::<Vec<_>>(), [0x1000]);
    events[1].kind = BLOCK;
    events[1].owner = 42;
    let plan = prepare_cold_prefix_plan(
        &memory,
        CompileOptions::default(),
        &events,
        &observations,
        &|_| false,
    )
    .unwrap()
    .unwrap();
    assert_eq!(plan.instruction_pcs().collect::<Vec<_>>(), [0x1000]);
}

#[test]
fn repeated_completed_span_reuses_decode_but_validates_the_last_record_and_union_budget() {
    let mut memory = memory();
    memory.write(GuestAddress(0x1040), &[0xc3]).unwrap();
    let mut observations: Vec<_> = (0..CAPACITY)
        .map(|_| cold(&memory, 0x1040, 0x1040))
        .collect();
    let events: Vec<_> = observations
        .iter()
        .enumerate()
        .map(|(index, item)| item.event(index).unwrap())
        .collect();
    let options = CompileOptions {
        instructions: 1,
        blocks: 1,
        ..CompileOptions::default()
    };
    let captured = capture_graph(
        &memory,
        GuestAddress(0x1040),
        options,
        &events,
        &[],
        memory.code_source_version(),
        &observations,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        captured.sources.len(),
        CAPACITY + 1,
        "one decode authority plus each independently held completed-event authority"
    );
    let plan = prepare_observed_plan(
        &memory,
        GuestAddress(0x1040),
        options,
        &events,
        &[],
        memory.code_source_version(),
        &observations,
        &[],
    )
    .unwrap()
    .unwrap();
    assert_eq!(plan.instruction_pcs().collect::<Vec<_>>(), [0x1040]);
    assert_eq!(
        plan.fingerprint
            .graph
            .edges
            .iter()
            .copied()
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([(0x1040, 0x1040)])
    );
    assert_eq!(plan.fingerprint.sources.len(), 1);
    observations.last_mut().unwrap().instruction.operation = Operation::Nop;
    assert!(
        prepare_observed_plan(
            &memory,
            GuestAddress(0x1040),
            options,
            &events,
            &[],
            memory.code_source_version(),
            &observations,
            &[]
        )
        .unwrap()
        .is_none(),
        "a reused PC cannot bypass the final completed instruction's semantic validation"
    );

    let a = family(&memory, &[(0x1000, 0x1001), (0x1001, 0x1010)]);
    let b = family(&memory, &[(0x1010, 0x1011), (0x1011, 0x1000)]);
    // Only A is newly confirmed. Its completed exit authorizes B's historical
    // nodes, but that inherited union must obey the requested three-node cap.
    assert!(
        prepare(
            &memory,
            &[(0x1000, 0x1001), (0x1001, 0x1010)],
            &[&a, &b],
            CompileOptions {
                instructions: 3,
                ..CompileOptions::default()
            }
        )
        .is_none()
    );
    assert_eq!(a.instruction_count(), 2);
    assert_eq!(b.instruction_count(), 2);
}

#[test]
fn artifact_and_exact_fingerprint_share_one_frozen_history_without_losing_value_equality() {
    fn send_sync<T: Send + Sync>() {}
    send_sync::<CompiledCode>();
    send_sync::<ObservedPlanFingerprint>();
    let memory = memory();
    let path = [(0x1000, 0x1001), (0x1001, 0x1010)];
    let plan = prepare(&memory, &path, &[], CompileOptions::default()).unwrap();
    let independently_prepared = prepare(&memory, &path, &[], CompileOptions::default()).unwrap();
    assert!(!Arc::ptr_eq(
        &plan.fingerprint.graph,
        &independently_prepared.fingerprint.graph
    ));
    assert_eq!(
        plan.fingerprint(),
        independently_prepared.fingerprint(),
        "exact equality compares immutable values rather than Arc identity"
    );
    let code = emit_observed_plan(&memory, plan).unwrap().unwrap();
    let history = code.observed.as_ref().unwrap();
    let fingerprint = code.observed_fingerprint().unwrap();
    assert!(Arc::ptr_eq(history, &fingerprint.graph));
    assert_eq!(Arc::strong_count(history), 2);
    assert_eq!(history.nodes.len(), 2);
    assert_eq!(history.ports.as_ref(), [0x1000, 0x1001]);
    let charge = code.resident_charge();
    let memo = fingerprint.clone();
    assert!(Arc::ptr_eq(history, &memo.graph));
    assert_eq!(Arc::strong_count(history), 3);
    assert_eq!(
        code.resident_charge(),
        charge,
        "one artifact charge does not duplicate its shared immutable history"
    );
    assert_eq!(
        memo.resident_charge(),
        std::mem::size_of::<ObservedPlanFingerprint>()
            + memo.metadata_charge()
            + history.resident_charge()
    );
}

#[test]
fn declared_charge_counts_reserved_code_and_dictionary_storage_not_only_encoded_length() {
    let memory = memory();
    let mut code = family(&memory, &[(0x1000, 0x1001), (0x1001, 0x1010)]);
    let encoded = code.wasm().to_vec();
    let before = code.resident_charge();
    assert!(before > code.wasm().len());
    let capacity = code.wasm.capacity();
    code.wasm.reserve(capacity + 4096);
    let reserved = code.wasm.capacity() - capacity;
    assert!(reserved >= 4096);
    assert_eq!(code.wasm(), encoded);
    assert_eq!(code.resident_charge(), before + reserved);
    let before = code.resident_charge();
    let capacity = code.native.capacity();
    code.native.reserve(capacity + 4);
    let reserved = (code.native.capacity() - capacity) * std::mem::size_of::<NativeInstruction>();
    assert!(reserved > 0);
    assert_eq!(code.resident_charge(), before + reserved);
    let before = code.resident_charge();
    let fingerprint = code.observed_fingerprint.as_mut().unwrap();
    let capacity = fingerprint.parents.capacity();
    fingerprint.parents.reserve(capacity + 3);
    let reserved = (fingerprint.parents.capacity() - capacity) * std::mem::size_of::<u64>();
    assert!(reserved > 0);
    assert_eq!(code.resident_charge(), before + reserved);
    assert!(code.observed.as_ref().unwrap().contains_node(0x1000));
    assert!(code.is_current(&memory));
}
