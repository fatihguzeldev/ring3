use super::*;
use crate::{
    abi::{
        arena::SEGMENTS_OFFSET,
        x86::{SEGMENTS_SIZE, encode_segment_context},
    },
    cpu::{
        dbt::CompileLimits,
        x86::{SegmentCache, X87State},
    },
    process::{ExecutionConfig32, calls::call::PendingOwner},
};

const KEY: u64 = 97;
const FLAGS: u32 = 0x46;

fn source(first: [u8; 2], rest: [u8; 2], count: usize) -> Vec<u8> {
    let mut bytes = first.to_vec();
    bytes.extend(rest.repeat(count - 1));
    bytes.extend([0xff, 0xe2]);
    bytes
}

fn fixture(
    code: &[u8],
) -> (
    EngineInstance,
    u64,
    u32,
    Vec<crate::cpu::dbt::X87PairStep32>,
) {
    let mut engine = EngineInstance::new(3, KEY).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.map(0x5000, 1, 3).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x1000), code)
        .unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x5000), &1.25_f32.to_le_bytes())
        .unwrap();
    engine
        .configure_execution_with_limits(
            ExecutionConfig32 {
                units: 4,
                wasm_bytes: 8 * 1024 * 1024,
                promotion_hits: 256,
            },
            CompileLimits {
                instructions: 256,
                wasm_bytes: 524288,
                ..CompileLimits::default()
            },
        )
        .unwrap();
    let request = engine.prepare_cache_entry(KEY, 0x1000).unwrap();
    let id = request.unit_id;
    engine
        .install_cached_entry(KEY, request.serial, id, request.slot)
        .unwrap();
    let region = engine.guard_material_resident_region(KEY, id).unwrap();
    let ordinal = (0..32)
        .find(|&ordinal| region.x87_group32(ordinal).is_some())
        .unwrap();
    let group = region.x87_group32(ordinal).unwrap();
    let steps = (0..group.len())
        .map(|index| group.descriptor(index).unwrap())
        .collect();
    let mut state = X87State {
        tag: 0xfffc,
        ..X87State::default()
    };
    state.registers[0] = [0, 0, 0, 0, 0, 0, 0, 0x80, 0xff, 0x3f];
    seed(&mut engine, state);
    (engine, id, ordinal, steps)
}

fn seed(engine: &mut EngineInstance, state: X87State) {
    encode_x87(
        &state,
        &mut engine.arena_mut().unwrap()[X87_OFFSET..X87_OFFSET + X87_SIZE],
    )
    .unwrap();
}

fn scalar(
    engine: &mut EngineInstance,
    id: u64,
    step: crate::cpu::dbt::X87PairStep32,
    address: u32,
) {
    assert_eq!(
        engine.x87_resident_op(
            KEY,
            id,
            step.operation.encode(),
            address,
            FLAGS,
            step.pc,
            u32::from(step.opcode),
            step.control
        ),
        Ok(StoreCompletion::Complete)
    );
}

fn same(grouped: &EngineInstance, ordinary: &EngineInstance) {
    assert_eq!(
        grouped.arena(),
        ordinary.arena(),
        "full records retain the scalar prefix"
    );
    assert_eq!(grouped.cr2(), ordinary.cr2());
    let mut left = [0; 4096];
    let mut right = [0; 4096];
    grouped
        .memory()
        .unwrap()
        .read(GuestAddress(0x5000), &mut left)
        .unwrap();
    ordinary
        .memory()
        .unwrap()
        .read(GuestAddress(0x5000), &mut right)
        .unwrap();
    assert_eq!(left, right);
    assert_eq!(
        grouped.memory().unwrap().transaction_version(),
        ordinary.memory().unwrap().transaction_version()
    );
}

#[test]
fn wide_lengths_and_partial_maxima_match_scalar_records() {
    for count in [9, 16, 31, 32] {
        for maximum in [1, count / 2, count] {
            let code = source([0xd9, 0xe0], [0xd9, 0xe0], count);
            let (mut grouped, id, ordinal, steps) = fixture(&code);
            let (mut ordinary, ordinary_id, _, _) = fixture(&code);
            assert_eq!(steps.len(), count);
            assert!(ordinal > 0, "legacy spans remain the catalog prefix");
            for step in steps.iter().take(maximum) {
                scalar(&mut ordinary, ordinary_id, *step, 0);
            }
            assert_eq!(
                grouped.x87_resident_group32(KEY, id, ordinal, [0; 32], FLAGS, maximum as u32),
                X87GroupOutcome::complete(maximum as u32, maximum as u32 - 1)
            );
            same(&grouped, &ordinary);
            let state = decode_x87(&grouped.arena()[X87_OFFSET..X87_OFFSET + X87_SIZE]).unwrap();
            assert_eq!(
                state.registers[0][9] & 0x80,
                if maximum % 2 == 0 { 0 } else { 0x80 }
            );
        }
    }
}

