use super::*;
use crate::{
    cpu::{
        core::{
            NativeInstruction, compile, emit_hot_window, emit_observed_plan, prepare_hot_window,
            prepare_observed_plan,
            trace::{BLOCK, CompletedColdObservation, PendingColdObservation, TraceEvent},
        },
        x86::decode::{BoundDecode, DecodeAuthority, DecodePolicy, decode_for_compilation},
    },
    memory::{PageRange, Permissions},
};

const ROOT: u32 = 0x1000;
const CALLEE: u32 = 0x2000;
const OTHER: u32 = 0x3000;

fn completed(memory: &AddressSpace, pc: u32, post: u32) -> CompletedColdObservation {
    let BoundDecode::Instruction(decoded) = decode_for_compilation(
        memory,
        GuestAddress(pc),
        DecodePolicy::Execution,
        None,
        DecodeAuthority::default(),
    )
    .unwrap() else {
        panic!("decoded fixture instruction")
    };
    PendingColdObservation::capture(
        memory,
        &NativeInstruction {
            pc,
            next: decoded.next_pc().0,
            operation: *decoded.operation(),
        },
    )
    .unwrap()
    .complete(memory, post)
    .unwrap()
}

pub(super) fn family(
    memory: &AddressSpace,
    path: &[(u32, u32)],
    parents: &[&CompiledCode],
) -> CompiledCode {
    let cold: Vec<_> = path
        .iter()
        .map(|&(pc, post)| completed(memory, pc, post))
        .collect();
    let events: Vec<_> = cold
        .iter()
        .enumerate()
        .map(|(i, item)| item.event(i).unwrap())
        .collect();
    let plan = prepare_observed_plan(
        memory,
        GuestAddress(path[0].0),
        CompileOptions::default(),
        &events,
        parents,
        memory.code_source_version(),
        &cold,
        parents,
    )
    .unwrap()
    .unwrap();
    emit_observed_plan(memory, plan).unwrap().unwrap()
}

pub(super) fn fixture() -> (AddressSpace, CoreCache32, RegionRef, RegionRef) {
    let mut memory = AddressSpace::new(3).unwrap();
    for pc in [ROOT, CALLEE, OTHER] {
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(pc), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
    }
    memory
        .write(
            GuestAddress(ROOT),
            &[0xe8, 0xfb, 0x0f, 0, 0, 0x40, 0xff, 0xe6],
        )
        .unwrap();
    memory.write(GuestAddress(CALLEE), &[0x41, 0xc3]).unwrap();
    memory
        .write(GuestAddress(OTHER), &[0x43, 0xff, 0xe6])
        .unwrap();
    let first = family(&memory, &[(ROOT, CALLEE)], &[]);
    let caller = family(
        &memory,
        &[(ROOT + 5, ROOT + 6), (ROOT + 6, ROOT), (ROOT, CALLEE)],
        &[&first],
    );
    let callee = family(
        &memory,
        &[(CALLEE, CALLEE + 1), (CALLEE + 1, ROOT + 5)],
        &[],
    );
    let mut cache = CoreCache32::default();
    let caller = cache.admit(&memory, caller).unwrap();
    let callee = cache.admit(&memory, callee).unwrap();
    cache.install_link(&caller, 7).unwrap();
    cache.install_link(&callee, 8).unwrap();
    (memory, cache, caller, callee)
}

fn events(caller: &CompiledCode, callee: &CompiledCode, rotated: bool) -> Vec<TraceEvent> {
    let mut rows = vec![
        (ROOT, ROOT + 5, caller.identity()),
        (CALLEE, CALLEE + 2, callee.identity()),
        (ROOT + 5, ROOT + 8, caller.identity()),
    ];
    if rotated {
        rows.rotate_left(1);
    }
    let cycle = rows.clone();
    rows.extend_from_slice(&cycle);
    rows.push(rows[0]);
    rows.into_iter()
        .map(|(pc, end, owner)| TraceEvent {
            kind: BLOCK,
            pc,
            end,
            offset: 0,
            owner,
        })
        .collect()
}

fn plan(
    memory: &AddressSpace,
    caller: &CompiledCode,
    callee: &CompiledCode,
    rotated: bool,
    options: CompileOptions,
) -> WindowPlan {
    let events = events(caller, callee, rotated);
    prepare_hot_window(
        memory,
        GuestAddress(events[0].pc),
        options,
        &events,
        &[caller, callee],
        memory.code_source_version(),
        &[],
    )
    .unwrap()
    .unwrap()
}

pub(super) fn install(
    memory: &AddressSpace,
    cache: &mut CoreCache32,
    caller: &CompiledCode,
    callee: &CompiledCode,
) -> RegionRef {
    let plan = plan(memory, caller, callee, false, CompileOptions::default());
    let admission = cache.preflight_window(memory, &plan).unwrap();
    let code = emit_hot_window(memory, plan).unwrap().unwrap();
    cache.commit_window(memory, admission, code).unwrap()
}

