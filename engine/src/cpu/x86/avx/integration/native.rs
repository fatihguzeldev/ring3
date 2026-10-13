use crate::{
    abi::{
        arena::{
            self, ARENA_SIZE, EXIT_OFFSET, HELPER_OFFSET, SIMD_OFFSET, STATE_OFFSET,
            TRANSFER_OFFSET, TRANSFER_SIZE, YMM_OFFSET,
        },
        cpu_helper::encode_cpu_result,
        x86::{
            AVX_ADDRESS_SIZE, AVX_ADDRESS_TRANSFER, AVX_OUTPUT_SIZE, AVX_OUTPUT_TRANSFER,
            EXIT_SIZE, SIMD_SIZE, STATE_SIZE, YMM_SIZE, encode_exit_v6, encode_simd,
            encode_state_current, encode_ymm,
        },
    },
    cpu::{
        ExecutionExit, ExitReason,
        x86::{
            SegmentContext, SegmentRegister, State32,
            avx::{
                AvxInstruction, AvxSemantic, MemoryEffect, MoveInstruction, VectorState, dispatch,
            },
            simd::{SimdState, ymm::YmmUpperState},
        },
    },
    memory::{Access, AddressSpace, GuestAddress, PageRange, Permissions},
};
use iced_x86::{Code, Decoder, DecoderError, DecoderOptions, OpKind, Register};
use std::collections::HashSet;

pub(super) const PC: u32 = 0x1000;
pub(super) const ADDRESS: u32 = 0x5020;
pub(super) const PAGE: u32 = 0x5000;

pub(super) struct Case {
    pub(super) name: String,
    pub(super) original: Vec<u8>,
    pub(super) code: Vec<u8>,
    pub(super) descriptor: u32,
    pub(super) operands: u32,
    pub(super) immediate: u32,
    pub(super) helper: bool,
    pub(super) length: u32,
    pub(super) access: u32,
    pub(super) alignment: u8,
    pub(super) selected: u32,
    pub(super) input: Vec<u8>,
    pub(super) expected: Vec<u8>,
    pub(super) ram: Vec<u8>,
    pub(super) expected_ram: Vec<u8>,
}

