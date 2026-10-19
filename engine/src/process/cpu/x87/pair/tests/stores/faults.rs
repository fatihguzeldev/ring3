use super::*;
use crate::cpu::x86::PagingAccess32;

#[test]
fn pending_mf_and_newly_pending_mf_precede_final_store_operand_access() {
    for pending_before_first in [true, false] {
        let (mut paired, id, _) = fixture(REGISTER_STORE);
        let (mut ordinary, ordinary_id, descriptors) = fixture(REGISTER_STORE);
        let mut initial = if pending_before_first {
            one()
        } else {
            X87State::default()
        };
        initial.control &= !1;
        initial.status = u16::from(pending_before_first);
        for engine in [&mut paired, &mut ordinary] {
            fp(engine, initial);
        }
        let data_before = ram(&paired);
        let version = paired.memory().unwrap().transaction_version();
        scalar(&mut ordinary, ordinary_id, descriptors[0], 0);
        let completed = u32::from(!pending_before_first);
        assert_eq!(
            paired.x87_resident_pair(KEY, id, 0x1000, [0, 0x7000], FLAGS, 2),
            X87PairOutcome::complete(completed, 0)
        );
        assert_same(&paired, &ordinary);
        let actual = actual_fp(&paired);
        assert_eq!(
            actual.status,
            if pending_before_first { 0x8081 } else { 0x80c1 }
        );
        assert_eq!(actual.tag, initial.tag);
        assert_eq!(actual.registers, initial.registers);
        assert_eq!(ram(&paired), data_before);
        assert_eq!(paired.memory().unwrap().transaction_version(), version);
        assert_eq!(paired.cr2(), None);
        if pending_before_first {
            assert_packet(&paired, 7, [4, 0, 0x8081, 0, 0, 0]);
        } else {
            assert_packet(&paired, 7, [0, FLAGS, 0, 0, 0, 0]);
        }
    }
}

#[test]
fn final_store_segment_fault_preserves_literal_first_prefix_before_paging() {
    for stack in [false, true] {
        let code = if stack {
            &[0xd9, 0xe0, 0x36, 0xd9, 0x19, 0xff, 0xe2][..]
        } else {
            REGISTER_STORE
        };
        let (mut paired, id, _) = fixture(code);
        let (mut ordinary, ordinary_id, descriptors) = fixture(code);
        for engine in [&mut paired, &mut ordinary] {
            fp(engine, one());
            let mut context = engine.segment_context().unwrap();
            let segment = if stack {
                SegmentRegister::Ss
            } else {
                SegmentRegister::Ds
            };
            context.caches[segment.index()] = SegmentCache::from_bits(0xf230_5012).unwrap();
            encode_segment_context(
                &context,
                &mut engine.arena_mut().unwrap()[SEGMENTS_OFFSET..SEGMENTS_OFFSET + SEGMENTS_SIZE],
            )
            .unwrap();
        }
        let data_before = ram(&paired);
        let version = paired.memory().unwrap().transaction_version();
        scalar(&mut ordinary, ordinary_id, descriptors[0], 0);
        scalar(&mut ordinary, ordinary_id, descriptors[1], 0x5010);
        assert_eq!(
            paired.x87_resident_pair(KEY, id, 0x1000, [0, 0x5010], FLAGS, 2),
            X87PairOutcome::complete(1, 1)
        );
        assert_same(&paired, &ordinary);
        assert_eq!(actual_fp(&paired), negative_prefix());
        assert_eq!(ram(&paired), data_before);
        assert_eq!(paired.memory().unwrap().transaction_version(), version);
        assert_eq!(paired.cr2(), None);
        assert_packet(&paired, 7, [if stack { 7 } else { 3 }, 0, 0, 0, 0, 4]);
    }
}

#[test]
fn final_store_write_pf_keeps_prefix_and_ram_with_exact_cross_page_geometry() {
    for (address, readonly, fault_address, error_code) in [
        (0x5010, true, 0x5010, 7),
        (0x7000, false, 0x7000, 6),
        (0x5ffe, false, 0x6000, 6),
    ] {
        let (mut paired, id, _) = fixture(REGISTER_STORE);
        let (mut ordinary, ordinary_id, descriptors) = fixture(REGISTER_STORE);
        for engine in [&mut paired, &mut ordinary] {
            fp(engine, one());
            if readonly {
                engine.protect(0x5000, 1, 1).unwrap();
            }
        }
        let epoch = paired
            .begin_exception_detection(PendingOwner::Resident(id))
            .unwrap();
        let ordinary_epoch = ordinary
            .begin_exception_detection(PendingOwner::Resident(ordinary_id))
            .unwrap();
        let data_before = ram(&paired);
        let version = paired.memory().unwrap().transaction_version();
        scalar(&mut ordinary, ordinary_id, descriptors[0], 0);
        scalar(&mut ordinary, ordinary_id, descriptors[1], address);
        let sequence = paired.cpu_page_fault_sequence32();
        assert_eq!(
            paired.x87_resident_pair(KEY, id, 0x1000, [0, address], FLAGS, 2),
            X87PairOutcome::complete(1, 1)
        );
        assert_same(&paired, &ordinary);
        assert_eq!(actual_fp(&paired), negative_prefix());
        assert_eq!(ram(&paired), data_before);
        assert_eq!(paired.memory().unwrap().transaction_version(), version);
        assert_eq!(paired.cr2(), Some(GuestAddress(fault_address)));
        assert_eq!(paired.cpu_page_fault_sequence32(), sequence + 1);
        assert_eq!(
            paired.certified_page_fault32(epoch),
            ordinary.certified_page_fault32(ordinary_epoch)
        );
        assert!(paired.certified_page_fault32(epoch).is_some());
        assert!(
            !paired.has_ready_exception_detection(),
            "native pair does not seal the detector"
        );
        assert_packet(&paired, 15, [9, 0, error_code, fault_address, 2, 4]);
    }
}

#[test]
fn exhausted_store_commit_after_successful_preflight_keeps_first_prefix_and_ram() {
    let (mut paired, id, _) = fixture(REGISTER_STORE);
    let (mut ordinary, ordinary_id, descriptors) = fixture(REGISTER_STORE);
    for engine in [&mut paired, &mut ordinary] {
        fp(engine, one());
        engine.memory.as_mut().unwrap().exhaust_versions_for_test();
        assert_eq!(
            engine.inspect_cpu_paging32(GuestAddress(0x5010), 4, PagingAccess32::Write),
            Ok(())
        );
    }
    let data_before = ram(&paired);
    scalar(&mut ordinary, ordinary_id, descriptors[0], 0);
    scalar(&mut ordinary, ordinary_id, descriptors[1], 0x5010);
    assert_eq!(
        paired.x87_resident_pair(KEY, id, 0x1000, [0, 0x5010], FLAGS, 2),
        X87PairOutcome::complete(1, 1)
    );
    assert_same(&paired, &ordinary);
    assert_eq!(actual_fp(&paired), negative_prefix());
    assert_eq!(ram(&paired), data_before);
    assert_eq!(paired.memory().unwrap().transaction_version(), u64::MAX);
    assert_eq!(paired.cr2(), None);
    assert_packet(&paired, 7, [2, 0, 1, 0, 0, 4]);
}
