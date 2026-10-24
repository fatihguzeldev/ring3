use super::*;
use crate::{
    cpu::core::{
        emit_hot_window, prepare_hot_window,
        trace::{BLOCK, TraceEvent},
    },
    memory::{PageRange, Permissions},
};

fn source() -> (AddressSpace, CompiledCode, CompiledCode) {
    let mut memory = AddressSpace::new(2).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    let mut path = Vec::new();
    for block in 0..10 {
        let pc = 0x1000 + block * 4;
        memory.write(GuestAddress(pc), &[0x40, 0xff, 0xe6]).unwrap();
        path.push((pc, pc + 1));
        path.push((pc + 1, if block == 9 { 0x1000 } else { pc + 4 }));
    }
    let first = super::super::window_tests::family(&memory, &path, &[]);
    let second = super::super::window_tests::family(&memory, &path, &[]);
    (memory, first, second)
}

fn plan(
    memory: &AddressSpace,
    first: &CompiledCode,
    second: &CompiledCode,
    blocks: u32,
) -> WindowPlan {
    let cycle: Vec<_> = (0..blocks)
        .map(|block| TraceEvent {
            kind: BLOCK,
            pc: 0x1000 + block * 4,
            end: 0x1003 + block * 4,
            offset: 0,
            owner: if block % 2 == 0 {
                first.identity()
            } else {
                second.identity()
            },
        })
        .collect();
    let events: Vec<_> = cycle
        .iter()
        .chain(cycle.iter())
        .chain(cycle.first())
        .copied()
        .collect();
    prepare_hot_window(
        memory,
        GuestAddress(0x1000),
        CompileOptions::default(),
        &events,
        &[first, second],
        memory.code_source_version(),
        &[],
    )
    .unwrap()
    .unwrap()
}

#[test]
fn geometric_prefixes_at_one_anchor_all_survive_two_distinct_evidence_rounds() {
    let (memory, first, second) = source();
    let plans: Vec<_> = (2..=10)
        .map(|blocks| plan(&memory, &first, &second, blocks))
        .collect();
    let mut cache = CoreCache32::default();
    let attempts = cache.policy.attempts;
    for plan in &plans {
        assert!(!cache.observe_window_candidate(plan, 1, false));
    }
    for plan in &plans {
        assert!(
            !cache.observe_window_candidate(plan, 1, false),
            "one capture cannot count twice"
        );
    }
    for plan in &plans {
        assert!(
            cache.observe_window_candidate(plan, 2, false),
            "all nine distinct geometric prefixes need stable buckets"
        );
    }
    assert_eq!(cache.policy.attempts, attempts);
    assert_eq!(cache.policy.credit, INITIAL_CREDIT);
    assert!(cache.policy.allocated() <= POLICY_BYTES);
    assert_eq!(cache.snapshot()[1], 0, "evidence never emits an artifact");
}

#[test]
fn fractional_completed_work_and_attempt_failure_debt_do_not_refill_on_owner_removal() {
    let (memory, first, second) = source();
    let mut cache = CoreCache32::default();
    cache.earn_completed_work(31);
    assert_eq!(cache.policy.credit, INITIAL_CREDIT);
    cache.earn_completed_work(1);
    assert_eq!(cache.policy.credit, INITIAL_CREDIT + 1);
    let pending = plan(&memory, &first, &second, 2);
    let admission = cache.preflight_window(&memory, &pending).unwrap();
    let after_reserve = cache.policy.credit;
    let mut code = emit_hot_window(&memory, pending).unwrap().unwrap();
    code.reserve_wasm_capacity_for_test(5 << 20);
    let actual = CoreCache32::code_charge(&code, code.entrypoints().len());
    let owner = cache.commit_window(&memory, admission, code).unwrap();
    assert!(cache.policy.credit < after_reserve);
    assert!(
        cache.policy.credit < 0,
        "positive underestimated output becomes lifetime debt"
    );
    let debt = cache.policy.credit;
    let attempts = cache.policy.attempts;
    cache.remove(owner.key);
    assert_eq!(cache.policy.credit, debt);
    let pending = plan(&memory, &first, &second, 2);
    assert!(cache.preflight_window(&memory, &pending).is_none());
    assert_eq!(cache.policy.attempts, attempts);
    assert!(
        cache.current_hot_port(&memory, 0x1000).is_none(),
        "a deferred record never becomes a frontier"
    );
    cache.earn_completed_work(((actual as i64 - debt) * CREDIT_WORK as i64) as u32);
    let retry = cache.preflight_window(&memory, &pending).unwrap();
    let balance = cache.policy.credit;
    cache.reject_window(retry);
    assert_eq!(
        cache.policy.credit, balance,
        "failed reserved attempt never refunds output credit"
    );
    assert!(cache.policy.credit <= MAX_CREDIT);
    assert!(cache.resident_charge() <= MAX_CODE_BYTES);
}

