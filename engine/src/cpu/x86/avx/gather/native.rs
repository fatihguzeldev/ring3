use super::*;
use crate::memory::{Access, FaultReason, PageRange, Permissions};

fn instruction(id: u8, scale: u8) -> GatherInstruction {
    GatherInstruction::new(
        GatherGeometry::from_id(id).unwrap(),
        VectorRegister::new(1).unwrap(),
        VectorRegister::new(2).unwrap(),
        VectorRegister::new(3).unwrap(),
        GatherScale::from_power(scale).unwrap(),
    )
    .unwrap()
}

fn input(geometry: GatherGeometry, selected: u8) -> GatherInputs {
    let mut inputs = GatherInputs {
        destination: [0xa5; 32],
        mask: [0xff; 32],
        index: [0xee; 32],
    };
    for lane in 0..geometry.lanes() {
        let at = lane * geometry.index_bytes();
        inputs.index[at..at + geometry.index_bytes()]
            .copy_from_slice(&((lane * 8) as u64).to_le_bytes()[..geometry.index_bytes()]);
        let at = lane * geometry.data_bytes();
        inputs.mask[at..at + geometry.data_bytes()].fill(0x11);
        inputs.mask[at + geometry.data_bytes() - 1] = if selected & (1 << lane) != 0 {
            0x80
        } else {
            0x7f
        };
    }
    inputs
}

fn memory() -> AddressSpace {
    let mut memory = AddressSpace::new(2).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x5000), 1).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    memory
        .write(
            GuestAddress(0x5000),
            &std::array::from_fn::<_, 4096, _>(|byte| (byte % 251) as u8),
        )
        .unwrap();
    memory
}

#[test]
fn captures_are_immutable_and_only_pairwise_distinct_registers_are_constructible() {
    for destination in 0..8 {
        for index in 0..8 {
            for mask in 0..8 {
                let value = GatherInstruction::new(
                    GatherGeometry::DwordDword128,
                    VectorRegister::new(destination).unwrap(),
                    VectorRegister::new(index).unwrap(),
                    VectorRegister::new(mask).unwrap(),
                    GatherScale::One,
                );
                assert_eq!(
                    value.is_some(),
                    destination != index && destination != mask && index != mask
                );
            }
        }
    }
    let mut vectors = VectorState::default();
    vectors.simd.xmm[1] = [1; 16];
    vectors.upper[1] = [2; 16];
    vectors.simd.xmm[2] = [3; 16];
    vectors.upper[2] = [4; 16];
    vectors.simd.xmm[3] = [5; 16];
    vectors.upper[3] = [6; 16];
    let before = vectors;
    let captured = instruction(1, 0).capture(&vectors);
    assert_eq!(&captured.destination[..16], &[1; 16]);
    assert_eq!(&captured.destination[16..], &[2; 16]);
    assert_eq!(&captured.index[..16], &[3; 16]);
    assert_eq!(&captured.index[16..], &[4; 16]);
    assert_eq!(&captured.mask[..16], &[5; 16]);
    assert_eq!(&captured.mask[16..], &[6; 16]);
    assert_eq!(vectors, before);
}

#[test]
fn zero_selection_merges_active_bytes_without_consuming_context_or_addresses() {
    let memory = AddressSpace::new(1).unwrap();
    for id in 1..=8 {
        let instruction = instruction(id, 3);
        let geometry = instruction.geometry();
        let inactive = input(geometry, 0);
        let outcome = instruction
            .evaluate(inactive, SegmentRegister::Ss, u32::MAX, None, &memory)
            .unwrap();
        assert_eq!(
            &outcome.destination[..geometry.active_bytes()],
            &inactive.destination[..geometry.active_bytes()]
        );
        assert!(
            outcome.destination[geometry.active_bytes()..]
                .iter()
                .all(|byte| *byte == 0)
        );
        assert_eq!(outcome.mask, [0; 32]);
        assert_eq!(outcome.completed_lanes, 0);
        assert_eq!(outcome.fault, None);
        let selected = input(geometry, 1);
        assert_eq!(
            instruction.evaluate(selected, SegmentRegister::Ds, 0, None, &memory),
            Err(GatherError::InvalidContext)
        );
    }
}

#[test]
fn partial_loads_normalize_pending_masks_and_preserve_same_size_l0_upper_destination() {
    let memory = memory();
    for id in 1..=8 {
        let instruction = instruction(id, 0);
        let geometry = instruction.geometry();
        let selected = ((1_u16 << geometry.lanes()) - 1) as u8;
        let mut inputs = input(geometry, selected);
        inputs.index[geometry.index_bytes()..geometry.index_bytes() * 2]
            .copy_from_slice(&0x1010_u64.to_le_bytes()[..geometry.index_bytes()]);
        let before = inputs;
        let outcome = instruction
            .evaluate(
                inputs,
                SegmentRegister::Ds,
                0x5ff0,
                Some(&SegmentContext::default()),
                &memory,
            )
            .unwrap();
        assert_eq!(outcome.completed_lanes, 1);
        assert_eq!(
            outcome.fault,
            Some(GatherFault {
                lane: 1,
                cause: GatherFaultCause::Memory(MemoryFault {
                    address: GuestAddress(0x7000),
                    access: Access::Read,
                    reason: FaultReason::Unmapped
                })
            })
        );
        let mut literal = [0; 8];
        memory
            .read(GuestAddress(0x5ff0), &mut literal[..geometry.data_bytes()])
            .unwrap();
        assert_eq!(
            &outcome.destination[..geometry.data_bytes()],
            &literal[..geometry.data_bytes()]
        );
        assert_eq!(
            &outcome.destination[geometry.data_bytes()..geometry.active_bytes()],
            &before.destination[geometry.data_bytes()..geometry.active_bytes()]
        );
        let unused = &outcome.destination[geometry.active_bytes()..];
        assert!(unused.iter().all(|byte| *byte
            == if geometry.data_bytes() == geometry.index_bytes() {
                0xa5
            } else {
                0
            }));
        assert!(
            outcome.mask[..geometry.data_bytes()]
                .iter()
                .all(|byte| *byte == 0)
        );
        assert!(
            outcome.mask[geometry.data_bytes()..geometry.active_bytes()]
                .iter()
                .all(|byte| *byte == 0xff)
        );
        assert!(
            outcome.mask[geometry.active_bytes()..]
                .iter()
                .all(|byte| *byte == 0)
        );
        assert_eq!(inputs, before);
    }
}

