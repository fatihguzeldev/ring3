use super::{
    AbiError,
    cpu_helper::{CpuMemoryError, encode_cpu_result},
};
use crate::{
    cpu::x86::CpuPagingError32,
    memory::{Access, MemoryError},
};

pub const GATE_HELPER_VERSION: u16 = 9;

pub(crate) fn encode_gate_result(
    slot: u32,
    target: u32,
    result: Result<(), CpuMemoryError>,
    output: &mut [u8],
) -> Result<(), AbiError> {
    if !matches!(slot, 0 | 2 | 4) || (slot == 2 && result.is_ok() && target > 0xffff) {
        return Err(AbiError::MemoryHelper);
    }
    match result {
        Err(CpuMemoryError::Paging(CpuPagingError32::Memory(error))) => {
            return encode_gate_result(slot, target, Err(CpuMemoryError::Memory(error)), output);
        }
        Err(CpuMemoryError::Paging(error)) => {
            return super::cpu_helper::paging::encode_stack(slot, error, output);
        }
        Err(CpuMemoryError::GeneralProtection(code)) if code > 0xfffc || code & 3 != 0 => {
            return Err(AbiError::MemoryHelper);
        }
        Err(CpuMemoryError::SegmentNotPresent(code))
            if code == 0 || code > 0xfffc || code & 3 != 0 =>
        {
            return Err(AbiError::MemoryHelper);
        }
        Err(CpuMemoryError::StackSegment(code)) if slot == 0 || code != 0 => {
            return Err(AbiError::MemoryHelper);
        }
        Err(CpuMemoryError::FloatingPoint(_) | CpuMemoryError::SimdFloatingPoint(_)) => {
            return Err(AbiError::MemoryHelper);
        }
        Err(CpuMemoryError::Memory(MemoryError::Fault(fault)))
            if slot == 0 || fault.access != Access::Write =>
        {
            return Err(AbiError::MemoryHelper);
        }
        Err(CpuMemoryError::Memory(_)) if slot == 0 => return Err(AbiError::MemoryHelper),
        _ => {}
    }
    encode_cpu_result(slot, target, result, output)?;
    output[4..6].copy_from_slice(&GATE_HELPER_VERSION.to_le_bytes());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abi::memory_helper::HELPER_SIZE;

    #[test]
    fn literal_gate_frames_report_selected_slot_without_broadening_v7() {
        for (slot, result, fields) in [
            (0, Ok(()), [0, 0x12345678, 0, 0, 0, 0]),
            (2, Ok(()), [0, 0x5678, 0, 0, 0, 2]),
            (
                4,
                Err(CpuMemoryError::GeneralProtection(0x48)),
                [3, 0, 0x48, 0, 0, 4],
            ),
            (
                2,
                Err(CpuMemoryError::SegmentNotPresent(0x48)),
                [6, 0, 0x48, 0, 0, 2],
            ),
            (4, Err(CpuMemoryError::StackSegment(0)), [7, 0, 0, 0, 0, 4]),
        ] {
            let mut expected = [0_u8; HELPER_SIZE];
            expected[..16].copy_from_slice(&[82, 51, 77, 72, 9, 0, 1, 0, 40, 0, 0, 0, 0, 0, 0, 0]);
            for (index, field) in fields.into_iter().enumerate() {
                expected[16 + index * 4..20 + index * 4].copy_from_slice(&u32::to_le_bytes(field));
            }
            let target = if slot == 2 { 0x5678 } else { 0x12345678 };
            let mut bytes = [0xa5; HELPER_SIZE];
            encode_gate_result(slot, target, result, &mut bytes).unwrap();
            assert_eq!(bytes, expected);
            let mut ordinary = [0; HELPER_SIZE];
            encode_cpu_result(slot, target, result, &mut ordinary).unwrap();
            expected[4] = 7;
            assert_eq!(
                ordinary, expected,
                "the existing encoder keeps its exact v7 envelope"
            );
        }
    }

    #[test]
    fn word_gate_target_is_bounded_before_output_publication() {
        for target in [0x10000, u32::MAX] {
            let mut bytes = [0xa5; HELPER_SIZE];
            assert!(encode_gate_result(2, target, Ok(()), &mut bytes).is_err());
            assert_eq!(bytes, [0xa5; HELPER_SIZE]);
        }
        for (slot, target) in [(2, 0xffff), (4, 0x10000), (4, u32::MAX)] {
            let mut bytes = [0; HELPER_SIZE];
            encode_gate_result(slot, target, Ok(()), &mut bytes).unwrap();
            assert_eq!(&bytes[20..24], &target.to_le_bytes());
        }
    }

    #[test]
    fn invalid_gate_packets_leave_output_untouched() {
        for (slot, result) in [
            (1, Ok(())),
            (0, Err(CpuMemoryError::StackSegment(0))),
            (4, Err(CpuMemoryError::StackSegment(0x48))),
            (2, Err(CpuMemoryError::GeneralProtection(0x49))),
            (2, Err(CpuMemoryError::GeneralProtection(0x10000))),
            (2, Err(CpuMemoryError::SegmentNotPresent(0))),
            (2, Err(CpuMemoryError::FloatingPoint(0x8081))),
            (4, Err(CpuMemoryError::SimdFloatingPoint(0x1f81))),
            (
                0,
                Err(CpuMemoryError::Memory(MemoryError::VersionExhausted)),
            ),
        ] {
            let mut bytes = [0xa5; HELPER_SIZE];
            assert!(encode_gate_result(slot, 0, result, &mut bytes).is_err());
            assert_eq!(bytes, [0xa5; HELPER_SIZE]);
        }
    }
}

#[cfg(test)]
#[path = "far/paging.rs"]
mod paging_tests;
