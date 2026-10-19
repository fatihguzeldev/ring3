use super::*;
use crate::{
    abi::{
        arena::SEGMENTS_OFFSET,
        x86::{SEGMENTS_SIZE, encode_segment_context},
    },
    cpu::{
        dbt::{CompileLimits, X87Group32},
        x86::SegmentCache,
    },
    process::{ExecutionConfig32, calls::call::PendingOwner},
};

const KEY: u64 = 97;
const FLAGS: u32 = 0x46;

fn fixture(code: &[u8]) -> (EngineInstance, u64, X87Group32) {
    let mut engine = EngineInstance::new(3, KEY).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.map(0x5000, 1, 3).unwrap();
    let memory = engine.memory.as_mut().unwrap();
    memory.write(GuestAddress(0x1000), code).unwrap();
    memory
        .write(GuestAddress(0x5000), &1.25_f32.to_le_bytes())
        .unwrap();
    engine
        .configure_execution_with_limits(
            ExecutionConfig32 {
                units: 4,
                wasm_bytes: 524288,
                promotion_hits: 256,
            },
            CompileLimits {
                instructions: 128,
                wasm_bytes: 131072,
                ..CompileLimits::default()
            },
        )
        .unwrap();
    let request = engine.prepare_cache_entry(KEY, 0x1000).unwrap();
    let id = request.unit_id;
    engine
        .install_cached_entry(KEY, request.serial, id, request.slot)
        .unwrap();
    let group = engine
        .guard_material_resident_region(KEY, id)
        .unwrap()
        .x87_group(0)
        .unwrap();
    (engine, id, group)
}

fn code(operation: [u8; 2], count: usize) -> Vec<u8> {
    let mut code = operation.repeat(count);
    code.extend([0xff, 0xe2]);
    code
}

fn scalar(engine: &mut EngineInstance, id: u64, group: X87Group32, index: usize, address: u32) {
    let step = group.steps()[index];
    assert_eq!(
        engine.x87_resident_op(
            KEY,
            id,
            step.operation.encode(),
            address,
            FLAGS,
            step.pc,
            u32::from(step.opcode),
            step.control,
        ),
        Ok(StoreCompletion::Complete)
    );
}

