use super::*;
use crate::cpu::x86::{
    CpuPagingError32, SegmentCache,
    avx::{
        Element, GatherFaultCause, GatherGeometry, GatherInstruction, GatherScale,
        MaskedInstruction, VectorRegister,
    },
};

fn engine() -> EngineInstance {
    let mut e = EngineInstance::new(2, 7).unwrap();

    e.map(0x5000, 1, 3).unwrap();

    e.memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x5ffc), &[0x78, 0x56, 0x34, 0x12])
        .unwrap();
    e
}
fn r(index: u8) -> VectorRegister {
    VectorRegister::new(index).unwrap()
}

#[test]
fn masked_runtime_checks_selected_bytes_only_and_late_pf_is_atomic() {
    let mut e = engine();
    let mut vectors = VectorState::default();
    vectors.simd.xmm[0] = [0xa5; 16];
    vectors.simd.xmm[1][3] = 0x80;
    let instruction = MaskedInstruction::Load {
        width: VectorWidth::Xmm,
        element: Element::Single,
        destination: r(0),
        mask: r(1),
    };
    vectors
        .execute_masked_with_port(
            instruction,
            &SegmentContext::default(),
            SegmentRegister::Ds,
            0x5ffc,
            &mut e.cpu_memory_port32(),
        )
        .unwrap();
    assert_eq!(
        vectors.read(r(0)),
        [
            0x78, 0x56, 0x34, 0x12, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0
        ]
    );
    assert_eq!(e.cr2(), None);
    vectors.simd.xmm[1][7] = 0x80;
    let before = vectors;
    let before_arena = e.arena().to_vec();
    let error = vectors
        .execute_masked_with_port(
            instruction,
            &SegmentContext::default(),
            SegmentRegister::Ds,
            0x5ffc,
            &mut e.cpu_memory_port32(),
        )
        .unwrap_err();
    let MaskedError::Paging(CpuPagingError32::PageFault(fault)) = error else {
        panic!("expected selected second-lane PF");
    };
    assert_eq!(
        (fault.linear(), fault.error_code()),
        (GuestAddress(0x6000), 4)
    );
    assert_eq!(e.cr2(), Some(GuestAddress(0x6000)));
    assert_eq!(vectors, before);
    assert_eq!(e.arena(), before_arena);

    let store = MaskedInstruction::Store {
        width: VectorWidth::Xmm,
        element: Element::Single,
        source: r(0),
        mask: r(1),
    };
    let version = e.memory().unwrap().code_validation_version();
    let error = vectors
        .execute_masked_with_port(
            store,
            &SegmentContext::default(),
            SegmentRegister::Ds,
            0x5ffc,
            &mut e.cpu_memory_port32(),
        )
        .unwrap_err();
    let MaskedError::Paging(CpuPagingError32::PageFault(fault)) = error else {
        panic!("expected atomic selected store PF");
    };
    assert_eq!(
        (fault.linear(), fault.error_code()),
        (GuestAddress(0x6000), 6)
    );
    let mut ram = [0; 4];
    e.memory()
        .unwrap()
        .read(GuestAddress(0x5ffc), &mut ram)
        .unwrap();
    assert_eq!(ram, [0x78, 0x56, 0x34, 0x12]);
    assert_eq!(e.memory().unwrap().code_validation_version(), version);
}
#[test]
fn gather_runtime_keeps_completed_lane_progress_before_typed_fault_and_segment_priority() {
    let mut e = engine();
    let instruction = GatherInstruction::new(
        GatherGeometry::DwordDword128,
        r(0),
        r(1),
        r(2),
        GatherScale::One,
    )
    .unwrap();
    let mut vectors = VectorState::default();
    vectors.simd.xmm[0] = [0xa5; 16];
    vectors.upper[0] = [0xa5; 16];
    vectors.simd.xmm[2][3] = 0x80;
    vectors.simd.xmm[2][7] = 0x80;
    vectors.simd.xmm[1][4..8].copy_from_slice(&4_u32.to_le_bytes());
    let inputs = instruction.capture(&vectors);
    let before = e.arena().to_vec();
    let outcome = instruction
        .evaluate_with_port(
            inputs,
            SegmentRegister::Ds,
            0x5ffc,
            Some(&SegmentContext::default()),
            &mut e.cpu_memory_port32(),
        )
        .unwrap();
    assert_eq!(outcome.completed_lanes, 1);
    let mut expected = inputs.destination;
    expected[..4].copy_from_slice(&[0x78, 0x56, 0x34, 0x12]);
    assert_eq!(outcome.destination, expected);
    assert_eq!(&outcome.mask[..8], &[0, 0, 0, 0, 0xff, 0xff, 0xff, 0xff]);
    let fault = outcome.fault.unwrap();
    assert_eq!(fault.lane, 1);
    let GatherFaultCause::Paging(CpuPagingError32::PageFault(fault)) = fault.cause else {
        panic!("expected lane1 PF");
    };
    assert_eq!(
        (fault.linear(), fault.error_code()),
        (GuestAddress(0x6000), 4)
    );
    assert_eq!(e.cr2(), Some(GuestAddress(0x6000)));
    assert_eq!(e.arena(), before);
    let mut context = SegmentContext::default();
    context.caches[SegmentRegister::Ds.index()] = SegmentCache::from_bits(0xf230_5fff).unwrap();
    let segment = instruction
        .evaluate_with_port(
            inputs,
            SegmentRegister::Ds,
            0x5ffc,
            Some(&context),
            &mut e.cpu_memory_port32(),
        )
        .unwrap();
    assert_eq!(segment.completed_lanes, 1);
    let fault = segment.fault.unwrap();
    assert_eq!(fault.lane, 1);
    assert!(matches!(fault.cause, GatherFaultCause::Segment { .. }));
    assert_eq!(segment.destination, expected);
}
