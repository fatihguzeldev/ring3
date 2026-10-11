use iced_x86::{Code, Instruction, Register};

use super::{DecodeError, operands::unsupported};
use crate::cpu::x86::{
    SegmentRegister,
    ir::{AddressSize, Operation, RepeatCondition, StringKind, StringWidth},
};

pub(super) fn lower(
    instruction: &Instruction,
    bytes: &[u8],
) -> Option<Result<Operation, DecodeError>> {
    let (kind, width) = match instruction.code() {
        Code::Movsb_m8_m8 => (StringKind::Move, StringWidth::Byte),
        Code::Movsw_m16_m16 => (StringKind::Move, StringWidth::Word),
        Code::Movsd_m32_m32 => (StringKind::Move, StringWidth::Dword),
        Code::Stosb_m8_AL => (StringKind::Store, StringWidth::Byte),
        Code::Stosw_m16_AX => (StringKind::Store, StringWidth::Word),
        Code::Stosd_m32_EAX => (StringKind::Store, StringWidth::Dword),
        Code::Lodsb_AL_m8 => (StringKind::Load, StringWidth::Byte),
        Code::Lodsw_AX_m16 => (StringKind::Load, StringWidth::Word),
        Code::Lodsd_EAX_m32 => (StringKind::Load, StringWidth::Dword),
        Code::Cmpsb_m8_m8 => (StringKind::Compare, StringWidth::Byte),
        Code::Cmpsw_m16_m16 => (StringKind::Compare, StringWidth::Word),
        Code::Cmpsd_m32_m32 => (StringKind::Compare, StringWidth::Dword),
        Code::Scasb_AL_m8 => (StringKind::Scan, StringWidth::Byte),
        Code::Scasw_AX_m16 => (StringKind::Scan, StringWidth::Word),
        Code::Scasd_EAX_m32 => (StringKind::Scan, StringWidth::Dword),
        _ => return None,
    };
    if instruction.has_lock_prefix() {
        return Some(Err(DecodeError::InvalidEncoding));
    }
    let comparison = matches!(kind, StringKind::Compare | StringKind::Scan);
    let repeat = if instruction.has_repne_prefix() && comparison {
        RepeatCondition::NotEqual
    } else if comparison {
        RepeatCondition::Equal
    } else {
        RepeatCondition::Always
    };
    let source_segment = match instruction.segment_prefix() {
        Register::None | Register::DS => None,
        Register::ES => Some(SegmentRegister::Es),
        Register::CS => Some(SegmentRegister::Cs),
        Register::SS => Some(SegmentRegister::Ss),
        Register::FS => Some(SegmentRegister::Fs),
        Register::GS => Some(SegmentRegister::Gs),
        _ => return Some(Err(unsupported())),
    };
    let address_size = if bytes
        .iter()
        .take_while(|byte| {
            matches!(
                **byte,
                0xf2 | 0xf3 | 0x66 | 0x67 | 0x26 | 0x2e | 0x36 | 0x3e | 0x64 | 0x65
            )
        })
        .any(|byte| *byte == 0x67)
    {
        AddressSize::Bits16
    } else {
        AddressSize::Bits32
    };
    if !instruction.has_rep_prefix() && !instruction.has_repne_prefix() {
        return Some(Ok(Operation::String {
            kind,
            width,
            address_size,
            source_segment,
        }));
    }
    Some(Ok(Operation::RepeatString {
        kind,
        width,
        repeat,
        address_size,
        source_segment,
    }))
}
