use iced_x86::{Code, Decoder, DecoderOptions};

use crate::{
    cpu::x86::{
        ControlOperation,
        decode::{DecodeError, decode_for_execution, decode_one},
        environment::USER_MODE_ENVIRONMENT,
        ir::Operation,
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};

fn rows() -> Vec<(Code, Vec<u8>)> {
    let mut result = vec![
        (Code::Rdpkru, vec![0x0f, 0x01, 0xee]),
        (Code::Wrpkru, vec![0x0f, 0x01, 0xef]),
    ];
    result.extend(
        (0..8).map(|register| (Code::Incsspd_r32, vec![0xf3, 0x0f, 0xae, 0xe8 | register])),
    );
    result.extend(shadow_stack_rows());
    result
}

fn shadow_stack_rows() -> [(Code, Vec<u8>); 6] {
    [
        (Code::Rstorssp_m64, vec![0xf3, 0x0f, 0x01, 0x28]),
        (Code::Saveprevssp, vec![0xf3, 0x0f, 0x01, 0xea]),
        (Code::Setssbsy, vec![0xf3, 0x0f, 0x01, 0xe8]),
        (Code::Clrssbsy_m64, vec![0xf3, 0x0f, 0xae, 0x30]),
        (Code::Wrssd_m32_r32, vec![0x0f, 0x38, 0xf6, 0x00]),
        (Code::Wrussd_m32_r32, vec![0x66, 0x0f, 0x38, 0xf5, 0x00]),
    ]
}

fn executable(bytes: &[u8], pc: u32) -> AddressSpace {
    let mut memory = AddressSpace::new(2).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(pc & !0xfff), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(pc), bytes).unwrap();
    memory
}

#[test]
fn exact_disabled_state_codes_fault_without_register_or_shadow_stack_lowering() {
    for (code, bytes) in rows() {
        for prefix in [&[][..], &[0x64, 0x67]] {
            let mut encoded = prefix.to_vec();
            encoded.extend(&bytes);
            let pc = 0x2000 - encoded.len() as u32;
            let mut raw = Decoder::with_ip(32, &encoded, u64::from(pc), DecoderOptions::NONE);
            let instruction = raw.decode();
            assert_eq!(instruction.code(), code, "{encoded:02x?}");
            assert_eq!(instruction.len(), encoded.len());
            assert!(!instruction.has_rep_prefix() && !instruction.has_repne_prefix());
            let memory = executable(&encoded, pc);
            assert!(matches!(
                decode_one(&memory, GuestAddress(pc)),
                Err(DecodeError::Unsupported(_))
            ));
            let decoded = decode_for_execution(&memory, GuestAddress(pc)).unwrap();
            assert_eq!(
                decoded.operation(),
                &Operation::Control(ControlOperation::InvalidOpcode)
            );
            assert_eq!(decoded.segment_accesses(), 0);
            assert_eq!(usize::from(decoded.length()), encoded.len());
            assert_eq!(decoded.next_pc(), GuestAddress(0x2000));
            assert!(memory.is_code_current(decoded.code_snapshot()));
        }
    }
}

#[test]
fn full_fetch_precedes_disabled_state_fault_until_the_last_byte_is_executable() {
    for (_, bytes) in rows() {
        let pc = 0x2000 - (bytes.len() as u32 - 1);
        let mut memory = executable(&bytes[..bytes.len() - 1], pc);
        let missing = decode_one(&memory, GuestAddress(pc)).unwrap_err();
        assert!(
            matches!(missing, DecodeError::MemoryFault { length, .. } if length == bytes.len() as u32)
        );
        assert_eq!(
            decode_for_execution(&memory, GuestAddress(pc)).unwrap_err(),
            missing
        );
        let page = PageRange::new(GuestAddress(0x2000), 1).unwrap();
        memory.map_zeroed(page, Permissions::ALL).unwrap();
        memory
            .write(GuestAddress(0x2000), &bytes[bytes.len() - 1..])
            .unwrap();
        memory.protect(page, Permissions::READ).unwrap();
        let denied = decode_one(&memory, GuestAddress(pc)).unwrap_err();
        assert!(
            matches!(denied, DecodeError::MemoryFault { length, .. } if length == bytes.len() as u32)
        );
        assert_eq!(
            decode_for_execution(&memory, GuestAddress(pc)).unwrap_err(),
            denied
        );
        memory.protect(page, Permissions::EXECUTE).unwrap();
        let decoded = decode_for_execution(&memory, GuestAddress(pc)).unwrap();
        assert_eq!(
            decoded.operation(),
            &Operation::Control(ControlOperation::InvalidOpcode)
        );
        assert_eq!(decoded.segment_accesses(), 0);
        assert!(memory.is_code_current(decoded.code_snapshot()));
    }
}

