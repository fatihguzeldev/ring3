use super::window_tests::{fixture, install, resources};
use super::*;
use crate::cpu::core::{
    emit_hot_window, prepare_hot_window,
    trace::{BLOCK, TraceEvent},
};

const ROOT: u32 = 0x1000;
const CALLEE: u32 = 0x2000;
const CONTEXT: u32 = 0x80;

fn rotated_plan(
    memory: &AddressSpace,
    caller: &RegionRef,
    callee: &RegionRef,
    rotate: usize,
) -> WindowPlan {
    let mut cycle = vec![
        (ROOT, ROOT + 5, caller.identity()),
        (CALLEE, CALLEE + 2, callee.identity()),
        (ROOT + 5, ROOT + 8, caller.identity()),
    ];
    cycle.rotate_left(rotate);
    let events: Vec<_> = cycle
        .iter()
        .chain(cycle.iter())
        .chain(cycle.first())
        .map(|&(pc, end, owner)| TraceEvent {
            kind: BLOCK,
            pc,
            end,
            offset: 0,
            owner,
        })
        .collect();
    prepare_hot_window(
        memory,
        GuestAddress(events[0].pc),
        CompileOptions::default(),
        &events,
        &[&**caller, &**callee],
        memory.code_source_version(),
        &[],
    )
    .unwrap()
    .unwrap()
}

#[test]
fn certified_window_entries_share_one_body_slot_charge_and_retention_node() {
    let (memory, mut cache, caller, callee) = fixture();
    let original = [caller.wasm().to_vec(), callee.wasm().to_vec()];
    let families = cache.trace_entries.clone();
    let coverage = cache.coverage.clone();
    let before = cache.snapshot();
    let window = install(&memory, &mut cache, &caller, &callee);
    assert_eq!(
        window.entrypoints(),
        &[
            GuestAddress(ROOT),
            GuestAddress(ROOT + 5),
            GuestAddress(CALLEE)
        ]
    );
    assert_eq!(cache.snapshot()[5], before[5] + 1);
    assert_eq!(cache.snapshot()[6], before[6] + window.wasm().len() as u64);
    assert_eq!(cache.window_entries.len(), 3);
    cache.install_link(&window, 9).unwrap();
    let encoded = window.wasm().to_vec();
    let pointer = window.wasm().as_ptr();
    let admitted = cache.compiled;
    let charge = cache.retained(&window).unwrap().charge;
    for (pc, minimum) in [(ROOT, 1), (ROOT + 5, 2), (CALLEE, 2)] {
        let span = window.hot_span(pc).unwrap();
        assert_eq!(span.instructions, minimum);
        let row = cache
            .route_rows
            .iter()
            .find(|row| row.state == route::PRESENT && row.pc == pc)
            .unwrap();
        assert_eq!(row.candidates[0].identity, window.identity());
        assert_eq!(
            (row.candidates[0].port, row.candidates[0].work),
            (span.port, minimum)
        );
        assert_eq!(row.candidates[0].flags, route::WINDOW_ROOT);
        assert_eq!(cache.window_entries.get(&pc), Some(&window.key));
        let selected = cache.lookup_executable(&memory, pc).unwrap();
        assert_eq!(selected.identity(), window.identity());
        assert_eq!(selected.wasm().as_ptr(), pointer);
        assert_eq!(cache.lookup_linked(&memory, pc).unwrap().1, 9);
        assert!(
            cache
                .lookup_executable_linked(&memory, pc, minimum - 1)
                .is_none()
        );
        assert_eq!(
            cache
                .lookup_executable_linked(&memory, pc, minimum)
                .unwrap()
                .0
                .identity(),
            window.identity()
        );
        assert_eq!(cache.retained(&window).unwrap().charge, charge);
        assert_eq!(window.wasm(), encoded);
    }
    assert!(!window.supports_entry(ROOT + 6));
    assert!(!window.supports_entry(CALLEE + 1));
    assert!(window.contains_instruction(ROOT + 6));
    assert!(window.contains_instruction(CALLEE + 1));
    assert_eq!(cache.compiled, admitted);
    assert_eq!(cache.trace_entries, families);
    assert_eq!(cache.coverage, coverage);
    for ((owner, bytes), slot) in [(&caller, &original[0]), (&callee, &original[1])]
        .into_iter()
        .zip([7, 8])
    {
        assert_eq!(owner.wasm(), bytes);
        assert_eq!(cache.link_owners[slot], owner.identity());
    }
    resources(&cache);
}

