#![allow(dead_code)]
use ring3_engine::{
    abi::arena::TRANSFER_OFFSET,
    cpu::x86::{decode::decode_one, ir::Operation},
    memory::GuestAddress,
    process::EngineInstance,
};

// legacy prefix rows validate the instruction actually consumed in address16 mode.
pub fn admitted(instruction: &[u8]) -> (Operation, usize) {
    let mut engine = EngineInstance::new(1, 0x5052_4546_4958).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + instruction.len()]
        .copy_from_slice(instruction);
    engine.upload(0x1000, instruction.len() as u32).unwrap();
    engine.protect(0x1000, 1, 4).unwrap();
    let decoded = decode_one(engine.memory().unwrap(), GuestAddress(0x1000)).unwrap();
    let operation = *decoded.operation();
    let length = usize::from(decoded.length());
    assert!(
        length > 0 && length <= instruction.len(),
        "{instruction:02x?}"
    );
    assert_eq!(decoded.next_pc(), GuestAddress(0x1000 + length as u32));
    assert!(
        engine
            .memory()
            .unwrap()
            .is_code_current(decoded.code_snapshot())
    );
    let terminal = matches!(
        operation,
        Operation::Jump { .. }
            | Operation::JumpWord { .. }
            | Operation::ReadJumpWord { .. }
            | Operation::ConditionalJump { .. }
            | Operation::CountBranch { .. }
            | Operation::CountBranchWord { .. }
            | Operation::Call { .. }
            | Operation::CallWord { .. }
            | Operation::CallRegisterWord { .. }
            | Operation::ReadCallWord { .. }
            | Operation::Return { .. }
            | Operation::ReturnWord { .. }
    );
    let mut bytes = instruction[..length].to_vec();
    if !terminal {
        bytes.extend_from_slice(&[0xeb, 0]);
    }
    engine.protect(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(&bytes);
    engine.upload(0x1000, bytes.len() as u32).unwrap();
    engine.protect(0x1000, 1, 4).unwrap();
    let snapshot = engine
        .memory()
        .unwrap()
        .snapshot_code(GuestAddress(0x1000), bytes.len())
        .unwrap();
    for entries in [false, true]
        .into_iter()
        .filter(|entries| !terminal || !entries)
    {
        let transfer = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
        transfer[..4].copy_from_slice(&0x1000_u32.to_le_bytes());
        transfer[4..].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
        let before = engine.arena().to_vec();
        if entries {
            engine.compile_entries(1, 0).unwrap();
        } else {
            engine.compile(1).unwrap();
        }
        assert_eq!(engine.arena(), before);
        assert!(engine.memory().unwrap().is_code_current(&snapshot));
        assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
        assert!(
            engine
                .memory()
                .unwrap()
                .resolve(GuestAddress(0x1000), ring3_engine::memory::Access::Read)
                .is_err()
        );
    }
    (operation, length)
}

// only known newly legal prefix families select this migration path.
pub fn admit_if_new_prefix(bytes: &[u8]) -> bool {
    let mut changed = false;
    let mut counts = [0_u8; 4];
    let mut opcode_offset = 0;
    for (offset, &byte) in bytes.iter().enumerate() {
        match byte {
            0x26 | 0x2e | 0x36 | 0x3e | 0x64 | 0x65 | 0x67 => changed = true,
            0x66 => counts[0] += 1,
            0xf0 => counts[1] += 1,
            0xf2 => counts[2] += 1,
            0xf3 => counts[3] += 1,
            _ => {
                opcode_offset = offset;
                break;
            }
        }
    }
    if counts.iter().any(|count| *count > 1) || counts[2] + counts[3] > 1 {
        return false;
    }
    let op = &bytes[opcode_offset..];
    let atomic_memory = counts[1] == 1
        && counts[2] + counts[3] == 0
        && match op {
            [
                opcode @ (0x00 | 0x01 | 0x08 | 0x09 | 0x10 | 0x11 | 0x18 | 0x19 | 0x20 | 0x21
                | 0x28 | 0x29 | 0x30 | 0x31),
                modrm,
                ..,
            ] => {
                let _ = opcode;
                modrm >> 6 != 3
            }
            [0x80 | 0x81 | 0x83, modrm, ..] => modrm >> 6 != 3 && (modrm >> 3 & 7) != 7,
            [0xfe | 0xff, modrm, ..] => modrm >> 6 != 3 && (modrm >> 3 & 7) <= 1,
            [0xf6 | 0xf7, modrm, ..] => modrm >> 6 != 3 && matches!(modrm >> 3 & 7, 2 | 3),
            [0x0f, 0xab | 0xb3 | 0xbb, modrm, ..] => modrm >> 6 != 3,
            [0x0f, 0xba, modrm, ..] => modrm >> 6 != 3 && matches!(modrm >> 3 & 7, 5 | 6 | 7),
            _ => false,
        };
    let locked_exchange = counts[1] == 1
        && counts[2] + counts[3] == 0
        && match op {
            [0x86, modrm, ..] => counts[0] == 0 && modrm >> 6 != 3,
            [0x87, modrm, ..] => modrm >> 6 != 3,
            [0x0f, 0xb1 | 0xc1, modrm, ..] => counts[0] == 1 && modrm >> 6 != 3,
            _ => false,
        };
    if !changed && !atomic_memory && !locked_exchange {
        return false;
    }
    admitted(bytes);
    true
}

// keep late-refusal owner checks after a former prefix poison became legal.
pub fn refusal_after_prefix_admission(
    bytes: Vec<u8>,
    cause: ring3_engine::cpu::x86::decode::DecodeError,
) -> (Vec<u8>, ring3_engine::cpu::x86::decode::DecodeError) {
    if !admit_if_new_prefix(&bytes) {
        return (bytes, cause);
    }
    assert!(bytes.len() >= 2);
    let mut poison = vec![0x90; bytes.len()];
    poison[..2].copy_from_slice(&[0x0f, 0x0b]);
    (
        poison,
        ring3_engine::cpu::x86::decode::DecodeError::Unsupported(
            ring3_engine::cpu::UnsupportedFeature::Opcode,
        ),
    )
}
