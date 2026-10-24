//! Portable immutable owner metadata. The getter reads the exact packed words
//! in the frozen owner's module allocation; it never walks a code dictionary.
use std::collections::{BTreeMap, BTreeSet};
use wasm_encoder::{BlockType, Function, MemArg, ValType};

#[cfg(test)]
mod tests;

const HEADER_BYTES: usize = 48;

struct Layout {
    word_count: u32,
    bits: u32,
    packed: Vec<u64>,
    dictionary: Vec<i64>,
    high_words: Vec<u32>,
    mode: u32,
    dictionary_offset: u32,
    high_offset: u32,
    byte_length: usize,
}

impl Layout {
    fn new(words: &[i64]) -> Option<Self> {
        if words.is_empty() {
            return None;
        }
        let word_count = u32::try_from(words.len()).expect("bounded recovery word count");
        let dictionary: Vec<_> = words
            .iter()
            .copied()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let dictionary_count =
            u32::try_from(dictionary.len()).expect("bounded recovery dictionary count");
        let bits = (u32::BITS - (dictionary_count - 1).leading_zeros()).max(1);
        let bit_length = word_count
            .checked_mul(bits)
            .expect("bounded immutable recovery bit stream");
        let codes: BTreeMap<_, _> = dictionary
            .iter()
            .enumerate()
            .map(|(index, &word)| (word, index as u32))
            .collect();
        let mut packed = vec![0u64; (bit_length as usize).div_ceil(64)];
        for (index, value) in words.iter().enumerate() {
            let position = index * bits as usize;
            let word = position >> 6;
            let shift = (position & 63) as u32;
            let code = u64::from(codes[value]);
            packed[word] |= code << shift;
            if shift + bits > 64 {
                packed[word + 1] |= code >> (64 - shift);
            }
        }
        let dictionary_offset = HEADER_BYTES
            .checked_add(packed.len().checked_mul(8).expect("bounded packed bytes"))
            .and_then(|offset| u32::try_from(offset).ok())
            .expect("bounded recovery dictionary offset");
        let high_words: Vec<_> = dictionary
            .iter()
            .map(|&word| ((word as u64) >> 32) as u32)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let candidate_mode = if high_words.len() <= 256 {
            1
        } else if high_words.len() <= 65536 {
            2
        } else {
            0
        };
        let raw_bytes = dictionary
            .len()
            .checked_mul(8)
            .expect("bounded dictionary bytes");
        let split_bytes = dictionary
            .len()
            .checked_mul(4 + candidate_mode as usize)
            .and_then(|length| length.checked_add(high_words.len().checked_mul(4)?))
            .expect("bounded split dictionary bytes");
        let mode = if candidate_mode != 0 && split_bytes < raw_bytes {
            candidate_mode
        } else {
            0
        };
        let high_words = if mode == 0 { Vec::new() } else { high_words };
        let entry_bytes = if mode == 0 { 8 } else { 4 + mode as usize };
        let high_offset = (dictionary_offset as usize)
            .checked_add(
                dictionary
                    .len()
                    .checked_mul(entry_bytes)
                    .expect("bounded dictionary entries"),
            )
            .and_then(|offset| u32::try_from(offset).ok())
            .expect("bounded recovery high-word offset");
        let byte_length = (high_offset as usize)
            .checked_add(high_words.len().checked_mul(4).expect("bounded high words"))
            .filter(|&length| u32::try_from(length).is_ok())
            .expect("bounded recovery metadata bytes");
        Some(Self {
            word_count,
            bits,
            packed,
            dictionary,
            high_words,
            mode,
            dictionary_offset,
            high_offset,
            byte_length,
        })
    }
}

/// Payload of the single `ring3.recovery` custom section. Every field is little
/// endian: R3RD, version, word count, bits/code, packed count, dictionary count,
/// packed offset, dictionary offset, owner identity, dictionary mode/high count;
/// then packed words and dictionary data. Mode0 stores raw i64 bits. Modes1/2
/// store low32 plus a u8/u16 high-word index, followed by distinct u32 high words.
/// A split is used only when strictly smaller. Empty programs omit the section.
pub(super) fn encode(words: &[i64], identity: u64) -> Vec<u8> {
    let Some(layout) = Layout::new(words) else {
        return Vec::new();
    };
    let mut bytes = Vec::with_capacity(layout.byte_length);
    bytes.extend_from_slice(b"R3RD");
    for value in [
        2,
        layout.word_count,
        layout.bits,
        layout.packed.len() as u32,
        layout.dictionary.len() as u32,
        HEADER_BYTES as u32,
        layout.dictionary_offset,
    ] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&identity.to_le_bytes());
    bytes.extend_from_slice(&layout.mode.to_le_bytes());
    bytes.extend_from_slice(&(layout.high_words.len() as u32).to_le_bytes());
    for word in layout.packed {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    for word in layout.dictionary {
        if layout.mode == 0 {
            bytes.extend_from_slice(&word.to_le_bytes());
        } else {
            bytes.extend_from_slice(&(word as u32).to_le_bytes());
            let high = ((word as u64) >> 32) as u32;
            let index = layout.high_words.binary_search(&high).unwrap();
            if layout.mode == 1 {
                bytes.push(index as u8);
            } else {
                bytes.extend_from_slice(&(index as u16).to_le_bytes());
            }
        }
    }
    for high in layout.high_words {
        bytes.extend_from_slice(&high.to_le_bytes());
    }
    assert_eq!(bytes.len(), layout.byte_length);
    bytes
}

