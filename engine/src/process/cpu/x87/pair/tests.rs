use super::*;
use crate::{
    abi::{
        arena::SEGMENTS_OFFSET,
        x86::{SEGMENTS_SIZE, encode_segment_context},
    },
    cpu::x86::SegmentCache,
    process::{ExecutionConfig32, calls::call::PendingOwner},
};

const KEY: u64 = 97;
const FLAGS: u32 = 0x46;

mod stores;

fn fixture(code: &[u8]) -> (EngineInstance, u64, [X87PairStep32; 2]) {
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
        .write(
            GuestAddress(0x5000),
            &[1.25_f32.to_le_bytes(), 2.5_f32.to_le_bytes()].concat(),
        )
        .unwrap();
    engine
        .configure_execution(ExecutionConfig32 {
            units: 4,
            wasm_bytes: 262144,
            promotion_hits: 256,
        })
        .unwrap();
    let request = engine.prepare_cache_entry(KEY, 0x1000).unwrap();
    let id = request.unit_id;
    engine
        .install_cached_entry(KEY, request.serial, id, request.slot)
        .unwrap();
    let descriptors = engine
        .guard_material_resident_region(KEY, id)
        .unwrap()
        .x87_pair(0x1000)
        .unwrap();
    (engine, id, descriptors)
}

fn scalar(engine: &mut EngineInstance, id: u64, step: X87PairStep32, address: u32) {
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
        Ok(StoreCompletion::Complete),
    );
}

fn fp(engine: &mut EngineInstance, state: X87State) {
    encode_x87(
        &state,
        &mut engine.arena_mut().unwrap()[X87_OFFSET..X87_OFFSET + X87_SIZE],
    )
    .unwrap();
}

fn assert_same(left: &EngineInstance, right: &EngineInstance) {
    assert_eq!(
        left.arena(),
        right.arena(),
        "all public CPU/helper/extended records match scalar execution"
    );
    assert_eq!(left.cr2(), right.cr2());
    let mut left_ram = [0; 4096];
    let mut right_ram = [0; 4096];
    left.memory()
        .unwrap()
        .read(GuestAddress(0x5000), &mut left_ram)
        .unwrap();
    right
        .memory()
        .unwrap()
        .read(GuestAddress(0x5000), &mut right_ram)
        .unwrap();
    assert_eq!(
        left_ram, right_ram,
        "read/register pairs never write guest bytes"
    );
    assert_eq!(
        left.memory().unwrap().transaction_version(),
        right.memory().unwrap().transaction_version()
    );
}

#[test]
fn register_and_read_pairs_match_each_scalar_prefix_and_keep_ram_unchanged() {
    for code in [
        &[0xd9, 0xe8, 0xd9, 0xe0, 0xff, 0xe2][..],
        &[0xd9, 0x03, 0xd9, 0x01, 0xff, 0xe2],
        &[0xd9, 0x03, 0xd8, 0xc0, 0xff, 0xe2],
    ] {
        for maximum in [1, 2] {
            let (mut paired, id, _) = fixture(code);
            let (mut ordinary, ordinary_id, descriptors) = fixture(code);
            for index in 0..maximum as usize {
                scalar(
                    &mut ordinary,
                    ordinary_id,
                    descriptors[index],
                    [0x5000, 0x5004][index],
                );
            }
            assert_eq!(
                paired.x87_resident_pair(KEY, id, 0x1000, [0x5000, 0x5004], FLAGS, maximum),
                X87PairOutcome::complete(maximum, maximum - 1),
            );
            assert_same(&paired, &ordinary);
        }
    }
}

#[test]
fn operand_page_fault_at_either_step_keeps_exact_prefix_and_detector_witness() {
    for failed in [0, 1] {
        let code = [0xd9, 0x03, 0xd9, 0x01, 0xff, 0xe2];
        let (mut paired, id, _) = fixture(&code);
        let (mut ordinary, ordinary_id, descriptors) = fixture(&code);
        let epoch = paired
            .begin_exception_detection(PendingOwner::Resident(id))
            .unwrap();
        let ordinary_epoch = ordinary
            .begin_exception_detection(PendingOwner::Resident(ordinary_id))
            .unwrap();
        let mut addresses = [0x5000, 0x5004];
        addresses[failed] = 0x7000;
        for index in 0..=failed {
            scalar(
                &mut ordinary,
                ordinary_id,
                descriptors[index],
                addresses[index],
            );
        }
        let sequence = paired.cpu_page_fault_sequence32();
        assert_eq!(
            paired.x87_resident_pair(KEY, id, 0x1000, addresses, FLAGS, 2),
            X87PairOutcome::complete(failed as u32, failed as u32),
        );
        assert_eq!(paired.cpu_page_fault_sequence32(), sequence + 1);
        assert_eq!(paired.cr2(), Some(GuestAddress(0x7000)));
        assert_same(&paired, &ordinary);
        assert_eq!(
            paired.certified_page_fault32(epoch),
            ordinary.certified_page_fault32(ordinary_epoch)
        );
        assert!(paired.certified_page_fault32(epoch).is_some());
        assert!(
            !paired.has_ready_exception_detection(),
            "pair leaves the active detector unsealed"
        );
    }
}

