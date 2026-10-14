use super::stopped_support::{KEY, expect_admitted, expect_captured_fetch, stopped};
use ring3_engine::{
    abi::arena::TRANSFER_OFFSET,
    cpu::{
        UnsupportedFeature,
        dbt::{CompileError, InstructionError},
        x86::decode::DecodeError,
    },
    process::HostError,
};

#[test]
fn present_msr_instructions_prepare_cpl3_faults_without_msr_value_state() {
    for opcode in [0x32, 0x30] {
        for prefix in [
            &[][..],
            &[0x66],
            &[0x67],
            &[0x26],
            &[0x2e],
            &[0x36],
            &[0x3e],
            &[0x64],
            &[0x65],
        ] {
            let bytes = [prefix, &[0x0f, opcode]].concat();
            expect_admitted(&bytes, 0x3000);
            expect_admitted(&bytes, 0x4000 - bytes.len() as u32);
        }
    }
}

#[test]
fn strict_msr_compilation_and_reserved_prefix_refusals_preserve_prior_artifact() {
    for opcode in [0x32, 0x30] {
        let mut engine = stopped(&[0x0f, opcode], 0x3000);
        for entries in [false, true] {
            let descriptors = if entries {
                0x3000_u32.to_le_bytes().to_vec()
            } else {
                [0x3000_u32.to_le_bytes(), 2_u32.to_le_bytes()].concat()
            };
            engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + descriptors.len()]
                .copy_from_slice(&descriptors);
            let before = engine.arena().to_vec();
            let artifact = engine.artifact_bytes().unwrap().to_vec();
            let result = if entries {
                engine.compile_entries(1, 0)
            } else {
                engine.compile(1)
            };
            assert!(matches!(
                result,
                Err(HostError::Compile(CompileError::Instruction {
                    cause: InstructionError::Decode(DecodeError::Unsupported(
                        UnsupportedFeature::Privileged
                    )),
                    ..
                }))
            ));
            assert_eq!(engine.arena(), before);
            assert_eq!(engine.artifact_bytes().unwrap(), artifact);
            assert_eq!(engine.generation(), 1);
        }
        for prefix in [&[0xf2][..], &[0xf3], &[0x66, 0x66]] {
            let bytes = [prefix, &[0x0f, opcode]].concat();
            let mut engine = stopped(&bytes, 0x3000);
            let before = engine.arena().to_vec();
            let artifact = engine.artifact_bytes().unwrap().to_vec();
            assert!(matches!(
                engine.compile_stopped_entry(KEY, 1),
                Err(HostError::Compile(CompileError::Instruction {
                    cause: InstructionError::Decode(DecodeError::Unsupported(_)),
                    ..
                }))
            ));
            assert_eq!(engine.arena(), before);
            assert_eq!(engine.artifact_bytes().unwrap(), artifact);
            assert_eq!(engine.generation(), 1);
        }
    }
}

#[test]
fn invalid_lock_msr_attempts_reuse_the_invalid_opcode_execution_path() {
    for opcode in [0x32, 0x30] {
        expect_admitted(&[0xf0, 0x0f, opcode], 0x3000);
    }
    expect_admitted(&[0xf4], 0x3000);
}

#[test]
fn incomplete_msr_fetch_remains_an_atomic_execution_page_fault() {
    let mut engine = stopped(&[0x0f], 0x3fff);
    expect_captured_fetch(&mut engine, 0x3fff, 0x4000, false, 2);
}
