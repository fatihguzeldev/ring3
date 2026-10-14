use super::*;

#[test]
fn default_gather_faults_only_on_the_first_selected_lane() {
    let memory = AddressSpace::new(1).unwrap();
    let instruction = GatherInstruction::new(
        GatherGeometry::DwordDword128,
        VectorRegister::new(0).unwrap(),
        VectorRegister::new(1).unwrap(),
        VectorRegister::new(2).unwrap(),
        GatherScale::One,
    )
    .unwrap();
    let mut inputs = GatherInputs {
        destination: [0xa5; 32],
        index: [0; 32],
        mask: [0; 32],
    };
    let empty = instruction
        .evaluate(inputs, SegmentRegister::Ss, u32::MAX, None, &memory)
        .unwrap();
    assert_eq!(empty.fault, None);
    assert_eq!(empty.completed_lanes, 0);
    inputs.mask[7] = 0x80;
    let outcome = instruction
        .evaluate(
            inputs,
            SegmentRegister::Ds,
            0x1000,
            Some(&SegmentContext::default()),
            &memory,
        )
        .unwrap();
    assert_eq!(outcome.completed_lanes, 0);
    assert_eq!(outcome.destination, inputs.destination);
    assert!(matches!(
        outcome.fault,
        Some(GatherFault {
            lane: 1,
            cause: GatherFaultCause::Paging(CpuPagingError32::PageFault(fault)),
        }) if fault.linear() == GuestAddress(0x1000) && fault.error_code() == 4
    ));
}
