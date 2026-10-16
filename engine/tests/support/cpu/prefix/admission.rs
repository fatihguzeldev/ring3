#![allow(dead_code)]
use iced_x86::{Code, Decoder, DecoderOptions};
use ring3_engine::{
    abi::arena::TRANSFER_OFFSET,
    cpu::x86::{decode::decode_one, ir::Operation},
    memory::GuestAddress,
    process::EngineInstance,
};

pub const MALFORMED_BYTES: [u8; 2] = [0xf0, 0x90];
pub const MALFORMED_ERROR: ring3_engine::cpu::x86::decode::DecodeError =
    ring3_engine::cpu::x86::decode::DecodeError::InvalidEncoding;

// LOCK NOP is malformed independently of which valid instructions are implemented.
pub fn malformed_fixture(length: usize) -> (Vec<u8>, ring3_engine::cpu::x86::decode::DecodeError) {
    assert!(length >= MALFORMED_BYTES.len());
    let mut bytes = vec![0x90; length];
    bytes[..MALFORMED_BYTES.len()].copy_from_slice(&MALFORMED_BYTES);
    (bytes, MALFORMED_ERROR)
}

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
    let operand = bytes.strip_prefix(&[0x66]).unwrap_or(bytes);
    let legacy_alias = match operand {
        [0x82, _, ..] => true,
        [0xd0 | 0xd1 | 0xd2 | 0xd3 | 0xc0 | 0xc1, modrm, ..] => modrm >> 3 & 7 == 6,
        _ => false,
    };
    if legacy_alias || matches!(bytes, [0x66, 0x0f, 0xb7 | 0xbf, _, ..]) {
        admitted(bytes);
        return true;
    }
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
            [0x0f, 0xba, modrm, ..] => modrm >> 6 != 3 && matches!(modrm >> 3 & 7, 5..=7),
            _ => false,
        };
    let locked_exchange = counts[1] == 1
        && counts[2] + counts[3] == 0
        && match op {
            [0x86, modrm, ..] => modrm >> 6 != 3,
            [0x0f, 0xb0 | 0xc0, modrm, ..] => modrm >> 6 != 3,
            [0x87, modrm, ..] => modrm >> 6 != 3,
            [0x0f, 0xb1 | 0xc1, modrm, ..] => counts[0] == 1 && modrm >> 6 != 3,
            _ => false,
        };
    let instruction = Decoder::with_ip(32, bytes, 0x1000, DecoderOptions::NONE).decode();
    let hle_hint = instruction.has_xacquire_prefix() || instruction.has_xrelease_prefix();
    let scalar_count = matches!(
        instruction.code(),
        Code::Tzcnt_r16_rm16
            | Code::Lzcnt_r16_rm16
            | Code::Lzcnt_r32_rm32
            | Code::Popcnt_r16_rm16
            | Code::Popcnt_r32_rm32
    );
    let ignored_byte_width = counts[0] == 1
        && counts[1] + counts[2] + counts[3] == 0
        && matches!(
            instruction.code(),
            Code::Mov_rm8_r8
                | Code::Mov_r8_rm8
                | Code::Mov_r8_imm8
                | Code::Mov_rm8_imm8
                | Code::Mov_AL_moffs8
                | Code::Mov_moffs8_AL
                | Code::Add_rm8_r8
                | Code::Add_r8_rm8
                | Code::Add_AL_imm8
                | Code::Add_rm8_imm8
                | Code::Adc_rm8_r8
                | Code::Adc_r8_rm8
                | Code::Adc_AL_imm8
                | Code::Adc_rm8_imm8
                | Code::Sub_rm8_r8
                | Code::Sub_r8_rm8
                | Code::Sub_AL_imm8
                | Code::Sub_rm8_imm8
                | Code::Sbb_rm8_r8
                | Code::Sbb_r8_rm8
                | Code::Sbb_AL_imm8
                | Code::Sbb_rm8_imm8
                | Code::And_rm8_r8
                | Code::And_r8_rm8
                | Code::And_AL_imm8
                | Code::And_rm8_imm8
                | Code::Or_rm8_r8
                | Code::Or_r8_rm8
                | Code::Or_AL_imm8
                | Code::Or_rm8_imm8
                | Code::Xor_rm8_r8
                | Code::Xor_r8_rm8
                | Code::Xor_AL_imm8
                | Code::Xor_rm8_imm8
                | Code::Cmp_rm8_r8
                | Code::Cmp_r8_rm8
                | Code::Cmp_AL_imm8
                | Code::Cmp_rm8_imm8
                | Code::Test_rm8_r8
                | Code::Test_AL_imm8
                | Code::Test_rm8_imm8
                | Code::Inc_rm8
                | Code::Dec_rm8
                | Code::Neg_rm8
                | Code::Not_rm8
                | Code::Shl_rm8_1
                | Code::Shl_rm8_CL
                | Code::Shl_rm8_imm8
                | Code::Shr_rm8_1
                | Code::Shr_rm8_CL
                | Code::Shr_rm8_imm8
                | Code::Sar_rm8_1
                | Code::Sar_rm8_CL
                | Code::Sar_rm8_imm8
                | Code::Rol_rm8_1
                | Code::Rol_rm8_CL
                | Code::Rol_rm8_imm8
                | Code::Ror_rm8_1
                | Code::Ror_rm8_CL
                | Code::Ror_rm8_imm8
                | Code::Rcl_rm8_1
                | Code::Rcl_rm8_CL
                | Code::Rcl_rm8_imm8
                | Code::Rcr_rm8_1
                | Code::Rcr_rm8_CL
                | Code::Rcr_rm8_imm8
                | Code::Seto_rm8
                | Code::Setno_rm8
                | Code::Setb_rm8
                | Code::Setae_rm8
                | Code::Sete_rm8
                | Code::Setne_rm8
                | Code::Setbe_rm8
                | Code::Seta_rm8
                | Code::Sets_rm8
                | Code::Setns_rm8
                | Code::Setp_rm8
                | Code::Setnp_rm8
                | Code::Setl_rm8
                | Code::Setge_rm8
                | Code::Setle_rm8
                | Code::Setg_rm8
                | Code::Xchg_rm8_r8
                | Code::Cmpxchg_rm8_r8
                | Code::Xadd_rm8_r8
                | Code::Mul_rm8
                | Code::Imul_rm8
                | Code::Div_rm8
                | Code::Idiv_rm8
                | Code::Nop_rm16
                | Code::Aam_imm8
                | Code::Aad_imm8
                | Code::Aaa
                | Code::Aas
                | Code::Daa
                | Code::Das
                | Code::Lahf
                | Code::Sahf
                | Code::Clc
                | Code::Stc
                | Code::Cmc
                | Code::Xlat_m8
        );
    let ignored_pause_width = counts == [1, 0, 0, 1] && instruction.code() == Code::Pause;
    if !changed
        && !atomic_memory
        && !locked_exchange
        && !scalar_count
        && !ignored_byte_width
        && !ignored_pause_width
        && !hle_hint
    {
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
    malformed_fixture(bytes.len())
}

// prove the newly admitted form before retaining the original refusal checks.
pub fn instruction_refusal(
    bytes: &[u8],
    cause: ring3_engine::cpu::dbt::InstructionError,
) -> (Vec<u8>, ring3_engine::cpu::dbt::InstructionError) {
    use ring3_engine::cpu::dbt::InstructionError;
    if let InstructionError::Decode(error) = cause {
        let (bytes, error) = refusal_after_prefix_admission(bytes.to_vec(), error);
        (bytes, InstructionError::Decode(error))
    } else {
        (bytes.to_vec(), cause)
    }
}

pub fn compile_refusal(
    bytes: &[u8],
    offset: usize,
    expected: ring3_engine::cpu::dbt::CompileError,
) -> (Vec<u8>, ring3_engine::cpu::dbt::CompileError) {
    use ring3_engine::cpu::dbt::CompileError;
    if let CompileError::Instruction { pc, cause } = expected {
        let (tail, cause) = instruction_refusal(&bytes[offset..], cause);
        let mut bytes = bytes[..offset].to_vec();
        bytes.extend(tail);
        (bytes, CompileError::Instruction { pc, cause })
    } else {
        (bytes.to_vec(), expected)
    }
}
