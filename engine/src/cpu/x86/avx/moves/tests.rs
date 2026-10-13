use super::*;

fn register(index: u8) -> VectorRegister { VectorRegister::new(index).unwrap() }

fn state() -> (VectorState, State32) {
    let mut vector = VectorState::default();
    vector.simd.mxcsr = 0xe5af;
    for index in 0..8 {
        vector.simd.xmm[index] = std::array::from_fn(|byte| (index * 37 + byte * 19 + 7) as u8);
        vector.upper[index] = std::array::from_fn(|byte| (index * 41 + byte * 23 + 149) as u8);
    }
    let general = State32 {
        registers: [0x8000_0000, 0x1234_5678, 0xabcd_ef12, 0xfeed_face, 0x8000_ffff, 0x7fc1_2345, 0xff80_0001, 0x0123_4567],
        eip: 0x1234_5678,
        eflags: 0x244cd7,
    };
    (vector, general)
}

fn expected_vector(mut original: VectorState, destination: VectorRegister, value: [u8; 32]) -> VectorState {
    original.simd.xmm[destination.index()].copy_from_slice(&value[..16]);
    original.upper[destination.index()].copy_from_slice(&value[16..]);
    original
}

#[test]
fn copy_scalar_and_half_merge_capture_all_alias_partitions_and_zero_upper() {
    for (d, a, b) in [(0, 1, 2), (0, 0, 2), (0, 1, 0), (0, 1, 1), (0, 0, 0)] {
        let (destination, merge, source) = (register(d), register(a), register(b));
        for width in [VectorWidth::Xmm, VectorWidth::Ymm] {
            let (mut vector, mut general) = state(); let before = vector; let before_general = general;
            let mut expected = [0; 32]; expected[..width.bytes()].copy_from_slice(&before.read(source)[..width.bytes()]);
            vector.execute_move(MoveInstruction::Copy { width, destination, source }, &mut general, 0, &mut []).unwrap();
            assert_eq!(vector, expected_vector(before, destination, expected)); assert_eq!(general, before_general);
        }
        for element in [Element::Single, Element::Double] {
            let (mut vector, mut general) = state(); let before = vector; let before_general = general;
            let first = before.read(merge); let second = before.read(source);
            let expected = std::array::from_fn(|index| if index < element.bytes() { second[index] } else if index < 16 { first[index] } else { 0 });
            vector.execute_move(MoveInstruction::Scalar { element, destination, merge, source }, &mut general, 0, &mut []).unwrap();
            assert_eq!(vector, expected_vector(before, destination, expected)); assert_eq!(general, before_general);
        }
        for kind in [HalfMerge::HighToLow, HalfMerge::LowToHigh] {
            let (mut vector, mut general) = state(); let before = vector; let before_general = general;
            let first = before.read(merge); let second = before.read(source);
            let expected = std::array::from_fn(|index| match (kind, index) {
                (HalfMerge::HighToLow, 0..8) => second[index + 8],
                (HalfMerge::HighToLow, 8..16) => first[index],
                (HalfMerge::LowToHigh, 0..8) => first[index],
                (HalfMerge::LowToHigh, 8..16) => second[index - 8],
                _ => 0,
            });
            vector.execute_move(MoveInstruction::MergeHalf { kind, destination, merge, source }, &mut general, 0, &mut []).unwrap();
            assert_eq!(vector, expected_vector(before, destination, expected)); assert_eq!(general, before_general);
        }
    }
}

#[test]
fn duplicate_lanes_use_each_original_128_bit_half_with_exact_memory_width() {
    for width in [VectorWidth::Xmm, VectorWidth::Ymm] {
        for kind in [DuplicateKind::SingleLow, DuplicateKind::SingleHigh, DuplicateKind::DoubleLow] {
            for source in [MoveSource::Register(register(0)), MoveSource::Register(register(1)), MoveSource::Memory] {
                let (mut vector, mut general) = state(); let before = vector; let before_general = general;
                let operation = MoveInstruction::Duplicate { kind, width, destination: register(0), source };
                let mut operand: Vec<u8> = (0..operation.memory().map_or(0, |r| r.bytes)).map(|i| i.wrapping_mul(29).wrapping_add(3)).collect();
                let original_operand = operand.clone();
                let input: Vec<u8> = match source { MoveSource::Register(r) => before.read(r).to_vec(), MoveSource::Memory => operand.clone() };
                let mut expected = [0; 32];
                for (index, byte) in expected.iter_mut().enumerate().take(width.bytes()) {
                    let half = index / 16 * 16; let local = index % 16;
                    let offset = match kind { DuplicateKind::SingleLow => (local / 8 * 8) + local % 4, DuplicateKind::SingleHigh => (local / 8 * 8) + 4 + local % 4, DuplicateKind::DoubleLow => local % 8 };
                    *byte = input[half + offset];
                }
                vector.execute_move(operation, &mut general, 0x1003, &mut operand).unwrap();
                assert_eq!(vector, expected_vector(before, register(0), expected)); assert_eq!(general, before_general); assert_eq!(operand, original_operand);
                if source == MoveSource::Memory { assert_eq!(operation.memory().unwrap().bytes as usize, if kind == DuplicateKind::DoubleLow && width == VectorWidth::Xmm { 8 } else { width.bytes() }); }
            }
        }
    }
}

