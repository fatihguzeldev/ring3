use crate::{
    cpu::x86::{
        ControlOperation, SegmentCache, SegmentContext,
        capabilities::CPU_ID_LEAVES,
        decode::{
            BoundDecode, CodeFetchContext, DecodeError, DecodePolicy, decode_bound,
            decode_for_execution, decode_one,
        },
        ir::Operation,
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};
use iced_x86::{Code, Decoder, DecoderOptions};
fn groups() -> [(u8, Code, Code); 5] {
    [
        (
            0x19,
            Code::Reservednop_rm16_r16_0F19,
            Code::Reservednop_rm32_r32_0F19,
        ),
        (
            0x1c,
            Code::Reservednop_rm16_r16_0F1C,
            Code::Reservednop_rm32_r32_0F1C,
        ),
        (
            0x1d,
            Code::Reservednop_rm16_r16_0F1D,
            Code::Reservednop_rm32_r32_0F1D,
        ),
        (
            0x1e,
            Code::Reservednop_rm16_r16_0F1E,
            Code::Reservednop_rm32_r32_0F1E,
        ),
        (
            0x1f,
            Code::Reservednop_rm16_r16_0F1F,
            Code::Reservednop_rm32_r32_0F1F,
        ),
    ]
}
fn executable(bytes: &[u8], pc: u32) -> AddressSpace {
    let mut m = AddressSpace::new(2).unwrap();
    m.map_zeroed(
        PageRange::new(GuestAddress(pc & !0xfff), 1).unwrap(),
        Permissions::ALL,
    )
    .unwrap();
    m.write(GuestAddress(pc), bytes).unwrap();
    m
}
#[test]
fn exact_selected_cells_are_separate_from_reserved_and_concrete_instruction_owners() {
    let leaf = CPU_ID_LEAVES.iter().find(|l| l.function == 1).unwrap();
    assert_eq!(leaf.registers[0], 0x600);
    assert_eq!((leaf.registers[0] >> 8) & 15, 6);
    for (opcode, word, dword) in groups() {
        for (prefix, expected) in [(&[][..], dword), (&[0x66][..], word)] {
            let bytes = [prefix, &[0x0f, opcode, 0xc8]].concat();
            let raw = Decoder::new(32, &bytes, DecoderOptions::NONE).decode();
            assert_eq!(raw.code(), expected);
            assert!(super::legacy_nop_alias(&raw));
        }
    }
    for bytes in [
        &[0x0f, 0x0d, 0xc0][..],
        &[0x0f, 0x18, 0xc0],
        &[0x0f, 0x1a, 0xc0],
        &[0x0f, 0x1b, 0xc0],
        &[0x0f, 0x1f, 0x00],
        &[0x0f, 0x1c, 0x00],
        &[0xf3, 0x0f, 0x1e, 0xc8],
        &[0xf3, 0x0f, 0x1e, 0xfb],
    ] {
        let raw = Decoder::new(32, bytes, DecoderOptions::NONE).decode();
        assert!(!super::legacy_nop_alias(&raw), "{bytes:02x?}");
    }
}
#[test]
fn both_defaults_and_widths_ignore_register_and_complex_memory_operands() {
    for (opcode, word, dword) in groups() {
        for default32 in [false, true] {
            for word_width in [false, true] {
                for memory in [false, true] {
                    let mut bytes = vec![];
                    if default32 == word_width {
                        bytes.push(0x66);
                    }
                    if memory {
                        bytes.push(0x64);
                        if !default32 {
                            bytes.push(0x67);
                        }
                    }
                    bytes.extend([0x0f, opcode]);
                    if memory {
                        bytes.extend([0x8c, 0x24, 0xef, 0xbe, 0xad, 0xde]);
                    } else {
                        bytes.push(0xc8);
                    }
                    let raw = Decoder::new(
                        if default32 { 32 } else { 16 },
                        &bytes,
                        DecoderOptions::NONE,
                    )
                    .decode();
                    assert_eq!(raw.code(), if word_width { word } else { dword });
                    assert_eq!(raw.len(), bytes.len());
                    let m = executable(&bytes, 0x1000);
                    let mut context = SegmentContext::default();
                    for index in [3, 4] {
                        context.state.selectors[index] = 0;
                        context.caches[index] = SegmentCache::UNUSABLE;
                    }
                    context.caches[2] = SegmentCache::from_bits(0xf230_0fff).unwrap();
                    if !default32 {
                        context.caches[1] = SegmentCache::from_bits(0xf1bf_ffff).unwrap();
                    }
                    let cs = CodeFetchContext::from_segments(&context).unwrap();
                    for policy in [DecodePolicy::Strict, DecodePolicy::Execution] {
                        let BoundDecode::Instruction(decoded) =
                            decode_bound(&m, GuestAddress(0x1000), cs, policy).unwrap()
                        else {
                            panic!("complete nop")
                        };
                        assert_eq!(decoded.operation(), &Operation::Nop);
                        assert_eq!(decoded.segment_accesses(), 0);
                        assert_eq!(usize::from(decoded.length()), bytes.len());
                        assert_eq!(decoded.next_pc(), GuestAddress(0x1000 + bytes.len() as u32));
                        assert!(m.is_code_current(decoded.code_snapshot()));
                    }
                }
            }
        }
    }
}
#[test]
fn every_original_sib_displacement_byte_is_fetched_and_snapshot_tracked() {
    let bytes = [0x0f, 0x19, 0x8c, 0x24, 0xef, 0xbe, 0xad, 0xde];
    let pc = 0x2001 - bytes.len() as u32;
    let mut m = executable(&bytes[..bytes.len() - 1], pc);
    assert!(
        matches!(decode_one(&m,GuestAddress(pc)),Err(DecodeError::Paging { attempted_length: length, error: crate::cpu::x86::CpuPagingError32::PageFault(_), .. }) if usize::from(length) == bytes.len())
    );
    let page = PageRange::new(GuestAddress(0x2000), 1).unwrap();
    m.map_zeroed(page, Permissions::ALL).unwrap();
    m.write(GuestAddress(0x2000), &bytes[bytes.len() - 1..])
        .unwrap();
    m.protect(page, Permissions::READ).unwrap();
    assert!(matches!(
        decode_for_execution(&m, GuestAddress(pc)),
        Err(DecodeError::Paging {
            error: crate::cpu::x86::CpuPagingError32::PageFault(_),
            ..
        })
    ));
    m.protect(page, Permissions::ALL).unwrap();
    let decoded = decode_one(&m, GuestAddress(pc)).unwrap();
    assert_eq!(decoded.operation(), &Operation::Nop);
    assert_eq!(decoded.code_snapshot().versions.len(), 2);
    assert_eq!(usize::from(decoded.length()), bytes.len());
    m.write(GuestAddress(0x2000), &[0xde]).unwrap();
    assert!(!m.is_code_current(decoded.code_snapshot()));
}
#[test]
fn lock_and_repeat_or_duplicate_prefix_boundaries_remain_distinct() {
    for (opcode, _, _) in groups() {
        let m = executable(&[0xf0, 0x0f, opcode, 0xc8], 0x1000);
        assert_eq!(
            decode_one(&m, GuestAddress(0x1000)).unwrap_err(),
            DecodeError::InvalidEncoding
        );
        assert_eq!(
            decode_for_execution(&m, GuestAddress(0x1000))
                .unwrap()
                .operation(),
            &Operation::Control(ControlOperation::InvalidOpcode)
        );
        for prefix in [&[0xf2][..], &[0xf3], &[0x66, 0x66]] {
            let bytes = [prefix, &[0x0f, opcode, 0xd0]].concat();
            let m = executable(&bytes, 0x1000);
            let err = decode_one(&m, GuestAddress(0x1000)).unwrap_err();
            assert!(matches!(err, DecodeError::Unsupported(_)));
            assert_eq!(
                decode_for_execution(&m, GuestAddress(0x1000)).unwrap_err(),
                err
            );
        }
    }
}
