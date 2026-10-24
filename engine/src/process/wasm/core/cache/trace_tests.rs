use super::*;
use crate::{
    cpu::core::{
        compile, compile_observed,
        trace::{BLOCK, TraceEvent},
    },
    memory::{PageRange, Permissions},
};

const ROOT: u32 = 0x1000;
const CALLEE: u32 = 0x2000;
const SOURCE: u32 = 0x3000;

fn options() -> CompileOptions {
    CompileOptions {
        instructions: 64,
        blocks: 8,
        ..CompileOptions::default()
    }
}

fn memory(fillers: usize) -> AddressSpace {
    let filler_pages = (fillers * 4).div_ceil(4096);
    let mut memory = AddressSpace::new((3 + filler_pages) as u32).unwrap();
    for pc in [ROOT, CALLEE, SOURCE] {
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(pc), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
    }
    // CALL 2000; DEC ECX; JNZ 1000; UD2. The callee remains an independent owner.
    memory
        .write(
            GuestAddress(ROOT),
            &[0xe8, 0xfb, 0x0f, 0, 0, 0x49, 0x75, 0xf8, 0x0f, 0x0b],
        )
        .unwrap();
    memory.write(GuestAddress(CALLEE), &[0x40, 0xc3]).unwrap();
    memory
        .write(GuestAddress(SOURCE), &[0x90, 0xff, 0xe6])
        .unwrap();
    if filler_pages != 0 {
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0x4000), filler_pages as u32).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
        memory
            .write(GuestAddress(0x4000), &[0x90, 0xff, 0xe6, 0].repeat(fillers))
            .unwrap();
    }
    memory
}

fn admit(cache: &mut CoreCache32, memory: &AddressSpace, pc: u32, slot: u32) -> RegionRef {
    let code = cache
        .admit(
            memory,
            compile(memory, GuestAddress(pc), options()).unwrap(),
        )
        .unwrap();
    cache.install_link(&code, slot).unwrap();
    code
}

fn sources(memory: &AddressSpace) -> (CoreCache32, RegionRef, RegionRef, RegionRef) {
    let mut cache = CoreCache32::default();
    let caller = admit(&mut cache, memory, ROOT, 1);
    let callee = admit(&mut cache, memory, CALLEE, 2);
    let source = admit(&mut cache, memory, SOURCE, 3);
    (cache, caller, callee, source)
}

fn observed(memory: &AddressSpace, caller: &CompiledCode, callee: &CompiledCode) -> CompiledCode {
    let block = |owner: &CompiledCode, pc, end| TraceEvent {
        kind: BLOCK,
        pc,
        end,
        offset: 0,
        owner: owner.identity(),
    };
    compile_observed(
        memory,
        GuestAddress(ROOT),
        options(),
        &[
            block(caller, ROOT, 0x1005),
            block(callee, CALLEE, 0x2002),
            block(caller, 0x1005, 0x1008),
            block(caller, ROOT, 0x1005), // final witness, not another occurrence
        ],
        &[caller, callee],
        memory.code_source_version(),
    )
    .unwrap()
    .unwrap()
}

fn selected(
    cache: &mut CoreCache32,
    memory: &AddressSpace,
    source: &RegionRef,
    pc: u32,
    expected: &RegionRef,
    slot: u32,
) {
    match cache.lookup_transition(memory, source, pc) {
        TransitionLookup::Linked(code, actual_slot) => {
            assert!(Rc::ptr_eq(&code.code, &expected.code));
            assert_eq!(actual_slot, slot);
            assert_eq!(cache.link_owners[slot as usize], expected.identity());
        }
        TransitionLookup::Miss => panic!("missing linked entry {pc:x}"),
        TransitionLookup::Stop => panic!("stale linked source for {pc:x}"),
    }
}