#[test]
fn every_window_ingress_has_a_real_ticket_but_private_instructions_and_aba_do_not() {
    let (memory, mut cache, caller, callee) = fixture();
    let window = install(&memory, &mut cache, &caller, &callee);
    cache.install_link(&window, 9).unwrap();
    let generation = cache.slot_generations[9];
    for pc in [ROOT, ROOT + 5, CALLEE] {
        assert!(cache.begin_continuation(&memory, CONTEXT, 0x100, Some(&window), pc));
        assert_eq!(cache.route_header.current_entry, pc);
        assert_eq!(
            cache.route_header.current_port,
            window.hot_span(pc).unwrap().port
        );
        assert_eq!(
            cache
                .continuation_source(&memory, CONTEXT, 9, generation, Some(&window), false, true)
                .unwrap()
                .identity(),
            window.identity()
        );
        cache.route_header.current_entry = CALLEE + 1;
        assert!(
            cache
                .continuation_source(&memory, CONTEXT, 9, generation, Some(&window), false, true)
                .is_none()
        );
        cache.route_header.current_entry = pc;
        cache.end_continuation();
    }
    cache.remove(window.key);
    assert!(cache.window_entries.is_empty());
    assert_eq!(cache.link_owners[9], 0);
    assert!(cache.slot_generations[9] > generation);
    assert!(!cache.precise_source_current(&memory, &window));
    for pc in [ROOT, ROOT + 5, CALLEE] {
        let expected = if pc == CALLEE {
            callee.identity()
        } else {
            caller.identity()
        };
        assert_eq!(
            cache.lookup_executable(&memory, pc).unwrap().identity(),
            expected
        );
    }
    cache.unlink(8, callee.identity()).unwrap();
    cache.install_link(&callee, 9).unwrap();
    assert!(cache.begin_continuation(&memory, CONTEXT, 0x100, Some(&callee), CALLEE));
    assert!(
        cache
            .continuation_source(&memory, CONTEXT, 9, generation, Some(&window), false, true)
            .is_none()
    );
    assert_eq!(
        cache.unlink(9, window.identity()),
        Err(LinkError::WrongOwner)
    );
    cache.end_continuation();
    resources(&cache);
}

#[test]
fn rotated_window_captures_reuse_all_published_entries_without_another_owner() {
    let (memory, mut cache, caller, callee) = fixture();
    let window = install(&memory, &mut cache, &caller, &callee);
    cache.install_link(&window, 9).unwrap();
    let original = window.wasm().to_vec();
    let before = cache.snapshot();
    for rotate in 0..3 {
        let pending = rotated_plan(&memory, &caller, &callee, rotate);
        assert!(cache.preflight_window(&memory, &pending).is_none());
        assert_eq!(cache.snapshot(), before);
        let root = [ROOT, CALLEE, ROOT + 5][rotate];
        assert_eq!(
            cache.lookup_linked(&memory, root).unwrap().0.identity(),
            window.identity()
        );
    }
    assert_eq!(window.wasm(), original);
    resources(&cache);
}

#[test]
fn multi_entry_window_commit_rechecks_all_capabilities_before_any_alias_is_published() {
    let (memory, mut cache, caller, callee) = fixture();
    let pending = rotated_plan(&memory, &caller, &callee, 0);
    let mut admission = cache.preflight_window(&memory, &pending).unwrap();
    let code = emit_hot_window(&memory, pending).unwrap().unwrap();
    assert_eq!(admission.ports.len(), 3);
    admission.ports[1].instructions += 1;
    let before = cache.snapshot();
    let families = cache.trace_entries.clone();
    assert!(matches!(
        cache.commit_window(&memory, admission, code),
        Err(CompileError::InvalidLimits)
    ));
    assert_eq!(cache.snapshot(), before);
    assert_eq!(cache.trace_entries, families);
    assert!(cache.window_entries.is_empty());
    assert_eq!(
        (cache.link_owners[7], cache.link_owners[8]),
        (caller.identity(), callee.identity())
    );
    resources(&cache);
}

#[test]
fn explicit_legacy_observed_traces_keep_their_original_root_only_entry_contract() {
    let (memory, mut cache, caller, callee) = fixture();
    let events: Vec<_> = [
        (ROOT, ROOT + 5, caller.identity()),
        (CALLEE, CALLEE + 2, callee.identity()),
        (ROOT + 5, ROOT + 8, caller.identity()),
        (ROOT, ROOT + 5, caller.identity()),
    ]
    .into_iter()
    .map(|(pc, end, owner)| TraceEvent {
        kind: BLOCK,
        pc,
        end,
        offset: 0,
        owner,
    })
    .collect();
    let legacy = crate::cpu::core::compile_observed(
        &memory,
        GuestAddress(ROOT),
        CompileOptions::default(),
        &events,
        &[&*caller, &*callee],
        memory.code_source_version(),
    )
    .unwrap()
    .unwrap();
    assert!(!legacy.is_execution_window());
    assert_eq!(legacy.entrypoints(), &[GuestAddress(ROOT)]);
    assert!(legacy.supports_entry(ROOT));
    assert!(!legacy.supports_entry(CALLEE));
    assert!(!legacy.supports_entry(ROOT + 5));
    assert!(legacy.contains_instruction(CALLEE));
    // A manual trace remains a root-only owner. Its separate cache avoids the
    // intentionally forbidden replacement of an already observed family.
    let mut manual_cache = CoreCache32::default();
    let legacy = manual_cache.admit(&memory, legacy).unwrap();
    manual_cache.install_link(&legacy, 9).unwrap();
    assert_eq!(manual_cache.lookup_linked(&memory, ROOT).unwrap().0.identity(), legacy.identity());
    assert!(manual_cache.lookup_linked(&memory, CALLEE).is_none());
    assert!(manual_cache.lookup_linked(&memory, ROOT + 5).is_none());
    assert_eq!(cache.lookup_linked(&memory, ROOT).unwrap().0.identity(), caller.identity());
    assert_eq!(cache.lookup_linked(&memory, CALLEE).unwrap().0.identity(), callee.identity());
    assert_eq!(cache.lookup_linked(&memory, ROOT + 5).unwrap().0.identity(), caller.identity());
    assert!(manual_cache.window_entries.is_empty());
    resources(&manual_cache);
    resources(&cache);
}
