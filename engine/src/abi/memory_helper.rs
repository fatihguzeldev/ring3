#![forbid(unsafe_code)]

use super::{AbiError, header};
use crate::memory::{Access, FaultReason, GuestAddress, MemoryError};

pub const HELPER_SIZE: usize = 40;
pub const NARROW_HELPER_VERSION: u16 = 2;
pub const BYTE_STORE_HELPER_VERSION: u16 = 3;
pub const WORD_STORE_HELPER_VERSION: u16 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NarrowReadWidth {
    Byte,
    Word,
}

pub fn encode_narrow_helper_result(
    address: GuestAddress,
    width: NarrowReadWidth,
    result: Result<u32, MemoryError>,
    output: &mut [u8],
) -> Result<(), AbiError> {
    if output.len() != HELPER_SIZE {
        return Err(AbiError::Length);
    }
    let (length, maximum) = match width {
        NarrowReadWidth::Byte => (1, 0xff),
        NarrowReadWidth::Word => (2, 0xffff),
    };
    let start = u64::from(address.0);
    let end = start + u64::from(length);
    let overflows = end > 1 << 32;
    let fields = match result {
        Ok(value) => {
            if overflows || value > maximum {
                return Err(AbiError::MemoryHelper);
            }
            [0, value, 0, 0, 0, length]
        }
        Err(MemoryError::Fault(fault)) => {
            if fault.access != Access::Read
                || overflows != (fault.reason == FaultReason::AddressOverflow)
            {
                return Err(AbiError::MemoryHelper);
            }
            let at = u64::from(fault.address.0);
            if (overflows && fault.address != address) || (!overflows && (at < start || at >= end))
            {
                return Err(AbiError::MemoryHelper);
            }
            let detail = match fault.reason {
                FaultReason::Unmapped => 1,
                FaultReason::Permission => 2,
                FaultReason::AddressOverflow => 3,
            };
            [1, 0, detail, fault.address.0, 1, length]
        }
        Err(error) => [
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
    header::write_version(output, *b"R3MH", NARROW_HELPER_VERSION);
    for (index, field) in fields.into_iter().enumerate() {
        header::write_u32(output, 16 + index * 4, field);
    }
    Ok(())
}

pub fn encode_byte_store_result(
    address: GuestAddress,
    result: Result<(), MemoryError>,
    output: &mut [u8],
) -> Result<(), AbiError> {
    if output.len() != HELPER_SIZE {
        return Err(AbiError::Length);
    }
    let fields = match result {
        Ok(()) => [0, 0, 0, 0, 0, 1],
        Err(MemoryError::Fault(fault)) => {
            if fault.access != Access::Write || fault.address != address {
                return Err(AbiError::MemoryHelper);
            }
            let detail = match fault.reason {
                FaultReason::Unmapped => 1,
                FaultReason::Permission => 2,
                FaultReason::AddressOverflow => return Err(AbiError::MemoryHelper),
            };
            [1, 0, detail, address.0, 2, 1]
        }
        Err(error) => [
            2,
            0,
            if error == MemoryError::VersionExhausted {
                1
            } else {
                2
            },
            0,
            0,
            1,
        ],
    };
    header::write_version(output, *b"R3MH", BYTE_STORE_HELPER_VERSION);
    for (index, field) in fields.into_iter().enumerate() {
        header::write_u32(output, 16 + index * 4, field);
    }
    Ok(())
}

pub fn encode_word_store_result(
    address: GuestAddress,
    result: Result<(), MemoryError>,
    output: &mut [u8],
) -> Result<(), AbiError> {
    if output.len() != HELPER_SIZE {
        return Err(AbiError::Length);
    }
    let start = u64::from(address.0);
    let end = start + 2;
    let overflows = end > 1 << 32;
    let fields = match result {
        Ok(()) => {
            if overflows {
                return Err(AbiError::MemoryHelper);
            }
            [0, 0, 0, 0, 0, 2]
        }
        Err(MemoryError::Fault(fault)) => {
            if fault.access != Access::Write
                || overflows != (fault.reason == FaultReason::AddressOverflow)
            {
                return Err(AbiError::MemoryHelper);
            }
            let at = u64::from(fault.address.0);
            if (overflows && fault.address != address) || (!overflows && (at < start || at >= end))
            {
                return Err(AbiError::MemoryHelper);
            }
            let detail = match fault.reason {
                FaultReason::Unmapped => 1,
                FaultReason::Permission => 2,
                FaultReason::AddressOverflow => 3,
            };
            [1, 0, detail, fault.address.0, 2, 2]
        }
        Err(error) => [
            2,
            0,
            if error == MemoryError::VersionExhausted {
                1
            } else {
                2
            },
            0,
            0,
            2,
        ],
    };
    header::write_version(output, *b"R3MH", WORD_STORE_HELPER_VERSION);
    for (index, field) in fields.into_iter().enumerate() {
        header::write_u32(output, 16 + index * 4, field);
    }
    Ok(())
}

pub fn encode_helper_result(
    result: Result<u32, MemoryError>,
    output: &mut [u8],
) -> Result<(), AbiError> {
    if output.len() != HELPER_SIZE {
        return Err(AbiError::Length);
    }
    let fields = match result {
        Ok(value) => [0, value, 0, 0, 0, 0],
        Err(MemoryError::Fault(fault)) => {
            let access = match fault.access {
                Access::Read => 1,
                Access::Write => 2,
                Access::Execute => return Err(AbiError::MemoryHelper),
            };
            let overflows = u64::from(fault.address.0) + 4 > 1 << 32;
            if overflows != (fault.reason == FaultReason::AddressOverflow) {
                return Err(AbiError::MemoryHelper);
            }
            let detail = match fault.reason {
                FaultReason::Unmapped => 1,
                FaultReason::Permission => 2,
                FaultReason::AddressOverflow => 3,
            };
            [1, 0, detail, fault.address.0, access, 4]
        }
        Err(error) => [
            2,
            0,
            if error == MemoryError::VersionExhausted {
                1
            } else {
                2
            },
            0,
            0,
            0,
        ],
    };
    header::write(output, *b"R3MH");
    for (index, field) in fields.into_iter().enumerate() {
        header::write_u32(output, 16 + index * 4, field);
    }
    Ok(())
}