#[test]
fn memory_loads_zero_or_merge_and_stores_preserve_all_cpu_state() {
    let destination = register(0); let merge = register(0); let source = register(2);
    let loads = [
        MoveInstruction::Load { width: VectorWidth::Xmm, destination, aligned: false },
        MoveInstruction::Load { width: VectorWidth::Ymm, destination, aligned: true },
        MoveInstruction::LoadScalar { element: Element::Single, destination },
        MoveInstruction::LoadScalar { element: Element::Double, destination },
        MoveInstruction::LoadHalf { half: Half::Low, destination, merge },
        MoveInstruction::LoadHalf { half: Half::High, destination, merge },
        MoveInstruction::QwordLoad { destination, source: MoveSource::Memory },
        MoveInstruction::DwordLoad { destination, source: WordSource::Memory },
    ];
    for operation in loads {
        assert_eq!(operation.memory().unwrap().access, Access::Read);
        let (mut vector, mut general) = state(); let before = vector; let before_general = general;
        let mut operand: Vec<u8> = (0..operation.memory().unwrap().bytes).map(|i| i.wrapping_mul(31).wrapping_add(5)).collect();
        let original = operand.clone(); let mut expected = [0; 32];
        if let MoveInstruction::LoadHalf { half, .. } = operation {
            expected[..16].copy_from_slice(&before.simd.xmm[0]);
            let at = if half == Half::Low { 0 } else { 8 }; expected[at..at + 8].copy_from_slice(&operand);
        } else { expected[..operand.len()].copy_from_slice(&operand); }
        vector.execute_move(operation, &mut general, 0x1000, &mut operand).unwrap();
        assert_eq!(vector, expected_vector(before, destination, expected)); assert_eq!(general, before_general); assert_eq!(operand, original);
    }
    let stores = [
        MoveInstruction::Store { width: VectorWidth::Xmm, source, aligned: true },
        MoveInstruction::Store { width: VectorWidth::Ymm, source, aligned: false },
        MoveInstruction::StoreScalar { element: Element::Single, source },
        MoveInstruction::StoreScalar { element: Element::Double, source },
        MoveInstruction::StoreHalf { half: Half::Low, source },
        MoveInstruction::StoreHalf { half: Half::High, source },
        MoveInstruction::QwordStore { source },
        MoveInstruction::DwordStore { source, destination: WordTarget::Memory },
    ];
    for operation in stores {
        assert_eq!(operation.memory().unwrap().access, Access::Write);
        let (mut vector, mut general) = state(); let before = vector; let before_general = general;
        let mut guarded = [0xa5; 40]; let bytes = usize::from(operation.memory().unwrap().bytes);
        vector.execute_move(operation, &mut general, 0x1000, &mut guarded[4..4 + bytes]).unwrap();
        let offset = if matches!(operation, MoveInstruction::StoreHalf { half: Half::High, .. }) { 8 } else { 0 };
        assert_eq!(&guarded[4..4 + bytes], &before.read(source)[offset..offset + bytes]);
        assert_eq!(&guarded[..4], &[0xa5; 4]); assert!(guarded[4 + bytes..].iter().all(|b| *b == 0xa5));
        assert_eq!(vector, before); assert_eq!(general, before_general);
    }
}