#[test]
fn useful_probation_is_protected_but_an_inactive_phase_loses_protection() {
    let (memory, first, second) = source();
    let mut cache = CoreCache32::default();
    let pending = plan(&memory, &first, &second, 2);
    let admission = cache.preflight_window(&memory, &pending).unwrap();
    let code = emit_hot_window(&memory, pending).unwrap().unwrap();
    let owner = cache.commit_window(&memory, admission, code).unwrap();
    assert!(!cache.protected(owner.key.index));
    assert_eq!(
        cache.policy_victim(),
        Some(owner.key.index),
        "zero work cannot promote"
    );
    cache.route_owners[owner.key.index].completed_work = 256;
    // no other probation exists: make-space must still terminate by demoting a
    // protected owner; successful reuse never becomes a permanent pin.
    assert_eq!(cache.policy_victim(), Some(owner.key.index));
    assert!(!cache.protected(owner.key.index));
    cache.route_owners[owner.key.index].completed_work += 256;
    cache.move_tier(owner.key.index, true);
    assert_eq!(cache.protected_count, 1);
    let before = cache.snapshot();
    cache.review_protected();
    assert_eq!(cache.protected_count, 0);
    assert!(!cache.protected(owner.key.index));
    assert_eq!(
        cache.snapshot(),
        before,
        "tier review changes no code/source/slot/accounting counters"
    );
    assert_eq!(cache.protected_charge, 0);
    assert!(cache.resident_charge() <= MAX_CODE_BYTES);
}

fn growing_plan(
    memory: &AddressSpace,
    first: &CompiledCode,
    second: &CompiledCode,
    blocks: u32,
) -> WindowPlan {
    let mut cycle = Vec::new();
    for block in 1..blocks {
        cycle.push(TraceEvent {
            kind: BLOCK,
            pc: 0x1000 + block * 4,
            end: 0x1003 + block * 4,
            offset: 0,
            owner: second.identity(),
        });
        cycle.push(TraceEvent {
            kind: BLOCK,
            pc: 0x1000,
            end: 0x1003,
            offset: 0,
            owner: first.identity(),
        });
    }
    let events: Vec<_> = cycle
        .iter()
        .chain(cycle.iter())
        .chain(cycle.first())
        .copied()
        .collect();
    prepare_hot_window(
        memory,
        GuestAddress(0x1004),
        CompileOptions::default(),
        &events,
        &[first, second],
        memory.code_source_version(),
        &[],
    )
    .unwrap()
    .unwrap()
}

