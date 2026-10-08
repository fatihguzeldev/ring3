use iced_x86::{Code, Instruction, OpKind};

use super::{
    DecodeError,
    operands::{
        byte_register, byte_value, effective_address, location, register, small_source,
        unsupported, value,
    },
    profile::check_profile,
};
use crate::cpu::x86::{
    Register32,
    ir::{CarryKind, ExtensionKind, Operation, SmallSource, SmallWidth},
};

pub(super) fn lower(instruction: &Instruction, bytes: &[u8]) -> Result<Operation, DecodeError> {
    check_profile(instruction, bytes)?;
    match instruction.code() {
        Code::Nopd => Ok(Operation::Nop),
        Code::Fninit => Ok(Operation::InitializeX87),
        Code::Fnclex => Ok(Operation::ClearX87Exceptions),
        Code::Fnstsw_AX => Ok(Operation::X87StatusToAx),
        Code::Fnstsw_m2byte => Ok(Operation::X87StatusToMemory {
            address: effective_address(instruction)?,
        }),
        Code::Fnstcw_m2byte => Ok(Operation::X87ControlToMemory {
            address: effective_address(instruction)?,
        }),
        Code::Clc => Ok(Operation::Carry {
            kind: CarryKind::Clear,
        }),
        Code::Stc => Ok(Operation::Carry {
            kind: CarryKind::Set,
        }),
        Code::Cmc => Ok(Operation::Carry {
            kind: CarryKind::Complement,
        }),
        Code::Cld => Ok(Operation::Direction { set: false }),
        Code::Std => Ok(Operation::Direction { set: true }),
        Code::Movsb_m8_m8 => Ok(Operation::MoveStringByte),
        Code::Movsd_m32_m32 => Ok(Operation::MoveStringDword),
        Code::Lodsb_AL_m8 => Ok(Operation::LoadStringByte),
        Code::Lodsw_AX_m16 => Ok(Operation::LoadStringWord),
        Code::Lodsd_EAX_m32 => Ok(Operation::LoadStringDword),
        Code::Stosb_m8_AL => Ok(Operation::StoreStringByte),
        Code::Stosd_m32_EAX => Ok(Operation::StoreStringDword),
        Code::Cmpsb_m8_m8 => Ok(Operation::CompareStringByte),
        Code::Scasb_AL_m8 => Ok(Operation::ScanStringByte),
        Code::Cmpsd_m32_m32 => Ok(Operation::CompareStringDword),
        Code::Scasd_EAX_m32 => Ok(Operation::ScanStringDword),
        Code::Xlat_m8 => Ok(Operation::TranslateByte),
        Code::Lahf => Ok(Operation::FlagsToAh),
        Code::Sahf => Ok(Operation::AhToFlags),
        Code::Nop_rm32 => {
            location(instruction, 0)?;
            Ok(Operation::Nop)
        }
        Code::Mov_rm32_r32
        | Code::Mov_r32_rm32
        | Code::Mov_r32_imm32
        | Code::Mov_rm32_imm32
        | Code::Mov_EAX_moffs32
        | Code::Mov_moffs32_EAX => Ok(Operation::Move {
            destination: location(instruction, 0)?,
            source: value(instruction, 1)?,
        }),
        Code::Mov_r8_rm8 | Code::Mov_AL_moffs8 => match instruction.op1_kind() {
            OpKind::Memory => Ok(Operation::LoadByte {
                destination: byte_register(instruction.op0_register())?,
                address: effective_address(instruction)?,
            }),
            OpKind::Register => Ok(Operation::MoveByte {
                destination: byte_register(instruction.op0_register())?,
                source: byte_value(instruction)?,
            }),
            _ => Err(unsupported()),
        },
        Code::Mov_rm8_r8 | Code::Mov_moffs8_AL => match instruction.op0_kind() {
            OpKind::Memory => Ok(Operation::StoreByte {
                address: effective_address(instruction)?,
                source: byte_value(instruction)?,
            }),
            OpKind::Register => Ok(Operation::MoveByte {
                destination: byte_register(instruction.op0_register())?,
                source: byte_value(instruction)?,
            }),
            _ => Err(unsupported()),
        },
        Code::Mov_r8_imm8 => Ok(Operation::MoveByte {
            destination: byte_register(instruction.op0_register())?,
            source: byte_value(instruction)?,
        }),
        Code::Mov_rm8_imm8 => {
            if instruction.op0_kind() == OpKind::Register {
                return Ok(Operation::MoveByte {
                    destination: byte_register(instruction.op0_register())?,
                    source: byte_value(instruction)?,
                });
            }
            if instruction.op0_kind() != OpKind::Memory {
                return Err(unsupported());
            }
            Ok(Operation::StoreByte {
                address: effective_address(instruction)?,
                source: byte_value(instruction)?,
            })
        }
        Code::Cwde => Ok(Operation::Extend {
            kind: ExtensionKind::Sign,
            destination: Register32::Eax,
            source: SmallSource::Register {
                register: Register32::Eax,
                width: SmallWidth::Word,
                high_byte: false,
            },
        }),
        Code::Cdq => Ok(Operation::SignExtendHigh),
        Code::Movzx_r32_rm8 | Code::Movsx_r32_rm8 | Code::Movzx_r32_rm16 | Code::Movsx_r32_rm16 => {
            let kind = match instruction.code() {
                Code::Movzx_r32_rm8 | Code::Movzx_r32_rm16 => ExtensionKind::Zero,
                _ => ExtensionKind::Sign,
            };
            let width = match instruction.code() {
                Code::Movzx_r32_rm8 | Code::Movsx_r32_rm8 => SmallWidth::Byte,
                _ => SmallWidth::Word,
            };
            Ok(Operation::Extend {
                kind,
                destination: register(instruction.op0_register())?,
                source: small_source(instruction, width)?,
            })
        }
        Code::Bswap_r32 => Ok(Operation::ByteSwap {
            destination: register(instruction.op0_register())?,
        }),
        Code::Xchg_rm32_r32 | Code::Xchg_r32_EAX => {
            if instruction.op0_kind() != OpKind::Register
                || instruction.op1_kind() != OpKind::Register
            {
                return Err(unsupported());
            }
            Ok(Operation::Exchange {
                left: register(instruction.op0_register())?,
                right: register(instruction.op1_register())?,
            })
        }
        Code::Xadd_rm32_r32 => {
            if instruction.op1_kind() != OpKind::Register {
                return Err(unsupported());
            }
            let source = register(instruction.op1_register())?;
            match instruction.op0_kind() {
                OpKind::Register => Ok(Operation::ExchangeAdd {
                    destination: register(instruction.op0_register())?,
                    source,
                }),
                OpKind::Memory => Ok(Operation::MemoryExchangeAdd {
                    address: effective_address(instruction)?,
                    source,
                }),
                _ => Err(unsupported()),
            }
        }
        Code::Xadd_rm8_r8 => {
            if instruction.op1_kind() != OpKind::Register {
                return Err(unsupported());
            }
            let source = byte_register(instruction.op1_register())?;
            match instruction.op0_kind() {
                OpKind::Register => Ok(Operation::ExchangeAddByte {
                    destination: byte_register(instruction.op0_register())?,
                    source,
                }),
                OpKind::Memory => Ok(Operation::MemoryExchangeAddByte {
                    address: effective_address(instruction)?,
                    source,
                }),
                _ => Err(unsupported()),
            }
        }
        Code::Cmpxchg_rm32_r32 => {
            if instruction.op1_kind() != OpKind::Register {
                return Err(unsupported());
            }
            let source = register(instruction.op1_register())?;
            match instruction.op0_kind() {
                OpKind::Register => Ok(Operation::CompareExchange {
                    destination: register(instruction.op0_register())?,
                    source,
                }),
                OpKind::Memory => Ok(Operation::MemoryCompareExchange {
                    address: effective_address(instruction)?,
                    source,
                }),
                _ => Err(unsupported()),
            }
        }
        Code::Cmpxchg_rm8_r8 => {
            if instruction.op1_kind() != OpKind::Register {
                return Err(unsupported());
            }
            let source = byte_register(instruction.op1_register())?;
            match instruction.op0_kind() {
                OpKind::Register => Ok(Operation::CompareExchangeByte {
                    destination: byte_register(instruction.op0_register())?,
                    source,
                }),
                OpKind::Memory => Ok(Operation::MemoryCompareExchangeByte {
                    address: effective_address(instruction)?,
                    source,
                }),
                _ => Err(unsupported()),
            }
        }
        Code::Xchg_rm8_r8 => {
            if instruction.op0_kind() != OpKind::Register
                || instruction.op1_kind() != OpKind::Register
            {
                return Err(unsupported());
            }
            Ok(Operation::ExchangeByte {
                left: byte_register(instruction.op0_register())?,
                right: byte_register(instruction.op1_register())?,
            })
        }
        Code::Lea_r32_m => Ok(Operation::Lea {
            destination: register(instruction.op0_register())?,
            address: effective_address(instruction)?,
        }),
        _ => super::integer::lower(instruction).unwrap_or_else(|| super::flow::lower(instruction)),
    }
}