fn resource_invariants(cache: &CoreCache32) {
    let retained = cache.regions.iter().flatten().count();
    assert_eq!(MAX_CODE_BYTES, 128 << 20);
    assert_eq!(retained, cache.regions.len() - cache.free.len());
    assert_eq!(cache.snapshot()[5], retained as u64);
    assert_eq!(
        cache.bytes,
        cache
            .regions
            .iter()
            .flatten()
            .map(|entry| entry.code.wasm().len())
            .sum::<usize>()
    );
    assert_eq!(cache.snapshot()[6], cache.bytes as u64);
    assert!(cache.bytes <= MAX_CODE_BYTES);
    assert!(cache.resident_charge() <= MAX_CODE_BYTES);
    assert_eq!(cache.snapshot()[8], cache.resident_charge() as u64);
    assert_eq!(cache.snapshot()[9], MAX_CODE_BYTES as u64);
    assert_eq!(cache.snapshot()[10], cache.link_capacity() as u64);
    for (&pc, &key) in &cache.trace_entries {
        let entry = cache.region(key).unwrap();
        assert_eq!(entry.code.trace_root(), Some(pc));
        assert_eq!(entry.admitted_entries, [GuestAddress(pc)]);
    }
    for key in cache.entries.values().chain(cache.coverage.values()) {
        assert!(cache.region(*key).unwrap().code.trace_root().is_none());
    }
}

#[test]
fn linking_an_unlinked_overlay_invalidates_a_previously_cached_baseline_target() {
    let memory = memory(0);
    let (mut cache, caller, callee, source) = sources(&memory);
    let trace = cache
        .admit(&memory, observed(&memory, &caller, &callee))
        .unwrap();
    // The overlay exists but has not yet reached a real SDK table slot.
    selected(&mut cache, &memory, &source, ROOT, &caller, 1);
    let snapshot = cache.snapshot();
    cache.install_link(&trace, 4).unwrap();
    selected(&mut cache, &memory, &source, ROOT, &trace, 4);
    assert_eq!(
        cache.lookup(&memory, ROOT).unwrap().identity(),
        trace.identity()
    );
    let mut linked = snapshot;
    linked[11] += 1;
    assert_eq!(cache.snapshot(), linked);
    resource_invariants(&cache);
}

#[test]
fn overlay_overlap_preserves_baseline_coverage_ports_bodies_and_identities() {
    let memory = memory(0);
    let (mut cache, caller, callee, source) = sources(&memory);
    assert!(matches!(
        cache.prepare_owned_entry(&memory, 0x1006, options(), &[]).unwrap(),
        OwnedEntryLookup::Ready(ref owner) if owner.identity() == caller.identity()
    ));
    let coverage = cache.coverage.clone();
    let entries = cache.entries.clone();
    let ports = cache.retained(&caller).unwrap().admitted_entries.clone();
    let caller_bytes = caller.wasm().to_vec();
    let callee_bytes = callee.wasm().to_vec();
    let before = cache.snapshot();
    let trace = cache
        .admit(&memory, observed(&memory, &caller, &callee))
        .unwrap();
    cache.install_link(&trace, 4).unwrap();
    assert_eq!(trace.trace_root(), Some(ROOT));
    assert_eq!(trace.instruction_count(), 5);
    assert_eq!(trace.entrypoints(), [GuestAddress(ROOT)]);
    assert!(!trace.supports_entry(CALLEE));
    assert!(!trace.supports_entry(0x1006));
    assert_eq!(cache.coverage, coverage);
    assert_eq!(cache.entries, entries);
    assert_eq!(cache.retained(&caller).unwrap().admitted_entries, ports);
    assert_eq!(caller.wasm(), caller_bytes);
    assert_eq!(callee.wasm(), callee_bytes);
    assert_eq!(cache.snapshot()[5], before[5] + 1);
    assert_eq!(cache.snapshot()[6], before[6] + trace.wasm().len() as u64);
    selected(&mut cache, &memory, &source, ROOT, &trace, 4);
    selected(&mut cache, &memory, &source, 0x1006, &caller, 1);
    selected(&mut cache, &memory, &source, CALLEE, &callee, 2);
    resource_invariants(&cache);
}