#[test]
fn state_fault_classifier_keeps_reserved_repeat_duplicate_and_invalid_lock_boundaries() {
    for (_, bytes) in rows() {
        let mut raw = Decoder::new(32, &bytes, DecoderOptions::NONE);
        let instruction = raw.decode();
        for repne in [false, true] {
            let mut repeated = instruction;
            repeated.set_has_rep_prefix(!repne);
            repeated.set_has_repne_prefix(repne);
            assert_eq!(
                super::super::for_instruction(&repeated, &bytes, USER_MODE_ENVIRONMENT),
                None
            );
        }
        let mut duplicate = vec![0x66, 0x66];
        duplicate.extend(&bytes);
        assert_eq!(
            super::super::for_instruction(&instruction, &duplicate, USER_MODE_ENVIRONMENT),
            None
        );
        let mut locked = vec![0xf0];
        locked.extend(&bytes);
        let memory = executable(&locked, 0x1000);
        assert_eq!(
            decode_one(&memory, GuestAddress(0x1000)).unwrap_err(),
            DecodeError::InvalidEncoding
        );
        assert_eq!(
            decode_for_execution(&memory, GuestAddress(0x1000))
                .unwrap()
                .operation(),
            &Operation::Control(ControlOperation::InvalidOpcode)
        );
    }
}

#[test]
fn rdssp_compatibility_nop_stays_distinct_from_disabled_incssp() {
    for register in [0, 4, 7] {
        let bytes = [0xf3, 0x0f, 0x1e, 0xc8 | register];
        let memory = executable(&bytes, 0x1000);
        for decoded in [
            decode_one(&memory, GuestAddress(0x1000)).unwrap(),
            decode_for_execution(&memory, GuestAddress(0x1000)).unwrap(),
        ] {
            assert_eq!(decoded.operation(), &Operation::Nop);
            assert_eq!(decoded.segment_accesses(), 0);
        }
    }
}

#[test]
fn six_disabled_shadow_stack_forms_keep_mode_and_operand_authority_separate() {
    use crate::cpu::x86::{
        SegmentCache, SegmentContext,
        decode::{BoundDecode, CodeFetchContext, DecodePolicy, decode_bound},
    };
    for default32 in [false, true] {
        for (code, bytes) in shadow_stack_rows() {
            let encoded = [&[0x64, 0x67][..], &bytes].concat();
            let raw = Decoder::new(
                if default32 { 32 } else { 16 },
                &encoded,
                DecoderOptions::NONE,
            )
            .decode();
            assert_eq!(raw.code(), code);
            let memory = executable(&encoded, 0x1000);
            let mut segments = SegmentContext::default();
            segments.state.selectors[4] = 0;
            segments.caches[4] = SegmentCache::from_bits(0).unwrap();
            if !default32 {
                segments.caches[1] = SegmentCache::from_bits(0xf1bf_ffff).unwrap();
            }
            let cs = CodeFetchContext::from_segments(&segments).unwrap();
            let BoundDecode::Instruction(decoded) =
                decode_bound(&memory, GuestAddress(0x1000), cs, DecodePolicy::Execution).unwrap()
            else {
                panic!("complete code span is valid");
            };
            assert_eq!(
                decoded.operation(),
                &Operation::Control(ControlOperation::InvalidOpcode)
            );
            assert_eq!(decoded.segment_accesses(), 0);
            assert_eq!(usize::from(decoded.length()), encoded.len());
            assert_eq!(
                decoded.next_pc(),
                GuestAddress(0x1000 + encoded.len() as u32)
            );
            assert!(memory.is_code_current(decoded.code_snapshot()));
        }
    }
}
