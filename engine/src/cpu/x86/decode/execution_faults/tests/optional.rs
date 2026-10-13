use iced_x86::{Code, Decoder, DecoderOptions};

use crate::{
    cpu::x86::{
        ControlOperation,
        capabilities::CPU_ID_LEAVES,
        decode::{DecodeError, decode_for_execution, decode_one},
        ir::Operation,
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};

fn executable(bytes: &[u8], pc: u32) -> AddressSpace {
    let mut memory = AddressSpace::new(2).unwrap();
    let page = PageRange::new(GuestAddress(pc & !0xfff), 1).unwrap();
    memory.map_zeroed(page, Permissions::ALL).unwrap();
    memory.write(GuestAddress(pc), bytes).unwrap();
    memory.protect(page, Permissions::EXECUTE).unwrap();
    memory
}

fn rows() -> Vec<(Code, Vec<u8>)> {
    vec![
        (Code::Ptwrite_rm32, vec![0xf3, 0x0f, 0xae, 0xe0]),
        (Code::Xsaveopt_mem, vec![0x0f, 0xae, 0x30]),
        (Code::Clwb_m8, vec![0x66, 0x0f, 0xae, 0x30]),
        (Code::Tpause_r32, vec![0x66, 0x0f, 0xae, 0xf0]),
        (Code::Umonitor_r16, vec![0x67, 0xf3, 0x0f, 0xae, 0xf0]),
        (Code::Umonitor_r32, vec![0xf3, 0x0f, 0xae, 0xf0]),
        (Code::Umwait_r32, vec![0xf2, 0x0f, 0xae, 0xf0]),
        (Code::Clflushopt_m8, vec![0x66, 0x0f, 0xae, 0x38]),
        (Code::Xsavec_mem, vec![0x0f, 0xc7, 0x20]),
        (Code::Sha1nexte_xmm_xmmm128, vec![0x0f, 0x38, 0xc8, 0xc1]),
        (Code::Sha1msg1_xmm_xmmm128, vec![0x0f, 0x38, 0xc9, 0xc1]),
        (Code::Sha1msg2_xmm_xmmm128, vec![0x0f, 0x38, 0xca, 0xc1]),
        (Code::Sha256rnds2_xmm_xmmm128, vec![0x0f, 0x38, 0xcb, 0xc1]),
        (Code::Sha256msg1_xmm_xmmm128, vec![0x0f, 0x38, 0xcc, 0xc1]),
        (Code::Sha256msg2_xmm_xmmm128, vec![0x0f, 0x38, 0xcd, 0xc1]),
        (
            Code::Gf2p8mulb_xmm_xmmm128,
            vec![0x66, 0x0f, 0x38, 0xcf, 0xc1],
        ),
        (
            Code::VEX_Vgf2p8mulb_xmm_xmm_xmmm128,
            vec![0xc4, 0xe2, 0x71, 0xcf, 0xc2],
        ),
        (
            Code::VEX_Vgf2p8mulb_ymm_ymm_ymmm256,
            vec![0xc4, 0xe2, 0x75, 0xcf, 0xc2],
        ),
        (Code::Adcx_r32_rm32, vec![0x66, 0x0f, 0x38, 0xf6, 0xc1]),
        (Code::Adox_r32_rm32, vec![0xf3, 0x0f, 0x38, 0xf6, 0xc1]),
        (
            Code::Movdir64b_r16_m512,
            vec![0x67, 0x66, 0x0f, 0x38, 0xf8, 0x08],
        ),
        (Code::Movdir64b_r32_m512, vec![0x66, 0x0f, 0x38, 0xf8, 0x08]),
        (Code::Movdiri_m32_r32, vec![0x0f, 0x38, 0xf9, 0x08]),
        (
            Code::Sha1rnds4_xmm_xmmm128_imm8,
            vec![0x0f, 0x3a, 0xcc, 0xc1, 0x00],
        ),
        (
            Code::Gf2p8affineqb_xmm_xmmm128_imm8,
            vec![0x66, 0x0f, 0x3a, 0xce, 0xc1, 0x00],
        ),
        (
            Code::VEX_Vgf2p8affineqb_xmm_xmm_xmmm128_imm8,
            vec![0xc4, 0xe3, 0xf1, 0xce, 0xc2, 0x00],
        ),
        (
            Code::VEX_Vgf2p8affineqb_ymm_ymm_ymmm256_imm8,
            vec![0xc4, 0xe3, 0xf5, 0xce, 0xc2, 0x00],
        ),
        (
            Code::Gf2p8affineinvqb_xmm_xmmm128_imm8,
            vec![0x66, 0x0f, 0x3a, 0xcf, 0xc1, 0x00],
        ),
        (
            Code::VEX_Vgf2p8affineinvqb_xmm_xmm_xmmm128_imm8,
            vec![0xc4, 0xe3, 0xf1, 0xcf, 0xc2, 0x00],
        ),
        (
            Code::VEX_Vgf2p8affineinvqb_ymm_ymm_ymmm256_imm8,
            vec![0xc4, 0xe3, 0xf5, 0xcf, 0xc2, 0x00],
        ),
        (Code::Serialize, vec![0x0f, 0x01, 0xe8]),
    ]
}

fn assert_ud(bytes: &[u8], code: Code, pc: u32) {
    let mut raw = Decoder::with_ip(32, bytes, u64::from(pc), DecoderOptions::NONE);
    let instruction = raw.decode();
    assert_eq!(instruction.code(), code, "{bytes:02x?}");
    assert_eq!(instruction.len(), bytes.len());
    assert!(!instruction.has_rep_prefix() && !instruction.has_repne_prefix());
    let memory = executable(bytes, pc);
    assert!(matches!(
        decode_one(&memory, GuestAddress(pc)),
        Err(DecodeError::Unsupported(_))
    ));
    let decoded = decode_for_execution(&memory, GuestAddress(pc)).unwrap();
    assert_eq!(
        decoded.operation(),
        &Operation::Control(ControlOperation::InvalidOpcode)
    );
    assert_eq!(decoded.length() as usize, bytes.len());
    assert_eq!(decoded.next_pc(), GuestAddress(pc + bytes.len() as u32));
    assert_eq!(decoded.segment_accesses(), 0);
    assert!(memory.is_code_current(decoded.code_snapshot()));
}

#[test]
fn exact_optional_application_codes_fault_when_their_virtual_feature_is_absent() {
    for (code, bytes) in rows() {
        assert_ud(&bytes, code, 0x2000 - bytes.len() as u32);
        if bytes
            .last()
            .is_some_and(|byte| *byte == 0xc1 || *byte == 0xc2 || *byte == 0xe0)
            || matches!(
                code,
                Code::Sha1rnds4_xmm_xmmm128_imm8
                    | Code::Gf2p8affineqb_xmm_xmmm128_imm8
                    | Code::VEX_Vgf2p8affineqb_xmm_xmm_xmmm128_imm8
                    | Code::VEX_Vgf2p8affineqb_ymm_ymm_ymmm256_imm8
                    | Code::Gf2p8affineinvqb_xmm_xmmm128_imm8
                    | Code::VEX_Vgf2p8affineinvqb_xmm_xmm_xmmm128_imm8
                    | Code::VEX_Vgf2p8affineinvqb_ymm_ymm_ymmm256_imm8
            )
        {
            let mut memory_form = bytes.clone();
            let modrm = memory_form.len() - if memory_form.last() == Some(&0) { 2 } else { 1 };
            memory_form[modrm] &= 0x3f;
            assert_ud(&memory_form, code, 0x1000);
        }
        let mut overridden = vec![0x64, 0x67];
        overridden.extend(&bytes);
        // umonitor's address-selected Code changes under a second 67, so retain its exact rows.
        if !matches!(
            code,
            Code::Umonitor_r16
                | Code::Umonitor_r32
                | Code::Movdir64b_r16_m512
                | Code::Movdir64b_r32_m512
        ) {
            assert_ud(&overridden, code, 0x1000);
        }
    }
}

#[test]
fn optional_instruction_fetch_precedes_ud_until_the_final_byte_is_executable() {
    for (code, bytes) in rows() {
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
            &Operation::Control(ControlOperation::InvalidOpcode),
            "{code:?}"
        );
        assert_eq!(decoded.segment_accesses(), 0);
        assert!(memory.is_code_current(decoded.code_snapshot()));
    }
}

