use super::*;
use crate::memory::MemoryFault;

fn fault(address: u32, access: Access, reason: FaultReason) -> Result<u32, MemoryError> {
    Err(MemoryError::Fault(MemoryFault {
        address: GuestAddress(address),
        access,
        reason,
    }))
}

#[test]
fn wrapping_fault_membership_and_value_contract_are_explicit() {
    let mut output = [0xa5; 40];
    encode_cpu_scalar_result(
        GuestAddress(0xffff_fffe),
        4,
        CpuScalarOperation::Read,
        Ok(0x4433_2211),
        &mut output,
    )
    .unwrap();
    assert_eq!(&output[4..6], &10_u16.to_le_bytes());
    assert_eq!(&output[20..24], &0x4433_2211_u32.to_le_bytes());
    assert_eq!(&output[36..40], &4_u32.to_le_bytes());
    for at in [0xffff_fffe, 0xffff_ffff, 0, 1] {
        encode_cpu_scalar_result(
            GuestAddress(0xffff_fffe),
            4,
            CpuScalarOperation::Read,
            fault(at, Access::Read, FaultReason::Permission),
            &mut output,
        )
        .unwrap();
        assert_eq!(&output[28..32], &at.to_le_bytes());
    }
    for (operation, result) in [
        (
            CpuScalarOperation::Read,
            fault(2, Access::Read, FaultReason::Permission),
        ),
        (
            CpuScalarOperation::Read,
            fault(0, Access::Write, FaultReason::Permission),
        ),
        (
            CpuScalarOperation::Read,
            fault(0xffff_fffe, Access::Read, FaultReason::AddressOverflow),
        ),
        (
            CpuScalarOperation::Modify,
            fault(0, Access::Write, FaultReason::Unmapped),
        ),
        (CpuScalarOperation::Store, Ok(1)),
    ] {
        output.fill(0xa5);
        assert!(
            encode_cpu_scalar_result(GuestAddress(0xffff_fffe), 4, operation, result, &mut output)
                .is_err()
        );
        assert_eq!(
            output, [0xa5; 40],
            "malformed CPU reply encoding is neutral"
        );
    }
    output.fill(0xa5);
    assert!(
        encode_cpu_scalar_result(
            GuestAddress(0xffff_ffff),
            2,
            CpuScalarOperation::Read,
            Ok(0x10000),
            &mut output
        )
        .is_err()
    );
    assert_eq!(output, [0xa5; 40]);
}