#[test]
fn monotone_growth_beyond_four_versions_migrates_all_old_contexts_and_revokes_old_slots() {
    let (memory, first, second) = source();
    let mut cache = CoreCache32::default();
    let mut previous: Option<RegionRef> = None;
    let mut contexts = Vec::new();
    for blocks in 2..=8 {
        let pending = growing_plan(&memory, &first, &second, blocks);
        if let Some(old) = &previous {
            assert!(
                pending
                    .fingerprint()
                    .strictly_covers(old.window_fingerprint().unwrap())
            );
        }
        let admission = cache
            .preflight_window(&memory, &pending)
            .expect("redundant covered versions do not consume overlap slots");
        assert_eq!(admission.replace.len(), usize::from(previous.is_some()));
        let code = emit_hot_window(&memory, pending).unwrap().unwrap();
        let old_generation = cache.slot_generations[7];
        let current = cache.commit_window(&memory, admission, code).unwrap();
        if let Some(old) = previous.take() {
            assert!(cache.region(old.key).is_none());
            assert_eq!(cache.link_owners[7], 0);
            assert!(cache.slot_generations[7] > old_generation);
            assert!(!cache.precise_source_current(&memory, &old));
        }
        cache.install_link(&current, 7).unwrap();
        let key = PreferredKey {
            origin: Some(blocks * 0x100),
            target: 0x1000,
        };
        assert!(cache.bind_window_preferences(&memory, &current, &[key]));
        contexts.push(key);
        for key in &contexts {
            assert_eq!(
                cache
                    .lookup_executable_context(&memory, key.origin, key.target)
                    .unwrap()
                    .identity(),
                current.identity()
            );
            let row = cache.route_rows[cache.route_position_key(*key).unwrap()];
            assert_eq!(row.candidates[0].identity, current.identity());
        }
        assert_eq!(cache.contexts.windows.len(), 1);
        assert_eq!(cache.snapshot()[5], 1);
        assert!(cache.resident_charge() <= MAX_CODE_BYTES);
        previous = Some(current);
    }
}

#[test]
fn four_incomparable_bodies_bound_overlap_but_a_retained_shorter_candidate_is_still_selectable() {
    let (memory, first, second) = source();
    let mut cache = CoreCache32::default();
    let mut retained = Vec::new();
    for blocks in 2..=5 {
        let pending = plan(&memory, &first, &second, blocks);
        let admission = cache.preflight_window(&memory, &pending).unwrap();
        assert!(admission.replace.is_empty());
        let code = emit_hot_window(&memory, pending).unwrap().unwrap();
        let owner = cache.commit_window(&memory, admission, code).unwrap();
        cache.install_link(&owner, blocks + 5).unwrap();
        let key = PreferredKey {
            origin: Some(blocks * 0x100),
            target: 0x1000,
        };
        assert!(cache.bind_window_preferences(&memory, &owner, &[key]));
        retained.push((owner, key));
    }
    let blocked = plan(&memory, &first, &second, 6);
    let attempts = cache.policy.attempts;
    let snapshot = cache.snapshot();
    assert!(!cache.can_select_window_candidate(&memory, &blocked));
    assert!(cache.preflight_window(&memory, &blocked).is_none());
    assert_eq!(cache.policy.attempts, attempts);
    assert_eq!(cache.snapshot(), snapshot);
    cache.policy.credit = -1;
    let reusable = plan(&memory, &first, &second, 2);
    assert!(
        cache.can_select_window_candidate(&memory, &reusable),
        "existing body reuse needs no output credit"
    );
    let next_context = PreferredKey {
        origin: Some(0),
        target: 0x1000,
    };
    let reused = cache
        .reuse_window(&memory, &reusable, &[next_context])
        .unwrap();
    assert_eq!(reused.identity(), retained[0].0.identity());
    assert_eq!(cache.policy.credit, -1);
    assert_eq!(cache.policy.attempts, attempts);
    for (owner, key) in &retained {
        assert_eq!(
            cache
                .lookup_executable_context(&memory, key.origin, key.target)
                .unwrap()
                .identity(),
            owner.identity()
        );
        let slot = cache.regions[owner.key.index].as_ref().unwrap().slot;
        assert_eq!(cache.link_owners[slot as usize], owner.identity());
        assert!(cache.precise_source_current(&memory, owner));
    }
    let generation = cache.slot_generations[7];
    cache.remove(retained[0].0.key);
    assert_eq!(cache.link_owners[7], 0);
    assert!(cache.slot_generations[7] > generation);
    assert!(cache.contexts.get(next_context).is_none());
    assert_eq!(
        cache.install_link(&retained[0].0, 7),
        Err(LinkError::Unretained)
    );
    assert!(cache.resident_charge() <= MAX_CODE_BYTES);
}

