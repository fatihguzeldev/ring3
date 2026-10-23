use super::*;
use crate::cpu::x86::{CpuPagingError32, PagingAccess32};
use crate::memory::{Access, FaultReason, PageRange, Permissions};

fn memory(callee_permissions: Option<Permissions>) -> AddressSpace {
    let mut memory = AddressSpace::new(2).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory
        .write(GuestAddress(0x1000), &[0xe8, 0xfb, 0x0f, 0, 0, 0x0f, 0x0b])
        .unwrap();
    if let Some(permissions) = callee_permissions {
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0x2000), 1).unwrap(),
                permissions,
            )
            .unwrap();
        memory.write(GuestAddress(0x2000), &[0x40, 0xc3]).unwrap();
    }
    memory
}

fn options() -> CompileOptions {
    CompileOptions {
        instructions: 16,
        blocks: 8,
        ..CompileOptions::default()
    }
}

fn pcs(code: &CompiledCode) -> Vec<u32> {
    code.instruction_pcs().collect()
}

#[test]
fn direct_call_owns_the_callee_and_continuation_once() {
    let mut memory = memory(Some(Permissions::ALL));
    let code = compile(&memory, GuestAddress(0x1000), options()).unwrap();
    assert_eq!(pcs(&code), [0x1000, 0x1005, 0x2000, 0x2001]);
    assert_eq!(
        code.entrypoints(),
        &[
            GuestAddress(0x1000),
            GuestAddress(0x1005),
            GuestAddress(0x2000),
        ]
    );
    assert!(code.is_current(&memory));
    memory.write(GuestAddress(0x2000), &[0x48]).unwrap();
    assert!(!code.is_current(&memory));
}

#[test]
fn retained_callee_frontier_preserves_independent_source_ownership() {
    let mut memory = memory(Some(Permissions::ALL));
    let code = compile_with_frontier_query(&memory, GuestAddress(0x1000), options(), &[], &|pc| {
        pc == 0x2000
    })
    .unwrap();
    assert_eq!(pcs(&code), [0x1000, 0x1005]);
    assert_eq!(
        code.entrypoints(),
        &[GuestAddress(0x1000), GuestAddress(0x1005)]
    );
    memory.write(GuestAddress(0x2000), &[0x48]).unwrap();
    assert!(code.is_current(&memory));
}

#[test]
fn direct_callee_discovery_uses_existing_instruction_and_block_budgets() {
    let memory = memory(Some(Permissions::ALL));
    for (instructions, blocks, expected) in [
        (1, 1, &[0x1000][..]),
        (2, 8, &[0x1000, 0x2000][..]),
        (16, 2, &[0x1000, 0x2000, 0x2001][..]),
    ] {
        let code = compile(
            &memory,
            GuestAddress(0x1000),
            CompileOptions {
                instructions,
                blocks,
                ..options()
            },
        )
        .unwrap();
        assert_eq!(pcs(&code), expected);
        assert!(code.entrypoints().len() <= blocks);
        assert!(code.wasm().len() <= options().wasm_bytes);
        assert!(code.is_current(&memory));
    }
}

#[test]
fn recursive_calls_decode_each_instruction_once_without_host_recursion() {
    let mut memory = memory(Some(Permissions::ALL));
    memory
        .write(GuestAddress(0x1000), &[0xe8, 0xfb, 0x0f, 0, 0, 0xc3])
        .unwrap();
    memory
        .write(GuestAddress(0x2000), &[0xe8, 0xfb, 0xef, 0xff, 0xff, 0xc3])
        .unwrap();
    let code = compile(&memory, GuestAddress(0x1000), options()).unwrap();
    assert_eq!(pcs(&code), [0x1000, 0x1005, 0x2000, 0x2005]);
    assert_eq!(code.entrypoints().len(), 4);
    memory
        .write(GuestAddress(0x1000), &[0xe8, 0xfb, 0xff, 0xff, 0xff, 0xc3])
        .unwrap();
    let code = compile(&memory, GuestAddress(0x1000), options()).unwrap();
    assert_eq!(pcs(&code), [0x1000, 0x1005]);
    assert_eq!(code.entrypoints().len(), 2);
}

