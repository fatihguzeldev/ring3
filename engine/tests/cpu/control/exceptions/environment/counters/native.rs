use super::stopped_support::{KEY, expect_admitted, stopped};
use ring3_engine::{
    abi::arena::TRANSFER_OFFSET,
    cpu::{
        dbt::{CompileError, InstructionError},
        x86::decode::DecodeError,
    },
    memory::{Access, FaultReason, GuestAddress, MemoryFault},
    process::HostError,
};

fn assert_refused(bytes: &[u8], strict: bool) {
    let mut engine = stopped(bytes, 0x3000);
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[0x3000_u32.to_le_bytes(), (bytes.len() as u32).to_le_bytes()].concat());
    let before = engine.arena().to_vec();
    let artifact = engine.artifact_bytes().unwrap().to_vec();
    let result = if strict {
        engine.compile(1).map(|_| 0)
    } else {
        engine.compile_stopped_entry(KEY, 1)
    };
    assert!(
        matches!(
            result,
            Err(HostError::Compile(CompileError::Instruction {
                cause: InstructionError::Decode(DecodeError::Unsupported(_)),
                ..
            }))
        ),
        "{bytes:02x?}: {result:?}"
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(engine.artifact_bytes().unwrap(), artifact);
    assert_eq!(engine.generation(), 1);
}

fn assert_incomplete(bytes: &[u8], length: u32) {
    let entry = 0x4000 - bytes.len() as u32;
    let mut engine = stopped(bytes, entry);
    let before = engine.arena().to_vec();
    let artifact = engine.artifact_bytes().unwrap().to_vec();
    assert_eq!(
        engine.compile_stopped_entry(KEY, 1),
        Err(HostError::Compile(CompileError::Instruction {
            pc: GuestAddress(entry),
            cause: InstructionError::Decode(DecodeError::MemoryFault {
                pc: GuestAddress(entry),
                fault: MemoryFault {
                    address: GuestAddress(0x4000),
                    access: Access::Execute,
                    reason: FaultReason::Unmapped,
                },
                length,
            }),
        }))
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(engine.artifact_bytes().unwrap(), artifact);
    assert_eq!(engine.generation(), 1);
}

#[test]
fn present_rdpmc_with_fixed_pce0_prepares_general_protection_at_cpl3() {
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
        let bytes = [prefix, &[0x0f, 0x33]].concat();
        expect_admitted(&bytes, 0x3000);
        expect_admitted(&bytes, 0x4000 - bytes.len() as u32);
    }
}

#[test]
fn strict_rdpmc_and_reserved_profile_prefixes_preserve_refusals() {
    assert_refused(&[0x0f, 0x33], true);
    for prefix in [&[0xf2][..], &[0xf3], &[0x66, 0x66]] {
        assert_refused(&[prefix, &[0x0f, 0x33]].concat(), false);
    }
    for bytes in [&[0x6c][..], &[0x6e], &[0xf3, 0x6c], &[0xf3, 0x6e]] {
        assert_refused(bytes, true);
    }
}

#[test]
fn rdpmc_invalid_lock_and_existing_hlt_are_independent_controls() {
    expect_admitted(&[0xf0, 0x0f, 0x33], 0x3000);
    expect_admitted(&[0xf4], 0x3000);
}

#[test]
fn incomplete_rdpmc_fetch_stays_an_atomic_execute_memory_error() {
    assert_incomplete(&[0x0f], 2);
}
