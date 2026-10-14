use super::*;
use crate::memory::{GuestAddress, MemoryFault};

fn literal(fields: [u32; 6]) -> [u8; 40] {
    let mut bytes = [0; 40];
    bytes[..16].copy_from_slice(&[82, 51, 77, 72, 11, 0, 1, 0, 40, 0, 0, 0, 0, 0, 0, 0]);
    for (index, value) in fields.into_iter().enumerate() {
        bytes[16 + index * 4..20 + index * 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes
}

#[test]
fn selected_frame_kind_and_target_have_an_exact_additive_envelope() {
    for (frame, target, expected) in [
        (2, 0xfffe, [0, 0xfffe, 0, 0, 0, 2]),
        (0x102, 0x1234, [0, 0x1234, 0, 0, 0, 0x102]),
        (4, 0x12345678, [0, 0x12345678, 0, 0, 0, 4]),
        (0x104, u32::MAX, [0, u32::MAX, 0, 0, 0, 0x104]),
    ] {
        let mut bytes = [0xa5; 40];
        encode_interrupt_result(0x80, frame, target, Ok(()), &mut bytes).unwrap();
        assert_eq!(bytes, literal(expected));
    }
}

#[test]
fn idt_and_selector_fault_phases_stay_distinct_from_existing_gate_packets() {
    for (frame, error, expected) in [
        (
            0,
            CpuMemoryError::GeneralProtection(0x402),
            [3, 0, 0x402, 0, 0, 0],
        ),
        (
            0,
            CpuMemoryError::SegmentNotPresent(0x402),
            [6, 0, 0x402, 0, 0, 0],
        ),
        (2, CpuMemoryError::GeneralProtection(0), [3, 0, 0, 0, 0, 2]),
        (
            4,
            CpuMemoryError::SegmentNotPresent(0x48),
            [6, 0, 0x48, 0, 0, 4],
        ),
        (
            0x104,
            CpuMemoryError::StackSegment(0),
            [7, 0, 0, 0, 0, 0x104],
        ),
    ] {
        let mut bytes = [0xa5; 40];
        encode_interrupt_result(
            0x80,
            frame,
            0xdeadbeef,
            Err(InterruptHelperError::Cpu(error)),
            &mut bytes,
        )
        .unwrap();
        assert_eq!(bytes, literal(expected));
    }
    let mut old = [0xa5; 40];
    assert!(
        crate::abi::far::encode_gate_result(
            0,
            0,
            Err(CpuMemoryError::GeneralProtection(0x402)),
            &mut old
        )
        .is_err()
    );
    assert_eq!(old, [0xa5; 40]);
}

#[test]
fn wrapping_write_fault_and_neutral_unsupported_preserve_exact_metadata() {
    let fault = CpuMemoryError::Memory(MemoryError::Fault(MemoryFault {
        address: GuestAddress(0),
        access: Access::Write,
        reason: FaultReason::Unmapped,
    }));
    let mut bytes = [0xa5; 40];
    encode_interrupt_result(
        0x80,
        4,
        0,
        Err(InterruptHelperError::Cpu(fault)),
        &mut bytes,
    )
    .unwrap();
    assert_eq!(bytes, literal([1, 0, 1, 0, 2, 4]));
    encode_interrupt_result(
        0x80,
        0,
        123,
        Err(InterruptHelperError::Unsupported),
        &mut bytes,
    )
    .unwrap();
    assert_eq!(bytes, literal([8, 0, 4, 0, 0, 0]));
}

#[test]
fn malformed_phase_frame_and_fault_inputs_leave_output_untouched() {
    for (frame, target, result) in [
        (0, 0, Ok(())),
        (1, 0, Ok(())),
        (0x204, 0, Ok(())),
        (2, 0x10000, Ok(())),
        (2, 0, Err(InterruptHelperError::Unsupported)),
        (
            0,
            0,
            Err(InterruptHelperError::Cpu(
                CpuMemoryError::GeneralProtection(0),
            )),
        ),
        (
            0,
            0,
            Err(InterruptHelperError::Cpu(
                CpuMemoryError::GeneralProtection(0x40a),
            )),
        ),
        (
            0,
            0,
            Err(InterruptHelperError::Cpu(
                CpuMemoryError::SegmentNotPresent(0x48),
            )),
        ),
        (
            4,
            0,
            Err(InterruptHelperError::Cpu(
                CpuMemoryError::GeneralProtection(0x402),
            )),
        ),
        (
            4,
            0,
            Err(InterruptHelperError::Cpu(
                CpuMemoryError::GeneralProtection(0x10000),
            )),
        ),
        (
            4,
            0,
            Err(InterruptHelperError::Cpu(
                CpuMemoryError::SegmentNotPresent(0),
            )),
        ),
        (
            2,
            0,
            Err(InterruptHelperError::Cpu(CpuMemoryError::StackSegment(
                0x48,
            ))),
        ),
        (
            4,
            0,
            Err(InterruptHelperError::Cpu(CpuMemoryError::FloatingPoint(
                0x8081,
            ))),
        ),
        (
            4,
            0,
            Err(InterruptHelperError::Cpu(CpuMemoryError::Memory(
                MemoryError::Fault(MemoryFault {
                    address: GuestAddress(0),
                    access: Access::Read,
                    reason: FaultReason::Unmapped,
                }),
            ))),
        ),
    ] {
        let mut bytes = [0xa5; 40];
        assert!(encode_interrupt_result(0x80, frame, target, result, &mut bytes).is_err());
        assert_eq!(bytes, [0xa5; 40]);
    }
    let mut short = [0xa5; 39];
    assert!(encode_interrupt_result(0x80, 4, 0, Ok(()), &mut short).is_err());
    assert_eq!(short, [0xa5; 39]);
}
