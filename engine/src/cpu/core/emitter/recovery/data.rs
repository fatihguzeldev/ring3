//! Exact immutable words in module code. Dictionary codes are packed into i64
//! constants; neither the dictionary nor its index ever occupies guest memory.
use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
mod tests;
use wasm_encoder::{BlockType, Function, Ieee64, InstructionSink, ValType};

fn constant(c: &mut InstructionSink<'_>, value: i64) {
    // A ten-byte signed LEB plus its opcode is larger than an exact eight-byte
    // bit literal plus reinterpret. No floating arithmetic touches these bits,
    // including NaN payloads: exec/numerics.html#op-reinterpret in the Wasm spec.
    if !(-(1i64 << 62)..(1i64 << 62)).contains(&value) {
        c.f64_const(Ieee64::new(value as u64)).i64_reinterpret_f64();
    } else {
        c.i64_const(value);
    }
}

enum Group {
    Constant(i64),
    Affine { first: i64, step: i64, start: usize },
}

fn chunk(words: &[i64]) -> Function {
    let mut groups = Vec::new();
    let mut labels = Vec::new();
    let mut index = 0;
    while index < words.len() {
        let step = words
            .get(index + 1)
            .map_or(0, |next| next.wrapping_sub(words[index]));
        let mut length = 1;
        while index + length < words.len()
            && words[index + length] == words[index].wrapping_add(step.wrapping_mul(length as i64))
        {
            length += 1;
        }
        if length >= 4 {
            let label = groups.len() as u32;
            groups.push(Group::Affine {
                first: words[index],
                step,
                start: index,
            });
            labels.extend(std::iter::repeat_n(label, length));
            index += length;
        } else {
            let label = groups
                .iter()
                .position(|group| matches!(group, Group::Constant(value) if *value == words[index]))
                .unwrap_or_else(|| {
                    groups.push(Group::Constant(words[index]));
                    groups.len() - 1
                });
            labels.push(label as u32);
            index += 1;
        }
    }
    let mut function = Function::new([]);
    let mut c = function.instructions();
    c.block(BlockType::Empty);
    for _ in groups.iter().rev() {
        c.block(BlockType::Empty);
    }
    c.local_get(0)
        .i32_const(63)
        .i32_and()
        .br_table(labels, groups.len() as u32);
    for group in groups {
        c.end();
        match group {
            Group::Constant(value) => {
                constant(&mut c, value);
            }
            Group::Affine { first, step, start } => {
                if step == 0 {
                    constant(&mut c, first);
                } else {
                    c.local_get(0)
                        .i32_const(63)
                        .i32_and()
                        .i32_const(start as i32)
                        .i32_sub()
                        .i64_extend_i32_u();
                    constant(&mut c, step);
                    c.i64_mul();
                    constant(&mut c, first);
                    c.i64_add();
                }
            }
        }
        c.return_();
    }
    c.end().i64_const(0).end();
    function
}

fn route(c: &mut InstructionSink<'_>, first: usize, last: usize, chunk_base: u32) {
    if first == last {
        c.i64_const(0);
    } else if last - first == 1 {
        c.local_get(0).call(chunk_base + first as u32);
    } else {
        let middle = (first + last) / 2;
        c.local_get(0)
            .i32_const((middle * 64) as i32)
            .i32_lt_u()
            .if_(BlockType::Result(ValType::I64));
        route(c, first, middle, chunk_base);
        c.else_();
        route(c, middle, last, chunk_base);
        c.end();
    }
}

fn dispatcher(words: &[i64], chunk_base: u32) -> Function {
    let mut function = Function::new([]);
    let mut c = function.instructions();
    c.local_get(0)
        .i32_const(words.len() as i32)
        .i32_lt_u()
        .if_(BlockType::Result(ValType::I64));
    route(&mut c, 0, words.len().div_ceil(64), chunk_base);
    c.else_().i64_const(0).end().end();
    function
}

pub(super) fn functions(words: &[i64], numeric: bool, kernels: usize) -> Vec<Function> {
    if words.is_empty() {
        let mut empty = Function::new([]);
        empty.instructions().i64_const(0).end();
        return vec![empty];
    }
    u32::try_from(words.len()).expect("bounded immutable recovery words");
    let dictionary: Vec<_> = words
        .iter()
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let codes: BTreeMap<_, _> = dictionary
        .iter()
        .enumerate()
        .map(|(index, &word)| {
            (
                word,
                u32::try_from(index).expect("bounded recovery dictionary"),
            )
        })
        .collect();
    let bits = (usize::BITS - (dictionary.len() - 1).leading_zeros()).max(1);
    assert!(
        bits <= 32,
        "dictionary codes fit the existing bounded word index"
    );
    // valid indices have an exact u32 bit position; no emitted multiply can wrap.
    let bit_length = words
        .len()
        .checked_mul(bits as usize)
        .and_then(|length| u32::try_from(length).ok())
        .expect("bounded immutable recovery bit stream");
    let mut packed = vec![0u64; (bit_length as usize).div_ceil(64)];
    for (index, value) in words.iter().enumerate() {
        let position = index * bits as usize;
        let word = position >> 6;
        let shift = (position & 63) as u32;
        let code = u64::from(codes[value]);
        packed[word] |= code << shift;
        // a crossing code always has shift > 0 and a second allocated word.
        if shift + bits > 64 {
            packed[word + 1] |= code >> (64 - shift);
        }
    }
    let packed: Vec<i64> = packed.into_iter().map(|word| word as i64).collect();
    // The public getter remains definition9; narrow arithmetic kernels occupy
    // definitions10.. before these private constant-only helpers.
    let first = super::super::support::owner_defined(10 + kernels as u32, numeric);
    let packed_chunks = packed.len().div_ceil(64);
    let mut root = Function::new([(2, ValType::I32), (1, ValType::I64)]);
    root.instructions()
        .local_get(0)
        .i32_const(words.len() as i32)
        .i32_lt_u()
        .if_(BlockType::Result(ValType::I64))
        .local_get(0)
        .i32_const(bits as i32)
        .i32_mul()
        .local_tee(1)
        .i32_const(63)
        .i32_and()
        .local_set(2)
        .local_get(1)
        .i32_const(6)
        .i32_shr_u()
        .call(first)
        .local_set(3)
        .local_get(3)
        .local_get(2)
        .i64_extend_i32_u()
        .i64_shr_u()
        .local_get(2)
        .i32_const(bits as i32)
        .i32_add()
        .i32_const(64)
        .i32_gt_u()
        .if_(BlockType::Result(ValType::I64))
        .local_get(1)
        .i32_const(6)
        .i32_shr_u()
        .i32_const(1)
        .i32_add()
        .call(first)
        .i32_const(64)
        .local_get(2)
        .i32_sub()
        .i64_extend_i32_u()
        .i64_shl()
        .else_()
        .i64_const(0)
        .end()
        .i64_or()
        .i64_const(((1u64 << bits) - 1) as i64)
        .i64_and()
        .i32_wrap_i64()
        .call(first + 1)
        .else_()
        .i64_const(0)
        .end()
        .end();
    let mut functions = vec![
        root,
        dispatcher(&packed, first + 2),
        dispatcher(&dictionary, first + 2 + packed_chunks as u32),
    ];
    functions.extend(packed.chunks(64).map(chunk));
    functions.extend(dictionary.chunks(64).map(chunk));
    functions
}