#[test]
fn dword_qword_and_sign_masks_preserve_raw_bits_and_unrelated_registers() {
    for destination in [Register32::Eax, Register32::Ecx, Register32::Edx, Register32::Ebx, Register32::Esp, Register32::Ebp, Register32::Esi, Register32::Edi] {
        let (mut vector, mut general) = state(); let before = vector; let before_general = general;
        vector.execute_move(MoveInstruction::DwordLoad { destination: register(0), source: WordSource::Register(destination) }, &mut general, 0, &mut []).unwrap();
        let mut expected = [0; 32]; expected[..4].copy_from_slice(&before_general.registers[destination.index()].to_le_bytes());
        assert_eq!(vector, expected_vector(before, register(0), expected)); assert_eq!(general, before_general);
        vector = before;
        vector.execute_move(MoveInstruction::DwordStore { source: register(2), destination: WordTarget::Register(destination) }, &mut general, 0, &mut []).unwrap();
        let mut expected = before_general; expected.registers[destination.index()] = u32::from_le_bytes(before.simd.xmm[2][..4].try_into().unwrap());
        assert_eq!(general, expected); assert_eq!(vector, before);
        for element in [Element::Single, Element::Double] {
            for width in [VectorWidth::Xmm, VectorWidth::Ymm] {
                general = before_general;
                vector.execute_move(MoveInstruction::Mask { element, width, destination, source: register(2) }, &mut general, 0, &mut []).unwrap();
                let input = before.read(register(2)); let mut expected = before_general;
                let literal_positions: &[usize] = match (element, width) { (Element::Single, VectorWidth::Xmm) => &[3, 7, 11, 15], (Element::Single, VectorWidth::Ymm) => &[3, 7, 11, 15, 19, 23, 27, 31], (Element::Double, VectorWidth::Xmm) => &[7, 15], (Element::Double, VectorWidth::Ymm) => &[7, 15, 23, 31] };
                expected.registers[destination.index()] = literal_positions.iter().enumerate().fold(0, |mask, (bit, &position)| mask | (u32::from(input[position] >= 128) << bit));
                assert_eq!(general, expected); assert_eq!(vector, before);
            }
        }
    }
    for source in [register(0), register(1)] {
        let (mut vector, mut general) = state(); let before = vector; let before_general = general;
        let mut expected = [0; 32]; expected[..8].copy_from_slice(&before.read(source)[..8]);
        vector.execute_move(MoveInstruction::QwordLoad { destination: register(0), source: MoveSource::Register(source) }, &mut general, 0, &mut []).unwrap();
        assert_eq!(vector, expected_vector(before, register(0), expected)); assert_eq!(general, before_general);
    }
}

#[test]
fn operand_and_alignment_refusal_is_neutral_and_control_reserved_bits_are_rejected() {
    for width in [VectorWidth::Xmm, VectorWidth::Ymm] {
        for operation in [MoveInstruction::Load { width, destination: register(0), aligned: true }, MoveInstruction::Store { width, source: register(0), aligned: true }] {
            for (address, length, error) in [(0x1000, width.bytes() - 1, MoveError::OperandLength), (0x1000, width.bytes() + 1, MoveError::OperandLength), (0x1001, width.bytes(), MoveError::Alignment), (0x1000 + width.bytes() as u32 / 2, width.bytes(), MoveError::Alignment)] {
                let (mut vector, mut general) = state(); let before = (vector, general); let mut operand = vec![0x91; length]; let original = operand.clone();
                assert_eq!(vector.execute_move(operation, &mut general, address, &mut operand), Err(error));
                assert_eq!((vector, general), before); assert_eq!(operand, original);
            }
        }
    }
    for control in [0_u32, 0x1f80, 0xffff, 0x1_0000, 0x8000_1f80] {
        let (mut vector, mut general) = state(); let before = (vector, general); let mut operand = control.to_le_bytes();
        let result = vector.execute_move(MoveInstruction::LoadControl, &mut general, 0x1001, &mut operand);
        if control > 0xffff { assert_eq!(result, Err(MoveError::InvalidControl)); assert_eq!((vector, general), before); }
        else { result.unwrap(); let mut expected = before.0; expected.simd.mxcsr = control; assert_eq!(vector, expected); assert_eq!(general, before.1); }
        assert_eq!(operand, control.to_le_bytes());
    }
    let (mut vector, mut general) = state(); let before = (vector, general); let mut operand = [0; 4];
    vector.execute_move(MoveInstruction::StoreControl, &mut general, 0x1001, &mut operand).unwrap();
    assert_eq!(operand, before.0.simd.mxcsr.to_le_bytes()); assert_eq!((vector, general), before);
    vector.simd.mxcsr = 0x1_0000; let before = (vector, general);
    assert_eq!(vector.execute_move(MoveInstruction::ZeroAll, &mut general, 0, &mut []), Err(MoveError::InvalidState)); assert_eq!((vector, general), before);
}

#[test]
fn zero_instructions_touch_exactly_the_ia32_vector_file() {
    for operation in [MoveInstruction::ZeroUpper, MoveInstruction::ZeroAll] {
        let (mut vector, mut general) = state(); let before = (vector, general);
        vector.execute_move(operation, &mut general, 0, &mut []).unwrap();
        assert_eq!(vector.upper, [[0; 16]; 8]); assert_eq!(vector.simd.mxcsr, before.0.simd.mxcsr); assert_eq!(general, before.1);
        assert_eq!(vector.simd.xmm, if operation == MoveInstruction::ZeroAll { [[0; 16]; 8] } else { before.0.simd.xmm });
    }
}
