use iced_x86::{Code, Instruction, OpKind};

use super::{
    DecodeError,
    operands::{
        byte_register, byte_value, effective_address, location, register, small_source,
        unsupported, value, word_register, word_value,
    },
    profile::check_profile,
};
use crate::cpu::x86::{
    Register32,
    ir::{
        CarryKind, ExtensionKind, Operation, SmallSource, SmallWidth, WordArithmeticKind,
        WordLogicalKind, WordMemoryArithmeticKind, WordReadArithmeticKind,
    },
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
        Code::Movsw_m16_m16 => Ok(Operation::MoveStringWord),
        Code::Movsd_m32_m32 => Ok(Operation::MoveStringDword),
        Code::Lodsb_AL_m8 => Ok(Operation::LoadStringByte),
        Code::Lodsw_AX_m16 => Ok(Operation::LoadStringWord),
        Code::Lodsd_EAX_m32 => Ok(Operation::LoadStringDword),
        Code::Stosb_m8_AL => Ok(Operation::StoreStringByte),
        Code::Stosd_m32_EAX => Ok(Operation::StoreStringDword),
        Code::Stosw_m16_AX => Ok(Operation::StoreStringWord),
        Code::Cmpsb_m8_m8 => Ok(Operation::CompareStringByte),
        Code::Scasb_AL_m8 => Ok(Operation::ScanStringByte),
        Code::Cmpsw_m16_m16 => Ok(Operation::CompareStringWord),
        Code::Scasw_AX_m16 => Ok(Operation::ScanStringWord),
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
        Code::Mov_r16_rm16 if instruction.op1_kind() == OpKind::Memory => Ok(Operation::LoadWord {
            destination: word_register(instruction.op0_register())?,
            address: effective_address(instruction)?,
        }),
        Code::Mov_rm16_r16 | Code::Mov_rm16_imm16 if instruction.op0_kind() == OpKind::Memory => {
            Ok(Operation::StoreWord {
                address: effective_address(instruction)?,
                source: word_value(instruction)?,
            })
        }
        Code::Mov_rm16_r16 | Code::Mov_r16_rm16 | Code::Mov_r16_imm16 | Code::Mov_rm16_imm16 => {
            Ok(Operation::MoveWord {
                destination: word_register(instruction.op0_register())?,
                source: word_value(instruction)?,
            })
        }
        Code::Add_r16_rm16 if instruction.op1_kind() == OpKind::Memory => {
            Ok(Operation::ReadArithmeticWord {
                kind: WordReadArithmeticKind::Add,
                destination: word_register(instruction.op0_register())?,
                address: effective_address(instruction)?,
            })
        }
        Code::Sub_r16_rm16 if instruction.op1_kind() == OpKind::Memory => {
            Ok(Operation::ReadArithmeticWord {
                kind: WordReadArithmeticKind::Sub,
                destination: word_register(instruction.op0_register())?,
                address: effective_address(instruction)?,
            })
        }
        Code::Add_rm16_r16 | Code::Add_rm16_imm16 | Code::Add_rm16_imm8
            if instruction.op0_kind() == OpKind::Memory =>
        {
            Ok(Operation::MemoryArithmeticWord {
                kind: WordMemoryArithmeticKind::Add,
                address: effective_address(instruction)?,
                source: word_value(instruction)?,
            })
        }
        Code::Sub_rm16_r16 | Code::Sub_rm16_imm16 | Code::Sub_rm16_imm8
            if instruction.op0_kind() == OpKind::Memory =>
        {
            Ok(Operation::MemoryArithmeticWord {
                kind: WordMemoryArithmeticKind::Sub,
                address: effective_address(instruction)?,
                source: word_value(instruction)?,
            })
        }
        Code::Add_rm16_r16
        | Code::Add_r16_rm16
        | Code::Add_AX_imm16
        | Code::Add_rm16_imm16
        | Code::Add_rm16_imm8 => Ok(Operation::ArithmeticWord {
            kind: WordArithmeticKind::Add,
            destination: word_register(instruction.op0_register())?,
            source: word_value(instruction)?,
        }),
        Code::Sub_rm16_r16
        | Code::Sub_r16_rm16
        | Code::Sub_AX_imm16
        | Code::Sub_rm16_imm16
        | Code::Sub_rm16_imm8 => Ok(Operation::ArithmeticWord {
            kind: WordArithmeticKind::Sub,
            destination: word_register(instruction.op0_register())?,
            source: word_value(instruction)?,
        }),
        Code::Adc_r16_rm16 if instruction.op1_kind() == OpKind::Memory => {
            Ok(Operation::ReadArithmeticWord {
                kind: WordReadArithmeticKind::Adc,
                destination: word_register(instruction.op0_register())?,
                address: effective_address(instruction)?,
            })
        }
        Code::Adc_rm16_r16 | Code::Adc_rm16_imm16 | Code::Adc_rm16_imm8
            if instruction.op0_kind() == OpKind::Memory =>
        {
            Ok(Operation::MemoryArithmeticWord {
                kind: WordMemoryArithmeticKind::Adc,
                address: effective_address(instruction)?,
                source: word_value(instruction)?,
            })
        }
        Code::Adc_rm16_r16
        | Code::Adc_r16_rm16
        | Code::Adc_AX_imm16
        | Code::Adc_rm16_imm16
        | Code::Adc_rm16_imm8 => Ok(Operation::ArithmeticWord {
            kind: WordArithmeticKind::Adc,
            destination: word_register(instruction.op0_register())?,
            source: word_value(instruction)?,
        }),
        Code::Sbb_r16_rm16 if instruction.op1_kind() == OpKind::Memory => {
            Ok(Operation::ReadArithmeticWord {
                kind: WordReadArithmeticKind::Sbb,
                destination: word_register(instruction.op0_register())?,
                address: effective_address(instruction)?,
            })
        }
        Code::Sbb_rm16_r16 | Code::Sbb_rm16_imm16 | Code::Sbb_rm16_imm8
            if instruction.op0_kind() == OpKind::Memory =>
        {
            Ok(Operation::MemoryArithmeticWord {
                kind: WordMemoryArithmeticKind::Sbb,
                address: effective_address(instruction)?,
                source: word_value(instruction)?,
            })
        }
        Code::Sbb_rm16_r16
        | Code::Sbb_r16_rm16
        | Code::Sbb_AX_imm16
        | Code::Sbb_rm16_imm16
        | Code::Sbb_rm16_imm8 => Ok(Operation::ArithmeticWord {
            kind: WordArithmeticKind::Sbb,
            destination: word_register(instruction.op0_register())?,
            source: word_value(instruction)?,
        }),
        Code::And_r16_rm16 if instruction.op1_kind() == OpKind::Memory => {
            Ok(Operation::ReadLogicalWord {
                kind: WordLogicalKind::And,
                destination: word_register(instruction.op0_register())?,
                address: effective_address(instruction)?,
            })
        }
        Code::And_rm16_r16 | Code::And_rm16_imm16 | Code::And_rm16_imm8
            if instruction.op0_kind() == OpKind::Memory =>
        {
            Ok(Operation::MemoryLogicalWord {
                kind: WordLogicalKind::And,
                address: effective_address(instruction)?,
                source: word_value(instruction)?,
            })
        }
        Code::And_rm16_r16
        | Code::And_r16_rm16
        | Code::And_AX_imm16
        | Code::And_rm16_imm16
        | Code::And_rm16_imm8 => Ok(Operation::LogicalWord {
            kind: WordLogicalKind::And,
            destination: word_register(instruction.op0_register())?,
            source: word_value(instruction)?,
        }),
        Code::Or_r16_rm16 if instruction.op1_kind() == OpKind::Memory => {
            Ok(Operation::ReadLogicalWord {
                kind: WordLogicalKind::Or,
                destination: word_register(instruction.op0_register())?,
                address: effective_address(instruction)?,
            })
        }
        Code::Or_rm16_r16 | Code::Or_rm16_imm16 | Code::Or_rm16_imm8
            if instruction.op0_kind() == OpKind::Memory =>
        {
            Ok(Operation::MemoryLogicalWord {
                kind: WordLogicalKind::Or,
                address: effective_address(instruction)?,
                source: word_value(instruction)?,
            })
        }
        Code::Or_rm16_r16
        | Code::Or_r16_rm16
        | Code::Or_AX_imm16
        | Code::Or_rm16_imm16
        | Code::Or_rm16_imm8 => Ok(Operation::LogicalWord {
            kind: WordLogicalKind::Or,
            destination: word_register(instruction.op0_register())?,
            source: word_value(instruction)?,
        }),
        Code::Xor_r16_rm16 if instruction.op1_kind() == OpKind::Memory => {
            Ok(Operation::ReadLogicalWord {
                kind: WordLogicalKind::Xor,
                destination: word_register(instruction.op0_register())?,
                address: effective_address(instruction)?,
            })
        }
        Code::Xor_rm16_r16 | Code::Xor_rm16_imm16 | Code::Xor_rm16_imm8
            if instruction.op0_kind() == OpKind::Memory =>
        {
            Ok(Operation::MemoryLogicalWord {
                kind: WordLogicalKind::Xor,
                address: effective_address(instruction)?,
                source: word_value(instruction)?,
            })
        }
        Code::Xor_rm16_r16
        | Code::Xor_r16_rm16
        | Code::Xor_AX_imm16
        | Code::Xor_rm16_imm16
        | Code::Xor_rm16_imm8 => Ok(Operation::LogicalWord {
            kind: WordLogicalKind::Xor,
            destination: word_register(instruction.op0_register())?,
            source: word_value(instruction)?,
        }),
        Code::Cmp_rm16_r16
        | Code::Cmp_r16_rm16
        | Code::Cmp_AX_imm16
        | Code::Cmp_rm16_imm16
        | Code::Cmp_rm16_imm8 => {
            if instruction.op0_kind() == OpKind::Memory {
                Ok(Operation::MemoryCompareWord {
                    address: effective_address(instruction)?,
                    right: word_value(instruction)?,
                })
            } else if instruction.op1_kind() == OpKind::Memory {
                Ok(Operation::ReadCompareWord {
                    left: word_register(instruction.op0_register())?,
                    address: effective_address(instruction)?,
                })
            } else {
                Ok(Operation::CompareWord {
                    left: word_register(instruction.op0_register())?,
                    right: word_value(instruction)?,
                })
            }
        }
        Code::Test_rm16_r16 | Code::Test_AX_imm16 | Code::Test_rm16_imm16 => {
            if instruction.op0_kind() == OpKind::Memory {
                Ok(Operation::MemoryTestWord {
                    address: effective_address(instruction)?,
                    right: word_value(instruction)?,
                })
            } else {
                Ok(Operation::TestWord {
                    left: word_register(instruction.op0_register())?,
                    right: word_value(instruction)?,
                })
            }
        }
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
        Code::Xadd_rm16_r16 => {
            let source = word_register(instruction.op1_register())?;
            match instruction.op0_kind() {
                OpKind::Register => Ok(Operation::ExchangeAddWord {
                    destination: word_register(instruction.op0_register())?,
                    source,
                }),
                OpKind::Memory => Ok(Operation::MemoryExchangeAddWord {
                    address: effective_address(instruction)?,
                    source,
                }),
                _ => Err(unsupported()),
            }
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
        Code::Cmpxchg_rm16_r16 => Ok(Operation::CompareExchangeWord {
            destination: word_register(instruction.op0_register())?,
            source: word_register(instruction.op1_register())?,
        }),
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
