use crate::{
    cpu::x86::{
        ControlOperation, SegmentContext,
        decode::{
            BoundDecode, CodeFetchContext, DecodeAuthority, DecodeError, DecodePolicy, decode_one,
            decode_with_authority,
        },
        ir::Operation,
    },
    memory::{Access, AddressSpace, FaultReason, GuestAddress, PageRange, Permissions},
};
fn memory(bytes: &[u8]) -> AddressSpace {
    let mut memory = AddressSpace::new(2).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(0x1000), bytes).unwrap();
    memory
}
fn decoded(
    memory: &AddressSpace,
    pc: u32,
    policy: DecodePolicy,
    cr0: Option<u32>,
    d32: bool,
) -> Result<crate::cpu::x86::decode::DecodedInstruction, DecodeError> {
    let mut context = SegmentContext::default();
    if !d32 {
        context.caches[1] = crate::cpu::x86::SegmentCache::from_bits(0xf1bfffff).unwrap();
    }
    let BoundDecode::Instruction(instruction) = decode_with_authority(
        memory,
        GuestAddress(pc),
        policy,
        Some(CodeFetchContext::from_segments(&context).unwrap()),
        DecodeAuthority::default().with_coprocessor_control(cr0),
    )?
    else {
        panic!("instruction expected")
    };
    Ok(instruction)
}
fn stop(operation: &Operation) -> Option<ControlOperation> {
    match operation {
        Operation::Control(
            value @ (ControlOperation::DeviceNotAvailable | ControlOperation::InvalidOpcode),
        ) => Some(*value),
        _ => None,
    }
}

#[test]
fn exact_ts_em_mp_table_distinguishes_x87_wait_mmx_sse_fx_avx_and_xsave() {
    use ControlOperation::{DeviceNotAvailable as N, InvalidOpcode as U};
    // x87, wait, mmx/sse, fxsave, avx/xsave: explicit intel table rows, not computed predicates.
    let rows = [
        (0x31, [None, None, None, None, None]),
        (0x33, [None, None, None, None, None]),
        (0x35, [Some(N), None, Some(U), Some(N), None]),
        (0x37, [Some(N), None, Some(U), Some(N), None]),
        (0x39, [Some(N), None, Some(N), Some(N), Some(N)]),
        (0x3b, [Some(N), Some(N), Some(N), Some(N), Some(N)]),
        (0x3d, [Some(N), None, Some(U), Some(N), Some(N)]),
        (0x3f, [Some(N), Some(N), Some(U), Some(N), Some(N)]),
    ];
    let cases: &[(&[u8], usize)] = &[
        (&[0xdb, 0xe3], 0),
        (&[0xdb, 0xe0], 0),
        (&[0xdb, 0xe1], 0),
        (&[0xdb, 0xe4], 0),
        (&[0xd9, 0x00], 0),
        (&[0xdf, 0xe0], 0),
        (&[0x9b], 1),
        (&[0x0f, 0x77], 2),
        (&[0x0f, 0xef, 0xc1], 2),
        (&[0x66, 0x0f, 0xef, 0xc1], 2),
        (&[0x0f, 0x28, 0x00], 2),
        (&[0x66, 0x0f, 0x3a, 0x63, 0xc1, 0], 2),
        (&[0x0f, 0xae, 0x00], 3),
        (&[0x0f, 0xae, 0x08], 3),
        (&[0xc5, 0xfc, 0x58, 0xc1], 4),
        (&[0xc5, 0xf8, 0x77], 4),
        (&[0x0f, 0xae, 0x20], 4),
        (&[0x0f, 0xae, 0x28], 4),
    ];
    for &(cr0, expected) in &rows {
        for &(bytes, family) in cases {
            let memory = memory(bytes);
            for policy in [DecodePolicy::Strict, DecodePolicy::Execution] {
                let instruction = decoded(&memory, 0x1000, policy, Some(cr0), true).unwrap();
                assert_eq!(
                    stop(instruction.operation()),
                    expected[family],
                    "{bytes:02x?} cr0={cr0:x}"
                );
                assert_eq!(instruction.length(), bytes.len() as u8);
                assert_eq!(
                    instruction.next_pc(),
                    GuestAddress(0x1000 + bytes.len() as u32)
                );
                if expected[family].is_some() {
                    assert_eq!(instruction.segment_accesses(), 0);
                }
                assert!(memory.is_code_current(instruction.code_snapshot()));
            }
            assert_eq!(
                stop(
                    decode_one(&memory, GuestAddress(0x1000))
                        .unwrap()
                        .operation()
                ),
                None
            );
        }
    }
}

