use super::*;
use crate::cpu::x86::{
    PagingAccess32, VerifiedPageFault32,
    avx::{GatherFault, GatherGeometry, GatherScale, VectorRegister},
};

#[test]
fn dword_and_qword_projection_preserve_partial_images_and_validate_captured_lane_geometry() {
    for (geometry, width) in [
        (GatherGeometry::DwordDword128, 4),
        (GatherGeometry::QwordDword256, 8),
    ] {
        let instruction = GatherInstruction::new(
            geometry,
            VectorRegister::new(1).unwrap(),
            VectorRegister::new(2).unwrap(),
            VectorRegister::new(3).unwrap(),
            GatherScale::One,
        )
        .unwrap();
        let mut inputs = GatherInputs {
            destination: [0xa5; 32],
            index: [0; 32],
            mask: [0; 32],
        };
        inputs.mask[width - 1] = 0x80;
        inputs.mask[2 * width - 1] = 0x80;
        inputs.index[4..8].copy_from_slice(&0xffff_fffe_u32.to_le_bytes());
        let context = SegmentContext::default();
        let error = CpuPagingError32::PageFault(VerifiedPageFault32::application(
            GuestAddress(0),
            PagingAccess32::Read,
            false,
        ));
        let mut outcome = GatherOutcome {
            destination: [0xa5; 32],
            mask: [0; 32],
            completed_lanes: 1,
            fault: Some(GatherFault {
                lane: 1,
                cause: GatherFaultCause::Paging(error),
            }),
        };
        outcome.destination[..width].fill(0x11);
        outcome.mask[width..2 * width].fill(0xff);
        let before = outcome;
        let bytes = packet(
            instruction,
            inputs,
            SegmentRegister::Ds,
            0,
            Some(&context),
            &outcome,
        )
        .unwrap();
        assert_eq!(&bytes[..12], b"R3GH\x02\0\x01\0\x28\0\0\0");
        assert_eq!(
            outcome, before,
            "projection never resets completed destination/mask progress"
        );
        assert_eq!(&bytes[32..40], &[1, 0, 0, 0, 1, 0, 0, 0]);
        let mut bad = outcome;
        bad.completed_lanes = 0;
        assert_eq!(
            packet(
                instruction,
                inputs,
                SegmentRegister::Ds,
                0,
                Some(&context),
                &bad
            ),
            Err(HostError::Infrastructure)
        );
        bad = outcome;
        bad.fault.as_mut().unwrap().cause = GatherFaultCause::Paging(CpuPagingError32::PageFault(
            VerifiedPageFault32::application(GuestAddress(0x8000), PagingAccess32::Read, false),
        ));
        assert_eq!(
            packet(
                instruction,
                inputs,
                SegmentRegister::Ds,
                0,
                Some(&context),
                &bad
            ),
            Err(HostError::Infrastructure)
        );
    }
}