#[test]
fn overlay_unlink_and_slot_aba_cannot_reuse_an_obsolete_linked_identity() {
    let memory = memory(0);
    let (mut cache, caller, callee, source) = sources(&memory);
    let trace = cache
        .admit(&memory, observed(&memory, &caller, &callee))
        .unwrap();
    cache.install_link(&trace, 4).unwrap();
    selected(&mut cache, &memory, &source, ROOT, &trace, 4);
    cache.unlink(4, trace.identity()).unwrap();
    selected(&mut cache, &memory, &source, ROOT, &caller, 1);
    cache.unlink(3, source.identity()).unwrap();
    cache.install_link(&source, 4).unwrap();
    assert_eq!(
        cache.unlink(4, trace.identity()),
        Err(LinkError::WrongOwner)
    );
    selected(&mut cache, &memory, &source, ROOT, &caller, 1);
    cache.install_link(&trace, 5).unwrap();
    selected(&mut cache, &memory, &source, ROOT, &trace, 5);
    cache.remove(trace.key);
    assert_eq!(cache.link_owners[5], 0);
    assert_eq!(cache.install_link(&trace, 5), Err(LinkError::Unretained));
    let replacement = cache
        .admit(&memory, observed(&memory, &caller, &callee))
        .unwrap();
    assert_ne!(replacement.identity(), trace.identity());
    cache.install_link(&replacement, 5).unwrap();
    selected(&mut cache, &memory, &source, ROOT, &replacement, 5);
    assert_eq!(
        cache.unlink(5, trace.identity()),
        Err(LinkError::WrongOwner)
    );
    resource_invariants(&cache);
}

#[test]
fn overlay_bypass_and_clear_change_cached_transition_selection_without_recompiling() {
    let memory = memory(0);
    let (mut cache, caller, callee, source) = sources(&memory);
    let trace = cache
        .admit(&memory, observed(&memory, &caller, &callee))
        .unwrap();
    cache.install_link(&trace, 4).unwrap();
    let snapshot = cache.snapshot();
    selected(&mut cache, &memory, &source, ROOT, &trace, 4);
    cache.bypass_trace(trace.identity());
    selected(&mut cache, &memory, &source, ROOT, &caller, 1);
    assert_eq!(
        cache.lookup(&memory, ROOT).unwrap().identity(),
        caller.identity()
    );
    cache.bypass_trace(0);
    selected(&mut cache, &memory, &source, ROOT, &trace, 4);
    assert_eq!(
        cache.lookup(&memory, ROOT).unwrap().identity(),
        trace.identity()
    );
    assert_eq!(cache.snapshot(), snapshot);
    resource_invariants(&cache);
}

#[test]
fn overlay_source_write_permission_or_identical_remap_rejects_cached_overlay_only() {
    for change in 0..3 {
        let mut memory = memory(0);
        let (mut cache, caller, callee, source) = sources(&memory);
        let trace = cache
            .admit(&memory, observed(&memory, &caller, &callee))
            .unwrap();
        cache.install_link(&trace, 4).unwrap();
        selected(&mut cache, &memory, &source, ROOT, &trace, 4);
        let range = PageRange::new(GuestAddress(CALLEE), 1).unwrap();
        match change {
            0 => memory.write(GuestAddress(CALLEE), &[0x48]).unwrap(),
            1 => memory.protect(range, Permissions::READ).unwrap(),
            _ => {
                memory.unmap(range).unwrap();
                memory.map_zeroed(range, Permissions::ALL).unwrap();
                memory.write(GuestAddress(CALLEE), &[0x40, 0xc3]).unwrap();
            }
        }
        assert!(!trace.is_current(&memory));
        assert!(!callee.is_current(&memory));
        assert!(caller.is_current(&memory));
        selected(&mut cache, &memory, &source, ROOT, &caller, 1);
        assert_eq!(cache.link_owners[4], 0);
        assert!(!cache.trace_entries.contains_key(&ROOT));
        assert!(cache.lookup(&memory, CALLEE).is_none());
        assert_eq!(cache.link_owners[2], 0);
        for pc in caller.instruction_pcs() {
            assert_eq!(cache.coverage.get(&pc), Some(&caller.key));
        }
        assert_eq!(cache.snapshot()[4], 2);
        resource_invariants(&cache);
    }
}