#[test]
fn speculative_callee_execute_faults_do_not_fail_the_decoded_caller() {
    for permissions in [None, Some(Permissions::READ_WRITE)] {
        let memory = memory(permissions);
        for gates in [Vec::new(), vec![(GuestAddress(0x2000), 77)]] {
            let code =
                compile_with_gates(&memory, GuestAddress(0x1000), options(), &gates).unwrap();
            assert_eq!(pcs(&code), [0x1000, 0x1005]);
            assert!(!code.supports_entry(0x2000));
            assert!(code.is_current(&memory));
            let error =
                compile_with_gates(&memory, GuestAddress(0x2000), options(), &gates).unwrap_err();
            let fault = match error {
                CompileError::Decode(DecodeError::Paging {
                    pc,
                    error: CpuPagingError32::PageFault(fault),
                    attempted_length,
                }) => {
                    assert_eq!((pc, attempted_length), (GuestAddress(0x2000), 1));
                    assert_eq!(fault.linear(), GuestAddress(0x2000));
                    assert_eq!(fault.access(), PagingAccess32::Execute);
                    assert_eq!(fault.error_code(), 0x14 | u32::from(permissions.is_some()));
                    continue;
                }
                CompileError::Decode(DecodeError::MemoryFault { fault, .. })
                | CompileError::Memory(MemoryError::Fault(fault)) => fault,
                _ => panic!("requested callee lost its execute fault: {error:?}"),
            };
            assert_eq!(fault.address, GuestAddress(0x2000));
            assert_eq!(fault.access, Access::Execute);
            assert_eq!(
                fault.reason,
                if permissions.is_some() {
                    FaultReason::Permission
                } else {
                    FaultReason::Unmapped
                }
            );
        }
    }
}

#[test]
fn an_invalid_speculative_callee_remains_an_uncompiled_execution_boundary() {
    let mut memory = memory(Some(Permissions::ALL));
    let mut invalid = vec![0x2e; 13];
    invalid.extend_from_slice(&[0x0f, 0x0a]);
    memory.write(GuestAddress(0x2000), &invalid).unwrap();
    let code = compile(&memory, GuestAddress(0x1000), options()).unwrap();
    assert_eq!(pcs(&code), [0x1000, 0x1005]);
    assert!(!code.supports_entry(0x2000));
    assert!(matches!(
        compile(&memory, GuestAddress(0x2000), options()),
        Err(CompileError::Decode(DecodeError::InvalidEncoding))
    ));
}

#[test]
fn discovered_callee_gates_keep_configuration_validation_authoritative() {
    let mut memory = memory(Some(Permissions::ALL));
    memory.write(GuestAddress(0x2000), &[0x0f, 0x0b]).unwrap();
    let code = compile_with_gates(
        &memory,
        GuestAddress(0x1000),
        options(),
        &[(GuestAddress(0x2000), 77)],
    )
    .unwrap();
    assert_eq!(pcs(&code), [0x1000, 0x1005, 0x2000]);
    for gates in [
        vec![(GuestAddress(0x2000), 0)],
        vec![(GuestAddress(0x2000), 77), (GuestAddress(0x2000), 78)],
    ] {
        assert!(matches!(
            compile_with_gates(&memory, GuestAddress(0x1000), options(), &gates),
            Err(CompileError::InvalidLimits)
        ));
    }
    memory.write(GuestAddress(0x2000), &[0x90]).unwrap();
    assert!(matches!(
        compile_with_gates(
            &memory,
            GuestAddress(0x1000),
            options(),
            &[(GuestAddress(0x2000), 77)],
        ),
        Err(CompileError::InvalidLimits)
    ));
}

#[test]
fn indirect_calls_do_not_discover_a_register_guessed_callee() {
    for call in [&[0xff, 0xd0][..], &[0x66, 0xff, 0xd0][..]] {
        let mut memory = memory(Some(Permissions::ALL));
        let mut bytes = call.to_vec();
        bytes.extend_from_slice(&[0x0f, 0x0b]);
        memory.write(GuestAddress(0x1000), &bytes).unwrap();
        let code = compile(&memory, GuestAddress(0x1000), options()).unwrap();
        assert_eq!(pcs(&code), [0x1000, 0x1000 + call.len() as u32]);
        memory.write(GuestAddress(0x2000), &[0x48]).unwrap();
        assert!(code.is_current(&memory));
    }
}
