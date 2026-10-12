use super::{AbiError, header, memory_helper::HELPER_SIZE};
use crate::memory::{Access, FaultReason, MemoryError};

pub const CPU_HELPER_VERSION: u16 = 7;
pub const CPU_HELPER_VERSION_8: u16 = 8;
pub const MAX_XSTATE_SPAN: usize = 832;
pub const MAX_CPU_TRANSFER: usize = 512;
pub const MAX_CPU_SPAN: usize = 576;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CpuMemoryError {
    Memory(MemoryError),
    GeneralProtection(u32),
    FloatingPoint(u16),
    SimdFloatingPoint(u32),
    SegmentNotPresent(u32),
    StackSegment(u32),
}

pub fn encode_cpu_result(
    length: u32,
    value: u32,
    result: Result<(), CpuMemoryError>,
    output: &mut [u8],
) -> Result<(), AbiError> {
    encode(
        length,
        value,
        result,
        output,
        CPU_HELPER_VERSION,
        MAX_CPU_SPAN,
    )
}

pub fn encode_cpu_result_v8(
    length: u32,
    value: u32,
    result: Result<(), CpuMemoryError>,
    output: &mut [u8],
) -> Result<(), AbiError> {
    encode(
        length,
        value,
        result,
        output,
        CPU_HELPER_VERSION_8,
        MAX_XSTATE_SPAN,
    )
}

fn encode(
    length: u32,
    value: u32,
    result: Result<(), CpuMemoryError>,
    output: &mut [u8],
    version: u16,
    max_span: usize,
) -> Result<(), AbiError> {
    if output.len() != HELPER_SIZE || length as usize > max_span {
        return Err(AbiError::Length);
    }
    let fields = match result {
        Ok(()) => [0, value, 0, 0, 0, length],
        Err(CpuMemoryError::GeneralProtection(code)) => [3, 0, code, 0, 0, length],
        Err(CpuMemoryError::FloatingPoint(status)) => [4, 0, u32::from(status), 0, 0, length],
        Err(CpuMemoryError::SimdFloatingPoint(mxcsr)) => [5, 0, mxcsr, 0, 0, length],
        Err(CpuMemoryError::SegmentNotPresent(code)) => [6, 0, code, 0, 0, length],
        Err(CpuMemoryError::StackSegment(code)) => [7, 0, code, 0, 0, length],
        Err(CpuMemoryError::Memory(MemoryError::Fault(fault))) => [
            1,
            0,
            match fault.reason {
                FaultReason::Unmapped => 1,
                FaultReason::Permission => 2,
                FaultReason::AddressOverflow => 3,
            },
            fault.address.0,
            match fault.access {
                Access::Read => 1,
                Access::Write => 2,
                Access::Execute => return Err(AbiError::MemoryHelper),
            },
            length,
        ],
        Err(CpuMemoryError::Memory(error)) => [
            2,
            0,
            if error == MemoryError::VersionExhausted {
                1
            } else {
                2
            },
            0,
            0,
            length,
        ],
    };
    header::write_version(output, *b"R3MH", version);
    for (index, value) in fields.into_iter().enumerate() {
        header::write_u32(output, 16 + index * 4, value);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exception_packets_keep_exact_cpu_exception_and_span() {
        for (error, status, detail) in [
            (CpuMemoryError::GeneralProtection(0), 3, 0),
            (CpuMemoryError::FloatingPoint(0x8081), 4, 0x8081),
            (CpuMemoryError::SimdFloatingPoint(0x1f81), 5, 0x1f81),
        ] {
            let mut record = [0; HELPER_SIZE];
            encode_cpu_result(108, 123, Err(error), &mut record).unwrap();
            assert_eq!(header::read_u32(&record, 4), 0x10007);
            assert_eq!(header::read_u32(&record, 16), status);
            assert_eq!(header::read_u32(&record, 20), 0);
            assert_eq!(header::read_u32(&record, 24), detail);
            assert_eq!(header::read_u32(&record, 36), 108);
        }
    }
}