pub(super) fn resources(cache: &CoreCache32) {
    assert!(cache.resident_charge() <= MAX_CODE_BYTES);
    assert_eq!(cache.snapshot()[8], cache.resident_charge() as u64);
    assert_eq!(
        cache.owner_charge,
        cache
            .regions
            .iter()
            .flatten()
            .map(|entry| entry.charge)
            .sum::<usize>()
    );
    assert_eq!(
        cache.snapshot()[5],
        cache.regions.iter().flatten().count() as u64
    );
    assert_eq!(
        cache.snapshot()[6],
        cache
            .regions
            .iter()
            .flatten()
            .map(|entry| entry.code.wasm().len() as u64)
            .sum::<u64>()
    );
    assert!(cache.window_entries.capacity() <= cache.window_entries_capacity);
    for (&root, &key) in &cache.window_entries {
        let entry = cache.region(key).unwrap();
        assert!(entry.code.is_execution_window());
        assert_eq!(entry.admitted_entries, entry.code.entrypoints());
        assert!(
            entry
                .admitted_entries
                .binary_search_by_key(&root, |entry| entry.0)
                .is_ok()
        );
        assert!(entry.code.hot_span(root).is_some());
    }
}

#[test]
fn hot_window_root_priority_keeps_family_aliases_and_refreshes_cached_link_resolution() {
    let (memory, mut cache, caller, callee) = fixture();
    let baseline = cache
        .admit(
            &memory,
            compile(
                &memory,
                GuestAddress(ROOT),
                CompileOptions {
                    instructions: 1,
                    blocks: 1,
                    ..CompileOptions::default()
                },
            )
            .unwrap(),
        )
        .unwrap();
    cache.install_link(&baseline, 1).unwrap();
    let family_aliases = cache.trace_entries.clone();
    let baseline_entries = cache.entries.clone();
    let coverage = cache.coverage.clone();
    assert_eq!(
        cache
            .lookup_linked_from(&memory, &caller, ROOT)
            .unwrap()
            .0
            .identity(),
        caller.identity()
    );
    let window = install(&memory, &mut cache, &caller, &callee);
    assert!(window.is_execution_window());
    assert!(!window.is_observed_family());
    assert!(window.supports_entry(ROOT));
    assert!(window.supports_entry(CALLEE));
    assert!(window.supports_entry(ROOT + 5));
    assert!(!window.supports_entry(CALLEE + 1));
    assert!(!window.supports_entry(ROOT + 6));
    assert_eq!(window.instruction_count(), 5);
    // an unbound window cannot displace the executable family target.
    assert_eq!(
        cache.lookup_linked(&memory, ROOT).unwrap().0.identity(),
        caller.identity()
    );
    cache.install_link(&window, 9).unwrap();
    assert_eq!(
        cache
            .lookup_linked_from(&memory, &caller, ROOT)
            .unwrap()
            .0
            .identity(),
        window.identity()
    );
    assert_eq!(
        cache.lookup(&memory, ROOT).unwrap().identity(),
        window.identity()
    );
    for (pc, identity, slot) in [
        (ROOT + 5, window.identity(), 9),
        (ROOT + 6, caller.identity(), 7),
        (CALLEE, window.identity(), 9),
        (CALLEE + 1, callee.identity(), 8),
    ] {
        let (selected, actual_slot) = cache.lookup_linked(&memory, pc).unwrap();
        assert_eq!((selected.identity(), actual_slot), (identity, slot));
    }
    assert_eq!(cache.trace_entries, family_aliases);
    assert_eq!(cache.entries, baseline_entries);
    assert_eq!(cache.coverage, coverage);
    assert_eq!(
        &cache.link_owners[7..9],
        &[caller.identity(), callee.identity()]
    );
    cache.bypass_trace(window.identity());
    assert_eq!(
        cache.lookup(&memory, ROOT).unwrap().identity(),
        caller.identity()
    );
    assert_eq!(cache.lookup_linked(&memory, ROOT).unwrap().1, 7);
    cache.bypass_trace(0);
    assert_eq!(
        cache.lookup(&memory, ROOT).unwrap().identity(),
        window.identity()
    );
    cache.unlink(9, window.identity()).unwrap();
    assert_eq!(
        cache.lookup_linked(&memory, ROOT).unwrap().0.identity(),
        caller.identity()
    );
    resources(&cache);
}

#[test]
fn hot_window_same_root_and_rotated_equal_graph_are_refused_before_emission() {
    let (memory, mut cache, caller, callee) = fixture();
    let window = install(&memory, &mut cache, &caller, &callee);
    let before = cache.snapshot();
    let equal = plan(&memory, &caller, &callee, false, CompileOptions::default());
    let rotated = plan(&memory, &caller, &callee, true, CompileOptions::default());
    assert!(cache.preflight_window(&memory, &equal).is_none());
    assert!(cache.preflight_window(&memory, &rotated).is_none());
    assert_eq!(cache.snapshot(), before);
    cache.remove(window.key);
    assert!(
        cache.preflight_window(&memory, &rotated).is_none(),
        "bounded feedback survives ordinary owner removal"
    );
    let other = cache
        .admit(
            &memory,
            family(&memory, &[(OTHER, OTHER + 1), (OTHER + 1, ROOT)], &[]),
        )
        .unwrap();
    cache.install_link(&other, 9).unwrap();
    assert!(cache.preflight_window(&memory, &equal).is_none());
    assert_eq!(
        cache.unlink(9, window.identity()),
        Err(LinkError::WrongOwner)
    );
    assert_eq!(
        cache.lookup_linked(&memory, ROOT).unwrap().0.identity(),
        caller.identity()
    );
    resources(&cache);
}