fn load(offset: u64) -> MemArg {
    // The portable custom payload need not begin at an eight-byte-aligned
    // module offset. Wasm's little-endian unaligned load preserves every bit.
    MemArg {
        offset,
        align: 0,
        memory_index: 0,
    }
}

/// `base_global` is the immutable i32 `ring3.recovery.base`, bound to the first
/// payload byte in this same owner's frozen module allocation. Its validated
/// range includes the complete canonical blob. No raw host pointer is encoded.
/// The unsigned index guard dominates all loads: two loads for an ordinary
/// raw word, three for its packed-code boundary crossing, four/five for split
/// dictionaries, and zero for out-of-range input.
pub(super) fn getter(words: &[i64], base_global: u32) -> Function {
    let Some(layout) = Layout::new(words) else {
        let mut empty = Function::new([]);
        empty.instructions().i64_const(0).end();
        return empty;
    };
    let mut function = Function::new([(2, ValType::I32), (1, ValType::I64)]);
    function
        .instructions()
        .local_get(0)
        .i32_const(layout.word_count as i32)
        .i32_lt_u()
        .if_(BlockType::Result(ValType::I64))
        .local_get(0)
        .i32_const(layout.bits as i32)
        .i32_mul()
        .local_tee(1)
        .i32_const(63)
        .i32_and()
        .local_set(2)
        .global_get(base_global)
        .local_get(1)
        .i32_const(6)
        .i32_shr_u()
        .i32_const(3)
        .i32_shl()
        .i32_add()
        .i64_load(load(HEADER_BYTES as u64))
        .local_set(3)
        .local_get(3)
        .local_get(2)
        .i64_extend_i32_u()
        .i64_shr_u()
        .local_get(2)
        .i32_const(layout.bits as i32)
        .i32_add()
        .i32_const(64)
        .i32_gt_u()
        .if_(BlockType::Result(ValType::I64))
        .global_get(base_global)
        .local_get(1)
        .i32_const(6)
        .i32_shr_u()
        .i32_const(1)
        .i32_add()
        .i32_const(3)
        .i32_shl()
        .i32_add()
        .i64_load(load(HEADER_BYTES as u64))
        .i32_const(64)
        .local_get(2)
        .i32_sub()
        .i64_extend_i32_u()
        .i64_shl()
        .else_()
        .i64_const(0)
        .end()
        .i64_or()
        .i64_const(((1u64 << layout.bits) - 1) as i64)
        .i64_and()
        .i32_wrap_i64()
        .local_set(1);
    let mut c = function.instructions();
    if layout.mode == 0 {
        c.global_get(base_global)
            .local_get(1)
            .i32_const(3)
            .i32_shl()
            .i32_add()
            .i64_load(load(u64::from(layout.dictionary_offset)));
    } else {
        c.global_get(base_global)
            .local_get(1)
            .i32_const(4 + layout.mode as i32)
            .i32_mul()
            .i32_add()
            .i32_load(load(u64::from(layout.dictionary_offset)))
            .i64_extend_i32_u()
            .local_set(3)
            .global_get(base_global)
            .local_get(1)
            .i32_const(4 + layout.mode as i32)
            .i32_mul()
            .i32_add();
        if layout.mode == 1 {
            c.i32_load8_u(load(u64::from(layout.dictionary_offset) + 4));
        } else {
            c.i32_load16_u(load(u64::from(layout.dictionary_offset) + 4));
        }
        c.local_set(1)
            .local_get(3)
            .global_get(base_global)
            .local_get(1)
            .i32_const(2)
            .i32_shl()
            .i32_add()
            .i32_load(load(u64::from(layout.high_offset)))
            .i64_extend_i32_u()
            .i64_const(32)
            .i64_shl()
            .i64_or();
    }
    c.else_().i64_const(0).end().end();
    function
}