#[test]
fn pending_mf_precedes_first_operand_access_and_publishes_without_retirement() {
    let code = [0xd9, 0x03, 0xd9, 0x01, 0xff, 0xe2];
    let (mut paired, id, _) = fixture(&code);
    let (mut ordinary, ordinary_id, descriptors) = fixture(&code);
    let state = X87State {
        control: 0x37e,
        status: 1,
        ..X87State::default()
    };
    fp(&mut paired, state);
    fp(&mut ordinary, state);
    scalar(&mut ordinary, ordinary_id, descriptors[0], 0x7000);
    assert_eq!(
        paired.x87_resident_pair(KEY, id, 0x1000, [0x7000, 0x7000], FLAGS, 2),
        X87PairOutcome::complete(0, 0)
    );
    assert_same(&paired, &ordinary);
    assert_eq!(paired.cr2(), None);
    assert_eq!(
        decode_x87(&paired.arena()[X87_OFFSET..X87_OFFSET + X87_SIZE])
            .unwrap()
            .status,
        0x8081
    );
}

#[test]
fn newly_pending_mf_returns_completed_prefix_for_original_second_boundary() {
    let code = [0xd9, 0xe0, 0xd9, 0x03, 0xff, 0xe2];
    let (mut paired, id, _) = fixture(&code);
    let (mut ordinary, ordinary_id, descriptors) = fixture(&code);
    let state = X87State {
        control: 0x37e,
        ..X87State::default()
    };
    fp(&mut paired, state);
    fp(&mut ordinary, state);
    scalar(&mut ordinary, ordinary_id, descriptors[0], 0);
    assert_eq!(
        paired.x87_resident_pair(KEY, id, 0x1000, [0, 0x7000], FLAGS, 2),
        X87PairOutcome::complete(1, 0)
    );
    assert_same(&paired, &ordinary);
    assert_eq!(paired.cr2(), None);
    assert_eq!(
        crate::abi::header::read_u32(paired.arena(), HELPER_OFFSET + 16),
        0,
        "completed first helper packet is preserved"
    );
    scalar(&mut paired, id, descriptors[1], 0x7000);
    scalar(&mut ordinary, ordinary_id, descriptors[1], 0x7000);
    assert_same(&paired, &ordinary);
    assert_eq!(
        crate::abi::header::read_u32(paired.arena(), HELPER_OFFSET + 16),
        4
    );
    assert_eq!(paired.cr2(), None);
}

#[test]
fn later_cancel_or_cs_span_returns_first_packet_for_original_generated_boundary() {
    let code = [0xd9, 0x03, 0xd9, 0x01, 0xff, 0xe2];
    for cancel in [false, true] {
        let (mut paired, id, _) = fixture(&code);
        let (mut ordinary, ordinary_id, descriptors) = fixture(&code);
        for engine in [&mut paired, &mut ordinary] {
            if cancel {
                header::write_u32(engine.arena_mut().unwrap(), CANCEL_OFFSET, 1);
            } else {
                let mut context = engine.segment_context().unwrap();
                let current = context.caches[SegmentRegister::Cs.index()].bits();
                context.caches[SegmentRegister::Cs.index()] =
                    SegmentCache::from_bits((current & !0x010f_ffff) | 0x1001).unwrap();
                encode_segment_context(
                    &context,
                    &mut engine.arena_mut().unwrap()
                        [SEGMENTS_OFFSET..SEGMENTS_OFFSET + SEGMENTS_SIZE],
                )
                .unwrap();
            }
        }
        scalar(&mut ordinary, ordinary_id, descriptors[0], 0x5000);
        assert_eq!(
            paired.x87_resident_pair(KEY, id, 0x1000, [0x5000, 0x7000], FLAGS, 2),
            X87PairOutcome::complete(1, 0)
        );
        assert_same(&paired, &ordinary);
        assert_eq!(
            paired.cr2(),
            None,
            "second operand never runs after a boundary stop"
        );
    }
}

#[test]
fn invalid_owner_pair_start_limits_and_fp_records_are_neutral() {
    for (key, first_pc, maximum) in [
        (KEY + 1, 0x1000, 2),
        (KEY, 0x1002, 2),
        (KEY, 0x1001, 2),
        (KEY, 0x1000, 0),
        (KEY, 0x1000, 3),
    ] {
        let (mut engine, id, _) = fixture(&[0xd9, 0xe8, 0xd9, 0xee, 0xff, 0xe2]);
        let before = engine.arena().to_vec();
        let result = engine.x87_resident_pair(key, id, first_pc, [0, 0], FLAGS, maximum);
        assert!(result.result.is_err());
        assert_eq!((result.completed, result.reported_step), (0, 0));
        assert_eq!(engine.arena(), before);
    }
    let (mut engine, id, _) = fixture(&[0xd9, 0xe8, 0xd9, 0xee, 0xff, 0xe2]);
    engine.arena_mut().unwrap()[X87_OFFSET] ^= 1;
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.x87_resident_pair(KEY, id, 0x1000, [0, 0], FLAGS, 2),
        X87PairOutcome::failure(0, 0, HostError::Infrastructure)
    );
    assert_eq!(engine.arena(), before);
}
