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

fn scalar_forms() -> Vec<Vec<u8>> {
    vec![
        vec![0xe4, 0x80],
        vec![0x66, 0xe5, 0x80],
        vec![0xe5, 0x80],
        vec![0xec],
        vec![0x66, 0xed],
        vec![0xed],
        vec![0xe6, 0x80],
        vec![0x66, 0xe7, 0x80],
        vec![0xe7, 0x80],
        vec![0xee],
        vec![0x66, 0xef],
        vec![0xef],
    ]
}

#[test]
fn every_scalar_port_form_prepares_denied_user_access_without_device_state() {
    for row in scalar_forms() {
        for prefix in [
            &[][..],
            &[0x67],
            &[0x26],
            &[0x2e],
            &[0x36],
            &[0x3e],
            &[0x64],
            &[0x65],
        ] {
            let bytes = [prefix, &row].concat();
            expect_admitted(&bytes, 0x3000);
            expect_admitted(&bytes, 0x4000 - bytes.len() as u32);
        }
    }
    for opcode in [0xe4, 0xe6, 0xec, 0xee] {
        let bytes = if opcode < 0xec {
            vec![0x66, opcode, 0x80]
        } else {
            vec![0x66, opcode]
        };
        expect_admitted(&bytes, 0x3000);
    }
}

#[test]
fn strict_scalar_ports_and_reserved_prefixes_keep_atomic_refusals() {
    for row in scalar_forms() {
        assert_refused(&row, true);
        for prefix in [&[0xf2][..], &[0xf3]] {
            assert_refused(&[prefix, &row].concat(), false);
        }
        let opcode = &row[usize::from(row[0] == 0x66)..];
        assert_refused(&[&[0x66, 0x66][..], opcode].concat(), false);
    }
    for bytes in [
        &[0x6c][..],
        &[0x66, 0x6d],
        &[0x6d],
        &[0x6e],
        &[0x66, 0x6f],
        &[0x6f],
        &[0xf3, 0x6c],
        &[0x67, 0xf3, 0x6e],
    ] {
        assert_refused(bytes, false);
    }
}

#[test]
fn scalar_port_invalid_lock_and_existing_hlt_are_independent_controls() {
    for row in scalar_forms() {
        expect_admitted(&[&[0xf0][..], &row].concat(), 0x3000);
    }
    expect_admitted(&[0xf4], 0x3000);
}

#[test]
fn missing_scalar_port_immediate_stays_an_atomic_execute_memory_error() {
    for opcode in [0xe4, 0xe5, 0xe6, 0xe7] {
        assert_incomplete(&[opcode], 2);
        assert_incomplete(&[0x66, opcode], 3);
    }
}
