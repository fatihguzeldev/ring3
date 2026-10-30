use super::*;
use crate::{
    cpu::x86::{
        X87Binary, X87Format,
        simd::{FloatKind, SimdRegister},
    },
    memory::{PageRange, Permissions},
};

fn fixture() -> (AddressSpace, NumericState, State32, SegmentContext) {
    let mut memory = AddressSpace::new(4).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x2000), 1).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    (
        memory,
        NumericState::default(),
        State32 {
            eip: 0x1000,
            ..State32::default()
        },
        SegmentContext::default(),
    )
}

fn address(displacement: u32) -> EffectiveAddress {
    EffectiveAddress {
        displacement,
        ..Default::default()
    }
}

fn x87(operation: X87Operation, displacement: Option<u32>) -> Operation {
    Operation::X87 {
        operation,
        address: displacement.map(address),
        waiting: true,
        opcode: 0x100,
    }
}

#[test]
fn strict_x87_load_add_store_preserves_empty_payload_and_metadata() {
    let (mut memory, mut numeric, mut scalar, segments) = fixture();
    memory
        .write(GuestAddress(0x2000), &3_f32.to_le_bytes())
        .unwrap();
    memory
        .write(GuestAddress(0x2004), &2_f32.to_le_bytes())
        .unwrap();
    for (pc, operation) in [
        (
            0x1002,
            x87(X87Operation::Load(X87Format::Float32), Some(0x2000)),
        ),
        (
            0x1004,
            x87(
                X87Operation::BinaryMemory {
                    kind: X87Binary::Add,
                    format: X87Format::Float32,
                },
                Some(0x2004),
            ),
        ),
        (
            0x1006,
            x87(
                X87Operation::Store {
                    format: X87Format::Float32,
                    pop: true,
                    truncate: false,
                },
                Some(0x2008),
            ),
        ),
    ] {
        assert!(
            !numeric
                .execute(&mut memory, &mut scalar, &segments, &operation, pc)
                .unwrap()
                .code_invalidated
        );
    }
    let mut output = [0; 4];
    memory.read(GuestAddress(0x2008), &mut output).unwrap();
    assert_eq!(output, 5_f32.to_le_bytes());
    assert_eq!((scalar.eip, scalar.eflags), (0x1006, 2));
    assert_eq!((numeric.x87.tag, numeric.x87.status), (0xffff, 0));
    assert_eq!(
        numeric.x87.registers[7],
        [0, 0, 0, 0, 0, 0, 0, 0xa0, 1, 0x40]
    );
    assert_eq!(
        (
            numeric.x87.instruction_pointer,
            numeric.x87.data_pointer,
            numeric.x87.code_selector,
            numeric.x87.data_selector
        ),
        (0x1004, 0x2008, 0x1b, 0x23)
    );
}

#[test]
fn raw80_roundtrip_does_not_round_through_f64() {
    let (mut memory, mut numeric, mut scalar, segments) = fixture();
    let raw = [1, 0, 0, 0, 0, 0, 0, 0x80, 0xff, 0x3f];
    memory.write(GuestAddress(0x2000), &raw).unwrap();
    for operation in [
        x87(X87Operation::Load(X87Format::Extended80), Some(0x2000)),
        x87(
            X87Operation::Store {
                format: X87Format::Extended80,
                pop: true,
                truncate: false,
            },
            Some(0x2010),
        ),
    ] {
        numeric
            .execute(&mut memory, &mut scalar, &segments, &operation, 0x1002)
            .unwrap();
    }
    let mut output = [0; 10];
    memory.read(GuestAddress(0x2010), &mut output).unwrap();
    assert_eq!(output, raw);
    assert_eq!(numeric.x87.registers[7], raw);
    assert_eq!(numeric.x87.tag, 0xffff);
}

#[test]
fn pending_x87_exception_precedes_unmapped_operand() {
    let (mut memory, mut numeric, mut scalar, segments) = fixture();
    numeric.x87.control &= !1;
    numeric.x87.status = 1;
    let before = scalar;
    assert_eq!(
        numeric.execute(
            &mut memory,
            &mut scalar,
            &segments,
            &x87(X87Operation::Load(X87Format::Float32), Some(0x3000)),
            0x1002
        ),
        Err(NumericError::Cpu {
            vector: 16,
            detail: 0x8081
        })
    );
    assert_eq!(scalar, before);
    assert_eq!(numeric.x87.status, 0x8081);
    assert_eq!(numeric.x87.instruction_pointer, 0);
}

