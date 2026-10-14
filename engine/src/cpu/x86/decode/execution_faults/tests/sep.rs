use crate::{
    cpu::x86::{
        ControlOperation, SegmentCache, SegmentContext,
        capabilities::CPU_ID_LEAVES,
        decode::{
            BoundDecode, CodeFetchContext, DecodeError, DecodePolicy, decode_bound, decode_one,
        },
        environment::USER_MODE_ENVIRONMENT,
        ir::Operation,
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};

fn executable(bytes: &[u8], pc: u32) -> AddressSpace {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(pc & !0xfff), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(pc), bytes).unwrap();
    memory
        .protect(
            PageRange::new(GuestAddress(pc & !0xfff), 1).unwrap(),
            Permissions::EXECUTE,
        )
        .unwrap();
    memory
}

#[test]
fn present_sysenter_is_a_user_os_entry_while_sysexit_faults_at_cpl3() {
    let leaf = CPU_ID_LEAVES
        .iter()
        .find(|leaf| leaf.function == 1)
        .unwrap();
    assert_ne!(leaf.registers[3] & (1 << 11), 0);
    assert_ne!(leaf.registers[3] & (1 << 5), 0);
    assert_eq!(USER_MODE_ENVIRONMENT.current_privilege(), 3);
    for default32 in [false, true] {
        let mut context = SegmentContext::default();
        if !default32 {
            context.caches[1] = SegmentCache::from_bits(0xf1bf_ffff).unwrap();
        }
        for index in [0, 3, 4, 5] {
            context.state.selectors[index] = 0;
            context.caches[index] = SegmentCache::UNUSABLE;
        }
        context.caches[2] = SegmentCache::from_bits(0xf230_0fff).unwrap();
        let cs = CodeFetchContext::from_segments(&context).unwrap();
        for prefix in [&[][..], &[0x66], &[0x67], &[0x64], &[0x36]] {
            for (opcode, expected) in [
                (0x34, ControlOperation::Sysenter),
                (0x35, ControlOperation::GeneralProtectionZero),
            ] {
                let bytes = [prefix, &[0x0f, opcode]].concat();
                let pc = 0x2000 - bytes.len() as u32;
                let memory = executable(&bytes, pc);
                let strict = decode_bound(&memory, GuestAddress(pc), cs, DecodePolicy::Strict);
                if opcode == 0x34 {
                    let BoundDecode::Instruction(decoded) = strict.unwrap() else {
                        panic!("complete entry")
                    };
                    assert_eq!(
                        decoded.operation(),
                        &Operation::Control(ControlOperation::Sysenter)
                    );
                } else {
                    assert!(matches!(strict, Err(DecodeError::Unsupported(_))));
                }
                let BoundDecode::Instruction(decoded) =
                    decode_bound(&memory, GuestAddress(pc), cs, DecodePolicy::Execution).unwrap()
                else {
                    panic!("complete entry or denied privileged return")
                };
                assert_eq!(decoded.operation(), &Operation::Control(expected));
                assert_eq!(decoded.segment_accesses(), 0);
                assert_eq!(decoded.next_pc(), GuestAddress(0x2000));
                assert_eq!(decoded.length() as usize, bytes.len());
                assert!(memory.is_code_current(decoded.code_snapshot()));
            }
        }
    }
}

#[test]
fn sysenter_repeat_duplicate_and_lock_controls_remain_explicit() {
    for prefix in [&[0xf2][..], &[0xf3], &[0x66, 0x66]] {
        let bytes = [prefix, &[0x0f, 0x34]].concat();
        assert!(matches!(
            decode_one(&executable(&bytes, 0x1000), GuestAddress(0x1000)),
            Err(DecodeError::Unsupported(_))
        ));
    }
    assert_eq!(
        decode_one(
            &executable(&[0xf0, 0x0f, 0x34], 0x1000),
            GuestAddress(0x1000)
        )
        .unwrap_err(),
        DecodeError::InvalidEncoding
    );
}
