use super::*;
use crate::{
    cpu::core::compile,
    memory::{PageRange, Permissions},
};

const A: u32 = 0x1000;
const B: u32 = 0x2000;
const C: u32 = 0x3000;
const CONTEXT: u32 = 0x1234;

fn fixture() -> (AddressSpace, CoreCache32, RegionRef, RegionRef) {
    let mut memory = AddressSpace::new(3).unwrap();
    for (pc, bytes) in [
        (A, [0x90, 0xff, 0xe7]),
        (B, [0x40, 0xff, 0xe6]),
        (C, [0x41, 0xff, 0xe6]),
    ] {
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(pc), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
        memory.write(GuestAddress(pc), &bytes).unwrap();
    }
    let options = CompileOptions {
        instructions: 2,
        blocks: 1,
        ..CompileOptions::default()
    };
    let first = compile(&memory, GuestAddress(A), options).unwrap();
    let second = compile(&memory, GuestAddress(B), options).unwrap();
    let mut cache = CoreCache32::default();
    let first = cache.admit(&memory, first).unwrap();
    let second = cache.admit(&memory, second).unwrap();
    cache.install_link(&first, 1).unwrap();
    cache.install_link(&second, 2).unwrap();
    (memory, cache, first, second)
}

fn row(cache: &CoreCache32, pc: u32) -> route::Row {
    *cache
        .route_rows
        .iter()
        .find(|row| row.state == route::PRESENT && row.pc == pc)
        .unwrap()
}

fn source(cache: &CoreCache32, memory: &AddressSpace, selected: &RegionRef) -> Option<RegionRef> {
    let ticket = cache.continuation_header();
    cache.continuation_source(
        memory,
        CONTEXT,
        ticket.current_slot,
        ticket.current_generation,
        Some(selected),
        false,
        true,
    )
}

#[test]
fn continuation_ticket_adopts_real_callee_without_changing_fragment_identity_or_bytes() {
    let (memory, mut cache, first, second) = fixture();
    let bytes = [first.wasm().to_vec(), second.wasm().to_vec()];
    let identities = [first.identity(), second.identity()];
    let admitted = cache.compiled;
    let first_row = row(&cache, A).candidates[2];
    let second_row = row(&cache, B).candidates[2];
    assert_eq!(
        (first_row.identity, first_row.work, first_row.port),
        (first.identity(), 2, 0)
    );
    assert_eq!(
        (second_row.identity, second_row.work, second_row.port),
        (second.identity(), 2, 0)
    );
    assert!(cache.begin_continuation(&memory, CONTEXT, 0x5678, Some(&first), A));
    let header_ptr = cache.continuation_header() as *const route::Header;
    let pointers = (
        cache.continuation_header().rows,
        cache.continuation_header().owners,
        cache.continuation_header().link_owners,
        cache.continuation_header().slot_generations,
    );

    // The generated transition writes a ticket from this exact directory row.
    // Rust's held selected owner is intentionally still the caller here.
    let record = cache.route_owners[second.key.index];
    let h = cache.continuation_header_mut();
    h.current_index = second_row.index;
    h.current_slot = record.slot;
    h.current_id = second_row.identity;
    h.current_generation = record.generation;
    h.current_entry = B;
    h.current_port = second_row.port;
    h.current_nonce = h.nonce;
    h.flags = 0;
    assert_eq!(
        source(&cache, &memory, &first).unwrap().identity(),
        second.identity()
    );
    assert_eq!(
        cache.continuation_header() as *const route::Header,
        header_ptr
    );
    assert_eq!(
        (
            cache.continuation_header().rows,
            cache.continuation_header().owners,
            cache.continuation_header().link_owners,
            cache.continuation_header().slot_generations
        ),
        pointers
    );
    assert_eq!([first.identity(), second.identity()], identities);
    assert_eq!(
        [first.wasm(), second.wasm()],
        [bytes[0].as_slice(), bytes[1].as_slice()]
    );
    assert_eq!(cache.compiled, admitted);
    assert!(cache.resident_charge() <= MAX_CODE_BYTES);
    cache.end_continuation();
    assert!(source(&cache, &memory, &first).is_none());
}