#[test]
fn cross_page_x87_store_fault_preserves_payload_top_metadata_and_ram() {
    let (mut memory, mut numeric, mut scalar, segments) = fixture();
    numeric.x87.registers[0] = [0, 0, 0, 0, 0, 0, 0, 0x80, 0xff, 0x3f];
    numeric.x87.tag = 0xfffc;
    memory.write(GuestAddress(0x2ffe), &[0xa5, 0x5a]).unwrap();
    let before = (numeric.x87, scalar, memory.code_validation_version());
    let error = numeric
        .execute(
            &mut memory,
            &mut scalar,
            &segments,
            &x87(
                X87Operation::Store {
                    format: X87Format::Float32,
                    pop: true,
                    truncate: false,
                },
                Some(0x2ffe),
            ),
            0x1002,
        )
        .unwrap_err();
    assert!(matches!(error, NumericError::Memory { width: 4, .. }));
    assert_eq!(
        (numeric.x87, scalar, memory.code_validation_version()),
        before
    );
    let mut output = [0; 2];
    memory.read(GuestAddress(0x2ffe), &mut output).unwrap();
    assert_eq!(output, [0xa5, 0x5a]);
}

fn single_divide() -> Operation {
    Operation::Simd(SimdOperation {
        kind: SimdKind::Float(FloatKind::Divide),
        destination: SimdOperand::Register(SimdRegister::Xmm(0)),
        source: SimdOperand::Register(SimdRegister::Xmm(1)),
        vector_bytes: 16,
        lane_bytes: 4,
        memory_bytes: 0,
        immediate: None,
        aligned: false,
        scalar: true,
        implicit_address: None,
    })
}

#[test]
fn simd_unmasked_divide_preserves_destination_and_scalar_but_latches_mxcsr() {
    let (mut memory, mut numeric, mut scalar, segments) = fixture();
    numeric.vectors.simd.mxcsr &= !(1 << 9);
    numeric.vectors.simd.xmm[0][..4].copy_from_slice(&1_f32.to_le_bytes());
    let before = (numeric.vectors.simd.xmm, scalar);
    assert_eq!(
        numeric.execute(
            &mut memory,
            &mut scalar,
            &segments,
            &single_divide(),
            0x1002
        ),
        Err(NumericError::Cpu {
            vector: 19,
            detail: 0x1d84
        })
    );
    assert_eq!((numeric.vectors.simd.xmm, scalar), before);
    assert_eq!(numeric.vectors.simd.mxcsr, 0x1d84);
}

#[test]
fn control_word_rounding_and_integer_store_follow_the_selected_rounding_mode() {
    let (mut memory, mut numeric, mut scalar, segments) = fixture();
    memory
        .write(GuestAddress(0x2000), &0x0f7f_u16.to_le_bytes())
        .unwrap();
    memory
        .write(GuestAddress(0x2008), &(-3.75_f64).to_le_bytes())
        .unwrap();
    for operation in [
        x87(X87Operation::LoadControl, Some(0x2000)),
        x87(X87Operation::Load(X87Format::Float64), Some(0x2008)),
        x87(
            X87Operation::Store {
                format: X87Format::Integer32,
                pop: true,
                truncate: false,
            },
            Some(0x2010),
        ),
    ] {
        numeric
            .execute(&mut memory, &mut scalar, &segments, &operation, 0x1002)
            .unwrap();
    }
    let mut output = [0; 4];
    memory.read(GuestAddress(0x2010), &mut output).unwrap();
    assert_eq!(output, (-3_i32).to_le_bytes());
    assert_eq!(numeric.x87.control, 0x0f7f);
    assert_eq!(numeric.x87.status & 0x3f, 0x20);
    assert_eq!(numeric.x87.tag, 0xffff);
}

fn decoded(memory: &mut AddressSpace, bytes: &[u8]) -> Operation {
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(0x1000), bytes).unwrap();
    *crate::cpu::x86::decode::decode_one(memory, GuestAddress(0x1000))
        .unwrap()
        .operation()
}

#[test]
fn mmx_load_aliases_raw_x87_payload_tags_and_top() {
    let (mut memory, mut numeric, mut scalar, segments) = fixture();
    let operation = decoded(&mut memory, &[0x0f, 0x6f, 0x05, 0, 0x20, 0, 0]);
    let payload = [1, 2, 3, 4, 5, 6, 7, 8];
    memory.write(GuestAddress(0x2000), &payload).unwrap();
    numeric.x87.status = 0x3800;
    numeric
        .execute(&mut memory, &mut scalar, &segments, &operation, 0x1007)
        .unwrap();
    assert_eq!(numeric.x87.registers[0], [1, 2, 3, 4, 5, 6, 7, 8, 255, 255]);
    assert_eq!((numeric.x87.tag, numeric.x87.status & 0x3800), (0, 0));
}