#[test]
fn explicit_capacity_can_retire_linked_protected_owner_when_probation_is_unlinked() {
    let (memory, first, second) = source();
    let mut cache = CoreCache32::default();
    let pending = plan(&memory, &first, &second, 2);
    let token = cache.preflight_window(&memory, &pending).unwrap();
    let linked = cache
        .commit_window(
            &memory,
            token,
            emit_hot_window(&memory, pending).unwrap().unwrap(),
        )
        .unwrap();
    cache.install_link(&linked, 7).unwrap();
    cache.route_owners[linked.key.index].completed_work = 256;
    cache.move_tier(linked.key.index, true);
    let pending = plan(&memory, &first, &second, 3);
    let token = cache.preflight_window(&memory, &pending).unwrap();
    let unlinked = cache
        .commit_window(
            &memory,
            token,
            emit_hot_window(&memory, pending).unwrap().unwrap(),
        )
        .unwrap();
    assert_eq!(cache.regions[unlinked.key.index].as_ref().unwrap().slot, 0);
    assert_eq!(cache.oldest_linked_slot(), 7);
    cache.remove(linked.key);
    assert_eq!(cache.oldest_linked_slot(), 0);
    assert!(cache.region(unlinked.key).is_some());
}

#[test]
fn outgoing_owner_accounting_rejects_wrong_ticket_and_never_credits_cold_work() {
    let (memory, first, second) = source();
    let mut cache = CoreCache32::default();
    let pending = plan(&memory, &first, &second, 2);
    let token = cache.preflight_window(&memory, &pending).unwrap();
    let owner = cache
        .commit_window(
            &memory,
            token,
            emit_hot_window(&memory, pending).unwrap().unwrap(),
        )
        .unwrap();
    cache.install_link(&owner, 7).unwrap();
    assert!(cache.begin_continuation(&memory, 0x80, 0x100, Some(&owner), 0x1000));
    let before = cache.snapshot();
    let credit = cache.policy.credit;
    cache.account_owner_retired(10);
    assert_eq!(cache.route_owners[owner.key.index].completed_work, 10);
    cache.route_header.current_id ^= 1;
    cache.account_owner_retired(20);
    cache.route_header.current_id = owner.identity();
    cache.account_owner_retired(21);
    assert_eq!(cache.route_owners[owner.key.index].completed_work, 11);
    let generation = cache.route_header.current_generation;
    cache.route_header.current_generation ^= 1;
    cache.account_owner_retired(30);
    cache.route_header.current_generation = generation;
    cache.account_owner_retired(31);
    assert_eq!(cache.route_owners[owner.key.index].completed_work, 12);
    let nonce = cache.route_header.current_nonce;
    cache.route_header.current_nonce ^= 1;
    cache.account_owner_retired(40);
    cache.route_header.current_nonce = nonce;
    cache.account_owner_retired(41);
    assert_eq!(cache.route_owners[owner.key.index].completed_work, 13);
    cache.clear_continuation();
    cache.account_owner_retired(50);
    assert!(cache.bind_continuation(&memory, &owner, 0x1000));
    cache.account_owner_retired(52);
    assert_eq!(cache.route_owners[owner.key.index].completed_work, 15);
    cache.route_owners[owner.key.index].completed_work = u64::MAX - 1;
    cache.account_owner_retired(54);
    assert_eq!(cache.route_owners[owner.key.index].completed_work, u64::MAX);
    assert_eq!(
        cache.policy.credit, credit,
        "owner reuse is not a second source of global credit"
    );
    assert_eq!(cache.snapshot(), before);
    cache.end_continuation();
}