#[test]
fn absent_cldemote_and_shadow_stack_reads_are_nops_without_operand_access() {
    let leaf = CPU_ID_LEAVES
        .iter()
        .find(|leaf| leaf.function == 7 && leaf.subfunction == Some(0))
        .unwrap();
    assert_eq!(leaf.registers[2] & ((1 << 7) | (1 << 25)), 0);
    for bytes in [
        &[0x0f, 0x1c, 0x00][..],
        &[0x0f, 0x1c, 0x05, 0xff, 0xff, 0xff, 0xff],
        &[0x36, 0x0f, 0x1c, 0x04, 0x24],
        &[0xf3, 0x0f, 0x1e, 0xc8],
        &[0xf3, 0x0f, 0x1e, 0xcf],
    ] {
        let memory = executable(bytes, 0x1000);
        for decoded in [
            decode_one(&memory, GuestAddress(0x1000)).unwrap(),
            decode_for_execution(&memory, GuestAddress(0x1000)).unwrap(),
        ] {
            assert_eq!(decoded.operation(), &Operation::Nop);
            assert_eq!(decoded.length() as usize, bytes.len());
            assert_eq!(decoded.segment_accesses(), 0);
            assert!(memory.is_code_current(decoded.code_snapshot()));
        }
    }
}

#[test]
fn optional_table_keeps_other_features_hints_and_prefix_boundaries_unchanged() {
    for bytes in [
        &[0x66, 0x66, 0x0f, 0x38, 0xf6, 0xc1][..],
        &[0xf3, 0xf3, 0x0f, 0x38, 0xf6, 0xc1],
        &[0xf2, 0x0f, 0xaa],
        &[0xf3, 0x0f, 0xaa],
        &[0x0f, 0x01, 0xee],
        &[0x0f, 0x0d, 0x08],
        &[0xcd, 0x80],
    ] {
        let memory = executable(bytes, 0x1000);
        let strict = decode_one(&memory, GuestAddress(0x1000)).unwrap_err();
        assert!(
            matches!(strict, DecodeError::Unsupported(_)),
            "{bytes:02x?}"
        );
        assert_eq!(
            decode_for_execution(&memory, GuestAddress(0x1000)).unwrap_err(),
            strict
        );
    }
    for bytes in [&[0xf2, 0x0f, 0x1c, 0x00][..], &[0xf3, 0x0f, 0x1c, 0x00]] {
        let memory = executable(bytes, 0x1000);
        match decode_one(&memory, GuestAddress(0x1000)).unwrap_err() {
            error @ DecodeError::Unsupported(_) => assert_eq!(
                decode_for_execution(&memory, GuestAddress(0x1000)).unwrap_err(),
                error
            ),
            DecodeError::InvalidEncoding => assert_eq!(
                decode_for_execution(&memory, GuestAddress(0x1000))
                    .unwrap()
                    .operation(),
                &Operation::Control(ControlOperation::InvalidOpcode)
            ),
            error => panic!("unexpected prefixed hint refusal: {error:?}"),
        }
    }
    for bytes in [
        &[0xf3, 0x0f, 0x1e, 0xfb][..],
        &[0xf3, 0x90],
        &[0x0f, 0x18, 0x00],
        &[0xf3, 0x0f, 0xbc, 0xc1],
        &[0xf3, 0x0f, 0xbd, 0xc1],
        &[0x66, 0x0f, 0x38, 0xdc, 0xc1],
        &[0xc4, 0xe2, 0x71, 0xdc, 0xc2],
        &[0xc4, 0xe3, 0x71, 0x44, 0xc2, 0x00],
    ] {
        let memory = executable(bytes, 0x1000);
        let strict = decode_one(&memory, GuestAddress(0x1000)).unwrap();
        assert_eq!(
            decode_for_execution(&memory, GuestAddress(0x1000))
                .unwrap()
                .operation(),
            strict.operation()
        );
    }
    for bytes in [
        &[0xf0, 0x0f, 0x1c, 0x00][..],
        &[0xf0, 0xf3, 0x0f, 0x1e, 0xc8],
    ] {
        let memory = executable(bytes, 0x1000);
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
