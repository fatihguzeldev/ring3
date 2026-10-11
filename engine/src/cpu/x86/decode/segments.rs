use iced_x86::{Code, Instruction, OpKind, Register};

use super::{
    DecodeError,
    operands::{effective_address, register, unsupported, word_register},
};
use crate::cpu::x86::{
    SegmentRegister,
    ir::{Location32, Operation, Value32},
};

pub(super) fn lower(instruction: &Instruction) -> Option<Result<Operation, DecodeError>> {
    match instruction.code() {
        Code::Mov_rm16_Sreg | Code::Mov_r32m16_Sreg => {
            if instruction.has_lock_prefix() {
                return Some(Err(DecodeError::InvalidEncoding));
            }
            let segment = match segment(instruction.op1_register()) {
                Ok(segment) => segment,
                Err(error) => return Some(Err(error)),
            };
            let word = instruction.code() == Code::Mov_rm16_Sreg
                || instruction.op0_kind() == OpKind::Memory;
            let destination = match instruction.op0_kind() {
                OpKind::Register => {
                    let destination = if word {
                        word_register(instruction.op0_register())
                    } else {
                        register(instruction.op0_register())
                    };
                    destination.map(Location32::Register)
                }
                OpKind::Memory => effective_address(instruction).map(Location32::Memory),
                _ => Err(unsupported()),
            };
            Some(destination.map(|destination| Operation::ReadSegment {
                segment,
                destination,
                word,
            }))
        }
        Code::Mov_Sreg_rm16 | Code::Mov_Sreg_r32m16 => {
            if instruction.has_lock_prefix() {
                return Some(Err(DecodeError::InvalidEncoding));
            }
            let target = match segment(instruction.op0_register()) {
                Ok(SegmentRegister::Cs) => return Some(Err(DecodeError::InvalidEncoding)),
                Ok(segment) => segment,
                Err(error) => return Some(Err(error)),
            };
            let source = match instruction.op1_kind() {
                OpKind::Register => {
                    let source = if instruction.code() == Code::Mov_Sreg_rm16 {
                        word_register(instruction.op1_register())
                    } else {
                        register(instruction.op1_register())
                    };
                    source.map(Value32::Register)
                }
                OpKind::Memory => effective_address(instruction).map(Value32::Memory),
                _ => Err(unsupported()),
            };
            Some(source.map(|source| Operation::LoadSegment {
                segment: target,
                source,
            }))
        }
        Code::Lds_r16_m1616
        | Code::Lds_r32_m1632
        | Code::Les_r16_m1616
        | Code::Les_r32_m1632
        | Code::Lss_r16_m1616
        | Code::Lss_r32_m1632
        | Code::Lfs_r16_m1616
        | Code::Lfs_r32_m1632
        | Code::Lgs_r16_m1616
        | Code::Lgs_r32_m1632 => {
            if instruction.has_lock_prefix() {
                return Some(Err(DecodeError::InvalidEncoding));
            }
            let (segment, word) = match instruction.code() {
                Code::Lds_r16_m1616 => (SegmentRegister::Ds, true),
                Code::Lds_r32_m1632 => (SegmentRegister::Ds, false),
                Code::Les_r16_m1616 => (SegmentRegister::Es, true),
                Code::Les_r32_m1632 => (SegmentRegister::Es, false),
                Code::Lss_r16_m1616 => (SegmentRegister::Ss, true),
                Code::Lss_r32_m1632 => (SegmentRegister::Ss, false),
                Code::Lfs_r16_m1616 => (SegmentRegister::Fs, true),
                Code::Lfs_r32_m1632 => (SegmentRegister::Fs, false),
                Code::Lgs_r16_m1616 => (SegmentRegister::Gs, true),
                Code::Lgs_r32_m1632 => (SegmentRegister::Gs, false),
                _ => unreachable!(),
            };
            let destination = if word {
                word_register(instruction.op0_register())
            } else {
                register(instruction.op0_register())
            };
            Some(destination.and_then(|destination| {
                effective_address(instruction).map(|address| Operation::LoadFarPointer {
                    segment,
                    destination,
                    address,
                    word,
                })
            }))
        }
        code => {
            let pop = match code {
                Code::Popw_ES => Some((SegmentRegister::Es, true)),
                Code::Popd_ES => Some((SegmentRegister::Es, false)),
                Code::Popw_SS => Some((SegmentRegister::Ss, true)),
                Code::Popd_SS => Some((SegmentRegister::Ss, false)),
                Code::Popw_DS => Some((SegmentRegister::Ds, true)),
                Code::Popd_DS => Some((SegmentRegister::Ds, false)),
                Code::Popw_FS => Some((SegmentRegister::Fs, true)),
                Code::Popd_FS => Some((SegmentRegister::Fs, false)),
                Code::Popw_GS => Some((SegmentRegister::Gs, true)),
                Code::Popd_GS => Some((SegmentRegister::Gs, false)),
                _ => None,
            };
            if let Some((segment, word)) = pop {
                if instruction.has_lock_prefix() {
                    return Some(Err(DecodeError::InvalidEncoding));
                }
                return Some(Ok(Operation::PopSegment { segment, word }));
            }
            let (segment, word) = match code {
                Code::Pushw_ES => (SegmentRegister::Es, true),
                Code::Pushd_ES => (SegmentRegister::Es, false),
                Code::Pushw_CS => (SegmentRegister::Cs, true),
                Code::Pushd_CS => (SegmentRegister::Cs, false),
                Code::Pushw_SS => (SegmentRegister::Ss, true),
                Code::Pushd_SS => (SegmentRegister::Ss, false),
                Code::Pushw_DS => (SegmentRegister::Ds, true),
                Code::Pushd_DS => (SegmentRegister::Ds, false),
                Code::Pushw_FS => (SegmentRegister::Fs, true),
                Code::Pushd_FS => (SegmentRegister::Fs, false),
                Code::Pushw_GS => (SegmentRegister::Gs, true),
                Code::Pushd_GS => (SegmentRegister::Gs, false),
                _ => return None,
            };
            if instruction.has_lock_prefix() {
                return Some(Err(DecodeError::InvalidEncoding));
            }
            Some(Ok(Operation::PushSegment { segment, word }))
        }
    }
}

fn segment(register: Register) -> Result<SegmentRegister, DecodeError> {
    match register {
        Register::ES => Ok(SegmentRegister::Es),
        Register::CS => Ok(SegmentRegister::Cs),
        Register::SS => Ok(SegmentRegister::Ss),
        Register::DS => Ok(SegmentRegister::Ds),
        Register::FS => Ok(SegmentRegister::Fs),
        Register::GS => Ok(SegmentRegister::Gs),
        _ => Err(unsupported()),
    }
}
