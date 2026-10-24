use super::*;
use crate::{
    cpu::core::{
        NativeInstruction, compile, emit_observed_plan, prepare_cold_prefix_plan,
        trace::{CompletedColdObservation, PendingColdObservation},
    },
    cpu::x86::decode::{BoundDecode, DecodeAuthority, DecodePolicy, decode_for_compilation},
    memory::{PageRange, Permissions},
};

const ROOT: u32 = 0x1000;
const CUT: u32 = 0x1005;
fn memory() -> AddressSpace {
    let mut memory = AddressSpace::new(3).unwrap();
    for pc in [ROOT, 0x2000, 0x3000] {
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
            &[0xb8, 7, 0, 0, 0, 0x8d, 0x40, 1, 0x8d, 0x40, 2, 0xff, 0xe6],
        )
        .unwrap();
    for pc in [0x2000, 0x3000] {
        memory.write(GuestAddress(pc), &[0x40, 0xff, 0xe6]).unwrap();
    }
    memory
}
fn completed(memory: &AddressSpace, pc: u32, post: u32) -> CompletedColdObservation {
    let BoundDecode::Instruction(decoded) = decode_for_compilation(
        memory,
        GuestAddress(pc),
        DecodePolicy::Execution,
        None,
        DecodeAuthority::default(),
    )
    .unwrap() else {
        panic!("literal instruction");
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
fn plan(memory: &AddressSpace) -> ObservedPlan {
    let cold: Vec<_> = [(CUT, 0x1008), (0x1008, 0x100b), (0x100b, CUT)]
        .into_iter()
        .map(|(pc, post)| completed(memory, pc, post))
        .collect();
    let events: Vec<_> = cold
        .iter()
        .enumerate()
        .map(|(i, item)| item.event(i).unwrap())
        .collect();
    prepare_cold_prefix_plan(memory, CompileOptions::default(), &events, &cold, &|_| {
        false
    })
    .unwrap()
    .unwrap()
}
fn fixture(memory: &AddressSpace) -> (CoreCache32, RegionRef) {
    let mut cache = CoreCache32::default();
    let original = cache
        .admit(
            memory,
            compile(
                memory,
                GuestAddress(ROOT),
                CompileOptions {
                    instructions: 4,
                    blocks: 1,
                    ..CompileOptions::default()
                },
            )
            .unwrap(),
        )
        .unwrap();
    cache.install_link(&original, 7).unwrap();
    (cache, original)
}
fn add_cut(memory: &AddressSpace, cache: &mut CoreCache32) -> RegionRef {
    let plan = plan(memory);
    let admission = cache.preflight_fragment(memory, &plan).unwrap();
    let code = emit_observed_plan(memory, plan).unwrap().unwrap();
    cache.commit_fragment(memory, admission, code).unwrap()
}

#[test]
fn source_owned_interior_gets_independent_hot_port_without_replacing_original() {
    let memory = memory();
    let (mut cache, original) = fixture(&memory);
    let bytes = original.wasm().to_vec();
    assert!(cache.owned_frontier_contains(&memory, CUT));
    assert!(cache.current_hot_port(&memory, CUT).is_none());
    assert!(matches!(
        cache
            .prepare_owned_entry(&memory, CUT, CompileOptions::default(), &[])
            .unwrap(),
        OwnedEntryLookup::Ready(_)
    ));
    let cut = add_cut(&memory, &mut cache);
    cache.install_link(&cut, 8).unwrap();
    assert_eq!(
        cache.lookup(&memory, CUT).unwrap().identity(),
        original.identity()
    );
    assert_eq!(
        cache.lookup_executable(&memory, CUT).unwrap().identity(),
        cut.identity()
    );
    assert_eq!(
        cache.lookup_executable(&memory, ROOT).unwrap().identity(),
        original.identity()
    );
    assert_eq!(original.wasm(), bytes);
    assert_eq!(cache.link_owners[7], original.identity());
    assert_eq!(cache.link_owners[8], cut.identity());
    assert_eq!(cache.snapshot()[5], 2);
    let snapshot = cache.snapshot();
    assert!(cache.preflight_fragment(&memory, &plan(&memory)).is_none());
    assert_eq!(cache.snapshot(), snapshot);
    assert!(cache.resident_charge() <= MAX_CODE_BYTES);
}

#[test]
fn hot_capability_is_quota_independent_but_execution_and_bypass_are_exact() {
    let memory = memory();
    let (mut cache, _) = fixture(&memory);
    let cut = add_cut(&memory, &mut cache);
    cache.install_link(&cut, 8).unwrap();
    assert_eq!(
        cache.current_hot_port(&memory, CUT).unwrap().1.instructions,
        3
    );
    for remaining in [0, 1, 2] {
        assert!(!cache.executable_frontier_contains(&memory, CUT, remaining, 0));
        assert!(
            cache
                .lookup_executable_linked(&memory, CUT, remaining)
                .is_none()
        );
    }
    assert_eq!(
        cache
            .lookup_executable_linked(&memory, CUT, 3)
            .unwrap()
            .0
            .identity(),
        cut.identity()
    );
    cache.bypass_trace(cut.identity());
    assert!(cache.current_hot_port(&memory, CUT).is_some());
    assert!(cache.lookup_executable(&memory, CUT).is_none());
    cache.bypass_trace(0);
    assert_eq!(
        cache.lookup_executable(&memory, CUT).unwrap().identity(),
        cut.identity()
    );
}

#[test]
fn fragment_commit_source_change_and_owner_aba_are_neutral() {
    let mut memory = memory();
    let (mut cache, original) = fixture(&memory);
    let p = plan(&memory);
    let admission = cache.preflight_fragment(&memory, &p).unwrap();
    let code = emit_observed_plan(&memory, p).unwrap().unwrap();
    memory.write(GuestAddress(CUT + 2), &[2]).unwrap();
    let snapshot = cache.snapshot();
    assert!(matches!(
        cache.commit_fragment(&memory, admission, code),
        Err(CompileError::InvalidLimits)
    ));
    assert_eq!(cache.snapshot(), snapshot);
    let p = plan(&memory);
    let admission = cache.preflight_fragment(&memory, &p).unwrap();
    let code = emit_observed_plan(&memory, p).unwrap().unwrap();
    cache.remove(original.key);
    let other = cache
        .admit(
            &memory,
            compile(
                &memory,
                GuestAddress(0x2000),
                CompileOptions {
                    instructions: 2,
                    blocks: 1,
                    ..CompileOptions::default()
                },
            )
            .unwrap(),
        )
        .unwrap();
    cache.install_link(&other, 7).unwrap();
    let snapshot = cache.snapshot();
    assert!(matches!(
        cache.commit_fragment(&memory, admission, code),
        Err(CompileError::InvalidLimits)
    ));
    assert_eq!(cache.snapshot(), snapshot);
    assert_eq!(cache.link_owners[7], other.identity());
    assert!(cache.current_hot_port(&memory, CUT).is_none());
}

#[test]
fn oversized_fragment_cannot_remove_or_credit_overlapping_source_owner() {
    let memory = memory();
    let (mut cache, original) = fixture(&memory);
    let p = plan(&memory);
    let admission = cache.preflight_fragment(&memory, &p).unwrap();
    let mut code = emit_observed_plan(&memory, p).unwrap().unwrap();
    code.reserve_wasm_capacity_for_test(MAX_CODE_BYTES);
    let snapshot = cache.snapshot();
    assert!(matches!(
        cache.commit_fragment(&memory, admission, code),
        Err(CompileError::WasmLimit)
    ));
    assert_eq!(cache.snapshot(), snapshot);
    assert_eq!(
        cache.lookup(&memory, ROOT).unwrap().identity(),
        original.identity()
    );
    assert_eq!(cache.link_owners[7], original.identity());
}

#[test]
fn true_byte_eviction_revokes_hot_cut_then_allows_source_current_relearning() {
    let memory = memory();
    let (mut cache, _) = fixture(&memory);
    let cut = add_cut(&memory, &mut cache);
    cache.install_link(&cut, 8).unwrap();
    for pc in [0x2000, 0x3000] {
        let mut filler = compile(
            &memory,
            GuestAddress(pc),
            CompileOptions {
                instructions: 2,
                blocks: 1,
                ..CompileOptions::default()
            },
        )
        .unwrap();
        filler.reserve_wasm_capacity_for_test(MAX_CODE_BYTES / 2);
        cache.admit(&memory, filler).unwrap();
    }
    assert!(cache.byte_evictions >= 2);
    assert!(cache.current_hot_port(&memory, CUT).is_none());
    assert!(!cache.precise_source_current(&memory, &cut));
    assert_eq!(cache.link_owners[8], 0);
    let restored = add_cut(&memory, &mut cache);
    cache.install_link(&restored, 8).unwrap();
    assert_ne!(restored.identity(), cut.identity());
    assert_eq!(
        cache.lookup_executable(&memory, CUT).unwrap().identity(),
        restored.identity()
    );
    assert!(cache.resident_charge() <= MAX_CODE_BYTES);
}

#[test]
fn budgeted_string_fragment_publishes_one_positive_fuel_hot_port() {
    let mut memory = memory();
    memory.write(GuestAddress(ROOT), &[0xf3, 0xa4]).unwrap();
    let cold = vec![completed(&memory, ROOT, ROOT + 2)];
    let events = vec![cold[0].event(0).unwrap()];
    let plan =
        prepare_cold_prefix_plan(&memory, CompileOptions::default(), &events, &cold, &|_| {
            false
        });
    let plan = plan.unwrap().unwrap();
    assert_eq!(plan.anchor(), ROOT);
    assert_eq!(plan.instruction_pcs().collect::<Vec<_>>(), [ROOT]);
    assert_eq!(
        plan.hot_ports()
            .iter()
            .map(|span| (span.entry, span.instructions))
            .collect::<Vec<_>>(),
        [(ROOT, 1)],
        "REP publishes minimum fuel, not a fixed element or retirement count"
    );
    let mut cache = CoreCache32::default();
    let admission = cache.preflight_fragment(&memory, &plan).unwrap();
    let code = emit_observed_plan(&memory, plan).unwrap().unwrap();
    assert!(code.eligible_hot_span(ROOT, 0).is_none());
    assert!(code.eligible_hot_span(ROOT, 1).is_some());
    assert!(code.eligible_hot_span(ROOT + 1, 1).is_none());
    let code = cache.commit_fragment(&memory, admission, code).unwrap();
    assert_eq!(
        cache.current_hot_port(&memory, ROOT).unwrap().0.identity(),
        code.identity()
    );
    assert!(cache.resident_charge() <= MAX_CODE_BYTES);
}

#[test]
fn legacy_linked_lookup_preserves_source_geometry_and_trace_baseline_priority() {
    let memory = memory();
    let mut cache = CoreCache32::default();
    let cut = add_cut(&memory, &mut cache);
    cache.install_link(&cut, 8).unwrap();
    assert_eq!(
        cache.lookup_linked(&memory, CUT).unwrap().0.identity(),
        cut.identity()
    );
    assert_eq!(
        cache.lookup_linked(&memory, 0x1008).unwrap().0.identity(),
        cut.identity(),
        "legacy chaining retains precise interior source ports"
    );
    let (mut cache, original) = fixture(&memory);
    let cut = add_cut(&memory, &mut cache);
    cache.install_link(&cut, 8).unwrap();
    assert!(matches!(
        cache
            .prepare_owned_entry(&memory, CUT, CompileOptions::default(), &[])
            .unwrap(),
        OwnedEntryLookup::Ready(_)
    ));
    assert_eq!(
        cache.lookup_linked(&memory, CUT).unwrap().0.identity(),
        original.identity(),
        "legacy baseline alias priority is unchanged"
    );
    use crate::cpu::core::{
        compile_observed,
        trace::{BLOCK, TraceEvent},
    };
    let event = TraceEvent {
        kind: BLOCK,
        pc: ROOT,
        end: 0x100d,
        offset: 1,
        owner: original.identity(),
    };
    let trace = compile_observed(
        &memory,
        GuestAddress(CUT),
        CompileOptions::default(),
        &[event, event],
        &[&original],
        memory.code_source_version(),
    )
    .unwrap()
    .unwrap();
    let trace = cache.admit(&memory, trace).unwrap();
    cache.install_link(&trace, 9).unwrap();
    assert_eq!(
        cache.lookup_linked(&memory, CUT).unwrap().0.identity(),
        trace.identity(),
        "legacy trace priority remains above its baseline alias"
    );
    assert_eq!(cache.link_owners[7], original.identity());
    assert_eq!(cache.link_owners[8], cut.identity());
}

#[test]
fn bypass_excludes_executable_and_private_geometry_fallbacks_without_retiring_owner() {
    let memory = memory();
    let (mut cache, original) = fixture(&memory);
    let cut = add_cut(&memory, &mut cache);
    cache.install_link(&cut, 8).unwrap();
    cache.remove(original.key);
    for pc in [CUT, 0x1008] {
        assert_eq!(
            cache.lookup(&memory, pc).unwrap().identity(),
            cut.identity()
        );
        assert_eq!(cache.lookup_linked(&memory, pc).unwrap().1, 8);
    }
    cache.bypass_trace(cut.identity());
    let before = cache.snapshot();
    let order = (cache.oldest(), cache.newest());
    for pc in [CUT, 0x1008] {
        assert!(cache.lookup(&memory, pc).is_none());
        assert!(cache.lookup_linked(&memory, pc).is_none());
        assert!(cache.current_hot_port(&memory, CUT).is_some());
    }
    assert_eq!(cache.snapshot(), before);
    assert_eq!((cache.oldest(), cache.newest()), order);
    cache.bypass_trace(0);
    for pc in [CUT, 0x1008] {
        assert_eq!(
            cache.lookup_linked(&memory, pc).unwrap().0.identity(),
            cut.identity()
        );
    }
    assert_eq!(cache.link_owners[8], cut.identity());
}