#[test]
fn explicit_lru_overlay_eviction_preserves_sources_and_allows_later_hot_rebuild() {
    let memory = memory(0);
    let (mut cache, caller, callee, source) = sources(&memory);
    let coverage = cache.coverage.clone();
    for _ in 0..1023 {
        assert!(!cache.trace_candidate(&caller, ROOT));
    }
    assert!(cache.trace_candidate(&caller, ROOT));
    let trace = cache
        .admit(&memory, observed(&memory, &caller, &callee))
        .unwrap();
    cache.install_link(&trace, 4).unwrap();
    for pc in [0x1005, CALLEE, SOURCE] {
        cache.lookup(&memory, pc).unwrap();
    }
    assert_eq!(cache.oldest(), Some(trace.key.index));
    cache.remove(trace.key);
    assert!(cache.retained(&trace).is_none());
    assert_eq!(cache.link_owners[4], 0);
    assert_eq!(
        cache.lookup(&memory, ROOT).unwrap().identity(),
        caller.identity()
    );
    assert_eq!(
        cache.lookup(&memory, CALLEE).unwrap().identity(),
        callee.identity()
    );
    assert_eq!(
        cache.lookup(&memory, SOURCE).unwrap().identity(),
        source.identity()
    );
    for (pc, key) in coverage {
        assert_eq!(cache.coverage.get(&pc), Some(&key));
    }
    for _ in 0..4095 {
        assert!(!cache.trace_candidate(&caller, ROOT));
    }
    assert!(cache.trace_candidate(&caller, ROOT));
    let rebuilt = cache
        .admit(&memory, observed(&memory, &caller, &callee))
        .unwrap();
    assert_ne!(rebuilt.identity(), trace.identity());
    cache.install_link(&rebuilt, 4).unwrap();
    selected(&mut cache, &memory, &source, ROOT, &rebuilt, 4);
    assert_eq!(
        cache.unlink(4, trace.identity()),
        Err(LinkError::WrongOwner)
    );
    assert_eq!(cache.snapshot()[2..5], [0, 0, 0]);
    resource_invariants(&cache);
}

#[test]
fn a_rare_first_port_cannot_permanently_suppress_a_different_hot_entry() {
    let memory = memory(0);
    let (mut cache, caller, _, _) = sources(&memory);
    let before = cache.snapshot();
    let coverage = cache.coverage.clone();
    assert!(!cache.trace_candidate(&caller, ROOT));
    for _ in 0..1023 {
        assert!(!cache.trace_candidate(&caller, 0x1005));
    }
    assert!(cache.trace_candidate(&caller, 0x1005));
    assert_eq!(cache.snapshot(), before);
    assert_eq!(cache.coverage, coverage);
    resource_invariants(&cache);
}

#[test]
fn a_declined_capture_backs_off_but_does_not_permanently_consume_the_site() {
    let memory = memory(0);
    let (mut cache, caller, _, _) = sources(&memory);
    let before = cache.snapshot();
    for _ in 0..1023 {
        assert!(!cache.trace_candidate(&caller, ROOT));
    }
    assert!(cache.trace_candidate(&caller, ROOT));
    // No artifact was admitted: the capture may have ended before a completed
    // span, or exceeded its bounds. Neither condition poisons the source root.
    for _ in 0..4095 {
        assert!(!cache.trace_candidate(&caller, ROOT));
    }
    assert!(cache.trace_candidate(&caller, ROOT));
    assert_eq!(cache.snapshot(), before);
    resource_invariants(&cache);
}

#[test]
fn an_installed_observed_owner_can_itself_become_a_growth_candidate() {
    let memory = memory(0);
    let (mut cache, caller, callee, _) = sources(&memory);
    let trace = cache
        .admit(&memory, observed(&memory, &caller, &callee))
        .unwrap();
    cache.install_link(&trace, 4).unwrap();
    let before = cache.snapshot();
    for _ in 0..1023 {
        assert!(!cache.trace_candidate(&trace, ROOT));
    }
    assert!(cache.trace_candidate(&trace, ROOT));
    assert_eq!(cache.snapshot(), before);
    resource_invariants(&cache);
}

