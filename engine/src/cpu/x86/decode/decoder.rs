use iced_x86::{Decoder, DecoderError, DecoderOptions};

use crate::{
    cpu::{UnsupportedFeature, x86::ir::Operation},
    memory::{AddressSpace, CodeSnapshot, GuestAddress, MemoryError, MemoryFault},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeError {
    MemoryFault {
        pc: GuestAddress,
        fault: MemoryFault,
        length: u32,
    },
    InvalidEncoding,
    Unsupported(UnsupportedFeature),
    Infrastructure(MemoryError),
}

#[derive(Debug)]
pub struct DecodedInstruction {
    pc: GuestAddress,
    length: u8,
    next_pc: GuestAddress,
    operation: Operation,
    code_snapshot: CodeSnapshot,
}

impl DecodedInstruction {
    pub fn pc(&self) -> GuestAddress {
        self.pc
    }

    pub fn length(&self) -> u8 {
        self.length
    }

    pub fn next_pc(&self) -> GuestAddress {
        self.next_pc
    }

    pub fn operation(&self) -> &Operation {
        &self.operation
    }

    pub fn code_snapshot(&self) -> &CodeSnapshot {
        &self.code_snapshot
    }
}

pub fn decode_one(
    memory: &AddressSpace,
    pc: GuestAddress,
) -> Result<DecodedInstruction, DecodeError> {
    let mut bytes = [0; 15];
    for length in 1..=bytes.len() {
        memory
            .fetch(pc, &mut bytes[..length])
            .map_err(|error| memory_error(error, pc, length))?;
        let mut decoder =
            Decoder::with_ip(32, &bytes[..length], u64::from(pc.0), DecoderOptions::NONE);
        let instruction = decoder.decode();
        match decoder.last_error() {
            DecoderError::NoMoreBytes => continue,
            DecoderError::InvalidInstruction => return Err(DecodeError::InvalidEncoding),
            DecoderError::None => {
                let consumed = instruction.len();
                let operation = super::lower::lower(&instruction, &bytes[..consumed])?;
                let code_snapshot = memory
                    .snapshot_code(pc, consumed)
                    .map_err(|error| memory_error(error, pc, consumed))?;
                return Ok(DecodedInstruction {
                    pc,
                    length: consumed as u8,
                    next_pc: GuestAddress(instruction.next_ip32()),
                    operation,
                    code_snapshot,
                });
            }
            _ => return Err(DecodeError::InvalidEncoding),
        }
    }
    Err(DecodeError::InvalidEncoding)
}

fn memory_error(error: MemoryError, pc: GuestAddress, length: usize) -> DecodeError {
    match error {
        MemoryError::Fault(fault) => DecodeError::MemoryFault {
            pc,
            fault,
            length: length as u32,
        },
        other => DecodeError::Infrastructure(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{PageRange, Permissions};

    #[test]
    fn decodes_immediate_move() {
        let mut memory = AddressSpace::new(1).unwrap();
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
        memory
            .write(GuestAddress(0), &[0xb8, 0x78, 0x56, 0x34, 0x12])
            .unwrap();
        let decoded = decode_one(&memory, GuestAddress(0)).unwrap();
        assert_eq!(decoded.length(), 5);
        assert_eq!(decoded.next_pc(), GuestAddress(5));
        assert!(memory.is_code_current(decoded.code_snapshot()));
    }
}