#[test]
fn hot_window_stale_source_or_intervening_owner_aba_cannot_commit_an_old_token() {
    let (mut memory, mut cache, caller, callee) = fixture();
    let pending = plan(&memory, &caller, &callee, false, CompileOptions::default());
    let admission = cache.preflight_window(&memory, &pending).unwrap();
    let code = emit_hot_window(&memory, pending).unwrap().unwrap();
    let before = cache.snapshot();
    let aliases = cache.trace_entries.clone();
    memory.write(GuestAddress(CALLEE), &[0x41]).unwrap();
    assert!(matches!(
        cache.commit_window(&memory, admission, code),
        Err(CompileError::InvalidLimits)
    ));
    assert_eq!(cache.snapshot(), before);
    assert_eq!(cache.trace_entries, aliases);
    assert!(cache.window_entries.is_empty());

    let (memory, mut cache, caller, callee) = fixture();
    let pending = plan(&memory, &caller, &callee, false, CompileOptions::default());
    let admission = cache.preflight_window(&memory, &pending).unwrap();
    let code = emit_hot_window(&memory, pending).unwrap().unwrap();
    cache.remove(callee.key);
    let other = cache
        .admit(
            &memory,
            family(&memory, &[(OTHER, OTHER + 1), (OTHER + 1, ROOT)], &[]),
        )
        .unwrap();
    cache.install_link(&other, 8).unwrap();
    let before = cache.snapshot();
    assert!(matches!(
        cache.commit_window(&memory, admission, code),
        Err(CompileError::InvalidLimits)
    ));
    assert_eq!(cache.snapshot(), before);
    assert!(cache.window_entries.is_empty());
    assert_eq!(
        cache.unlink(8, callee.identity()),
        Err(LinkError::WrongOwner)
    );
    resources(&cache);
}

#[test]
fn hot_window_source_is_independent_of_parent_residency_but_actual_source_change_revokes_it() {
    let (mut memory, mut cache, caller, callee) = fixture();
    let window = install(&memory, &mut cache, &caller, &callee);
    cache.install_link(&window, 9).unwrap();
    cache.remove(caller.key);
    assert!(window.is_current(&memory));
    assert_eq!(
        cache.lookup_linked(&memory, ROOT).unwrap().0.identity(),
        window.identity()
    );
    memory.write(GuestAddress(ROOT), &[0xe8]).unwrap();
    assert!(cache.lookup_linked(&memory, ROOT).is_none());
    assert!(cache.retained(&window).is_none());
    assert!(cache.window_entries.is_empty());
    assert_eq!(cache.link_owners[9], 0);
    assert!(cache.retained(&callee).is_some());
    resources(&cache);
}

#[test]
fn hot_window_refusal_memory_is_bounded_charged_and_released_before_byte_pressure_eviction() {
    let (memory, mut cache, caller, callee) = fixture();
    for bytes in 128..128 + MAX_OBSERVED_REFUSALS + 2 {
        let options = CompileOptions {
            wasm_bytes: bytes,
            ..CompileOptions::default()
        };
        let pending = plan(&memory, &caller, &callee, false, options);
        let admission = cache.preflight_window(&memory, &pending).unwrap();
        assert!(emit_hot_window(&memory, pending).unwrap().is_none());
        cache.reject_window(admission);
        assert!(cache.window_refusals.len() <= MAX_OBSERVED_REFUSALS);
        assert!(
            cache
                .preflight_window(&memory, &plan(&memory, &caller, &callee, false, options))
                .is_none()
        );
        resources(&cache);
    }
    assert_eq!(cache.window_refusals.len(), MAX_OBSERVED_REFUSALS);
    let mut candidate = compile(
        &memory,
        GuestAddress(OTHER),
        CompileOptions {
            instructions: 2,
            blocks: 1,
            ..CompileOptions::default()
        },
    )
    .unwrap();
    let empty = CoreCache32::default();
    let projected = empty.projected_directory(
        candidate.entrypoints().len(),
        candidate.instruction_count(),
        false,
    );
    let extra = MAX_CODE_BYTES
        - projected
        - CoreCache32::code_charge(&candidate, candidate.entrypoints().len())
        - 1024;
    candidate.reserve_wasm_capacity_for_test(extra);
    cache
        .validate_candidate(&candidate, candidate.entrypoints().len())
        .unwrap();
    let bytes = candidate.wasm().to_vec();
    let selected = cache.admit(&memory, candidate).unwrap();
    assert_eq!(selected.wasm(), bytes);
    assert!(cache.window_refusals.is_empty());
    assert!(cache.resident_charge() > MAX_CODE_BYTES - (1 << 20));
    resources(&cache);
}
