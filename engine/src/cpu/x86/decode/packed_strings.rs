use iced_x86::{EncodingKind, Instruction, Mnemonic, OpKind, Register};

use super::{
    DecodeError,
    operands::{effective_address, unsupported},
};
use crate::cpu::x86::packed_strings::{
    PackedStringCompare, PackedStringLength, PackedStringOutput, PackedStringSource,
};

fn xmm(register: Register) -> Result<u8, DecodeError> {
    if (Register::XMM0..=Register::XMM7).contains(&register) {
        Ok((register as u32 - Register::XMM0 as u32) as u8)
    } else {
        Err(unsupported())
    }
}

pub(super) fn lower(instruction: &Instruction) -> Option<Result<PackedStringCompare, DecodeError>> {
    let (length, output) = match instruction.mnemonic() {
        Mnemonic::Pcmpestri => (PackedStringLength::Explicit, PackedStringOutput::Index),
        Mnemonic::Pcmpestrm => (PackedStringLength::Explicit, PackedStringOutput::Mask),
        Mnemonic::Pcmpistri => (
            PackedStringLength::NullTerminated,
            PackedStringOutput::Index,
        ),
        Mnemonic::Pcmpistrm => (PackedStringLength::NullTerminated, PackedStringOutput::Mask),
        _ => return None,
    };
    if instruction.encoding() != EncodingKind::Legacy {
        return None;
    }
    Some((|| {
        Ok(PackedStringCompare {
            left: xmm(instruction.op0_register())?,
            right: match instruction.op1_kind() {
                OpKind::Register => PackedStringSource::Xmm(xmm(instruction.op1_register())?),
                OpKind::Memory => PackedStringSource::Memory(effective_address(instruction)?),
                _ => return Err(unsupported()),
            },
            length,
            output,
            control: instruction.immediate8(),
        })
    })())
}