#[test]
fn continuation_ticket_rejects_context_nonce_memory_and_reciprocal_slot_forgery() {
    let (memory, mut cache, first, _) = fixture();
    assert!(cache.begin_continuation(&memory, CONTEXT, 0, Some(&first), A));
    let generation = cache.continuation_header().current_generation;
    assert!(
        cache
            .continuation_source(
                &memory,
                CONTEXT + 4,
                1,
                generation,
                Some(&first),
                false,
                true
            )
            .is_none()
    );
    assert!(
        cache
            .continuation_source(&memory, CONTEXT, 2, generation, Some(&first), false, true)
            .is_none()
    );
    assert!(
        cache
            .continuation_source(
                &memory,
                CONTEXT,
                1,
                generation + 1,
                Some(&first),
                false,
                true
            )
            .is_none()
    );
    let other_memory = AddressSpace::new(1).unwrap();
    assert!(source(&cache, &other_memory, &first).is_none());
    cache.continuation_header_mut().current_nonce = 0;
    assert!(source(&cache, &memory, &first).is_none());
    let nonce = cache.continuation_header().nonce;
    cache.continuation_header_mut().current_nonce = nonce;
    cache.slot_regions[1] = NO_REGION;
    assert!(source(&cache, &memory, &first).is_none());
    cache.slot_regions[1] = first.key.index as u32;
    assert_eq!(
        source(&cache, &memory, &first).unwrap().identity(),
        first.identity()
    );
    cache.end_continuation();
}

#[test]
fn continuation_slot_aba_revokes_old_ticket_even_for_same_retained_owner() {
    let (memory, mut cache, first, _) = fixture();
    assert!(cache.begin_continuation(&memory, CONTEXT, 0, Some(&first), A));
    let original_generation = cache.continuation_header().current_generation;
    cache.unlink(1, first.identity()).unwrap();
    assert!(source(&cache, &memory, &first).is_none());
    cache.install_link(&first, 1).unwrap();
    assert!(cache.slot_generations[1] > original_generation);
    assert!(source(&cache, &memory, &first).is_none());
    assert!(cache.bind_continuation(&memory, &first, A));
    assert_eq!(
        source(&cache, &memory, &first).unwrap().identity(),
        first.identity()
    );
    cache.end_continuation();
    cache.remove(first.key);
    let replacement = cache
        .admit(
            &memory,
            compile(
                &memory,
                GuestAddress(C),
                CompileOptions {
                    instructions: 2,
                    blocks: 1,
                    ..CompileOptions::default()
                },
            )
            .unwrap(),
        )
        .unwrap();
    cache.install_link(&replacement, 1).unwrap();
    assert_eq!(replacement.key.index, first.key.index);
    assert_ne!(replacement.identity(), first.identity());
    assert!(cache.begin_continuation(&memory, CONTEXT, 0, Some(&replacement), C));
    assert!(
        cache
            .continuation_source(
                &memory,
                CONTEXT,
                1,
                original_generation,
                Some(&first),
                false,
                true
            )
            .is_none()
    );
    cache.end_continuation();
}

#[test]
fn continuation_source_change_only_allows_explicit_stale_metadata_validation() {
    let (mut memory, mut cache, first, _) = fixture();
    assert!(cache.begin_continuation(&memory, CONTEXT, 0, Some(&first), A));
    let generation = cache.continuation_header().current_generation;
    memory.write(GuestAddress(A), &[0x90]).unwrap();
    assert!(source(&cache, &memory, &first).is_none());
    assert_eq!(
        cache
            .continuation_source(&memory, CONTEXT, 1, generation, Some(&first), true, false)
            .unwrap()
            .identity(),
        first.identity()
    );
    cache.end_continuation();
    assert!(!cache.begin_continuation(&memory, CONTEXT, 0, Some(&first), A));
}

#[test]
fn continuation_unbound_and_transient_capabilities_require_the_exact_held_owner() {
    let (memory, mut cache, first, second) = fixture();
    cache.unlink(1, first.identity()).unwrap();
    assert!(cache.begin_continuation(&memory, CONTEXT, 0, Some(&first), A));
    assert_eq!(cache.continuation_header().flags, route::SELECTED_UNBOUND);
    assert!(source(&cache, &memory, &second).is_none());
    assert_eq!(
        source(&cache, &memory, &first).unwrap().identity(),
        first.identity()
    );
    cache.end_continuation();
    let transient = RegionRef::transient(
        compile(
            &memory,
            GuestAddress(C),
            CompileOptions {
                instructions: 2,
                blocks: 1,
                ..CompileOptions::default()
            },
        )
        .unwrap(),
    );
    assert!(cache.begin_continuation(&memory, CONTEXT, 0, Some(&transient), C));
    assert_eq!(cache.continuation_header().flags, route::TRANSIENT);
    assert!(source(&cache, &memory, &first).is_none());
    assert_eq!(
        source(&cache, &memory, &transient).unwrap().identity(),
        transient.identity()
    );
    cache.end_continuation();
    assert!(cache.begin_continuation(&memory, CONTEXT, 0, None, A));
    assert_eq!(cache.continuation_header().flags, route::COLD);
    assert!(source(&cache, &memory, &first).is_none());
    cache.end_continuation();
}

