use super::execute_descriptor;
use crate::cpu::x86::{
    X87State,
    simd::{
        self, PackedKind as P, SimdError, SimdKind as K, SimdOperand as O, SimdOperation,
        SimdRegister as R, SimdState,
    },
};

fn operation(kind: K, width: u8) -> SimdOperation {
    SimdOperation {
        kind,
        destination: O::Register(R::Xmm(0)),
        source: O::Register(R::Xmm(1)),
        vector_bytes: 16,
        lane_bytes: width,
        memory_bytes: 0,
        immediate: None,
        aligned: false,
        scalar: false,
        implicit_address: None,
    }
}

#[test]
fn descriptor_errors_precede_invalid_mxcsr_without_arena_mutation() {
    use crate::abi::{
        arena::{ARENA_SIZE, SIMD_OFFSET, TRANSFER_OFFSET, X87_OFFSET},
        x86::{SIMD_MXCSR_OFFSET, SIMD_SIZE, X87_SIZE, encode_simd, encode_x87},
    };
    let mut arena = vec![0; ARENA_SIZE];
    encode_x87(
        &X87State::default(),
        &mut arena[X87_OFFSET..X87_OFFSET + X87_SIZE],
    )
    .unwrap();
    encode_simd(
        &SimdState {
            xmm: std::array::from_fn(|index| [index as u8 + 0xa0; 16]),
            ..SimdState::default()
        },
        &mut arena[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE],
    )
    .unwrap();
    arena[TRANSFER_OFFSET..TRANSFER_OFFSET + 16].fill(0x5a);
    let mxcsr = SIMD_OFFSET + SIMD_MXCSR_OFFSET;
    arena[mxcsr..mxcsr + 4].copy_from_slice(&0x0001_1f80u32.to_le_bytes());
    let before = arena.clone();
    assert_eq!(
        operation(K::Packed(P::Add), 1).descriptor().unwrap(),
        (0x2404, 0x0908, 0)
    );
    for (descriptor, operands, immediate) in [
        (0x8000_2404, 0x0908, 0),
        (0x2404, 0x093f, 0),
        (0x2404, 0x0908, 256),
        (0x2404, 0x0908, 1),
        (0x3c04, 0x0908, 0),
        (0x3404, 0x0908, 0),
    ] {
        assert_eq!(
            execute_descriptor(&mut arena, descriptor, operands, immediate, 0, 0, 0xcd7),
            Err(SimdError::InvalidDescriptor)
        );
        assert_eq!(arena, before);
    }
    assert_eq!(
        execute_descriptor(&mut arena, 0x2404, 0x0908, 0, 0, 0, 0xcd7),
        Err(SimdError::InvalidState)
    );
    assert_eq!(arena, before);
}

#[test]
fn descriptor_mmx_reads_physical_slots_and_publishes_only_its_destination() {
    use crate::abi::{
        arena::{ARENA_SIZE, SIMD_OFFSET, X87_OFFSET},
        x86::{
            SIMD_SIZE, X87_REGISTERS_OFFSET, X87_SIZE, X87_STATUS_OFFSET, X87_TAG_OFFSET,
            encode_simd, encode_x87,
        },
    };
    let mut arena = vec![0xa5; ARENA_SIZE];
    let mut x87 = X87State {
        status: 0x6f00,
        tag: 0xa5a5,
        opcode: 0x321,
        instruction_pointer: 0x1234_5678,
        data_pointer: 0x8765_4321,
        registers: std::array::from_fn(|index| [0x30 + index as u8; 10]),
        ..X87State::default()
    };
    x87.registers[6] = [0xff, 0x01, 0x7f, 0x80, 0x55, 0xaa, 0x00, 0xfe, 0xff, 0x3f];
    x87.registers[7] = [0x02, 0xfe, 0x01, 0x80, 0xaa, 0x55, 0xff, 0x03, 0x00, 0x40];
    encode_x87(&x87, &mut arena[X87_OFFSET..X87_OFFSET + X87_SIZE]).unwrap();
    encode_simd(
        &SimdState {
            mxcsr: 0x5f80,
            xmm: std::array::from_fn(|index| [0x90 + index as u8; 16]),
        },
        &mut arena[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE],
    )
    .unwrap();
    let mut expected = arena.clone();
    let destination = X87_OFFSET + X87_REGISTERS_OFFSET + 6 * 10;
    expected[destination..destination + 10]
        .copy_from_slice(&[0x01, 0xff, 0x80, 0x00, 0xff, 0xff, 0xff, 0x01, 0xff, 0xff]);
    expected[X87_OFFSET + X87_STATUS_OFFSET..X87_OFFSET + X87_STATUS_OFFSET + 2]
        .copy_from_slice(&0x4700u16.to_le_bytes());
    expected[X87_OFFSET + X87_TAG_OFFSET..X87_OFFSET + X87_TAG_OFFSET + 2].fill(0);
    let (descriptor, operands, immediate) = SimdOperation {
        destination: O::Register(R::Mmx(6)),
        source: O::Register(R::Mmx(7)),
        vector_bytes: 8,
        ..operation(K::Packed(P::Add), 1)
    }
    .descriptor()
    .unwrap();
    assert_eq!(
        execute_descriptor(
            &mut arena,
            descriptor,
            operands,
            immediate,
            0x1234_5678,
            0x89ab_cdef,
            0xcd7,
        ),
        Ok(simd::SimdResult {
            gpr: 0x89ab_cdef,
            flags: 0xcd7,
        })
    );
    assert_eq!(arena, expected);
}

#[test]
fn pending_x87_fault_precedes_mmx_but_not_sse_publication() {
    use crate::abi::{
        arena::{ARENA_SIZE, SIMD_OFFSET, X87_OFFSET},
        x86::{SIMD_SIZE, X87_SIZE, X87_STATUS_OFFSET, encode_simd, encode_x87},
    };
    let mut arena = vec![0; ARENA_SIZE];
    let mut x87 = X87State::default();
    x87.control &= !1;
    x87.status = 1;
    encode_x87(&x87, &mut arena[X87_OFFSET..X87_OFFSET + X87_SIZE]).unwrap();
    encode_simd(
        &SimdState::default(),
        &mut arena[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE],
    )
    .unwrap();
    let op = SimdOperation {
        destination: O::Register(R::Mmx(0)),
        source: O::Register(R::Mmx(1)),
        vector_bytes: 8,
        ..operation(K::Packed(P::Add), 1)
    };
    let (d, o, i) = op.descriptor().unwrap();
    let mut expected = arena.clone();
    expected[X87_OFFSET + X87_STATUS_OFFSET..X87_OFFSET + X87_STATUS_OFFSET + 2]
        .copy_from_slice(&0x8081u16.to_le_bytes());
    assert_eq!(
        execute_descriptor(&mut arena, d, o, i, 0, 0, 0xcd7),
        Err(SimdError::X87FloatingPoint(0x8081))
    );
    assert_eq!(arena, expected);
    let (d, o, i) = operation(K::Packed(P::Add), 1).descriptor().unwrap();
    assert!(execute_descriptor(&mut arena, d, o, i, 0, 0, 0xcd7).is_ok());
}
