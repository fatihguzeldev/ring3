use super::{
    DecodeError,
    operands::{byte_register, effective_address, register, unsupported, word_register},
};
use crate::cpu::x86::{
    atomic::AtomicKind,
    ir::{AddressSize, AtomicSource, Operation},
};
use iced_x86::{Instruction, Mnemonic, OpKind};

pub(super) fn lower(instruction: &Instruction) -> Option<Result<Operation, DecodeError>> {
    if instruction.mnemonic() == Mnemonic::Cmpxchg8b {
        return Some(
            effective_address(instruction).map(|address| Operation::CompareExchange8B { address }),
        );
    }
    if !instruction.has_lock_prefix() || instruction.op0_kind() != OpKind::Memory {
        return None;
    }
    let kind = match instruction.mnemonic() {
        Mnemonic::Add => AtomicKind::Add,
        Mnemonic::Adc => AtomicKind::AddCarry,
        Mnemonic::Sub => AtomicKind::Subtract,
        Mnemonic::Sbb => AtomicKind::SubtractBorrow,
        Mnemonic::And => AtomicKind::And,
        Mnemonic::Or => AtomicKind::Or,
        Mnemonic::Xor => AtomicKind::Xor,
        Mnemonic::Inc => AtomicKind::Increment,
        Mnemonic::Dec => AtomicKind::Decrement,
        Mnemonic::Neg => AtomicKind::Negate,
        Mnemonic::Not => AtomicKind::Not,
        Mnemonic::Bts => AtomicKind::BitSet,
        Mnemonic::Btr => AtomicKind::BitReset,
        Mnemonic::Btc => AtomicKind::BitComplement,
        _ => return None,
    };
    let address = match effective_address(instruction) {
        Ok(address) => address,
        Err(error) => return Some(Err(error)),
    };
    let width = instruction.memory_size().size() as u8;
    if !matches!(width, 1 | 2 | 4) {
        return Some(Err(unsupported()));
    }
    let legacy = width == 4
        && match kind {
            AtomicKind::And | AtomicKind::Or | AtomicKind::Xor => true,
            AtomicKind::BitSet | AtomicKind::BitReset => {
                instruction.op1_kind() == OpKind::Register
                    && address.address_size == AddressSize::Bits32
            }
            _ => false,
        };
    if legacy {
        return None;
    }
    let source = if instruction.op_count() == 1 {
        Ok(AtomicSource::Immediate(0))
    } else {
        match instruction.op1_kind() {
            OpKind::Register if width == 1 => {
                byte_register(instruction.op1_register()).map(AtomicSource::ByteRegister)
            }
            OpKind::Register if width == 2 => {
                word_register(instruction.op1_register()).map(AtomicSource::Register)
            }
            OpKind::Register => register(instruction.op1_register()).map(AtomicSource::Register),
            OpKind::Immediate8
            | OpKind::Immediate16
            | OpKind::Immediate32
            | OpKind::Immediate8to16
            | OpKind::Immediate8to32 => {
                Ok(AtomicSource::Immediate(instruction.immediate(1) as u32))
            }
            _ => Err(unsupported()),
        }
    };
    Some(source.map(|source| Operation::AtomicMemory {
        kind,
        width,
        address,
        source,
    }))
}