#[test]
fn fxsave_restore_preserves_raw_registers_and_reserved_tail() {
    let (mut memory, mut numeric, mut scalar, segments) = fixture();
    memory.write(GuestAddress(0x2000), &[0xa5; 512]).unwrap();
    numeric.x87.registers[3] = [1, 2, 3, 4, 5, 6, 7, 0x80, 0xff, 0x3f];
    numeric.x87.tag = 0xff3f;
    numeric.vectors.simd.xmm[4] = [0x6c; 16];
    let before = (numeric.x87, numeric.vectors);
    let mut operation = SimdOperation {
        kind: SimdKind::SaveExtended,
        destination: SimdOperand::None,
        source: SimdOperand::None,
        vector_bytes: 16,
        lane_bytes: 4,
        memory_bytes: 512,
        immediate: None,
        aligned: true,
        scalar: false,
        implicit_address: Some(address(0x2000)),
    };
    numeric
        .execute(
            &mut memory,
            &mut scalar,
            &segments,
            &Operation::Simd(operation),
            0x1002,
        )
        .unwrap();
    let mut tail = [0; 48];
    memory.read(GuestAddress(0x21d0), &mut tail).unwrap();
    assert_eq!(tail, [0xa5; 48]);
    numeric.x87 = X87State::default();
    numeric.vectors.simd = Default::default();
    operation.kind = SimdKind::RestoreExtended;
    numeric
        .execute(
            &mut memory,
            &mut scalar,
            &segments,
            &Operation::Simd(operation),
            0x1004,
        )
        .unwrap();
    assert_eq!((numeric.x87, numeric.vectors), before);
}

#[test]
fn avx_packed_add_publishes_all_eight_lanes_without_touching_x87() {
    let (mut memory, mut numeric, mut scalar, segments) = fixture();
    let operation = decoded(&mut memory, &[0xc5, 0xf4, 0x58, 0xc2]);
    for chunk in numeric.vectors.simd.xmm[1]
        .chunks_exact_mut(4)
        .chain(numeric.vectors.upper[1].chunks_exact_mut(4))
    {
        chunk.copy_from_slice(&1_f32.to_le_bytes());
    }
    for chunk in numeric.vectors.simd.xmm[2]
        .chunks_exact_mut(4)
        .chain(numeric.vectors.upper[2].chunks_exact_mut(4))
    {
        chunk.copy_from_slice(&2_f32.to_le_bytes());
    }
    let x87 = numeric.x87;
    numeric
        .execute(&mut memory, &mut scalar, &segments, &operation, 0x1004)
        .unwrap();
    for chunk in numeric.vectors.simd.xmm[0]
        .chunks_exact(4)
        .chain(numeric.vectors.upper[0].chunks_exact(4))
    {
        assert_eq!(chunk, &3_f32.to_le_bytes());
    }
    assert_eq!(numeric.x87, x87);
}

#[test]
fn xsave7_roundtrip_preserves_raw80_and_upper_vectors_without_writing_reserved_bytes() {
    let (mut memory, mut numeric, mut scalar, segments) = fixture();
    memory.write(GuestAddress(0x2000), &[0xa5; 832]).unwrap();
    memory.write(GuestAddress(0x2200), &[0; 24]).unwrap();
    numeric.x87.registers[0] = [1, 0, 0, 0, 0, 0, 0, 0x80, 0xff, 0x3f];
    numeric.x87.tag = 0xfffc;
    numeric.vectors.simd.xmm[3] = [0x3c; 16];
    numeric.vectors.upper[5] = [0x5a; 16];
    scalar.registers[0] = 7;
    let before = (numeric.x87, numeric.vectors);
    numeric
        .execute(
            &mut memory,
            &mut scalar,
            &segments,
            &Operation::XState(crate::cpu::x86::xstate::XStateOperation::Save {
                address: address(0x2000),
            }),
            0x1002,
        )
        .unwrap();
    let mut reserved = [0; 128];
    memory.read(GuestAddress(0x22c0), &mut reserved).unwrap();
    assert_eq!(reserved, [0xa5; 128]);
    numeric.x87 = X87State::default();
    numeric.vectors = VectorState::default();
    numeric
        .execute(
            &mut memory,
            &mut scalar,
            &segments,
            &Operation::XState(crate::cpu::x86::xstate::XStateOperation::Restore {
                address: address(0x2000),
            }),
            0x1004,
        )
        .unwrap();
    assert_eq!((numeric.x87, numeric.vectors), before);
}

#[test]
fn segmented_x87_load_records_logical_offset_and_actual_selector() {
    let (mut memory, mut numeric, mut scalar, mut segments) = fixture();
    memory
        .write(GuestAddress(0x2000), &1_f32.to_le_bytes())
        .unwrap();
    segments.state.bases[SegmentRegister::Fs.index()] = 0x1000;
    let operation = Operation::X87 {
        operation: X87Operation::Load(X87Format::Float32),
        address: Some(EffectiveAddress {
            segment: Some(SegmentRegister::Fs),
            ..address(0x1000)
        }),
        waiting: true,
        opcode: 0x105,
    };
    numeric
        .execute(&mut memory, &mut scalar, &segments, &operation, 0x1002)
        .unwrap();
    assert_eq!(
        numeric.x87.registers[7],
        [0, 0, 0, 0, 0, 0, 0, 0x80, 0xff, 0x3f]
    );
    assert_eq!(
        (
            numeric.x87.data_pointer,
            numeric.x87.data_selector,
            numeric.x87.opcode
        ),
        (0x1000, 0x3b, 0x105)
    );
}