#[test]
fn later_page_faults_preserve_wide_prefix_and_certified_witness() {
    let code = source([0xd9, 0x03], [0xd8, 0x03], 32);
    for failed in [0, 8, 31] {
        let (mut grouped, id, ordinal, steps) = fixture(&code);
        let (mut ordinary, ordinary_id, _, _) = fixture(&code);
        let epoch = grouped
            .begin_exception_detection(PendingOwner::Resident(id))
            .unwrap();
        let ordinary_epoch = ordinary
            .begin_exception_detection(PendingOwner::Resident(ordinary_id))
            .unwrap();
        let mut addresses = [0x5000; 32];
        addresses[failed] = 0x7000;
        for index in 0..=failed {
            scalar(&mut ordinary, ordinary_id, steps[index], addresses[index]);
        }
        let sequence = grouped.cpu_page_fault_sequence32();
        assert_eq!(
            grouped.x87_resident_group32(KEY, id, ordinal, addresses, FLAGS, 32),
            X87GroupOutcome::complete(failed as u32, failed as u32)
        );
        assert_eq!(grouped.cpu_page_fault_sequence32(), sequence + 1);
        same(&grouped, &ordinary);
        assert_eq!(
            grouped.certified_page_fault32(epoch),
            ordinary.certified_page_fault32(ordinary_epoch)
        );
        assert!(grouped.certified_page_fault32(epoch).is_some());
        assert!(!grouped.has_ready_exception_detection());
    }
}

#[test]
fn later_cs_cancel_and_new_mf_leave_the_original_boundary_in_control() {
    let code = source([0xd9, 0xe0], [0xd9, 0xe0], 32);
    for completed in [1, 8, 31] {
        let (mut grouped, id, ordinal, steps) = fixture(&code);
        let (mut ordinary, ordinary_id, _, _) = fixture(&code);
        for engine in [&mut grouped, &mut ordinary] {
            let mut context = engine.segment_context().unwrap();
            let current = context.caches[SegmentRegister::Cs.index()].bits();
            context.caches[SegmentRegister::Cs.index()] = SegmentCache::from_bits(
                (current & !0x010f_ffff) | (0x1000 + completed as u32 * 2 - 1),
            )
            .unwrap();
            encode_segment_context(
                &context,
                &mut engine.arena_mut().unwrap()[SEGMENTS_OFFSET..SEGMENTS_OFFSET + SEGMENTS_SIZE],
            )
            .unwrap();
        }
        for step in steps.iter().take(completed) {
            scalar(&mut ordinary, ordinary_id, *step, 0);
        }
        assert_eq!(
            grouped.x87_resident_group32(KEY, id, ordinal, [0; 32], FLAGS, 32),
            X87GroupOutcome::complete(completed as u32, completed as u32 - 1)
        );
        same(&grouped, &ordinary);
    }
    for new_mf in [false, true] {
        let code = source([0xd9, 0xe0], [0xd9, 0xe0], 9);
        let (mut grouped, id, ordinal, steps) = fixture(&code);
        let (mut ordinary, ordinary_id, _, _) = fixture(&code);
        for engine in [&mut grouped, &mut ordinary] {
            if new_mf {
                seed(
                    engine,
                    X87State {
                        control: 0x37e,
                        ..X87State::default()
                    },
                );
            } else {
                header::write_u32(engine.arena_mut().unwrap(), CANCEL_OFFSET, 1);
            }
        }
        scalar(&mut ordinary, ordinary_id, steps[0], 0);
        assert_eq!(
            grouped.x87_resident_group32(KEY, id, ordinal, [0; 32], FLAGS, 9),
            X87GroupOutcome::complete(1, 0)
        );
        same(&grouped, &ordinary);
    }
}

#[test]
fn wrong_kind_ordinal_and_invalid_requests_are_neutral_before_fp_decode() {
    let code = source([0xd9, 0x03], [0xd8, 0x03], 9);
    let (mut engine, id, ordinal, _) = fixture(&code);
    for (key, unit, selected, maximum) in [
        (KEY + 1, id, ordinal, 9),
        (KEY, id + 1, ordinal, 9),
        (KEY, id, 0, 9),
        (KEY, id, u32::MAX, 9),
        (KEY, id, ordinal, 0),
        (KEY, id, ordinal, 10),
    ] {
        let before = engine.arena().to_vec();
        let outcome =
            engine.x87_resident_group32(key, unit, selected, [0x7000; 32], FLAGS, maximum);
        assert!(outcome.result.is_err());
        assert_eq!((outcome.completed, outcome.reported_step), (0, 0));
        assert_eq!(engine.arena(), before);
        assert_eq!(engine.cr2(), None);
    }
    let before = engine.arena().to_vec();
    engine.arena_mut().unwrap()[X87_OFFSET] = 0;
    let malformed = engine.arena().to_vec();
    assert_eq!(
        engine.x87_resident_group32(KEY, id, ordinal, [0x7000; 32], FLAGS, 0),
        X87GroupOutcome::failure(0, 0, HostError::InvalidRequest)
    );
    assert_eq!(engine.arena(), malformed);
    engine.arena_mut().unwrap().copy_from_slice(&before);
    seed(
        &mut engine,
        X87State {
            control: 0x37e,
            status: 1,
            ..X87State::default()
        },
    );
    assert_eq!(
        engine.x87_resident_group32(KEY, id, ordinal, [0x7000; 32], FLAGS, 9),
        X87GroupOutcome::complete(0, 0)
    );
    assert_eq!(
        engine.cr2(),
        None,
        "pending mf precedes the first operand page fault"
    );
}

