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
    memory
        .write(GuestAddress(0x1000), &[0x74, 0x2e, 0x90, 0xeb, 0x0b])
        .unwrap();
    memory
        .write(GuestAddress(0x1010), &[0x90, 0xeb, 0xed])
        .unwrap();
    memory
        .write(GuestAddress(0x1030), &[0x90, 0xeb, 0xcd])
        .unwrap();
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

fn family(memory: &AddressSpace, path: &[(u32, u32)]) -> CompiledCode {
    let cold: Vec<_> = path
        .iter()
        .map(|&(pc, post)| completed(memory, pc, post))
        .collect();
    let events: Vec<_> = cold
        .iter()
        .enumerate()
        .map(|(index, observation)| observation.event(index).unwrap())
        .collect();
    let plan = prepare_observed_plan(
        memory,
        GuestAddress(path[0].0),
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

fn parents(memory: &AddressSpace) -> (CompiledCode, CompiledCode) {
    (
        family(
            memory,
            &[
                (0x1000, 0x1030),
                (0x1030, 0x1031),
                (0x1031, 0x1000),
                (0x1000, 0x1002),
                (0x1002, 0x1003),
                (0x1003, 0x1010),
            ],
        ),
        family(memory, &[(0x1010, 0x1011), (0x1011, 0x1000)]),
    )
}

fn block(code: &CompiledCode, pc: u32) -> TraceEvent {
    let block = code.blocks.iter().find(|block| block.pc == pc).unwrap();
    TraceEvent {
        kind: BLOCK,
        pc,
        end: block.end,
        offset: 0,
        owner: code.identity(),
    }
}

fn timeline(a: &CompiledCode, b: &CompiledCode) -> Vec<TraceEvent> {
    vec![
        block(a, 0x1000),
        block(a, 0x1002),
        block(b, 0x1010),
        block(a, 0x1000),
        block(a, 0x1002),
        block(b, 0x1010),
        block(a, 0x1000),
    ]
}

fn prepare(
    memory: &AddressSpace,
    root: u32,
    events: &[TraceEvent],
    owners: &[&CompiledCode],
    options: CompileOptions,
) -> Option<WindowPlan> {
    prepare_hot_window(
        memory,
        GuestAddress(root),
        options,
        events,
        owners,
        memory.code_source_version(),
        &[],
    )
    .unwrap()
}

#[test]
fn short_cross_owner_window_does_not_inherit_or_replace_family_history() {
    let memory = memory();
    let (a, b) = parents(&memory);
    let original_a = a.wasm().to_vec();
    let original_b = b.wasm().to_vec();
    let options = CompileOptions {
        instructions: 5,
        blocks: 3,
        ..CompileOptions::default()
    };
    let plan = prepare(&memory, 0x1000, &timeline(&a, &b), &[&a, &b], options).unwrap();
    assert_eq!(plan.cross_owner_edge_count(), 2);
    assert_eq!(plan.completed_cycle_count(), 2);
    assert_eq!(plan.cycle_instruction_count(), 5);
    assert!(plan.has_executable_gain());
    assert_eq!(
        plan.instruction_pcs().collect::<Vec<_>>(),
        [0x1000, 0x1002, 0x1003, 0x1010, 0x1011]
    );
    assert_eq!(
        plan.entrypoints(),
        [
            GuestAddress(0x1000),
            GuestAddress(0x1002),
            GuestAddress(0x1010)
        ]
    );
    assert_eq!(
        plan.hot_ports(),
        [
            HotSpan {
                entry: 0x1000,
                end: 0x1002,
                instructions: 1,
                port: 0
            },
            HotSpan {
                entry: 0x1002,
                end: 0x1005,
                instructions: 2,
                port: 1
            },
            HotSpan {
                entry: 0x1010,
                end: 0x1013,
                instructions: 2,
                port: 2
            },
        ]
    );
    assert_eq!(a.instruction_count() + b.instruction_count(), 7);
    let code = emit_hot_window(&memory, plan).unwrap().unwrap();
    assert!(code.is_execution_window());
    assert!(!code.is_observed_family());
    assert!(code.observed_fingerprint().is_none());
    assert!(code.window_fingerprint().is_some());
    assert!(code.supports_entry(0x1000));
    assert!(code.supports_entry(0x1002));
    assert!(code.supports_entry(0x1010));
    assert!(!code.supports_entry(0x1003));
    assert!(code.instruction_pcs().any(|pc| pc == 0x1010));
    assert_eq!(
        code.entrypoints(),
        [
            GuestAddress(0x1000),
            GuestAddress(0x1002),
            GuestAddress(0x1010)
        ]
    );
    assert_eq!(a.wasm(), original_a);
    assert_eq!(b.wasm(), original_b);
    assert!(a.supports_entry(0x1030) && b.supports_entry(0x1011));
}

#[test]
fn window_fingerprint_ignores_owner_ids_and_only_root_for_rotated_graphs() {
    let memory = memory();
    let (a, b) = parents(&memory);
    let original = prepare(
        &memory,
        0x1000,
        &timeline(&a, &b),
        &[&a, &b],
        CompileOptions::default(),
    )
    .unwrap();
    let (next_a, next_b) = parents(&memory);
    let repeated = prepare(
        &memory,
        0x1000,
        &timeline(&next_a, &next_b),
        &[&next_a, &next_b],
        CompileOptions::default(),
    )
    .unwrap();
    assert_ne!(a.identity(), next_a.identity());
    assert_eq!(original.fingerprint(), repeated.fingerprint());
    let rotated = prepare(
        &memory,
        0x1010,
        &[
            block(&b, 0x1010),
            block(&a, 0x1000),
            block(&a, 0x1002),
            block(&b, 0x1010),
            block(&a, 0x1000),
            block(&a, 0x1002),
            block(&b, 0x1010),
        ],
        &[&a, &b],
        CompileOptions::default(),
    )
    .unwrap();
    assert_ne!(original.fingerprint(), rotated.fingerprint());
    assert!(original.fingerprint().same_graph(rotated.fingerprint()));
}

#[test]
fn interior_root_partition_is_exact_monotone_public_entry_growth() {
    let memory = memory();
    let (a, b) = parents(&memory);
    let original = prepare(
        &memory,
        0x1000,
        &timeline(&a, &b),
        &[&a, &b],
        CompileOptions::default(),
    )
    .unwrap();
    let mut interior = block(&a, 0x1002);
    interior.offset = 1;
    let rotated = prepare(
        &memory,
        0x1003,
        &[
            interior,
            block(&b, 0x1010),
            block(&a, 0x1000),
            block(&a, 0x1002),
            block(&b, 0x1010),
            block(&a, 0x1000),
            block(&a, 0x1002),
            block(&b, 0x1010),
        ],
        &[&a, &b],
        CompileOptions::default(),
    )
    .unwrap();
    assert_eq!(
        original.fingerprint.graph.nodes,
        rotated.fingerprint.graph.nodes
    );
    assert_eq!(
        original.fingerprint.graph.edges,
        rotated.fingerprint.graph.edges
    );
    assert_eq!(
        original.fingerprint.graph.ports,
        rotated.fingerprint.graph.ports
    );
    assert_ne!(
        original.fingerprint.graph.starts,
        rotated.fingerprint.graph.starts
    );
    assert_ne!(original.fingerprint(), rotated.fingerprint());
    assert!(!original.fingerprint().same_graph(rotated.fingerprint()));
    assert!(
        rotated
            .fingerprint()
            .strictly_covers(original.fingerprint())
    );
    assert!(
        !original
            .fingerprint()
            .strictly_covers(rotated.fingerprint())
    );
    assert_eq!(
        rotated.entrypoints(),
        [
            GuestAddress(0x1000),
            GuestAddress(0x1002),
            GuestAddress(0x1003),
            GuestAddress(0x1010)
        ]
    );
    let original_span = original
        .hot_ports()
        .into_iter()
        .find(|port| port.entry == 0x1002)
        .unwrap();
    let split_span = rotated
        .hot_ports()
        .into_iter()
        .find(|port| port.entry == 0x1002)
        .unwrap();
    assert_eq!((original_span.end, original_span.instructions), (0x1005, 2));
    assert_eq!((split_span.end, split_span.instructions), (0x1003, 1));

    let mut lost = rotated.fingerprint().clone();
    lost.ports.retain(|port| port.entry != 0x1002);
    assert!(!lost.strictly_covers(original.fingerprint()));
    let mut increased_quota = rotated.fingerprint().clone();
    increased_quota
        .ports
        .iter_mut()
        .find(|port| port.entry == 0x1002)
        .unwrap()
        .instructions = 3;
    assert!(!increased_quota.strictly_covers(original.fingerprint()));
}

#[test]
fn same_owner_and_unconfirmed_final_boundary_have_no_window_gain() {
    let memory = memory();
    let (a, b) = parents(&memory);
    let whole = family(
        &memory,
        &[
            (0x1000, 0x1002),
            (0x1002, 0x1003),
            (0x1003, 0x1010),
            (0x1010, 0x1011),
            (0x1011, 0x1000),
        ],
    );
    assert!(
        prepare(
            &memory,
            0x1000,
            &timeline(&whole, &whole),
            &[&whole],
            CompileOptions::default(),
        )
        .is_none()
    );
    assert!(
        prepare(
            &memory,
            0x1002,
            &[block(&a, 0x1002), block(&b, 0x1010)],
            &[&a, &b],
            CompileOptions::default(),
        )
        .is_none()
    );
}

#[test]
fn cold_capture_and_forged_owner_geometry_cannot_become_hot_windows() {
    let memory = memory();
    let (a, b) = parents(&memory);
    let observation = completed(&memory, 0x1002, 0x1003);
    assert!(
        prepare_hot_window(
            &memory,
            GuestAddress(0x1002),
            CompileOptions::default(),
            &[observation.event(0).unwrap()],
            &[],
            memory.code_source_version(),
            &[observation],
        )
        .unwrap()
        .is_none()
    );
    let mut forged = timeline(&a, &b);
    forged[2].end = forged[2].end.wrapping_add(1);
    assert!(
        prepare(
            &memory,
            0x1000,
            &forged,
            &[&a, &b],
            CompileOptions::default(),
        )
        .is_none()
    );
}

#[test]
fn window_bounds_decline_before_emission_and_source_is_rechecked_after_planning() {
    let mut memory = memory();
    let (a, b) = parents(&memory);
    for options in [
        CompileOptions {
            instructions: 4,
            ..CompileOptions::default()
        },
        CompileOptions {
            blocks: 2,
            ..CompileOptions::default()
        },
    ] {
        assert!(prepare(&memory, 0x1000, &timeline(&a, &b), &[&a, &b], options).is_none());
    }
    let plan = prepare(
        &memory,
        0x1000,
        &timeline(&a, &b),
        &[&a, &b],
        CompileOptions::default(),
    )
    .unwrap();
    drop(a);
    drop(b);
    assert!(plan.is_current(&memory));
    memory.write(GuestAddress(0x1002), &[0x40]).unwrap();
    assert!(!plan.is_current(&memory));
    assert!(emit_hot_window(&memory, plan).unwrap().is_none());
}

#[test]
fn window_call_return_and_numeric_dictionary_use_the_new_owner() {
    let mut memory = memory();
    memory
        .write(
            GuestAddress(0x1000),
            &[0xe8, 0x0b, 0, 0, 0, 0xd9, 0xe0, 0xeb, 0xf7],
        )
        .unwrap();
    memory
        .write(GuestAddress(0x1010), &[0xd8, 0xc1, 0xc3])
        .unwrap();
    let a = family(
        &memory,
        &[(0x1005, 0x1007), (0x1007, 0x1000), (0x1000, 0x1010)],
    );
    let b = family(&memory, &[(0x1010, 0x1012), (0x1012, 0x1005)]);
    let plan = prepare(
        &memory,
        0x1000,
        &[
            block(&a, 0x1000),
            block(&b, 0x1010),
            block(&a, 0x1005),
            block(&a, 0x1000),
            block(&b, 0x1010),
            block(&a, 0x1005),
            block(&a, 0x1000),
        ],
        &[&a, &b],
        CompileOptions::default(),
    )
    .unwrap();
    assert!(
        plan.fingerprint
            .graph
            .edges
            .binary_search(&(0x1000, 0x1010))
            .is_ok()
    );
    assert!(
        plan.fingerprint
            .graph
            .edges
            .binary_search(&(0x1012, 0x1005))
            .is_ok()
    );
    let code = emit_hot_window(&memory, plan).unwrap().unwrap();
    assert_eq!(b.native_instructions()[0].pc, 0x1010);
    assert_eq!(
        code.native_instructions()
            .iter()
            .map(|instruction| instruction.pc)
            .collect::<Vec<_>>(),
        [0x1005, 0x1010]
    );
    assert_ne!(code.identity(), a.identity());
    assert_ne!(code.identity(), b.identity());
}

// OLD_COMPAT_POLICY_RED_BEGIN
#[test]
fn valid_linear_and_single_cycle_captures_do_not_admit_hot_windows() {
    let mut declined = Vec::new();
    for linear in [false, true] {
        let mut memory = memory();
        if linear {
            memory.write(GuestAddress(0x1011), &[0xeb, 0x1d]).unwrap();
        }
        let a = family(
            &memory,
            &[
                (0x1000, 0x1030),
                (0x1030, 0x1031),
                (0x1031, 0x1000),
                (0x1000, 0x1002),
                (0x1002, 0x1003),
                (0x1003, 0x1010),
            ],
        );
        let b = family(
            &memory,
            &[
                (0x1010, 0x1011),
                (0x1011, if linear { 0x1030 } else { 0x1000 }),
            ],
        );
        let events = [
            block(&a, 0x1000),
            block(&a, 0x1002),
            block(&b, 0x1010),
            block(&a, if linear { 0x1030 } else { 0x1000 }),
        ];
        let options = CompileOptions::default();
        let graph = capture_graph(
            &memory,
            GuestAddress(0x1000),
            options,
            &events,
            &[&a, &b],
            memory.code_source_version(),
            &[],
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            graph.graph.nodes.keys().copied().collect::<Vec<_>>(),
            [0x1000, 0x1002, 0x1003, 0x1010, 0x1011]
        );
        assert!(graph.graph.edges.contains(&(0x1003, 0x1010)));
        assert!(
            graph
                .sources
                .iter()
                .all(|source| memory.is_code_current(source))
        );
        let a_bytes = a.wasm().to_vec();
        let b_bytes = b.wasm().to_vec();
        let a_ports = a.entrypoints().to_vec();
        let b_ports = b.entrypoints().to_vec();
        let mut before = [0; 4096];
        memory.read(GuestAddress(0x1000), &mut before).unwrap();
        let revision = memory.transaction_version();
        let plan = prepare_hot_window(
            &memory,
            GuestAddress(0x1000),
            options,
            &events,
            &[&a, &b],
            memory.code_source_version(),
            &[],
        )
        .unwrap();
        let mut after = [0; 4096];
        memory.read(GuestAddress(0x1000), &mut after).unwrap();
        assert_eq!(after, before);
        assert_eq!(memory.transaction_version(), revision);
        assert_eq!(a.wasm(), a_bytes);
        assert_eq!(b.wasm(), b_bytes);
        assert_eq!(a.entrypoints(), a_ports);
        assert_eq!(b.entrypoints(), b_ports);
        assert!(a.is_current(&memory) && b.is_current(&memory));
        declined.push(plan.is_none());
    }
    assert!(
        declined.into_iter().all(|declined| declined),
        "valid cross-owner geometry needs two completed identical root cycles"
    );
}
// OLD_COMPAT_POLICY_RED_END

#[test]
fn different_completed_root_cycles_are_not_a_recurrence_certificate() {
    let memory = memory();
    let (a, b) = parents(&memory);
    let events = [
        block(&a, 0x1000),
        block(&a, 0x1002),
        block(&b, 0x1010),
        block(&a, 0x1000),
        block(&a, 0x1030),
        block(&a, 0x1000),
    ];
    assert!(
        capture_graph(
            &memory,
            GuestAddress(0x1000),
            CompileOptions::default(),
            &events,
            &[&a, &b],
            memory.code_source_version(),
            &[],
        )
        .unwrap()
        .is_some()
    );
    assert!(
        prepare(
            &memory,
            0x1000,
            &events,
            &[&a, &b],
            CompileOptions::default()
        )
        .is_none()
    );
}

#[test]
fn recurrence_word_bound_does_not_narrow_normal_family_capture() {
    let mut memory = memory();
    memory.write(GuestAddress(0x1000), &[0xeb, 0x0e]).unwrap();
    memory
        .write(GuestAddress(0x1010), &[0x90, 0x75, 0xfd, 0xeb, 0xeb])
        .unwrap();
    let a = family(&memory, &[(0x1000, 0x1010)]);
    let b = family(
        &memory,
        &[
            (0x1010, 0x1011),
            (0x1011, 0x1010),
            (0x1010, 0x1011),
            (0x1011, 0x1013),
            (0x1013, 0x1000),
        ],
    );
    let events = [
        block(&a, 0x1000),
        block(&b, 0x1010),
        block(&b, 0x1010),
        block(&b, 0x1013),
        block(&a, 0x1000),
        block(&b, 0x1010),
        block(&b, 0x1010),
        block(&b, 0x1013),
        block(&a, 0x1000),
    ];
    let options = CompileOptions {
        instructions: 4,
        ..CompileOptions::default()
    };
    let ordinary = capture_graph(
        &memory,
        GuestAddress(0x1000),
        options,
        &events,
        &[&a, &b],
        memory.code_source_version(),
        &[],
    )
    .unwrap()
    .unwrap();
    assert_eq!(ordinary.graph.nodes.len(), 4);
    assert!(prepare(&memory, 0x1000, &events, &[&a, &b], options).is_none());
}

#[test]
fn numeric_markers_do_not_double_count_completed_root_cycles() {
    let mut memory = memory();
    memory
        .write(GuestAddress(0x1000), &[0xd8, 0xc1, 0xff, 0xe6])
        .unwrap();
    memory.write(GuestAddress(0x1010), &[0xff, 0xe7]).unwrap();
    let a = family(&memory, &[(0x1000, 0x1002), (0x1002, 0x1010)]);
    let b = family(&memory, &[(0x1010, 0x1000)]);
    let numeric = TraceEvent {
        kind: NUMERIC,
        pc: 0x1000,
        end: 0x1002,
        offset: 0,
        owner: a.identity(),
    };
    let mut suffix = block(&a, 0x1000);
    suffix.offset = 1;
    // A cross-owner return must have its BLOCK marker before a NUMERIC
    // boundary. These old malformed timelines remain rejection controls.
    let malformed_one = [
        block(&a, 0x1000),
        numeric,
        suffix,
        block(&b, 0x1010),
        numeric,
    ];
    let malformed_two = [
        block(&a, 0x1000),
        numeric,
        suffix,
        block(&b, 0x1010),
        block(&a, 0x1000),
        numeric,
        suffix,
        block(&b, 0x1010),
        numeric,
    ];
    for events in [malformed_one.as_slice(), malformed_two.as_slice()] {
        assert!(
            prepare(
                &memory,
                0x1000,
                events,
                &[&a, &b],
                CompileOptions::default()
            )
            .is_none()
        );
    }

    let one = [
        block(&a, 0x1000),
        numeric,
        suffix,
        block(&b, 0x1010),
        block(&a, 0x1000),
        numeric,
    ];
    let two = [
        block(&a, 0x1000),
        numeric,
        suffix,
        block(&b, 0x1010),
        block(&a, 0x1000),
        numeric,
        suffix,
        block(&b, 0x1010),
        block(&a, 0x1000),
        numeric,
    ];
    for events in [one.as_slice(), two.as_slice()] {
        let captured = capture_graph(
            &memory,
            GuestAddress(0x1000),
            CompileOptions::default(),
            events,
            &[&a, &b],
            memory.code_source_version(),
            &[],
        )
        .unwrap()
        .unwrap();
        assert_eq!(captured.graph.nodes.len(), 3);
    }
    // BLOCK -> NUMERIC at the same root consumes an empty prefix, and the
    // final NUMERIC remains unconfirmed. Neither adds a completed cycle.
    assert!(prepare(&memory, 0x1000, &one, &[&a, &b], CompileOptions::default()).is_none());
    let plan = prepare(&memory, 0x1000, &two, &[&a, &b], CompileOptions::default()).unwrap();
    assert_eq!(plan.completed_cycle_count(), 2);
    assert_eq!(plan.cycle_instruction_count(), 3);
    assert_eq!(
        plan.instruction_pcs().collect::<Vec<_>>(),
        [0x1000, 0x1002, 0x1010]
    );
}

#[test]
fn recurrent_window_does_not_keep_unrepeated_tail_nodes_or_source_authority() {
    let mut memory = AddressSpace::new(3).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 3).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory
        .write(GuestAddress(0x1000), &[0xe8, 0xfb, 0x0f, 0, 0, 0xff, 0xe6])
        .unwrap();
    memory.write(GuestAddress(0x2000), &[0xc3]).unwrap();
    memory
        .write(GuestAddress(0x3000), &[0x90, 0xff, 0xe7])
        .unwrap();
    let a = family(&memory, &[(0x1005, 0x1000), (0x1000, 0x2000)]);
    let b = family(&memory, &[(0x2000, 0x1005)]);
    let c = family(&memory, &[(0x3000, 0x3001), (0x3001, 0x1000)]);
    let base = vec![
        block(&a, 0x1000),
        block(&b, 0x2000),
        block(&a, 0x1005),
        block(&a, 0x1000),
        block(&b, 0x2000),
        block(&a, 0x1005),
        block(&a, 0x1000),
    ];
    let original = prepare(&memory, 0x1000, &base, &[&a, &b], CompileOptions::default()).unwrap();
    let mut extended = base.clone();
    extended.extend([block(&b, 0x2000), block(&c, 0x3000), block(&a, 0x1000)]);
    let with_tail = prepare(
        &memory,
        0x1000,
        &extended,
        &[&a, &b, &c],
        CompileOptions::default(),
    )
    .unwrap();
    assert_eq!(original.fingerprint(), with_tail.fingerprint());
    assert_eq!(
        with_tail.instruction_pcs().collect::<Vec<_>>(),
        [0x1000, 0x1005, 0x2000]
    );
    memory.write(GuestAddress(0x3000), &[0x40]).unwrap();
    assert!(with_tail.is_current(&memory));
    assert!(!c.is_current(&memory));
    let changed = family(&memory, &[(0x3000, 0x3001), (0x3001, 0x1000)]);
    let mut changed_tail = base;
    changed_tail.extend([
        block(&b, 0x2000),
        block(&changed, 0x3000),
        block(&a, 0x1000),
    ]);
    let changed_plan = prepare(
        &memory,
        0x1000,
        &changed_tail,
        &[&a, &b, &changed],
        CompileOptions::default(),
    )
    .unwrap();
    assert_eq!(original.fingerprint(), changed_plan.fingerprint());
    assert!(emit_hot_window(&memory, changed_plan).unwrap().is_some());
}
