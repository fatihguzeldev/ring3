use super::window_tests::{family, resources};
use super::*;
use crate::{
    cpu::core::{
        emit_hot_window, prepare_hot_window,
        trace::{BLOCK, TraceEvent},
    },
    memory::{PageRange, Permissions},
};

const A: u32 = 0x1000;
const B: u32 = 0x2000;
const C: u32 = 0x3000;

fn fixture() -> (AddressSpace, CoreCache32, [RegionRef; 3]) {
    let mut memory = AddressSpace::new(3).unwrap();
    for pc in [A, B, C] {
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(pc), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
    }
    // a: jmp b. b: dec ecx; jnz a; jmp c. c: mov ecx,edx; jmp a.
    // the old a/b cycle repeats while ecx>1. with edx=3, the larger c cycle
    // traverses that old cycle twice before taking b's actual exit back to c.
    memory
        .write(GuestAddress(A), &[0xe9, 0xfb, 0x0f, 0, 0])
        .unwrap();
    memory
        .write(
            GuestAddress(B),
            &[
                0x49, 0x0f, 0x85, 0xf9, 0xef, 0xff, 0xff, 0xe9, 0xf4, 0x0f, 0, 0,
            ],
        )
        .unwrap();
    memory
        .write(GuestAddress(C), &[0x89, 0xd1, 0xe9, 0xf9, 0xdf, 0xff, 0xff])
        .unwrap();
    let codes = [
        family(&memory, &[(A, B)], &[]),
        family(&memory, &[(B, B + 1), (B + 1, B + 7), (B + 7, C)], &[]),
        family(&memory, &[(C, C + 2), (C + 2, A)], &[]),
    ];
    let mut cache = CoreCache32::default();
    let owners = codes.map(|code| cache.admit(&memory, code).unwrap());
    for (owner, slot) in owners.iter().zip([7, 8, 10]) {
        cache.install_link(owner, slot).unwrap();
    }
    (memory, cache, owners)
}

fn capture(owners: &[RegionRef; 3], large: bool, preserve: bool) -> Vec<TraceEvent> {
    let a = (A, A + 5, owners[0].identity());
    let b = (B, B + 7, owners[1].identity());
    let exit = (B + 7, B + 12, owners[1].identity());
    let c = (C, C + 7, owners[2].identity());
    let cycle = if !large {
        vec![a, b]
    } else if preserve {
        vec![c, a, b, a, b, a, b, exit]
    } else {
        // edx=1 is a different real closed c cycle. its graph has more nodes
        // than the old executor, but omits the old taken b->a edge.
        vec![c, a, b, exit]
    };
    cycle
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
        .collect()
}

fn plan(memory: &AddressSpace, owners: &[RegionRef; 3], large: bool, preserve: bool) -> WindowPlan {
    let events = capture(owners, large, preserve);
    prepare_hot_window(
        memory,
        GuestAddress(events[0].pc),
        CompileOptions::default(),
        &events,
        &owners.iter().map(|owner| &**owner).collect::<Vec<_>>(),
        memory.code_source_version(),
        &[],
    )
    .unwrap()
    .unwrap()
}

fn install_old(
    memory: &AddressSpace,
    cache: &mut CoreCache32,
    owners: &[RegionRef; 3],
) -> RegionRef {
    let plan = plan(memory, owners, false, true);
    let admission = cache.preflight_window(memory, &plan).unwrap();
    let code = emit_hot_window(memory, plan).unwrap().unwrap();
    let old = cache.commit_window(memory, admission, code).unwrap();
    cache.install_link(&old, 9).unwrap();
    old
}

