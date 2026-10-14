use super::required;
use crate::{
    cpu::x86::{
        ControlOperation,
        capabilities::CPU_ID_LEAVES,
        decode::{DecodeError, decode_for_execution, decode_one},
        ir::Operation,
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};
use iced_x86::{Code, CpuidFeature as F, Decoder, DecoderOptions, Instruction};

#[test]
fn qualified_amd_prerequisites_match_the_immutable_extended_leaf() {
    for (feature, register, bit) in [
        (F::SSE4A, 2, 6),
        (F::D3NOW, 3, 31),
        (F::D3NOWEXT, 3, 30),
        (F::XOP, 2, 11),
        (F::FMA4, 2, 16),
    ] {
        assert_eq!(required(feature), Some((0x8000_0001, 0, register, bit)));
        let leaf = CPU_ID_LEAVES
            .iter()
            .find(|leaf| leaf.function == 0x8000_0001)
            .unwrap();
        assert_eq!(
            leaf.subfunction, None,
            "extended leaf ignores ECX subfunction"
        );
        assert_eq!(leaf.registers[register] & (1 << bit), 0);
    }
    for feature in [F::TBM, F::FMA, F::PREFETCHW, F::MMX] {
        assert_eq!(required(feature), None);
    }
}

#[test]
fn pinned_decoder_metadata_has_only_the_five_qualified_single_requirements() {
    let tags = [F::SSE4A, F::D3NOW, F::D3NOWEXT, F::XOP, F::FMA4];
    let mut counts = [0; 5];
    for code in Code::values() {
        let mut instruction = Instruction::default();
        instruction.set_code(code);
        for (index, feature) in tags.iter().enumerate() {
            if instruction.cpuid_features().contains(feature) {
                assert_eq!(instruction.cpuid_features(), &[*feature]);
                assert!(required(*feature).is_some());
                counts[index] += 1;
            }
        }
    }
    assert_eq!(
        counts,
        [6, 20, 5, 83, 64],
        "review changed decoder metadata before expanding this policy"
    );
    assert_eq!(counts.iter().sum::<u32>(), 178);
}

fn executable(bytes: &[u8], pc: u32) -> AddressSpace {
    let mut memory = AddressSpace::new(2).unwrap();
    let page = PageRange::new(GuestAddress(pc & !0xfff), 1).unwrap();
    memory.map_zeroed(page, Permissions::ALL).unwrap();
    memory.write(GuestAddress(pc), bytes).unwrap();
    memory.protect(page, Permissions::EXECUTE).unwrap();
    memory
}

#[test]
fn exact_absent_forms_fault_before_operand_or_numeric_lowering() {
    for (feature, bytes) in [
        (F::SSE4A, &[0xf3, 0x0f, 0x2b, 0x00][..]),
        (F::D3NOW, &[0x0f, 0x0e]),
        (F::D3NOW, &[0x36, 0x0f, 0x0f, 0x04, 0x24, 0x9e]),
        (F::D3NOWEXT, &[0x0f, 0x0f, 0xc1, 0xbb]),
        (F::XOP, &[0x64, 0x8f, 0xe9, 0x78, 0x90, 0x00]),
        (F::XOP, &[0xc4, 0xe3, 0x79, 0x48, 0xc1, 0x20]),
        (F::FMA4, &[0xc4, 0xe3, 0x79, 0x68, 0xc1, 0x20]),
        (F::FMA4, &[0xc4, 0xe3, 0xfd, 0x68, 0x00, 0x20]),
    ] {
        let raw = Decoder::new(32, bytes, DecoderOptions::NONE).decode();
        assert_eq!(raw.cpuid_features(), &[feature]);
        assert_eq!(raw.len(), bytes.len());
        let memory = executable(bytes, 0x1000);
        assert!(matches!(
            decode_one(&memory, GuestAddress(0x1000)),
            Err(DecodeError::Unsupported(_))
        ));
        let decoded = decode_for_execution(&memory, GuestAddress(0x1000)).unwrap();
        assert_eq!(
            decoded.operation(),
            &Operation::Control(ControlOperation::InvalidOpcode)
        );
        assert_eq!(decoded.segment_accesses(), 0);
        assert_eq!(usize::from(decoded.length()), bytes.len());
        assert!(memory.is_code_current(decoded.code_snapshot()));
    }
}

#[test]
fn prefix_and_neighbor_codes_do_not_become_generic_amd_faults() {
    for bytes in [
        &[0x0f, 0x77][..],
        &[0xc4, 0xe2, 0x79, 0x98, 0xc1],
        &[0x0f, 0x0d, 0x00],
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
        &[0x8f, 0xea, 0x78, 0x10, 0xc1, 0, 0, 0, 0][..],
        &[0x66, 0x66, 0x0f, 0x78, 0xc0, 4, 8],
    ] {
        let memory = executable(bytes, 0x1000);
        let strict = decode_one(&memory, GuestAddress(0x1000)).unwrap_err();
        assert!(matches!(strict, DecodeError::Unsupported(_)));
        assert_eq!(
            decode_for_execution(&memory, GuestAddress(0x1000)).unwrap_err(),
            strict
        );
    }
    let memory = executable(&[0xf0, 0x8f, 0xe9, 0x78, 0x90, 0xc1], 0x1000);
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

#[test]
fn additional_lwp_and_broadcast_tlb_prerequisites_are_exact() {
    assert_eq!(required(F::LWP), Some((0x8000_0001, 0, 2, 15)));
    assert_eq!(required(F::INVLPGB), Some((0x8000_0008, 0, 1, 3)));
    assert_eq!(required(F::WBNOINVD), None);
    assert_eq!(required(F::PCONFIG), None);
    assert!(
        !CPU_ID_LEAVES
            .iter()
            .any(|leaf| leaf.function == 0x8000_0001 && leaf.registers[2] & (1 << 15) != 0)
    );
    assert!(
        !CPU_ID_LEAVES
            .iter()
            .any(|leaf| leaf.function == 0x8000_0008 && leaf.registers[1] & (1 << 3) != 0)
    );
    assert!(!CPU_ID_LEAVES.iter().any(|leaf| leaf.function == 7
        && leaf.subfunction == Some(0)
        && leaf.registers[3] & (1 << 18) != 0));
    let mut counts = [0; 2];
    for code in Code::values() {
        let mut instruction = Instruction::default();
        instruction.set_code(code);
        for (index, feature) in [F::LWP, F::INVLPGB].iter().enumerate() {
            if instruction.cpuid_features().contains(feature) {
                assert_eq!(instruction.cpuid_features(), &[*feature]);
                counts[index] += 1;
            }
        }
    }
    // includes unreachable r64/q metadata companions; protected16/32 yields only seven selected codes.
    assert_eq!(counts, [8, 4]);
}
