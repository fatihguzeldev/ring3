#![allow(dead_code)]
// prefix migration checks keep the original typed operand width and non-address fields.
// iced supplies the independent instruction boundary and address metadata; the shared
// admission helper also exercises both bound compilation entry points.
use super::support_prefix_admission as prefix_admission;
use iced_x86::{Decoder, DecoderOptions, Instruction, OpKind, Register};
use ring3_engine::{
    cpu::x86::{
        Register32, SegmentRegister,
        decode::decode_one,
        ir::{
            AddressSize, BitIndex, BranchTarget, EffectiveAddress, Operation, ShiftCount, WordValue,
        },
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};

pub fn admitted(bytes: &[u8]) -> (Operation, usize) {
    let (actual, length) = prefix_admission::admitted(bytes);
    let instruction = Decoder::with_ip(32, bytes, 0x1000, DecoderOptions::NONE).decode();
    assert!(!instruction.is_invalid(), "{bytes:02x?}");
    assert_eq!(length, instruction.len(), "{bytes:02x?}");
    let mut canonical = Vec::new();
    let mut prefix = true;
    for &byte in bytes {
        let leading = prefix
            && matches!(
                byte,
                0x66 | 0x67 | 0xf0 | 0xf2 | 0xf3 | 0x26 | 0x2e | 0x36 | 0x3e | 0x64 | 0x65
            );
        prefix = leading;
        if leading && matches!(byte, 0x67 | 0x26 | 0x2e | 0x36 | 0x3e | 0x64 | 0x65) {
            continue;
        }
        canonical.push(byte);
    }
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(0x1000), &canonical).unwrap();
    let mut expected = *decode_one(&memory, GuestAddress(0x1000))
        .unwrap()
        .operation();
    if let Some(address) = address_mut(&mut expected) {
        let address16 = bytes
            .iter()
            .take_while(|b| {
                matches!(
                    **b,
                    0x66 | 0x67 | 0xf0 | 0xf2 | 0xf3 | 0x26 | 0x2e | 0x36 | 0x3e | 0x64 | 0x65
                )
            })
            .any(|b| *b == 0x67);
        *address = EffectiveAddress {
            base: parent(instruction.memory_base()),
            index: parent(instruction.memory_index()),
            scale: instruction.memory_index_scale() as u8,
            displacement: instruction.memory_displacement32(),
            address_size: if address16 {
                AddressSize::Bits16
            } else {
                AddressSize::Bits32
            },
            segment: match instruction.segment_prefix() {
                Register::ES => Some(SegmentRegister::Es),
                Register::CS => Some(SegmentRegister::Cs),
                Register::SS => Some(SegmentRegister::Ss),
                Register::DS => Some(SegmentRegister::Ds),
                Register::FS => Some(SegmentRegister::Fs),
                Register::GS => Some(SegmentRegister::Gs),
                Register::None => None,
                other => panic!("unexpected segment {other:?}"),
            },
        };
        // a former SIB/displacement byte can become an immediate under address16.
        patch_immediate(&mut expected, &instruction);
    }
    match &mut expected {
        Operation::Jump {
            target: BranchTarget::Direct(target),
        }
        | Operation::ConditionalJump { target, .. }
        | Operation::CallWord { target } => *target = GuestAddress(instruction.near_branch32()),
        _ => {}
    }
    assert_eq!(actual, expected, "{bytes:02x?}");
    (actual, length)
}

fn parent(register: Register) -> Option<Register32> {
    Some(match register {
        Register::None => return None,
        Register::EAX | Register::AX => Register32::Eax,
        Register::ECX | Register::CX => Register32::Ecx,
        Register::EDX | Register::DX => Register32::Edx,
        Register::EBX | Register::BX => Register32::Ebx,
        Register::ESP | Register::SP => Register32::Esp,
        Register::EBP | Register::BP => Register32::Ebp,
        Register::ESI | Register::SI => Register32::Esi,
        Register::EDI | Register::DI => Register32::Edi,
        other => panic!("unexpected address register {other:?}"),
    })
}

