use super::*;
use crate::{
    cpu::{UnsupportedFeature, dbt::cold},
    memory::{PageRange, Permissions},
};

const PC: GuestAddress = GuestAddress(0x1000);
const FORMS: [[u8; 5]; 4] = [
    [0xc5, 0xfc, 0x58, 0xc1, 0],
    [0xc5, 0xfc, 0x58, 0x00, 0],
    [0xc5, 0xfe, 0x7f, 0x00, 0],
    [0xc4, 0xe2, 0x7d, 0x2c, 0x00],
];

fn code(bytes: &[u8]) -> AddressSpace {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(PageRange::new(PC, 1).unwrap(), Permissions::ALL)
        .unwrap();
    memory.write(PC, bytes).unwrap();
    memory
        .write(GuestAddress(PC.0 + bytes.len() as u32), &[0xeb, 0xfe])
        .unwrap();
    memory
}

fn instruction_error(cause: InstructionError) -> CompileError {
    CompileError::Instruction { pc: PC, cause }
}

fn disabled(memory: &AddressSpace) -> bool {
    if crate::cpu::x86::capabilities::AVX_ENABLED {
        return false;
    }
    assert_eq!(
        decode_one(memory, PC).unwrap_err(),
        DecodeError::Unsupported(UnsupportedFeature::Simd)
    );
    true
}

fn prepared(result: Result<PreparedRegion, CompileError>, memory: &AddressSpace) {
    let result = result.unwrap();
    assert_eq!((result.block_count(), result.instruction_count()), (1, 2));
    assert!(matches!(
        result.blocks[0].instructions[0].operation(),
        Operation::Avx(_)
    ));
    assert!(result.is_current(memory));
}

#[test]
fn embedded_admits_typed_avx_without_relaxing_explicit_block_ends() {
    for form in FORMS {
        let length = if form[0] == 0xc4 { 5 } else { 4 };
        let memory = code(&form[..length]);
        if disabled(&memory) {
            continue;
        }
        let limits = CompileLimits::default();
        let spec = BlockSpec {
            entry: PC,
            byte_length: length as u32 + 2,
        };
        prepared(
            prepare_embedded_region(&memory, &[spec], limits, &[]),
            &memory,
        );
        assert_eq!(
            prepare_embedded_region(
                &memory,
                &[BlockSpec {
                    byte_length: length as u32 - 1,
                    ..spec
                }],
                limits,
                &[],
            )
            .unwrap_err(),
            instruction_error(InstructionError::InvalidBlockEnd)
        );
        assert_eq!(
            prepare_embedded_region(
                &memory,
                &[BlockSpec {
                    byte_length: length as u32 + 3,
                    ..spec
                }],
                limits,
                &[],
            )
            .unwrap_err(),
            CompileError::Instruction {
                pc: GuestAddress(PC.0 + length as u32),
                cause: InstructionError::InvalidBlockEnd,
            }
        );
        assert_eq!(
            prepare_region(&memory, &[spec], limits).unwrap_err(),
            instruction_error(InstructionError::BackendUnsupported)
        );
    }
}

#[test]
fn resident_admits_typed_avx_and_preserves_instruction_limits() {
    for form in FORMS {
        let length = if form[0] == 0xc4 { 5 } else { 4 };
        let memory = code(&form[..length]);
        if disabled(&memory) {
            continue;
        }
        let spec = BlockSpec {
            entry: PC,
            byte_length: length as u32 + 2,
        };
        prepared(
            prepare_resident_region(&memory, &[spec], CompileLimits::default(), &[]),
            &memory,
        );
        assert_eq!(
            prepare_resident_region(
                &memory,
                &[spec],
                CompileLimits {
                    instructions: 1,
                    ..CompileLimits::default()
                },
                &[],
            )
            .unwrap_err(),
            CompileError::InstructionLimit
        );
        assert_eq!(
            prepare_resident_region(
                &memory,
                &[BlockSpec {
                    byte_length: length as u32 - 1,
                    ..spec
                }],
                CompileLimits::default(),
                &[],
            )
            .unwrap_err(),
            instruction_error(InstructionError::InvalidBlockEnd)
        );
    }
}

#[test]
fn entry_paths_admit_typed_avx_only_with_cpu_state_authority() {
    for form in FORMS {
        let length = if form[0] == 0xc4 { 5 } else { 4 };
        let memory = code(&form[..length]);
        if disabled(&memory) {
            continue;
        }
        let limits = CompileLimits::default();
        prepared(
            cold::prepare_embedded_entry_region(&memory, &[PC], limits, &[]),
            &memory,
        );
        prepared(
            cold::prepare_resident_entry_region(&memory, &[PC], limits, &[]),
            &memory,
        );
        prepared(cold::prepare_runtime_entry(&memory, PC, limits), &memory);
        assert_eq!(
            cold::prepare_entry_region(&memory, &[PC], limits).unwrap_err(),
            instruction_error(InstructionError::BackendUnsupported)
        );
        assert_eq!(
            cold::prepare_embedded_entry_region(
                &memory,
                &[PC, GuestAddress(PC.0 + length as u32 - 1)],
                limits,
                &[],
            )
            .unwrap_err(),
            CompileError::InvalidBlocks
        );
    }
}
