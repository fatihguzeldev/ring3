use std::sync::Mutex;

use crate::cpu::x86::{
    State32,
    avx::{AvxInstruction, MemoryEffect, VectorState, dispatch},
};

// this module is added only to the producer's isolated source snapshot.
static WORDS: Mutex<[u32; 96]> = Mutex::new([0; 96]);

fn instruction(operation: u32, registers: u32, immediate: u32) -> Option<AvxInstruction> {
    let destination = registers & 255;
    let first = (registers >> 8) & 255;
    let second = (registers >> 16) & 255;
    if registers & 0xfe00_0000 != 0 || [destination, first, second].iter().any(|&r| r > 7) {
        return None;
    }
    let (family, kind, uses_first) = match operation {
        38..=58 => (12, operation - 37, !(50..=53).contains(&operation)),
        113..=160 => (
            1,
            operation - 112,
            !matches!(operation, 113..=116 | 119..=120 | 122..=123),
        ),
        161..=176 => (
            2,
            ((operation - 161) / 4) * 2 + (operation - 161) % 2 + 1,
            true,
        ),
        177..=180 => (2, (operation - 177) * 2 + 1, true),
        _ => return None,
    };
    // the old protocol permits ignored register fields. the canonical factory
    // requires those slots to be zero, including the captured-memory source.
    let memory = registers & (1 << 24) != 0;
    let operands = destination
        | if uses_first { first << 3 } else { 0 }
        | if memory { 1 << 12 } else { second << 6 };
    AvxInstruction::decode_descriptor(0x0100_0000 | family << 8 | kind, operands, immediate).ok()
}

fn decode(words: &[u32; 96]) -> (VectorState, State32, [u8; 32]) {
    let mut vectors = VectorState::default();
    vectors.simd.mxcsr = words[10];
    for register in 0..8 {
        for lane in 0..4 {
            let offset = lane * 4;
            vectors.simd.xmm[register][offset..offset + 4]
                .copy_from_slice(&words[12 + register * 4 + lane].to_le_bytes());
            vectors.upper[register][offset..offset + 4]
                .copy_from_slice(&words[44 + register * 4 + lane].to_le_bytes());
        }
    }
    let general = State32 {
        registers: words[..8].try_into().unwrap(),
        eip: words[8],
        eflags: words[9],
    };
    let mut operand = [0; 32];
    for lane in 0..8 {
        operand[lane * 4..lane * 4 + 4].copy_from_slice(&words[76 + lane].to_le_bytes());
    }
    (vectors, general, operand)
}

fn encode(words: &mut [u32; 96], vectors: VectorState, general: State32) {
    words[..8].copy_from_slice(&general.registers);
    words[8] = general.eip;
    words[9] = general.eflags;
    words[10] = vectors.simd.mxcsr;
    for register in 0..8 {
        for lane in 0..4 {
            let offset = lane * 4;
            words[12 + register * 4 + lane] = u32::from_le_bytes(
                vectors.simd.xmm[register][offset..offset + 4]
                    .try_into()
                    .unwrap(),
            );
            words[44 + register * 4 + lane] = u32::from_le_bytes(
                vectors.upper[register][offset..offset + 4]
                    .try_into()
                    .unwrap(),
            );
        }
    }
}

#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn ring3_avx_probe_write(index: u32, value: u32) -> u32 {
    let Ok(mut words) = WORDS.lock() else {
        return 2;
    };
    let Some(word) = words.get_mut(index as usize) else {
        return 1;
    };
    *word = value;
    0
}

#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn ring3_avx_probe_read(index: u32) -> u32 {
    WORDS
        .lock()
        .ok()
        .and_then(|words| words.get(index as usize).copied())
        .unwrap_or(u32::MAX)
}

#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn ring3_avx_probe_run(
    operation: u32,
    registers: u32,
    immediate: u32,
    operand_len: u32,
) -> u32 {
    let Some(instruction) = instruction(operation, registers, immediate) else {
        return 1;
    };
    let required = match instruction.memory() {
        MemoryEffect::None => 0,
        MemoryEffect::Full(requirement) => u32::from(requirement.bytes),
        _ => return 1,
    };
    if operand_len != required || operand_len > 32 {
        return 1;
    }
    let Ok(mut words) = WORDS.lock() else {
        return 2;
    };
    let (mut vectors, mut general, mut operand) = decode(&words);
    match dispatch::execute(
        instruction,
        &mut vectors,
        &mut general,
        0,
        &mut operand[..operand_len as usize],
    ) {
        Ok(()) => {
            encode(&mut words, vectors, general);
            0
        }
        Err(dispatch::DispatchError::FloatingPoint(mxcsr)) => {
            words[10] = mxcsr;
            3
        }
        Err(_) => 2,
    }
}
