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
    assert!(plan.has_executable_gain());
    assert_eq!(
        plan.instruction_pcs().collect::<Vec<_>>(),
        [0x1000, 0x1002, 0x1003, 0x1010, 0x1011]
    );
    assert_eq!(plan.entrypoints(), [GuestAddress(0x1000)]);
    assert_eq!(a.instruction_count() + b.instruction_count(), 7);
    let code = emit_hot_window(&memory, plan).unwrap().unwrap();
    assert!(code.is_execution_window());
    assert!(!code.is_observed_family());
    assert!(code.observed_fingerprint().is_none());
    assert!(code.window_fingerprint().is_some());
    assert!(code.supports_entry(0x1000));
    assert!(!code.supports_entry(0x1002));
    assert!(!code.supports_entry(0x1010));
    assert!(code.instruction_pcs().any(|pc| pc == 0x1010));
    assert_eq!(code.entrypoints(), [GuestAddress(0x1000)]);
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
        ],
        &[&a, &b],
        CompileOptions::default(),
    )
    .unwrap();
    assert_ne!(original.fingerprint(), rotated.fingerprint());
    assert!(original.fingerprint().same_graph(rotated.fingerprint()));
}

#[test]
fn interior_root_partition_is_neutral_for_window_dedup() {
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
    assert!(original.fingerprint().same_graph(rotated.fingerprint()));
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