#[test]
fn pressure_compaction_reserves_all_incoming_hot_aliases_after_suppliers_are_evicted() {
    let mut memory = AddressSpace::new(2).unwrap();
    for pc in [0x1000, 0x2000] {
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(pc), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
    }
    let mut bytes = Vec::new();
    let mut path = Vec::new();
    for index in 0..80 {
        bytes.extend_from_slice(&[0x0f, 0xa2]);
        path.push((0x1000 + index * 2, 0x1002 + index * 2));
    }
    bytes.extend_from_slice(&[0xff, 0xe6]);
    path.push((0x10a0, 0x2000));
    memory.write(GuestAddress(0x1000), &bytes).unwrap();
    memory.write(GuestAddress(0x2000), &[0xff, 0xe6]).unwrap();
    let mut first = super::super::window_tests::family(&memory, &path, &[]);
    let mut second = super::super::window_tests::family(&memory, &[(0x2000, 0x1000)], &[]);
    first.reserve_wasm_capacity_for_test(30 << 20);
    second.reserve_wasm_capacity_for_test(30 << 20);
    let mut cache = CoreCache32::default();
    let first = cache.admit(&memory, first).unwrap();
    let second = cache.admit(&memory, second).unwrap();
    cache.install_link(&first, 7).unwrap();
    cache.install_link(&second, 8).unwrap();
    let cycle = [
        TraceEvent {
            kind: BLOCK,
            pc: 0x1000,
            end: 0x10a2,
            offset: 0,
            owner: first.identity(),
        },
        TraceEvent {
            kind: BLOCK,
            pc: 0x2000,
            end: 0x2002,
            offset: 0,
            owner: second.identity(),
        },
    ];
    let events: Vec<_> = cycle
        .iter()
        .chain(cycle.iter())
        .chain(cycle.first())
        .copied()
        .collect();
    let pending = prepare_hot_window(
        &memory,
        GuestAddress(0x1000),
        CompileOptions::default(),
        &events,
        &[&first, &second],
        memory.code_source_version(),
        &[],
    )
    .unwrap()
    .unwrap();
    assert!(pending.hot_ports().len() >= 80);
    assert!(
        pending
            .hot_ports()
            .iter()
            .all(|port| cache.route_position(port.entry).is_ok())
    );
    let initial_capacity = cache.route_rows.len();
    let token = cache.preflight_window(&memory, &pending).unwrap();
    let mut code = emit_hot_window(&memory, pending).unwrap().unwrap();
    code.reserve_wasm_capacity_for_test(95 << 20);
    let incoming: Vec<_> = code.hot_spans().collect();
    let before = cache.snapshot();
    let owner = cache.commit_window(&memory, token, code).unwrap();
    assert!(
        cache.snapshot()[3] >= before[3] + 2,
        "both real suppliers must leave under byte pressure"
    );
    assert!(cache.region(first.key).is_none());
    assert!(cache.region(second.key).is_none());
    assert_eq!(&cache.link_owners[7..9], &[0, 0]);
    assert_eq!(cache.snapshot()[5], 1);
    assert!(cache.route_rows.len() >= CoreCache32::route_capacity(incoming.len()));
    assert!(
        cache.route_rows.len()
            <= initial_capacity.max(CoreCache32::route_capacity(incoming.len() + 1))
    );
    for port in incoming {
        let row = cache.route_rows[cache.route_position(port.entry).unwrap()];
        assert_eq!(row.state, route::PRESENT);
        assert_eq!(row.candidates[0].identity, owner.identity());
        assert_eq!(row.candidates[0].port, port.port);
        assert_eq!(row.candidates[0].work, port.instructions);
    }
    assert!(cache.resident_charge() <= MAX_CODE_BYTES);
}
