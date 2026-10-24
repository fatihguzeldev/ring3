use super::*;
use crate::cpu::core::trace::PendingColdObservation;
use crate::memory::{PageRange, Permissions};

const P: u32 = 0x1000;
const A: u32 = 0x1010;
const B: u32 = 0x1020;

fn memory() -> AddressSpace {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(P), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(P), &[0xeb, 0x0e]).unwrap();
    memory.write(GuestAddress(A), &[0x90, 0xeb, 0x0d]).unwrap();
    memory.write(GuestAddress(B), &[0x90, 0xeb, 0xed]).unwrap();
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

fn parents(memory: &AddressSpace) -> [CompiledCode; 3] {
    [
        family(memory, &[(P, A)]),
        family(memory, &[(A, A + 1), (A + 1, B)]),
        family(memory, &[(B, B + 1), (B + 1, A)]),
    ]
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

fn timeline(parents: &[CompiledCode; 3]) -> Vec<TraceEvent> {
    let [p, a, b] = parents;
    vec![
        block(p, P),
        block(a, A),
        block(b, B),
        block(a, A),
        block(b, B),
        block(a, A),
    ]
}

fn prepare(
    memory: &AddressSpace,
    root: u32,
    events: &[TraceEvent],
    owners: &[&CompiledCode],
    cold: &[CompletedColdObservation],
) -> Option<WindowPlan> {
    prepare_hot_window(
        memory,
        GuestAddress(root),
        CompileOptions::default(),
        events,
        owners,
        memory.code_source_version(),
        cold,
    )
    .unwrap()
}

#[test]
fn one_shot_prefix_selects_only_the_first_certified_interior_cycle() {
    let memory = memory();
    let parents = parents(&memory);
    let owners: Vec<_> = parents.iter().collect();
    let bytes: Vec<_> = parents.iter().map(|code| code.wasm().to_vec()).collect();
    let events = timeline(&parents);
    let direct = prepare(&memory, A, &events[1..], &owners, &[]).unwrap();
    let selected = prepare(&memory, P, &events, &owners, &[]).unwrap();
    assert_eq!(selected.anchor(), A);
    assert_eq!(selected.fingerprint(), direct.fingerprint());
    assert_eq!(selected.completed_cycle_count(), 2);
    assert_eq!(selected.cross_owner_edge_count(), 2);
    assert_eq!(selected.cycle_instruction_count(), 4);
    assert_eq!(
        selected.instruction_pcs().collect::<Vec<_>>(),
        [A, A + 1, B, B + 1]
    );
    let code = emit_hot_window(&memory, selected).unwrap().unwrap();
    assert!(code.is_execution_window() && code.supports_entry(A));
    assert!(!code.supports_entry(P) && code.supports_entry(B));
    assert!(!code.supports_entry(A + 1));
    for (parent, bytes) in parents.iter().zip(bytes) {
        assert_eq!(parent.wasm(), bytes);
        assert!(parent.is_current(&memory));
    }
}

#[test]
fn cold_prefix_and_unconfirmed_tail_do_not_enter_the_compiled_cycle() {
    let memory = memory();
    let parents = parents(&memory);
    let owners: Vec<_> = parents.iter().collect();
    let events = timeline(&parents);
    let direct = prepare(&memory, A, &events[1..], &owners, &[]).unwrap();
    let cold = [completed(&memory, P, A), completed(&memory, A, A + 1)];
    let mut mixed = events;
    mixed[0] = cold[0].event(0).unwrap();
    // The final BLOCK confirms the prior B return. Its following precise NOP
    // is outside the completed two-cycle word and must not enter the window.
    mixed.push(cold[1].event(1).unwrap());
    let selected = prepare(&memory, P, &mixed, &owners, &cold).unwrap();
    assert_eq!(selected.fingerprint(), direct.fingerprint());
    assert_eq!(selected.cycle_instruction_count(), 4);
}

#[test]
fn cold_inside_every_cycle_and_a_single_complete_cycle_are_not_promoted() {
    let memory = memory();
    let parents = parents(&memory);
    let [p, a, b] = &parents;
    let owners: Vec<_> = parents.iter().collect();
    let cold = [completed(&memory, A, A + 1), completed(&memory, A + 1, B)];
    let events = [
        block(p, P),
        block(a, A),
        cold[0].event(0).unwrap(),
        cold[1].event(1).unwrap(),
        block(b, B),
        block(a, A),
        cold[0].event(0).unwrap(),
        cold[1].event(1).unwrap(),
        block(b, B),
        block(a, A),
    ];
    assert!(prepare(&memory, P, &events, &owners, &cold).is_none());
    let events = timeline(&parents);
    assert!(prepare(&memory, P, &events[..4], &owners, &[]).is_none());
}

#[test]
fn outer_geometry_source_epoch_and_same_owner_cycles_are_still_required() {
    let memory = memory();
    let parents = parents(&memory);
    let owners: Vec<_> = parents.iter().collect();
    let events = timeline(&parents);
    let mut forged = events.clone();
    forged[0].offset = 1;
    assert!(prepare(&memory, P, &forged, &owners, &[]).is_none());
    assert!(
        prepare_hot_window(
            &memory,
            GuestAddress(P),
            CompileOptions::default(),
            &events,
            &owners,
            memory.code_source_version().wrapping_add(1),
            &[]
        )
        .unwrap()
        .is_none()
    );
    let whole = family(
        &memory,
        &[(P, A), (A, A + 1), (A + 1, B), (B, B + 1), (B + 1, A)],
    );
    let same_owner = [
        block(&whole, P),
        block(&whole, A),
        block(&whole, B),
        block(&whole, A),
        block(&whole, B),
        block(&whole, A),
    ];
    assert!(prepare(&memory, P, &same_owner, &[&whole], &[]).is_none());
}

#[test]
fn interior_offset_is_a_certified_complete_entry_without_importing_skipped_prefix() {
    let mut memory = memory();
    memory.write(GuestAddress(P), &[0xff, 0xe7]).unwrap();
    memory.write(GuestAddress(A), &[0x90, 0xff, 0xe6]).unwrap();
    memory.write(GuestAddress(B), &[0xff, 0xe7]).unwrap();
    let p = family(&memory, &[(P, A + 1)]);
    let a = family(&memory, &[(A, A + 1), (A + 1, B)]);
    let b = family(&memory, &[(B, A + 1)]);
    let mut interior = block(&a, A);
    interior.offset = 1;
    let events = [
        block(&p, P),
        interior,
        block(&b, B),
        interior,
        block(&b, B),
        interior,
    ];
    let plan = prepare(&memory, P, &events, &[&p, &a, &b], &[]).unwrap();
    assert_eq!(plan.anchor(), A + 1);
    assert_eq!(plan.instruction_pcs().collect::<Vec<_>>(), [A + 1, B]);
    let code = emit_hot_window(&memory, plan).unwrap().unwrap();
    assert!(code.supports_entry(A + 1));
    assert!(!code.supports_entry(A) && code.supports_entry(B));
}

#[test]
fn duplicate_numeric_markers_do_not_hide_two_actual_interior_cycles() {
    let mut memory = memory();
    memory
        .write(GuestAddress(A), &[0xd8, 0xc1, 0xff, 0xe6])
        .unwrap();
    memory.write(GuestAddress(B), &[0xff, 0xe7]).unwrap();
    let p = family(&memory, &[(P, A)]);
    let a = family(&memory, &[(A, A + 2), (A + 2, B)]);
    let b = family(&memory, &[(B, A)]);
    let numeric = TraceEvent {
        kind: NUMERIC,
        pc: A,
        end: A + 2,
        offset: 0,
        owner: a.identity(),
    };
    let mut suffix = block(&a, A);
    suffix.offset = 1;
    let events = [
        block(&p, P),
        block(&a, A),
        numeric,
        suffix,
        block(&b, B),
        block(&a, A),
        numeric,
        suffix,
        block(&b, B),
        block(&a, A),
        numeric,
    ];
    let plan = prepare(&memory, P, &events, &[&p, &a, &b], &[]).unwrap();
    assert_eq!(plan.anchor(), A);
    assert_eq!(plan.completed_cycle_count(), 2);
    assert_eq!(plan.cycle_instruction_count(), 3);
    assert_eq!(plan.instruction_pcs().collect::<Vec<_>>(), [A, A + 2, B]);
}

#[test]
fn internal_recurrent_site_is_skipped_for_the_first_actual_owner_entry() {
    let mut memory = memory();
    // NOT EDX; TEST EDX,EDX; JZ A; JMP B. Consecutive root cycles
    // alternate, while the internal JMP and external B both recur exactly.
    memory
        .write(
            GuestAddress(P),
            &[0xf7, 0xd2, 0x85, 0xd2, 0x74, 0x0a, 0xeb, 0x18],
        )
        .unwrap();
    memory.write(GuestAddress(A), &[0xeb, 0xee]).unwrap();
    memory.write(GuestAddress(B), &[0xeb, 0xde]).unwrap();
    let c = family(
        &memory,
        &[(P, P + 2), (P + 2, P + 4), (P + 4, P + 6), (P + 6, B)],
    );
    let a = family(&memory, &[(A, P)]);
    let b = family(&memory, &[(B, P)]);
    let events = [
        block(&c, P),
        block(&c, P + 6),
        block(&b, B),
        block(&c, P),
        block(&a, A),
        block(&c, P),
        block(&c, P + 6),
        block(&b, B),
        block(&c, P),
        block(&a, A),
        block(&c, P),
        block(&c, P + 6),
        block(&b, B),
    ];
    let owners = [&c, &a, &b];
    // The internal site is a valid graph certificate, but normal C execution
    // reaches it without selecting a new owner, so it is an ineffective root.
    let internal = prepare_root_window(
        &memory,
        GuestAddress(P + 6),
        CompileOptions::default(),
        &events[1..],
        &owners,
        memory.code_source_version(),
        &[],
    )
    .unwrap()
    .unwrap();
    assert_eq!(internal.anchor(), P + 6);
    let selected = prepare(&memory, P, &events, &owners, &[]).unwrap();
    assert_eq!(selected.anchor(), B);
    assert_eq!(selected.cycle_instruction_count(), 9);
    assert_eq!(selected.completed_cycle_count(), 2);
    assert!(selected.fingerprint().same_graph(internal.fingerprint()));
    let code = emit_hot_window(&memory, selected).unwrap().unwrap();
    assert!(code.supports_entry(B));
    assert!(code.supports_entry(P + 6));
    assert!(!code.supports_entry(P + 2));
}