#[test]
fn strictly_covering_actual_window_replaces_only_the_old_execution_tier() {
    let (memory, mut cache, owners) = fixture();
    let old = install_old(&memory, &mut cache, &owners);
    let old_generation = cache.slot_generations[9];
    let bytes: Vec<_> = owners.iter().map(|owner| owner.wasm().to_vec()).collect();
    let before = cache.snapshot();
    let pending = plan(&memory, &owners, true, true);
    assert!(
        pending
            .fingerprint()
            .strictly_covers(old.window_fingerprint().unwrap())
    );
    let admission = cache.preflight_window(&memory, &pending).unwrap();
    assert_eq!(admission.replace, [old.key]);
    assert!(admission.stale.is_empty());
    let code = emit_hot_window(&memory, pending).unwrap().unwrap();
    let new_bytes = code.wasm().len();
    let replacement = cache.commit_window(&memory, admission, code).unwrap();
    assert_eq!(
        cache.snapshot()[5],
        before[5],
        "one optimized executor replaces one executor"
    );
    assert_eq!(
        cache.snapshot()[6],
        before[6] - old.wasm().len() as u64 + new_bytes as u64
    );
    assert_eq!(
        cache.snapshot()[3..5],
        before[3..5],
        "supersession is neither byte eviction nor SMC"
    );
    assert_eq!(cache.window_entries.len(), 1);
    assert!(cache.region(old.key).is_none());
    assert_eq!(cache.link_owners[9], 0);
    assert!(cache.slot_generations[9] > old_generation);
    assert!(!cache.precise_source_current(&memory, &old));
    for ((owner, bytes), slot) in owners.iter().zip(bytes).zip([7, 8, 10]) {
        assert_eq!(cache.region(owner.key).unwrap().code.wasm(), bytes);
        assert_eq!(cache.link_owners[slot], owner.identity());
        assert!(owner.is_current(&memory));
    }
    assert_eq!(
        cache.lookup_executable(&memory, A).unwrap().identity(),
        owners[0].identity(),
        "the retired window root falls back to its original immutable owner"
    );
    assert_eq!(
        cache.lookup_executable(&memory, C).unwrap().identity(),
        replacement.identity()
    );
    cache.install_link(&replacement, 9).unwrap();
    assert!(cache.slot_generations[9] > old_generation);
    assert!(cache.begin_continuation(&memory, 0x80, 0x100, Some(&replacement), C));
    assert!(
        cache
            .continuation_source(&memory, 0x80, 9, old_generation, Some(&old), false, true)
            .is_none()
    );
    cache.end_continuation();
    let checkpoint = cache.snapshot();
    assert!(
        cache
            .preflight_window(&memory, &plan(&memory, &owners, true, true))
            .is_none()
    );
    assert!(
        cache
            .preflight_window(&memory, &plan(&memory, &owners, false, true))
            .is_none()
    );
    assert_eq!(
        cache.snapshot(),
        checkpoint,
        "equal or covered-old graph does not emit or mutate residency"
    );
    resources(&cache);
}

#[test]
fn larger_incomparable_overlap_is_declined_before_emission_without_retiring_any_owner() {
    let (memory, mut cache, owners) = fixture();
    let old = install_old(&memory, &mut cache, &owners);
    let pending = plan(&memory, &owners, true, false);
    assert!(pending.instruction_pcs().count() > old.instruction_count());
    assert!(
        !pending
            .fingerprint()
            .strictly_covers(old.window_fingerprint().unwrap())
    );
    let before = cache.snapshot();
    let order = (cache.oldest(), cache.newest());
    assert!(cache.preflight_window(&memory, &pending).is_none());
    assert_eq!(cache.snapshot(), before);
    assert_eq!((cache.oldest(), cache.newest()), order);
    assert_eq!(cache.window_entries.get(&A), Some(&old.key));
    assert_eq!(cache.link_owners[9], old.identity());
    resources(&cache);
}

#[test]
fn weighted_hot_samples_preserve_residual_visits_and_per_site_backoff() {
    let mut sites = [TraceHeat::default(); 4];
    for _ in 0..100 {
        assert!(!TraceHeat::visit(&mut sites, A));
    }
    assert!(TraceHeat::visit_weight(&mut sites, A, 1024));
    assert_eq!((sites[0].visits, sites[0].interval), (100, 4096));
    for _ in 0..3 {
        assert!(!TraceHeat::visit_weight(&mut sites, A, 1024));
    }
    assert!(TraceHeat::visit_weight(&mut sites, A, 1024));
    assert_eq!((sites[0].visits, sites[0].interval), (100, 16384));
    assert!(TraceHeat::visit_weight(&mut sites, B, 1024));
    assert_eq!((sites[1].visits, sites[1].interval), (0, 4096));
    assert_eq!((sites[0].visits, sites[0].interval), (100, 16384));
}

#[test]
fn weighted_candidates_require_the_actual_retained_nonwindow_owner() {
    let (memory, mut cache, owners) = fixture();
    let old = install_old(&memory, &mut cache, &owners);
    assert!(!cache.sample_candidate(&old, A));
    assert!(!cache.sample_candidate(&owners[0], C));
    assert!(cache.sample_candidate(&owners[0], A));
    for _ in 0..3 {
        assert!(!cache.sample_candidate(&owners[0], A));
    }
    assert!(cache.sample_candidate(&owners[0], A));
    cache.remove(owners[0].key);
    let before = cache.snapshot();
    assert!(!cache.sample_candidate(&owners[0], A));
    assert_eq!(cache.snapshot(), before);
    resources(&cache);
}