fn patch_immediate(operation: &mut Operation, instruction: &Instruction) {
    let immediate = (0..instruction.op_count()).find(|&i| {
        matches!(
            instruction.op_kind(i),
            OpKind::Immediate8
                | OpKind::Immediate16
                | OpKind::Immediate8to16
                | OpKind::Immediate32
                | OpKind::Immediate8to32
        )
    });
    let Some(index) = immediate else {
        return;
    };
    let value = instruction.immediate(index);
    match operation {
        Operation::MemoryArithmeticWord {
            source: WordValue::Immediate(old),
            ..
        }
        | Operation::MemoryLogicalWord {
            source: WordValue::Immediate(old),
            ..
        }
        | Operation::MemoryCompareWord {
            right: WordValue::Immediate(old),
            ..
        }
        | Operation::MemoryTestWord {
            right: WordValue::Immediate(old),
            ..
        }
        | Operation::StoreWord {
            source: WordValue::Immediate(old),
            ..
        }
        | Operation::ReadSignedMultiplyImmediateWord { immediate: old, .. } => *old = value as u16,
        Operation::MemoryShiftWord {
            count: ShiftCount::Immediate(old),
            ..
        }
        | Operation::MemoryRotateWord {
            count: ShiftCount::Immediate(old),
            ..
        }
        | Operation::MemoryRotateThroughCarryWord {
            count: ShiftCount::Immediate(old),
            ..
        }
        | Operation::MemoryDoubleShiftWord {
            count: ShiftCount::Immediate(old),
            ..
        }
        | Operation::ReadBitTestWord {
            index: BitIndex::Immediate(old),
            ..
        }
        | Operation::MemoryBitMutationWord {
            index: BitIndex::Immediate(old),
            ..
        } => *old = value as u8,
        _ => {}
    }
}

fn address_mut(operation: &mut Operation) -> Option<&mut EffectiveAddress> {
    match operation {
        Operation::AtomicMemory { address, .. }
        | Operation::ReadArithmeticWord { address, .. }
        | Operation::MemoryArithmeticWord { address, .. }
        | Operation::ReadLogicalWord { address, .. }
        | Operation::MemoryLogicalWord { address, .. }
        | Operation::MemoryCompareWord { address, .. }
        | Operation::ReadCompareWord { address, .. }
        | Operation::MemoryTestWord { address, .. }
        | Operation::ReadConditionalMoveWord { address, .. }
        | Operation::LoadWord { address, .. }
        | Operation::StoreWord { address, .. }
        | Operation::ReadExtendByteToWord { address, .. }
        | Operation::MemoryExchangeWord { address, .. }
        | Operation::MemoryExchangeAddWord { address, .. }
        | Operation::LockedMemoryExchangeAddWord { address, .. }
        | Operation::MemoryCompareExchangeWord { address, .. }
        | Operation::LockedMemoryCompareExchangeWord { address, .. }
        | Operation::LeaWord { address, .. }
        | Operation::MemoryUnaryWord { address, .. }
        | Operation::MemoryDoubleShiftWord { address, .. }
        | Operation::ReadSignedMultiplyWord { address, .. }
        | Operation::ReadSignedMultiplyImmediateWord { address, .. }
        | Operation::ReadWordMultiplyAccumulator { address, .. }
        | Operation::ReadWordDivideAccumulator { address, .. }
        | Operation::ReadBitTestWord { address, .. }
        | Operation::MemoryBitMutationWord { address, .. }
        | Operation::ReadBitScanWord { address, .. }
        | Operation::ReadJumpWord { address, .. }
        | Operation::ReadCallWord { address, .. }
        | Operation::MemoryShiftWord { address, .. }
        | Operation::MemoryRotateWord { address, .. }
        | Operation::MemoryRotateThroughCarryWord { address, .. } => Some(address),
        Operation::PushMemoryWord { source } => Some(source),
        Operation::PopMemoryWord { destination } => Some(destination),
        _ => None,
    }
}

pub fn admit_if_new_prefix(bytes: &[u8]) -> bool {
    let mut changed = false;
    let mut word_prefixes = 0;
    for &byte in bytes {
        match byte {
            0x26 | 0x2e | 0x36 | 0x3e | 0x64 | 0x65 | 0x67 => changed = true,
            0x66 => word_prefixes += 1,
            0xf0 | 0xf2 | 0xf3 => return prefix_admission::admit_if_new_prefix(bytes),
            _ => break,
        }
    }
    if !changed || word_prefixes > 1 {
        return prefix_admission::admit_if_new_prefix(bytes);
    }
    admitted(bytes);
    true
}
