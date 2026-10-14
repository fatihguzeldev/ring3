use iced_x86::{Code, Decoder, DecoderOptions, EncodingKind};

use crate::{
    cpu::{
        UnsupportedFeature,
        dbt::{BlockSpec, CompileError, CompileLimits, InstructionError, compile_region},
        x86::{
            ControlOperation,
            decode::{DecodeError, decode_for_execution, decode_one},
            ir::Operation,
        },
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

fn assert_unavailable(memory: &AddressSpace, pc: u32, length: usize, strict: DecodeError) {
    assert_eq!(decode_one(memory, GuestAddress(pc)).unwrap_err(), strict);
    assert_eq!(
        compile_region(
            memory,
            &[BlockSpec {
                entry: GuestAddress(pc),
                byte_length: length as u32,
            }],
            CompileLimits::default(),
        )
        .err(),
        Some(CompileError::Instruction {
            pc: GuestAddress(pc),
            cause: InstructionError::Decode(strict),
        })
    );
    let instruction = decode_for_execution(memory, GuestAddress(pc))
        .expect("known unavailable instruction must produce an architectural execution fault");
    assert_eq!(
        instruction.operation(),
        &Operation::Control(ControlOperation::InvalidOpcode)
    );
    assert_eq!(instruction.length() as usize, length);
    assert_eq!(
        instruction.next_pc(),
        GuestAddress(pc.wrapping_add(length as u32))
    );
    assert_eq!(instruction.segment_accesses(), 0);
    assert!(memory.is_code_current(instruction.code_snapshot()));
}

#[test]
fn rsm_outside_smm_faults_on_execution_while_strict_apis_keep_refusal() {
    for prefix in [&[][..], &[0x66], &[0x67], &[0x64], &[0x36]] {
        let mut bytes = prefix.to_vec();
        bytes.extend([0x0f, 0xaa]);
        let pc = 0x2000 - bytes.len() as u32;
        let memory = executable(&bytes, pc);
        assert_unavailable(
            &memory,
            pc,
            bytes.len(),
            DecodeError::Unsupported(UnsupportedFeature::Privileged),
        );
    }
}

#[test]
fn known_evex_vector_widths_fault_before_register_or_memory_operands() {
    for vector_length in [0x08, 0x28, 0x48] {
        for operand in [0xc1, 0x00] {
            let bytes = [0x62, 0xf1, 0x7c, vector_length, 0x58, operand];
            let pc = 0x2000 - bytes.len() as u32;
            let memory = executable(&bytes, pc);
            assert_unavailable(
                &memory,
                pc,
                bytes.len(),
                DecodeError::Unsupported(UnsupportedFeature::Simd),
            );
        }
    }
    // memory addressing bytes are part of the instruction snapshot, not a ram access.
    for (bytes, strict) in [
        (
            &[0x62, 0xf1, 0x7c, 0x48, 0x58, 0x05, 0x78, 0x56, 0x34, 0x12][..],
            UnsupportedFeature::Simd,
        ),
        // this vsib prefetch has only a memory operand, with zmm3 as its vector index.
        (
            &[0x62, 0xf2, 0x7d, 0x49, 0xc6, 0x0c, 0x18],
            UnsupportedFeature::Opcode,
        ),
    ] {
        let memory = executable(bytes, 0x1000);
        assert_unavailable(
            &memory,
            0x1000,
            bytes.len(),
            DecodeError::Unsupported(strict),
        );
    }
}

#[test]
fn known_vex_opmask_forms_fault_before_register_or_memory_operands() {
    for (bytes, code) in [
        (&[0xc5, 0xec, 0x47, 0xcb][..], Code::VEX_Kxorw_kr_kr_kr),
        (&[0xc5, 0xf8, 0x90, 0x0b], Code::VEX_Kmovw_kr_km16),
        (&[0xc5, 0xf8, 0x91, 0x0b], Code::VEX_Kmovw_m16_kr),
    ] {
        let pc = 0x2000 - bytes.len() as u32;
        let mut decoder = Decoder::with_ip(32, bytes, u64::from(pc), DecoderOptions::NONE);
        let instruction = decoder.decode();
        assert!(!instruction.is_invalid());
        assert_eq!(instruction.code(), code);
        assert_eq!(instruction.encoding(), EncodingKind::VEX);
        assert_eq!(instruction.len(), bytes.len());
        let memory = executable(bytes, pc);
        assert_unavailable(
            &memory,
            pc,
            bytes.len(),
            DecodeError::Unsupported(UnsupportedFeature::Simd),
        );
    }
}

#[test]
fn instruction_fetch_fault_precedes_unavailability_until_final_byte_is_repaired() {
    for (bytes, strict) in [
        (&[0x0f, 0xaa][..], UnsupportedFeature::Privileged),
        (
            &[0x62, 0xf1, 0x7c, 0x48, 0x58, 0xc1],
            UnsupportedFeature::Simd,
        ),
        (
            &[0x62, 0xf1, 0x7c, 0x48, 0x58, 0x05, 0x78, 0x56, 0x34, 0x12],
            UnsupportedFeature::Simd,
        ),
        (
            &[0xc5, 0xf8, 0x90, 0x0d, 0x12, 0x34, 0x56, 0x78],
            UnsupportedFeature::Simd,
        ),
    ] {
        let pc = 0x2000 - (bytes.len() as u32 - 1);
        let mut memory = executable(&bytes[..bytes.len() - 1], pc);
        let missing = decode_one(&memory, GuestAddress(pc)).unwrap_err();
        assert!(matches!(
            missing,
            DecodeError::Paging { attempted_length: length, error: crate::cpu::x86::CpuPagingError32::PageFault(_), .. } if usize::from(length) == bytes.len()
        ));
        assert_eq!(
            decode_for_execution(&memory, GuestAddress(pc)).unwrap_err(),
            missing
        );
        let second = PageRange::new(GuestAddress(0x2000), 1).unwrap();
        memory.map_zeroed(second, Permissions::ALL).unwrap();
        memory
            .write(GuestAddress(0x2000), &bytes[bytes.len() - 1..])
            .unwrap();
        memory.protect(second, Permissions::READ).unwrap();
        let denied = decode_one(&memory, GuestAddress(pc)).unwrap_err();
        assert!(matches!(
            denied,
            DecodeError::Paging { attempted_length: length, error: crate::cpu::x86::CpuPagingError32::PageFault(_), .. } if usize::from(length) == bytes.len()
        ));
        assert_eq!(
            decode_for_execution(&memory, GuestAddress(pc)).unwrap_err(),
            denied
        );
        memory.protect(second, Permissions::EXECUTE).unwrap();
        assert_unavailable(&memory, pc, bytes.len(), DecodeError::Unsupported(strict));
    }
}

#[test]
fn availability_does_not_replace_present_paths_unknown_features_or_profile_refusals() {
    // legacy bound, present vex vectors and vex gpr forms retain their execution paths.
    for bytes in [
        &[0x62, 0x00][..],
        &[0xc5, 0xfa, 0x58, 0xc1],
        &[0xc4, 0xe3, 0x7b, 0xf0, 0xc1, 0x04],
    ] {
        let memory = executable(bytes, 0x1000);
        let strict = decode_one(&memory, GuestAddress(0x1000)).unwrap();
        let runtime = decode_for_execution(&memory, GuestAddress(0x1000)).unwrap();
        assert_eq!(runtime.operation(), strict.operation());
        assert!(!matches!(
            runtime.operation(),
            Operation::Control(ControlOperation::InvalidOpcode)
        ));
        assert_eq!(runtime.length(), strict.length());
    }
    for bytes in [
        &[0xf2, 0x0f, 0xaa][..],
        &[0xf3, 0x0f, 0xaa],
        &[0x66, 0x66, 0x0f, 0xaa],
    ] {
        let memory = executable(bytes, 0x1000);
        let strict = decode_one(&memory, GuestAddress(0x1000)).unwrap_err();
        assert!(matches!(strict, DecodeError::Unsupported(_)));
        assert_eq!(
            decode_for_execution(&memory, GuestAddress(0x1000)).unwrap_err(),
            strict
        );
    }
    let locked = [0xf0, 0x0f, 0xaa];
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
