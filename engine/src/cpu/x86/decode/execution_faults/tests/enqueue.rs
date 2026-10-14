use super::{available, unavailable};
use crate::{
    cpu::x86::{
        ControlOperation, SegmentCache, SegmentContext,
        decode::{
            BoundDecode, CodeFetchContext, DecodeError, DecodePolicy, decode_bound,
            decode_for_execution, decode_one,
        },
        environment::USER_MODE_ENVIRONMENT,
        ir::Operation,
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};
use iced_x86::{Code, CpuidFeature, Decoder, DecoderOptions};

fn executable(bytes: &[u8], pc: u32) -> AddressSpace {
    let mut memory = AddressSpace::new(2).unwrap();
    let page = PageRange::new(GuestAddress(pc & !0xfff), 1).unwrap();
    memory.map_zeroed(page, Permissions::ALL).unwrap();
    memory.write(GuestAddress(pc), bytes).unwrap();
    memory.protect(page, Permissions::EXECUTE).unwrap();
    memory
}
fn fetch_context(default32: bool) -> CodeFetchContext {
    let mut context = SegmentContext::default();
    for index in [0, 3, 4] {
        context.state.selectors[index] = 0;
        context.caches[index] = SegmentCache::UNUSABLE;
    }
    context.caches[2] = SegmentCache::from_bits(0xf230_0fff).unwrap();
    if !default32 {
        context.caches[1] = SegmentCache::from_bits(0xf1bf_ffff).unwrap();
    }
    CodeFetchContext::from_segments(&context).unwrap()
}

#[test]
fn exact_absent_enqueue_forms_precede_privilege_and_unmapped_operand_work() {
    assert!(!available((7, 0, 2, 29)));
    assert_eq!(USER_MODE_ENVIRONMENT.current_privilege(), 3);
    for default32 in [false, true] {
        for address_override in [false, true] {
            for supervisor in [false, true] {
                let mut bytes = vec![if supervisor { 0xf3 } else { 0xf2 }, 0x0f, 0x38, 0xf8, 0x08];
                if address_override {
                    bytes.insert(0, 0x67);
                }
                let raw = Decoder::new(
                    if default32 { 32 } else { 16 },
                    &bytes,
                    DecoderOptions::NONE,
                )
                .decode();
                let code = match (supervisor, default32 != address_override) {
                    (false, false) => Code::Enqcmd_r16_m512,
                    (false, true) => Code::Enqcmd_r32_m512,
                    (true, false) => Code::Enqcmds_r16_m512,
                    (true, true) => Code::Enqcmds_r32_m512,
                };
                assert_eq!(raw.code(), code);
                assert_eq!(raw.cpuid_features(), &[CpuidFeature::ENQCMD]);
                assert_eq!(raw.len(), bytes.len());
                assert!(!raw.has_rep_prefix() && !raw.has_repne_prefix());
                assert!(unavailable(&raw));
                let memory = executable(&bytes, 0x1000);
                let cs = fetch_context(default32);
                assert!(matches!(
                    decode_bound(&memory, GuestAddress(0x1000), cs, DecodePolicy::Strict),
                    Err(DecodeError::Unsupported(_))
                ));
                let BoundDecode::Instruction(decoded) =
                    decode_bound(&memory, GuestAddress(0x1000), cs, DecodePolicy::Execution)
                        .unwrap()
                else {
                    panic!("complete enqueue instruction");
                };
                assert_eq!(
                    decoded.operation(),
                    &Operation::Control(ControlOperation::InvalidOpcode)
                );
                assert_eq!(decoded.segment_accesses(), 0);
                assert_eq!(usize::from(decoded.length()), bytes.len());
                assert_eq!(decoded.next_pc(), GuestAddress(0x1000 + bytes.len() as u32));
                assert!(memory.is_code_current(decoded.code_snapshot()));
            }
        }
    }
    for code in [
        Code::Enqcmd_r64_m512,
        Code::Enqcmds_r64_m512,
        Code::Int_imm8,
    ] {
        let mut raw = iced_x86::Instruction::default();
        raw.set_code(code);
        assert!(!unavailable(&raw), "exact legacy-code selection only");
    }
}

#[test]
fn enqueue_final_modrm_and_existing_prefix_boundaries_keep_their_owners() {
    let bytes = [0xf2, 0x0f, 0x38, 0xf8, 0x08];
    let pc = 0x2000 - bytes.len() as u32 + 1;
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
    assert!(matches!(denied, DecodeError::MemoryFault { .. }));
    assert_eq!(
        decode_for_execution(&memory, GuestAddress(pc)).unwrap_err(),
        denied
    );
    memory.protect(page, Permissions::EXECUTE).unwrap();
    assert_eq!(
        decode_for_execution(&memory, GuestAddress(pc))
            .unwrap()
            .operation(),
        &Operation::Control(ControlOperation::InvalidOpcode)
    );
    let duplicate = executable(&[0xf2, 0xf2, 0x0f, 0x38, 0xf8, 0x08], 0x1000);
    let strict = decode_one(&duplicate, GuestAddress(0x1000)).unwrap_err();
    assert!(matches!(strict, DecodeError::Unsupported(_)));
    assert_eq!(
        decode_for_execution(&duplicate, GuestAddress(0x1000)).unwrap_err(),
        strict
    );
    let locked = executable(&[0xf0, 0xf2, 0x0f, 0x38, 0xf8, 0x08], 0x1000);
    assert_eq!(
        decode_one(&locked, GuestAddress(0x1000)).unwrap_err(),
        DecodeError::InvalidEncoding
    );
    assert_eq!(
        decode_for_execution(&locked, GuestAddress(0x1000))
            .unwrap()
            .operation(),
        &Operation::Control(ControlOperation::InvalidOpcode)
    );
}