fn same(grouped: &EngineInstance, ordinary: &EngineInstance) {
    assert_eq!(
        grouped.arena(),
        ordinary.arena(),
        "all CPU/helper/extended records match scalar execution"
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
    assert_eq!(left, right, "complete guest operand/output page matches");
    assert_eq!(
        grouped.memory().unwrap().transaction_version(),
        ordinary.memory().unwrap().transaction_version()
    );
}

#[test]
fn four_through_eight_reads_match_every_scalar_prefix() {
    for count in 4..=8 {
        for maximum in 1..=count {
            let source = code([0xd9, 0x03], count);
            let (mut grouped, id, _) = fixture(&source);
            let (mut ordinary, ordinary_id, group) = fixture(&source);
            assert_eq!(group.len(), count);
            for index in 0..maximum {
                scalar(&mut ordinary, ordinary_id, group, index, 0x5000);
            }
            assert_eq!(
                grouped.x87_resident_group(KEY, id, 0, [0x5000; 8], FLAGS, maximum as u32),
                X87GroupOutcome::complete(maximum as u32, maximum as u32 - 1)
            );
            same(&grouped, &ordinary);
            let state = decode_x87(&grouped.arena()[X87_OFFSET..X87_OFFSET + X87_SIZE]).unwrap();
            assert_eq!((state.status >> 11) & 7, (8 - maximum as u16) & 7);
        }
    }
}

#[test]
fn every_operand_page_fault_retains_exact_prefix_and_active_witness() {
    for failed in 0..8 {
        let source = code([0xd9, 0x03], 8);
        let (mut grouped, id, _) = fixture(&source);
        let (mut ordinary, ordinary_id, group) = fixture(&source);
        let epoch = grouped
            .begin_exception_detection(PendingOwner::Resident(id))
            .unwrap();
        let ordinary_epoch = ordinary
            .begin_exception_detection(PendingOwner::Resident(ordinary_id))
            .unwrap();
        let mut addresses = [0x5000; 8];
        addresses[failed] = 0x7000;
        for (index, address) in addresses.iter().copied().enumerate().take(failed + 1) {
            scalar(&mut ordinary, ordinary_id, group, index, address);
        }
        let sequence = grouped.cpu_page_fault_sequence32();
        assert_eq!(
            grouped.x87_resident_group(KEY, id, 0, addresses, FLAGS, 8),
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
fn original_later_boundary_receives_prior_packet_for_cs_cancel_and_new_mf() {
    let source = code([0xd9, 0x03], 8);
    for completed in 1..8 {
        let (mut grouped, id, _) = fixture(&source);
        let (mut ordinary, ordinary_id, group) = fixture(&source);
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
        for index in 0..completed {
            scalar(&mut ordinary, ordinary_id, group, index, 0x5000);
        }
        assert_eq!(
            grouped.x87_resident_group(KEY, id, 0, [0x5000; 8], FLAGS, 8),
            X87GroupOutcome::complete(completed as u32, completed as u32 - 1)
        );
        same(&grouped, &ordinary);
    }
    for new_mf in [false, true] {
        let source = if new_mf {
            vec![0xd9, 0xe0, 0xd9, 0xe8, 0xd9, 0xe0, 0xd9, 0xe0, 0xff, 0xe2]
        } else {
            code([0xd9, 0x03], 4)
        };
        let (mut grouped, id, _) = fixture(&source);
        let (mut ordinary, ordinary_id, group) = fixture(&source);
        for engine in [&mut grouped, &mut ordinary] {
            if new_mf {
                let state = X87State {
                    control: 0x37e,
                    ..X87State::default()
                };
                encode_x87(
                    &state,
                    &mut engine.arena_mut().unwrap()[X87_OFFSET..X87_OFFSET + X87_SIZE],
                )
                .unwrap();
            } else {
                header::write_u32(engine.arena_mut().unwrap(), CANCEL_OFFSET, 1);
            }
        }
        scalar(&mut ordinary, ordinary_id, group, 0, 0x5000);
        assert_eq!(
            grouped.x87_resident_group(KEY, id, 0, [0x5000; 8], FLAGS, 4),
            X87GroupOutcome::complete(1, 0)
        );
        same(&grouped, &ordinary);
    }
}

#[test]
fn pending_mf_and_invalid_guards_preserve_original_priority_and_neutrality() {
    let source = code([0xd9, 0x03], 4);
    let (mut grouped, id, _) = fixture(&source);
    let (mut ordinary, ordinary_id, group) = fixture(&source);
    let state = X87State {
        control: 0x37e,
        status: 1,
        ..X87State::default()
    };
    for engine in [&mut grouped, &mut ordinary] {
        encode_x87(
            &state,
            &mut engine.arena_mut().unwrap()[X87_OFFSET..X87_OFFSET + X87_SIZE],
        )
        .unwrap();
    }
    scalar(&mut ordinary, ordinary_id, group, 0, 0x7000);
    assert_eq!(
        grouped.x87_resident_group(KEY, id, 0, [0x7000; 8], FLAGS, 4),
        X87GroupOutcome::complete(0, 0)
    );
    same(&grouped, &ordinary);
    assert_eq!(grouped.cr2(), None);
    for (key, unit, ordinal, maximum) in [
        (KEY + 1, id, 0, 4),
        (KEY, id + 1, 0, 4),
        (KEY, id, 1, 4),
        (KEY, id, 0, 0),
        (KEY, id, 0, 5),
    ] {
        let before = grouped.arena().to_vec();
        let result = grouped.x87_resident_group(key, unit, ordinal, [0x5000; 8], FLAGS, maximum);
        assert!(result.result.is_err());
        assert_eq!((result.completed, result.reported_step), (0, 0));
        assert_eq!(grouped.arena(), before);
    }
}

#[test]
fn final_store_success_or_write_fault_keeps_all_prior_group_progress() {
    let source = [0xd9, 0xe8, 0xd9, 0xe0, 0xd9, 0xe0, 0xd9, 0x1b, 0xff, 0xe2];
    for readonly in [false, true] {
        let (mut grouped, id, _) = fixture(&source);
        let (mut ordinary, ordinary_id, group) = fixture(&source);
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
        let addresses = [0, 0, 0, 0x5008, 0, 0, 0, 0];
        for (index, address) in addresses.iter().copied().enumerate().take(4) {
            scalar(&mut ordinary, ordinary_id, group, index, address);
        }
        assert_eq!(
            grouped.x87_resident_group(KEY, id, 0, addresses, FLAGS, 4),
            X87GroupOutcome::complete(if readonly { 3 } else { 4 }, 3)
        );
        same(&grouped, &ordinary);
        if readonly {
            assert_eq!(
                grouped.certified_page_fault32(epoch),
                ordinary.certified_page_fault32(ordinary_epoch)
            );
            assert!(grouped.certified_page_fault32(epoch).is_some());
        } else {
            let mut stored = [0; 4];
            grouped
                .memory()
                .unwrap()
                .read(GuestAddress(0x5008), &mut stored)
                .unwrap();
            assert_eq!(stored, 1_f32.to_le_bytes());
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
fn suppressed_final_store_retires_four_without_writing_or_popping() {
    use crate::cpu::x86::Ext80;
    let source = [0xd9, 0xe0, 0xd9, 0xe0, 0xd9, 0xe0, 0xd9, 0x1b, 0xff, 0xe2];
    let (mut grouped, id, _) = fixture(&source);
    let (mut ordinary, ordinary_id, group) = fixture(&source);
    let mut initial = X87State {
        tag: 0xfffc,
        ..X87State::default()
    };
    initial.control &= !8;
    initial.registers[0] = Ext80::from_parts(false, 16383 + 128, 1 << 63).0;
    for engine in [&mut grouped, &mut ordinary] {
        encode_x87(
            &initial,
            &mut engine.arena_mut().unwrap()[X87_OFFSET..X87_OFFSET + X87_SIZE],
        )
        .unwrap();
    }
    let version = grouped.memory().unwrap().transaction_version();
    for (index, address) in [0, 0, 0, 0x5008].into_iter().enumerate() {
        scalar(&mut ordinary, ordinary_id, group, index, address);
    }
    assert_eq!(
        grouped.x87_resident_group(KEY, id, 0, [0, 0, 0, 0x5008, 0, 0, 0, 0], FLAGS, 4),
        X87GroupOutcome::complete(4, 3)
    );
    same(&grouped, &ordinary);
    let mut expected = initial;
    expected.registers[0][9] |= 0x80;
    expected.status = 0x8088;
    expected.instruction_pointer = 0x1006;
    expected.opcode = 0x11b;
    expected.code_selector = 0x1b;
    expected.data_pointer = 0x5008;
    expected.data_selector = 0x23;
    assert_eq!(
        decode_x87(&grouped.arena()[X87_OFFSET..X87_OFFSET + X87_SIZE]).unwrap(),
        expected
    );
    assert_eq!(grouped.memory().unwrap().transaction_version(), version);
    let mut bytes = [1; 4];
    grouped
        .memory()
        .unwrap()
        .read(GuestAddress(0x5008), &mut bytes)
        .unwrap();
    assert_eq!(bytes, [0; 4]);
}

#[test]
fn exhausted_final_store_commit_preserves_three_retired_operations() {
    use crate::cpu::x86::PagingAccess32;
    let source = [0xd9, 0xe0, 0xd9, 0xe0, 0xd9, 0xe0, 0xd9, 0x1b, 0xff, 0xe2];
    let (mut grouped, id, _) = fixture(&source);
    let (mut ordinary, ordinary_id, group) = fixture(&source);
    for engine in [&mut grouped, &mut ordinary] {
        let mut state = X87State {
            tag: 0xfffc,
            ..X87State::default()
        };
        state.registers[0] = [0, 0, 0, 0, 0, 0, 0, 0x80, 0xff, 0x3f];
        encode_x87(
            &state,
            &mut engine.arena_mut().unwrap()[X87_OFFSET..X87_OFFSET + X87_SIZE],
        )
        .unwrap();
        engine.memory.as_mut().unwrap().exhaust_versions_for_test();
        assert_eq!(
            engine.inspect_cpu_paging32(GuestAddress(0x5008), 4, PagingAccess32::Write),
            Ok(())
        );
    }
    for (index, address) in [0, 0, 0, 0x5008].into_iter().enumerate() {
        scalar(&mut ordinary, ordinary_id, group, index, address);
    }
    assert_eq!(
        grouped.x87_resident_group(KEY, id, 0, [0, 0, 0, 0x5008, 0, 0, 0, 0], FLAGS, 4),
        X87GroupOutcome::complete(3, 3)
    );
    same(&grouped, &ordinary);
    let state = decode_x87(&grouped.arena()[X87_OFFSET..X87_OFFSET + X87_SIZE]).unwrap();
    assert_eq!(state.tag, 0xfffc);
    assert_eq!(state.registers[0], [0, 0, 0, 0, 0, 0, 0, 0x80, 0xff, 0xbf]);
    assert_eq!(state.instruction_pointer, 0x1004);
    assert_eq!(grouped.memory().unwrap().transaction_version(), u64::MAX);
    assert_eq!(header::read_u32(grouped.arena(), HELPER_OFFSET + 16), 2);
    assert_eq!(header::read_u32(grouped.arena(), HELPER_OFFSET + 24), 1);
}