#[test]
fn repaired_resume_skips_cleared_lanes_even_when_their_memory_changes() {
    let mut memory = memory();
    let instruction = instruction(5, 0);
    let mut inputs = input(instruction.geometry(), 3);
    inputs.index[8..16].copy_from_slice(&0x1010_u64.to_le_bytes());
    let partial = instruction
        .evaluate(
            inputs,
            SegmentRegister::Ds,
            0x5ff0,
            Some(&SegmentContext::default()),
            &memory,
        )
        .unwrap();
    assert_eq!(partial.completed_lanes, 1);
    assert_eq!(partial.fault.unwrap().lane, 1);
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x7000), 1).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    memory
        .write(GuestAddress(0x7000), &[0x11, 0x22, 0x33, 0x44])
        .unwrap();
    memory
        .write(GuestAddress(0x5ff0), &[0xde, 0xad, 0xbe, 0xef])
        .unwrap();
    inputs.destination = partial.destination;
    inputs.mask = partial.mask;
    let completed = instruction
        .evaluate(
            inputs,
            SegmentRegister::Ds,
            0x5ff0,
            Some(&SegmentContext::default()),
            &memory,
        )
        .unwrap();
    assert_eq!(completed.completed_lanes, 2);
    assert_eq!(completed.fault, None);
    assert_eq!(&completed.destination[..4], &partial.destination[..4]);
    assert_eq!(&completed.destination[4..8], &[0x11, 0x22, 0x33, 0x44]);
    assert!(completed.destination[8..].iter().all(|byte| *byte == 0));
    assert_eq!(completed.mask, [0; 32]);
}

#[test]
fn each_lane_checks_segment_then_paging_before_considering_any_higher_lane() {
    let memory = memory();
    let instruction = instruction(1, 0);
    let mut inputs = input(instruction.geometry(), 3);
    inputs.index[4..8].copy_from_slice(&0x2000_u32.to_le_bytes());
    let mut context = SegmentContext::default();
    context.caches[SegmentRegister::Ds.index()] =
        crate::cpu::x86::SegmentCache::from_bits(0xf2307fff).unwrap();
    let outcome = instruction
        .evaluate(inputs, SegmentRegister::Ds, 0x6000, Some(&context), &memory)
        .unwrap();
    assert_eq!(outcome.completed_lanes, 0);
    assert_eq!(
        outcome.fault.unwrap().cause,
        GatherFaultCause::Memory(MemoryFault {
            address: GuestAddress(0x6000),
            access: Access::Read,
            reason: FaultReason::Unmapped
        })
    );
    context.caches[SegmentRegister::Ds.index()] =
        crate::cpu::x86::SegmentCache::from_bits(0xf2305fff).unwrap();
    let outcome = instruction
        .evaluate(inputs, SegmentRegister::Ds, 0x6000, Some(&context), &memory)
        .unwrap();
    assert_eq!(
        outcome.fault.unwrap().cause,
        GatherFaultCause::Segment {
            segment: SegmentRegister::Ds,
            cause: SegmentAccessError::Limit
        }
    );
}

#[test]
fn signed_scaled_indices_wrap_before_the_selected_segment_span_is_checked() {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    memory
        .write(GuestAddress(0x1000), &[0x55, 0x66, 0x77, 0x88])
        .unwrap();
    let mut context = SegmentContext::default();
    context.caches[SegmentRegister::Ds.index()] =
        crate::cpu::x86::SegmentCache::from_bits(0xf2301fff).unwrap();
    for (id, scale, base, index) in [
        (1, 0, 0x2000, 0xfffff000_u64),
        (5, 0, 0x2000, 0xfffffffffffff000),
        (5, 2, 0, 0x100000400),
    ] {
        let instruction = instruction(id, scale);
        let mut inputs = input(instruction.geometry(), 1);
        inputs.index[..instruction.geometry().index_bytes()]
            .copy_from_slice(&index.to_le_bytes()[..instruction.geometry().index_bytes()]);
        let outcome = instruction
            .evaluate(inputs, SegmentRegister::Ds, base, Some(&context), &memory)
            .unwrap();
        assert_eq!(&outcome.destination[..4], &[0x55, 0x66, 0x77, 0x88]);
        assert_eq!(outcome.completed_lanes, 1);
        assert_eq!(outcome.fault, None);
        assert_eq!(outcome.mask, [0; 32]);
    }
}