#[test]
fn strictly_larger_same_root_executor_replaces_only_its_predecessor_and_slot() {
    let memory = memory(0);
    let (mut cache, caller, callee, source) = sources(&memory);
    let event = |owner: &CompiledCode, pc, end| TraceEvent {
        kind: BLOCK,
        pc,
        end,
        offset: 0,
        owner: owner.identity(),
    };
    let short = compile_observed(
        &memory,
        GuestAddress(ROOT),
        options(),
        &[event(&caller, ROOT, 0x1005), event(&callee, CALLEE, 0x2002)],
        &[&caller, &callee],
        memory.code_source_version(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(short.instruction_count(), 1);
    let short = cache.admit(&memory, short).unwrap();
    cache.install_link(&short, 4).unwrap();
    selected(&mut cache, &memory, &source, ROOT, &short, 4);
    let before = cache.snapshot();
    let coverage = cache.coverage.clone();
    let entries = cache.entries.clone();
    let bigger = compile_observed(
        &memory,
        GuestAddress(ROOT),
        options(),
        &[
            event(&short, ROOT, 0x1005),
            event(&callee, CALLEE, 0x2002),
            event(&caller, 0x1005, 0x1008),
            event(&short, ROOT, 0x1005),
        ],
        &[&short, &caller, &callee],
        memory.code_source_version(),
    )
    .unwrap()
    .unwrap();
    assert!(
        bigger
            .trace_score()
            .unwrap()
            .strictly_grows(short.trace_score().unwrap())
    );
    let bigger = cache.admit(&memory, bigger).unwrap();
    assert!(cache.retained(&short).is_none());
    assert_eq!(cache.link_owners[4], 0);
    assert_eq!(cache.snapshot()[5], before[5]);
    assert_eq!(
        cache.snapshot()[6],
        before[6] - short.wasm().len() as u64 + bigger.wasm().len() as u64
    );
    assert_eq!(cache.snapshot()[2..5], before[2..5]);
    assert_eq!(cache.coverage, coverage);
    assert_eq!(cache.entries, entries);
    cache.install_link(&bigger, 4).unwrap();
    assert_eq!(
        cache.unlink(4, short.identity()),
        Err(LinkError::WrongOwner)
    );
    selected(&mut cache, &memory, &source, ROOT, &bigger, 4);
    selected(&mut cache, &memory, &source, CALLEE, &callee, 2);
    let snapshot = cache.snapshot();
    assert!(matches!(
        cache.admit(&memory, observed(&memory, &caller, &callee)),
        Err(CompileError::InvalidLimits)
    ));
    assert_eq!(cache.snapshot(), snapshot);
    selected(&mut cache, &memory, &source, ROOT, &bigger, 4);
    resource_invariants(&cache);
}

#[test]
fn real_observed_serialized_bytes_use_the_same_128_mib_budget_as_baseline_owners() {
    let mut memory = AddressSpace::new(4).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(ROOT), 4).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory
        .write(GuestAddress(ROOT), &[0x83, 0x03, 1].repeat(4096))
        .unwrap();
    memory
        .write(GuestAddress(0x4000), &[0x90, 0xff, 0xe6])
        .unwrap();
    let limits = CompileOptions {
        instructions: 4096,
        blocks: 1,
        wasm_bytes: 16 << 20,
    };
    let mut cache = CoreCache32::default();
    let body = cache
        .admit(
            &memory,
            compile(&memory, GuestAddress(ROOT), limits).unwrap(),
        )
        .unwrap();
    let witness = cache
        .admit(
            &memory,
            compile(&memory, GuestAddress(0x4000), options()).unwrap(),
        )
        .unwrap();
    let mut admitted_bytes = cache.snapshot()[1];
    let mut traces = 0;
    // Each real overlay covers a different observed suffix of the same immutable
    // source owner. No serialized bytes or cache accounting are synthesized.
    for offset in 0..512 {
        let root = ROOT + offset * 3;
        let code = compile_observed(
            &memory,
            GuestAddress(root),
            limits,
            &[
                TraceEvent {
                    kind: BLOCK,
                    pc: ROOT,
                    end: 0x4000,
                    offset,
                    owner: body.identity(),
                },
                TraceEvent {
                    kind: BLOCK,
                    pc: 0x4000,
                    end: 0x4003,
                    offset: 0,
                    owner: witness.identity(),
                },
            ],
            &[&body, &witness],
            memory.code_source_version(),
        )
        .unwrap()
        .unwrap();
        assert!(code.wasm().len() > MAX_CODE_BYTES / MAX_REGIONS);
        admitted_bytes += code.wasm().len() as u64;
        cache.admit(&memory, code).unwrap();
        traces += 1;
        resource_invariants(&cache);
        if cache.snapshot()[3] != 0 {
            break;
        }
    }
    assert!(
        cache.snapshot()[3] > 0,
        "actual serialized overlays must cause byte pressure"
    );
    assert_eq!(cache.snapshot()[2], 0);
    assert_eq!(cache.snapshot()[0], traces + 2);
    assert_eq!(cache.snapshot()[1], admitted_bytes);
    assert!(cache.snapshot()[5] < traces + 2);
    assert!(cache.snapshot()[5] < MAX_REGIONS as u64);
    resource_invariants(&cache);
}