#[test]
fn continuation_window_ticket_keeps_public_root_and_parent_aliases() {
    let (memory, mut cache, caller, callee) = super::window_tests::fixture();
    let aliases = cache.trace_entries.clone();
    let window = super::window_tests::install(&memory, &mut cache, &caller, &callee);
    cache.install_link(&window, 9).unwrap();
    assert_eq!(cache.trace_entries, aliases);
    let root = window.trace_root().unwrap();
    assert_eq!(row(&cache, root).candidates[0].identity, window.identity());
    assert_eq!(row(&cache, root).candidates[0].flags, route::WINDOW_ROOT);
    assert!(cache.begin_continuation(&memory, CONTEXT, 0, Some(&window), root));
    assert_eq!(
        source(&cache, &memory, &caller).unwrap().identity(),
        window.identity()
    );
    // The complete callee block is another public capability on the same
    // owner. Its private RET instruction still cannot forge a public ticket.
    assert!(cache.bind_continuation(&memory, &window, 0x2000));
    assert_eq!(source(&cache, &memory, &caller).unwrap().identity(), window.identity());
    assert!(window.contains_instruction(0x2001));
    assert!(!window.supports_entry(0x2001));
    cache.continuation_header_mut().current_entry = 0x2001;
    assert!(source(&cache, &memory, &caller).is_none());
    assert!(cache.bind_continuation(&memory, &window, root));
    assert_eq!(
        source(&cache, &memory, &caller).unwrap().identity(),
        window.identity()
    );
    cache.end_continuation();
    cache.bypass_trace(window.identity());
    assert_eq!(
        cache.lookup_linked(&memory, root).unwrap().0.identity(),
        caller.identity()
    );
    cache.bypass_trace(0);
    assert_eq!(
        cache.lookup_linked(&memory, root).unwrap().0.identity(),
        window.identity()
    );
    assert_eq!(cache.trace_entries, aliases);
}

#[test]
fn continuation_lru_uses_one_shared_pod_list_and_route_allocation_is_charged() {
    let (memory, mut cache, first, second) = fixture();
    assert_eq!(cache.oldest(), Some(first.key.index));
    assert_eq!(cache.newest(), Some(second.key.index));
    cache.touch(first.key.index);
    assert_eq!(cache.oldest(), Some(second.key.index));
    assert_eq!(cache.newest(), Some(first.key.index));
    assert_eq!(
        cache.route_owners[first.key.index].older,
        second.key.index as u32 + 1
    );
    assert_eq!(
        cache.route_owners[second.key.index].newer,
        first.key.index as u32 + 1
    );
    let charge = cache.resident_charge();
    let capacity = cache.route_rows.capacity();
    cache.reserve_routes(20);
    assert_eq!(
        cache.resident_charge() - charge,
        (cache.route_rows.capacity() - capacity) * size_of::<route::Row>()
    );
    let removed_row = cache
        .route_rows
        .iter()
        .position(|row| row.state == route::PRESENT && row.pc == B)
        .unwrap();
    cache.remove(second.key);
    assert_eq!(cache.oldest(), Some(first.key.index));
    assert_eq!(cache.newest(), Some(first.key.index));
    assert_eq!(cache.route_owners[first.key.index].older, 0);
    assert_eq!(cache.route_owners[first.key.index].newer, 0);
    assert_eq!(cache.route_count, 1);
    assert_eq!(cache.route_rows[removed_row].state, route::DELETED);
    assert!(
        !cache
            .route_rows
            .iter()
            .any(|row| row.state == route::PRESENT && row.pc == B)
    );
    let third = cache
        .admit(
            &memory,
            compile(
                &memory,
                GuestAddress(C),
                CompileOptions {
                    instructions: 2,
                    blocks: 1,
                    ..CompileOptions::default()
                },
            )
            .unwrap(),
        )
        .unwrap();
    cache.install_link(&third, 3).unwrap();
    let mask = cache.route_rows.len() as u32 - 1;
    assert_eq!(
        route::hash(A) & mask,
        route::hash(C) & mask,
        "the real instruction PCs share a probe chain"
    );
    assert_eq!(
        cache.route_rows[removed_row].pc, C,
        "a new source-current fragment can reuse the deleted bucket"
    );
    assert_eq!(cache.route_count, 2);
    cache.remove(first.key);
    assert_eq!(cache.route_count, 1);
    // Refresh must find the existing C beyond the deleted A bucket, rather
    // than stop there and manufacture a duplicate live directory entry.
    cache.refresh_route_pc(C);
    assert_eq!(cache.route_count, 1);
    assert_eq!(
        cache
            .route_rows
            .iter()
            .filter(|row| row.state == route::PRESENT && row.pc == C)
            .count(),
        1
    );
    assert_eq!(
        cache.lookup_linked(&memory, C).unwrap().0.identity(),
        third.identity()
    );
    assert_eq!(cache.oldest(), Some(third.key.index));
    assert_eq!(cache.newest(), Some(third.key.index));
    assert!(cache.resident_charge() <= MAX_CODE_BYTES);
    assert!(first.is_current(&memory));
}