#[test]
fn terminal_store_success_or_fault_uses_the_wide_reported_step() {
    let mut code = source([0xd9, 0xe0], [0xd9, 0xe0], 32);
    code[62..64].copy_from_slice(&[0xd9, 0x1b]);
    for readonly in [false, true] {
        let (mut grouped, id, ordinal, steps) = fixture(&code);
        let (mut ordinary, ordinary_id, _, _) = fixture(&code);
        if readonly {
            grouped.protect(0x5000, 1, 1).unwrap();
            ordinary.protect(0x5000, 1, 1).unwrap();
        }
        let epoch = grouped
            .begin_exception_detection(PendingOwner::Resident(id))
            .unwrap();
        let ordinary_epoch = ordinary
            .begin_exception_detection(PendingOwner::Resident(ordinary_id))
            .unwrap();
        let mut addresses = [0; 32];
        addresses[31] = 0x5008;
        for index in 0..32 {
            scalar(&mut ordinary, ordinary_id, steps[index], addresses[index]);
        }
        assert_eq!(
            grouped.x87_resident_group32(KEY, id, ordinal, addresses, FLAGS, 32),
            X87GroupOutcome::complete(if readonly { 31 } else { 32 }, 31)
        );
        same(&grouped, &ordinary);
        if readonly {
            assert_eq!(
                grouped.certified_page_fault32(epoch),
                ordinary.certified_page_fault32(ordinary_epoch)
            );
            assert!(grouped.certified_page_fault32(epoch).is_some());
        } else {
            let mut bytes = [0; 4];
            grouped
                .memory()
                .unwrap()
                .read(GuestAddress(0x5008), &mut bytes)
                .unwrap();
            assert_eq!(bytes, (-1_f32).to_le_bytes());
            assert_eq!(
                decode_x87(&grouped.arena()[X87_OFFSET..X87_OFFSET + X87_SIZE])
                    .unwrap()
                    .tag,
                0xffff
            );
        }
    }
}

#[test]
fn suppressed_and_exhausted_terminal_stores_keep_distinct_retirement() {
    use crate::cpu::x86::Ext80;
    let mut code = source([0xd9, 0xe0], [0xd9, 0xe0], 32);
    code[62..64].copy_from_slice(&[0xd9, 0x1b]);
    for exhausted in [false, true] {
        let (mut grouped, id, ordinal, steps) = fixture(&code);
        let (mut ordinary, ordinary_id, _, _) = fixture(&code);
        for engine in [&mut grouped, &mut ordinary] {
            if exhausted {
                engine.memory.as_mut().unwrap().exhaust_versions_for_test();
            } else {
                let mut state = X87State {
                    tag: 0xfffc,
                    ..X87State::default()
                };
                state.control &= !8;
                state.registers[0] = Ext80::from_parts(false, 16383 + 128, 1 << 63).0;
                seed(engine, state);
            }
        }
        let version = grouped.memory().unwrap().transaction_version();
        let mut addresses = [0; 32];
        addresses[31] = 0x5008;
        for index in 0..32 {
            scalar(&mut ordinary, ordinary_id, steps[index], addresses[index]);
        }
        assert_eq!(
            grouped.x87_resident_group32(KEY, id, ordinal, addresses, FLAGS, 32),
            X87GroupOutcome::complete(if exhausted { 31 } else { 32 }, 31)
        );
        same(&grouped, &ordinary);
        let state = decode_x87(&grouped.arena()[X87_OFFSET..X87_OFFSET + X87_SIZE]).unwrap();
        assert_eq!(state.tag, 0xfffc, "failed or suppressed store does not pop");
        assert_eq!(grouped.memory().unwrap().transaction_version(), version);
        assert_eq!(
            header::read_u32(grouped.arena(), HELPER_OFFSET + 16),
            if exhausted { 2 } else { 0 }
        );
        if !exhausted {
            assert_eq!(state.status, 0x8088);
        }
    }
}