fn bytes(text: &str) -> Vec<u8> {
    assert!(text.len().is_multiple_of(2));
    text.as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

fn iced(bytes: &[u8]) -> iced_x86::Instruction {
    let mut decoder = Decoder::with_ip(32, bytes, u64::from(PC), DecoderOptions::NONE);
    let value = decoder.decode();
    assert_eq!(decoder.last_error(), DecoderError::None);
    assert_eq!(value.len(), bytes.len());
    value
}

fn relocate(original: &[u8], memory: MemoryEffect) -> Vec<u8> {
    let instruction = iced(original);
    let mut result = original.to_vec();
    if memory == MemoryEffect::None {
        return result;
    }
    if instruction.code() == Code::VEX_Vmaskmovdqu_rDI_xmm_xmm {
        assert_eq!(instruction.op0_kind(), OpKind::MemorySegEDI);
        return result;
    }
    assert_eq!(instruction.memory_base(), Register::None);
    assert_eq!(instruction.memory_index(), Register::None);
    assert_eq!(instruction.segment_prefix(), Register::None);
    assert_eq!(instruction.memory_segment(), Register::DS);
    assert_eq!(instruction.memory_displacement32(), 0x1122_3344);
    let offsets: Vec<_> = original
        .windows(4)
        .enumerate()
        .filter_map(|(index, bytes)| (bytes == [0x44, 0x33, 0x22, 0x11]).then_some(index))
        .collect();
    assert_eq!(
        offsets.len(),
        1,
        "canonical absolute displacement occurs once"
    );
    result[offsets[0]..offsets[0] + 4].copy_from_slice(&ADDRESS.to_le_bytes());
    let relocated = iced(&result);
    assert_eq!(relocated.code(), instruction.code());
    assert_eq!(relocated.memory_displacement32(), ADDRESS);
    assert_eq!(relocated.memory_base(), instruction.memory_base());
    assert_eq!(relocated.memory_index(), instruction.memory_index());
    result
}

fn vectors() -> VectorState {
    let mut result = VectorState {
        simd: SimdState {
            mxcsr: 0x1f80,
            ..SimdState::default()
        },
        ..VectorState::default()
    };
    for register in 0usize..8 {
        for lane in 0usize..4 {
            let sign = if (register + lane / 2).is_multiple_of(2) {
                0x8000_0000
            } else {
                0
            };
            let lower =
                sign | 0x3f80_0000 | ((register as u32 + 1) << 16) | ((lane as u32 + 1) << 8);
            let upper = (sign ^ 0x8000_0000)
                | 0x4000_0000
                | ((register as u32 + 1) << 16)
                | ((lane as u32 + 1) << 8);
            result.simd.xmm[register][lane * 4..lane * 4 + 4].copy_from_slice(&lower.to_le_bytes());
            result.upper[register][lane * 4..lane * 4 + 4].copy_from_slice(&upper.to_le_bytes());
        }
    }
    result
}

fn encode_cpu(output: &mut [u8], state: &State32, vectors: &VectorState) {
    encode_state_current(state, &mut output[STATE_OFFSET..STATE_OFFSET + STATE_SIZE]).unwrap();
    encode_simd(
        &vectors.simd,
        &mut output[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE],
    )
    .unwrap();
    encode_ymm(
        &YmmUpperState {
            registers: vectors.upper,
        },
        &mut output[YMM_OFFSET..YMM_OFFSET + YMM_SIZE],
    )
    .unwrap();
}

fn reference(name: &str, original: Vec<u8>, instruction: AvxInstruction) -> Case {
    let memory = instruction.memory();
    let code = relocate(&original, memory);
    let mut vectors = vectors();
    let mut state = State32 {
        registers: [
            0x1020_3040,
            0x1122_3344,
            0x1234_5678,
            0x1357_9bdf,
            0x5800,
            0x2468_ace0,
            0x0001_0203,
            ADDRESS,
        ],
        eip: PC,
        eflags: 0x244cd7,
    };
    let mut input = vec![0; ARENA_SIZE];
    arena::initialize(&mut input).unwrap();
    input[TRANSFER_OFFSET..TRANSFER_OFFSET + TRANSFER_SIZE].fill(0xa5);
    encode_cpu(&mut input, &state, &vectors);
    let mut ram: Vec<_> = (0..4096).map(|offset| (offset * 37 + 19) as u8).collect();
    let start = (ADDRESS - PAGE) as usize;
    ram[start..start + 16].copy_from_slice(&vectors.simd.xmm[3]);
    ram[start + 16..start + 32].copy_from_slice(&vectors.upper[3]);
    if matches!(
        instruction.semantic(),
        AvxSemantic::Move(MoveInstruction::LoadControl)
    ) {
        ram[start..start + 4].copy_from_slice(&0x1f80_u32.to_le_bytes());
    }
    let mut expected_ram = ram.clone();
    let mut operand = [0; 32];
    let (length, access, alignment, selected) = match memory {
        MemoryEffect::Gather(_) => panic!("gather uses its dedicated partial-fault harness"),
        MemoryEffect::None => (0, 0, 1, 0),
        MemoryEffect::Full(value) => {
            let length = usize::from(value.bytes);
            assert!(length <= 32 && ADDRESS.is_multiple_of(u32::from(value.alignment)));
            if value.access == Access::Read {
                operand[..length].copy_from_slice(&ram[start..start + length]);
            }
            (
                u32::from(value.bytes),
                if value.access == Access::Read { 1 } else { 2 },
                value.alignment,
                0,
            )
        }
        MemoryEffect::Selected(value) => (
            value.width().bytes() as u32,
            if value.access() == Access::Read { 1 } else { 2 },
            1,
            value.selected_bytes(vectors.read(value.mask())),
        ),
    };
    if let MemoryEffect::Selected(value) = memory {
        assert_ne!(
            selected, 0,
            "{name}: each selected form exercises guest RAM"
        );
        let mut address_space = AddressSpace::new(1).unwrap();
        address_space
            .map_zeroed(
                PageRange::new(GuestAddress(PAGE), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
        address_space.write(GuestAddress(PAGE), &ram).unwrap();
        vectors
            .execute_masked(
                value,
                &SegmentContext::default(),
                SegmentRegister::Ds,
                ADDRESS,
                &mut address_space,
            )
            .unwrap();
        address_space
            .read(GuestAddress(PAGE), &mut expected_ram)
            .unwrap();
    } else {
        dispatch::execute(
            instruction,
            &mut vectors,
            &mut state,
            ADDRESS,
            &mut operand[..length as usize],
        )
        .unwrap();
        if access == 2 {
            expected_ram[start..start + length as usize]
                .copy_from_slice(&operand[..length as usize]);
        }
    }
    let helper = !matches!(
        instruction.semantic(),
        AvxSemantic::Move(_)
            | AvxSemantic::Logical(_)
            | AvxSemantic::Permute(_)
            | AvxSemantic::Integer(_)
            | AvxSemantic::Rearrange(_)
            | AvxSemantic::Shift(_)
            | AvxSemantic::Multiply(_)
            | AvxSemantic::Scalar(_)
            | AvxSemantic::CrossLane(_)
    );
    let mut expected = input.clone();
    state.eip = PC + code.len() as u32;
    encode_cpu(&mut expected, &state, &vectors);
    encode_exit_v6(
        &ExecutionExit {
            retired: 1,
            reason: ExitReason::Budget,
        },
        &mut expected[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    if helper || memory != MemoryEffect::None {
        encode_cpu_result(
            length,
            0,
            Ok(()),
            &mut expected[HELPER_OFFSET..HELPER_OFFSET + 40],
        )
        .unwrap();
    }
    if helper {
        let start = TRANSFER_OFFSET + AVX_OUTPUT_TRANSFER;
        expected[start..start + AVX_OUTPUT_SIZE].fill(0);
        let output = instruction
            .output()
            .map_or(0, |register| state.registers[register.index()]);
        expected[start..start + 4].copy_from_slice(&output.to_le_bytes());
        expected[start + 4..start + 8].copy_from_slice(&state.eflags.to_le_bytes());
        if memory != MemoryEffect::None {
            let start = TRANSFER_OFFSET + AVX_ADDRESS_TRANSFER;
            let record = &mut expected[start..start + AVX_ADDRESS_SIZE];
            record.fill(0);
            record[..16].copy_from_slice(b"R3AM\x01\0\x01\0\x18\0\0\0\0\0\0\0");
            record[16..20].copy_from_slice(&(SegmentRegister::Ds.index() as u32).to_le_bytes());
            record[20..24].copy_from_slice(&ADDRESS.to_le_bytes());
        }
    } else if memory != MemoryEffect::None {
        expected[TRANSFER_OFFSET..TRANSFER_OFFSET + length as usize]
            .copy_from_slice(&operand[..length as usize]);
    }
    let (descriptor, operands, immediate) = instruction.encode();
    Case {
        name: name.to_owned(),
        original,
        code,
        descriptor,
        operands,
        immediate,
        helper,
        length,
        access,
        alignment,
        selected,
        input,
        expected,
        ram,
        expected_ram,
    }
}

pub(super) fn corpus() -> Vec<Case> {
    let cases: Vec<_> = include_str!("../../decode/avx/native/fixtures/canonical.tsv")
        .lines()
        .filter(|line| !line.starts_with('#'))
        .map(|line| {
            let columns: Vec<_> = line.split('\t').collect();
            assert_eq!(columns.len(), 13);
            let original = bytes(columns[1]);
            assert_eq!(format!("{:?}", iced(&original).code()), columns[0]);
            let number = |index| u32::from_str_radix(columns[index], 16).unwrap();
            let instruction =
                AvxInstruction::decode_descriptor(number(2), number(3), number(4)).unwrap();
            reference(columns[0], original, instruction)
        })
        .collect();
    assert_eq!(cases.len(), 688);
    assert_eq!(
        cases
            .iter()
            .map(|case| case.name.as_str())
            .collect::<HashSet<_>>()
            .len(),
        374
    );
    cases
}