#[test]
fn integer_hints_fences_and_xgetbv_ignore_ts_em_while_unavailable_and_prefix_owners_stay_first() {
    let neutral: &[&[u8]] = &[
        &[0xf3, 0x90],
        &[0x0f, 0x18, 0x00],
        &[0x0f, 0xae, 0xf8],
        &[0x0f, 0xae, 0xe8],
        &[0x0f, 0xae, 0xf0],
        &[0x0f, 0xc3, 0x00],
        &[0xf2, 0x0f, 0x38, 0xf1, 0xc1],
        &[0xf3, 0x0f, 0xb8, 0xc1],
        &[0x0f, 0x01, 0xd0],
    ];
    for &bytes in neutral {
        let memory = memory(bytes);
        let baseline = decoded(&memory, 0x1000, DecodePolicy::Strict, None, true).unwrap();
        let controlled = decoded(&memory, 0x1000, DecodePolicy::Strict, Some(0x3f), true).unwrap();
        assert_eq!(controlled.operation(), baseline.operation(), "{bytes:02x?}");
    }
    for bytes in [&[0x0f, 0xc7, 0x18][..], &[0xf0, 0x0f, 0xae, 0x20][..]] {
        let memory = memory(bytes);
        let instruction =
            decoded(&memory, 0x1000, DecodePolicy::Execution, Some(0x3f), true).unwrap();
        assert_eq!(
            stop(instruction.operation()),
            Some(ControlOperation::InvalidOpcode)
        );
    }
    for bytes in [&[0x66, 0x66, 0xd9, 0x00][..], &[0xf3, 0x89, 0xc0][..]] {
        let memory = memory(bytes);
        for policy in [DecodePolicy::Strict, DecodePolicy::Execution] {
            assert!(matches!(
                decoded(&memory, 0x1000, policy, Some(0x3f), true),
                Err(DecodeError::Unsupported(_))
            ));
        }
    }
}

#[test]
fn complete_cross_page_fetch_and_cs_limits_precede_nm_and_repair_keeps_both_snapshots() {
    let mut memory = memory(&[]);
    memory.write(GuestAddress(0x1ffe), &[0x0f, 0xae]).unwrap();
    for d32 in [false, true] {
        assert!(
            matches!(decoded(&memory, 0x1ffe, DecodePolicy::Strict, Some(0x3b), d32),
            Err(DecodeError::MemoryFault { fault, length:3, .. }) if fault.address==GuestAddress(0x2000) && fault.access==Access::Execute && fault.reason==FaultReason::Unmapped)
        );
    }
    let page = PageRange::new(GuestAddress(0x2000), 1).unwrap();
    memory.map_zeroed(page, Permissions::READ_WRITE).unwrap();
    memory.write(GuestAddress(0x2000), &[0]).unwrap();
    assert!(
        matches!(decoded(&memory, 0x1ffe, DecodePolicy::Strict, Some(0x3b), true),
        Err(DecodeError::MemoryFault { fault, length:3, .. }) if fault.access==Access::Execute && fault.reason==FaultReason::Permission)
    );
    memory.protect(page, Permissions::ALL).unwrap();
    for d32 in [false, true] {
        let instruction = decoded(&memory, 0x1ffe, DecodePolicy::Strict, Some(0x3b), d32).unwrap();
        assert_eq!(
            stop(instruction.operation()),
            Some(ControlOperation::DeviceNotAvailable)
        );
        assert_eq!(instruction.length(), 3);
        assert_eq!(instruction.next_pc(), GuestAddress(0x2001));
        assert_eq!(instruction.segment_accesses(), 0);
        memory.write(GuestAddress(0x2000), &[0]).unwrap();
        assert!(!memory.is_code_current(instruction.code_snapshot()));
        let prefix = decoded(&memory, 0x1ffe, DecodePolicy::Strict, Some(0x3b), d32).unwrap();
        memory.write(GuestAddress(0x1ffe), &[0x0f]).unwrap();
        assert!(!memory.is_code_current(prefix.code_snapshot()));
    }
    let mut context = SegmentContext::default();
    context.caches[1] = crate::cpu::x86::SegmentCache::from_bits(0xf2b01fff).unwrap();
    assert_eq!(
        decode_with_authority(
            &memory,
            GuestAddress(0x1ffe),
            DecodePolicy::Strict,
            Some(CodeFetchContext::from_segments(&context).unwrap()),
            DecodeAuthority::default().with_coprocessor_control(Some(0x3b))
        )
        .unwrap_err(),
        DecodeError::CodeSegment {
            pc: GuestAddress(0x1ffe),
            attempted_length: 3
        }
    );
}
