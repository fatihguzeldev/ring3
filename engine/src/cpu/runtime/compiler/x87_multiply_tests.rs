use super::*;
use crate::{
    cpu::{
        runtime::numeric::NumericState,
        x86::{SegmentContext, State32, X87Operation, X87State, decode::decode_one},
    },
    memory::{PageRange, Permissions},
};

#[derive(Clone, Copy)]
struct Value {
    sig: u64,
    exp: u16,
    negative: bool,
}

fn value(sig: u64, exp: u16) -> Value {
    Value {
        sig,
        exp,
        negative: false,
    }
}
fn neg(mut v: Value) -> Value {
    v.negative = !v.negative;
    v
}

fn raw(state: X87State) -> Vec<u8> {
    let mut bytes = vec![0; 100];
    for (at, v) in [
        (0, state.control),
        (2, state.status),
        (4, state.tag),
        (6, state.opcode),
        (16, state.code_selector),
        (18, state.data_selector),
    ] {
        bytes[at..at + 2].copy_from_slice(&v.to_le_bytes());
    }
    bytes[8..12].copy_from_slice(&state.instruction_pointer.to_le_bytes());
    bytes[12..16].copy_from_slice(&state.data_pointer.to_le_bytes());
    bytes[20..].copy_from_slice(state.registers.as_flattened());
    bytes
}

fn initial(cw: u16, top: u16, flags: u16) -> X87State {
    X87State {
        control: cw,
        status: (top << 11) | flags,
        opcode: 0x321,
        instruction_pointer: 0x11223344,
        data_pointer: 0x55667788,
        code_selector: 0x73,
        data_selector: 0x7b,
        registers: std::array::from_fn(|i| std::array::from_fn(|j| (i * 31 + j * 13) as u8)),
        ..X87State::default()
    }
}

fn put(state: &mut X87State, index: u8, v: Value) {
    let at = (state.top() + usize::from(index)) & 7;
    state.registers[at][..8].copy_from_slice(&v.sig.to_le_bytes());
    state.registers[at][8..]
        .copy_from_slice(&(v.exp | if v.negative { 0x8000 } else { 0 }).to_le_bytes());
    // nonempty tag classes are independent of payload classification.
    let tag = if at % 3 == 0 {
        2
    } else {
        u16::from(v.sig == 0)
    };
    state.tag = (state.tag & !(3 << (at * 2))) | (tag << (at * 2));
}

fn ieee(v: Value, width: u32) -> Option<[u32; 2]> {
    if v.sig == 0 && v.exp == 0 {
        return Some(if width == 4 {
            [u32::from(v.negative) << 31, 0]
        } else {
            [0, u32::from(v.negative) << 31]
        });
    }
    if width == 4 {
        if !(16257..=16510).contains(&v.exp) || v.sig & ((1 << 40) - 1) != 0 {
            return None;
        }
        Some([
            ((u32::from(v.exp) - 16256) << 23)
                | ((v.sig >> 40) as u32 & 0x7fffff)
                | (u32::from(v.negative) << 31),
            0,
        ])
    } else {
        if !(15361..=17406).contains(&v.exp) || v.sig & 0x7ff != 0 {
            return None;
        }
        let bits = ((u64::from(v.exp) - 15360) << 52)
            | ((v.sig >> 11) & 0x000fffffffffffff)
            | (u64::from(v.negative) << 63);
        Some([bits as u32, (bits >> 32) as u32])
    }
}

fn binary64(bits: u64) -> Value {
    let exponent = ((bits >> 52) & 0x7ff) as u16;
    assert!((1..0x7ff).contains(&exponent));
    Value {
        sig: ((bits & 0x000f_ffff_ffff_ffff) | (1 << 52)) << 11,
        exp: exponent + 15360,
        negative: bits >> 63 != 0,
    }
}

fn binary32(bits: u32) -> Value {
    let exponent = ((bits >> 23) & 255) as u16;
    assert!((1..255).contains(&exponent));
    Value {
        sig: u64::from((bits & 0x7f_ffff) | (1 << 23)) << 40,
        exp: exponent + 16256,
        negative: bits >> 31 != 0,
    }
}

// the residual is exact product minus the literal rounded result. its tuple
// records sign and power of two, independently of any floating implementation.
const BOUNDED_LITERALS: &[(u64, u32, u64, Option<(bool, i32)>, bool)] = &[
    (
        0x4008_0000_0000_0000,
        0x4000_0000,
        0x4018_0000_0000_0000,
        None,
        false,
    ),
    (
        0x3ff0_0000_0000_0001,
        0x3f7f_ffff,
        0x3fef_ffff_e000_0002,
        Some((true, -76)),
        true,
    ),
    (
        0x3ff0_0000_0000_0001,
        0x3f80_0001,
        0x3ff0_0000_2000_0001,
        Some((false, -75)),
        false,
    ),
    (
        0x3ff0_0000_0000_0001,
        0x3fc0_0000,
        0x3ff8_0000_0000_0002,
        Some((true, -53)),
        true,
    ),
    (
        0x3ff0_0000_0000_0003,
        0x3fc0_0000,
        0x3ff8_0000_0000_0004,
        Some((false, -53)),
        false,
    ),
    (
        0x3ff5_5555_5555_5555,
        0x3fc0_0000,
        0x4000_0000_0000_0000,
        Some((true, -53)),
        true,
    ),
];

fn bounded_pairs() -> Vec<(Value, Value)> {
    let mut pairs = Vec::new();
    for &(a, b, ..) in BOUNDED_LITERALS {
        let a = binary64(a);
        let b = binary32(b);
        pairs.extend([(a, b), (neg(a), b), (a, neg(b)), (neg(a), neg(b))]);
    }
    for exponent in [-450, 450] {
        for memory_exponent in [-126, 127] {
            for &(a, b, ..) in &BOUNDED_LITERALS[1..3] {
                let mut a = binary64(a);
                let mut b = binary32(b);
                a.exp = (16383 + exponent) as u16;
                b.exp = (16383 + memory_exponent) as u16;
                pairs.push((a, b));
            }
        }
    }
    // these normal values remain admitted by the existing integer product.
    // a proposed native predicate must fall back without introducing service.
    for exponent in [-451, 451] {
        let mut a = binary64(0x3ff0_0000_0000_0001);
        a.exp = (16383 + exponent) as u16;
        pairs.push((a, binary32(0x3f80_0001)));
    }
    let mut wider = binary64(0x3ff0_0000_0000_0001);
    wider.sig |= 1;
    pairs.push((wider, binary32(0x3f80_0001)));
    for bits in [
        0x3ff0_0000_0000_0000,
        0x3ff0_0000_07ff_ffff,
        0x3ff0_0000_0800_0000,
        0x3ff0_0000_0800_0001,
        0x3fff_ffff_ffff_ffff,
    ] {
        pairs.push((binary64(bits), binary32(0x3fff_ffff)));
    }
    pairs
}

#[test]
fn strict_pc53_memory32_products_preserve_literal_residuals_ties_and_signs() {
    let mut memory = AddressSpace::new(2).unwrap();
    for address in [0x1000, 0x3000] {
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(address), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
    }
    memory.write(GuestAddress(0x1000), &[0xd8, 0x0b]).unwrap();
    let decoded = decode_one(&memory, GuestAddress(0x1000)).unwrap();
    let mut segments = SegmentContext::default();
    segments.state.selectors = [0x23, 0x1b, 0x23, 0x23, 0x3b, 0x43];
    let segments = SegmentContext::legacy(segments.state);
    for &(a_bits, b_bits, rounded_bits, residual, incremented) in BOUNDED_LITERALS {
        let original_a = binary64(a_bits);
        let original_b = binary32(b_bits);
        let original_result = binary64(rounded_bits);
        for ea in [-450, i32::from(original_a.exp) - 16383, 450] {
            for eb in [-126, i32::from(original_b.exp) - 16383, 127] {
                let scale =
                    ea + eb - (i32::from(original_a.exp) + i32::from(original_b.exp) - 32766);
                for signs in 0..4 {
                    let mut a = original_a;
                    let mut b = original_b;
                    let mut rounded = original_result;
                    a.exp = (ea + 16383) as u16;
                    b.exp = (eb + 16383) as u16;
                    a.negative = signs & 1 != 0;
                    b.negative = signs & 2 != 0;
                    rounded.exp = (i32::from(rounded.exp) + scale) as u16;
                    rounded.negative = a.negative ^ b.negative;
                    let exact = u128::from(a.sig) * u128::from(b.sig);
                    let shift = i32::from(rounded.exp) - (ea + eb + 16383) + 63;
                    assert!((0..128).contains(&shift));
                    let retained = u128::from(rounded.sig) << shift;
                    if let Some((negative, power)) = residual {
                        let residual_shift = power + scale - (ea + eb - 126);
                        assert!((0..128).contains(&residual_shift));
                        assert_eq!(exact.abs_diff(retained), 1_u128 << residual_shift);
                        assert_eq!(exact < retained, negative);
                        assert_eq!(exact < retained, incremented);
                    } else {
                        assert_eq!(exact, retained);
                        assert!(!incremented);
                    }
                    let words = ieee(b, 4).unwrap();
                    memory
                        .write(GuestAddress(0x3000), &words[0].to_le_bytes())
                        .unwrap();
                    for cw in [0x023f, 0x027f] {
                        for top in [0, 7] {
                            for flags in [0x450f, 0x452f, 0x470f, 0x472f] {
                                let mut before = initial(cw, top, flags);
                                put(&mut before, 0, a);
                                let mut expected = before;
                                let physical = usize::from(top);
                                expected.registers[physical][..8]
                                    .copy_from_slice(&rounded.sig.to_le_bytes());
                                expected.registers[physical][8..].copy_from_slice(
                                    &(rounded.exp | if rounded.negative { 0x8000 } else { 0 })
                                        .to_le_bytes(),
                                );
                                expected.tag &= !(3 << (physical * 2));
                                expected.status = (before.status & !0x200)
                                    | (if residual.is_some() { 0x20 } else { 0 })
                                    | (if incremented { 0x200 } else { 0 });
                                expected.opcode = 0x0b;
                                expected.instruction_pointer = 0x1000;
                                expected.data_pointer = 0x3000;
                                expected.code_selector = 0x1b;
                                expected.data_selector = 0x23;
                                let mut scalar = State32 {
                                    registers: [1, 2, 3, 0x3000, 5, 6, 7, 8],
                                    eip: 0x1000,
                                    eflags: 0xcd7,
                                };
                                let mut numeric = NumericState::default();
                                numeric.x87 = before;
                                numeric
                                    .execute(
                                        &mut memory,
                                        &mut scalar,
                                        &segments,
                                        decoded.operation(),
                                        decoded.next_pc().0,
                                    )
                                    .unwrap();
                                assert_eq!(
                                    raw(numeric.x87),
                                    raw(expected),
                                    "a={a_bits:x} b={b_bits:x} ea={ea} eb={eb} signs={signs} cw={cw:x} top={top} flags={flags:x}"
                                );
                                assert_eq!(scalar.registers, [1, 2, 3, 0x3000, 5, 6, 7, 8]);
                                assert_eq!(
                                    (scalar.eip, scalar.eflags),
                                    (decoded.next_pc().0, 0xcd7)
                                );
                                let mut stored = [0; 4];
                                memory.read(GuestAddress(0x3000), &mut stored).unwrap();
                                assert_eq!(stored, words[0].to_le_bytes());
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn generated_raw80_multiply_matches_strict_product_rounding_and_neutral_boundaries() {
    use std::{
        fs,
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };
    let scratch = std::env::temp_dir().join(format!(
        "ring3-runtime-x87-multiply-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&scratch).unwrap();
    let mut memory = AddressSpace::new(2).unwrap();
    for address in [0x1000, 0x3000] {
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(address), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
    }
    let one = value(1 << 63, 0x3fff);
    let zero = value(0, 0);
    let mut pairs = vec![
        (value(0x8000000000000001, 0x3fff), one),
        (one, one),
        (value(u64::MAX, 0x3fff), value(u64::MAX, 0x3fff)),
        (value(0x8000008000000000, 0x3fff), one),
        (value(0x8000018000000000, 0x3fff), one),
        (value(0x8000000000000400, 0x3fff), one),
        (value(0x8000000000000c00, 0x3fff), one),
        (neg(value(0x8000018000000000, 0x3fff)), one),
        (neg(value(0x8000000000000c00, 0x3fff)), one),
        (
            value(0xabcdef0123456789, 0x3fff),
            value(0xfedcba9876543210, 0x3fff),
        ),
        (value(1 << 63, 1), one),
        (value(1 << 63, 0x7ffe), one),
        (value(1 << 63, 0x3f81), value(1 << 63, 0x3ffe)), // min binary32 times one half
        (value(1 << 63, 0x3f81), value(0xffffff0000000000, 0x3ffe)),
        (value(1 << 63, 0x3c01), value(1 << 63, 0x3ffe)), // min binary64 times one half
        (zero, neg(one)),
        (neg(zero), neg(one)),
        (neg(one), zero),
        (value(1 << 63, 1), value(1 << 63, 0x3ffe)), // pre-round exponent underflow
        (value(1 << 63, 0x7ffe), value(1 << 63, 0x4000)), // pre-round overflow
        (value(u64::MAX, 0x7ffe), one),              // rounding carry overflows the exponent
    ];
    pairs.extend(bounded_pairs());
    let carry = u128::from(u64::MAX) * u128::from(u64::MAX);
    assert_eq!(carry as u64, 1);
    assert_eq!((carry >> 64) as u64, 0xfffffffffffffffe);
    let forms = [
        ("reg", &[0xd8, 0xc9][..], 0),
        ("dest", &[0xdc, 0xc9][..], 0),
        ("pop", &[0xde, 0xc9][..], 0),
        ("self", &[0xd8, 0xc8][..], 0),
        ("m32", &[0xd8, 0x0b][..], 4),
        ("m64", &[0xdc, 0x0b][..], 8),
        ("fs32", &[0x64, 0xd8, 0x0b][..], 4),
        ("fs64", &[0x64, 0xdc, 0x0b][..], 8),
    ];
    let mut cases = Vec::new();
    for (name, code, width) in forms {
        memory.write(GuestAddress(0x1000), code).unwrap();
        fs::write(
            scratch.join(format!("{name}.wasm")),
            compile(&memory, GuestAddress(0x1000), 1).unwrap().wasm,
        )
        .unwrap();
        let decoded = decode_one(&memory, GuestAddress(0x1000)).unwrap();
        let Operation::X87 { operation, .. } = *decoded.operation() else {
            panic!("expected x87")
        };
        for &(a, mut b) in &pairs {
            if name == "self" {
                b = a;
            }
            let words = if width == 0 {
                [0, 0]
            } else {
                let Some(words) = ieee(b, width) else {
                    continue;
                };
                words
            };
            for cw in [0x003f, 0x007f, 0x023f, 0x027f] {
                for top in 0..8 {
                    for flags in [0x4220, 0x450f] {
                        let mut before = initial(cw, top, flags);
                        let destination = match operation {
                            X87Operation::Binary {
                                destination,
                                source,
                                ..
                            } => {
                                put(&mut before, destination, a);
                                put(&mut before, source, b);
                                destination
                            }
                            _ => {
                                put(&mut before, 0, a);
                                0
                            }
                        };
                        let fs = name.starts_with("fs");
                        let address = if fs { 0x2001 } else { 0x3000 };
                        let linear = if fs { 0x3001 } else { 0x3000 };
                        let mut segments = SegmentContext::default();
                        segments.state.selectors = [0x23, 0x1b, 0x23, 0x23, 0x3b, 0x43];
                        if fs {
                            segments.state.bases[4] = 0x1000;
                        }
                        let mut scalar = State32 {
                            registers: [
                                0x11223344, 0x55667788, 0x99aabbcc, address, 0x4800, 0x12345678,
                                0x87654321, 0xaabbccdd,
                            ],
                            eip: 0x1000,
                            eflags: 0xcd7,
                        };
                        let mut input = Vec::new();
                        for word in words {
                            input.extend_from_slice(&word.to_le_bytes());
                        }
                        if width != 0 {
                            memory
                                .write(GuestAddress(linear), &input[..width as usize])
                                .unwrap();
                        }
                        let mut numeric = NumericState::default();
                        numeric.x87 = before;
                        numeric
                            .execute(
                                &mut memory,
                                &mut scalar,
                                &segments,
                                decoded.operation(),
                                decoded.next_pc().0,
                            )
                            .unwrap();
                        let product = u128::from(a.sig) * u128::from(b.sig);
                        let pre =
                            i32::from(a.exp) + i32::from(b.exp) - 16383 + ((product >> 127) as i32);
                        let result = numeric.x87.registers
                            [(usize::from(top) + usize::from(destination)) & 7];
                        let post = u16::from_le_bytes([result[8], result[9]]) & 0x7fff;
                        let admitted = a.sig == 0
                            || b.sig == 0
                            || (1..=0x7ffe).contains(&pre) && (1..=0x7ffe).contains(&post);
                        if admitted && name == "reg" && top == 0 && flags == 0x450f {
                            if a.sig == u64::MAX && a.exp == 0x3fff && b.sig == u64::MAX {
                                assert_eq!(result, [0, 0, 0, 0, 0, 0, 0, 0x80, 1, 0x40]);
                                assert_eq!(numeric.x87.status, 0x472f);
                            }
                            let p = if cw & 0x300 == 0 { 24 } else { 53 };
                            let half = 1u64 << (63 - p);
                            if a.exp == 0x3fff
                                && b.sig == 1 << 63
                                && b.exp == 0x3fff
                                && a.sig == (1 << 63) + half
                            {
                                assert_eq!(
                                    result,
                                    [
                                        0,
                                        0,
                                        0,
                                        0,
                                        0,
                                        0,
                                        0,
                                        0x80,
                                        0xff,
                                        if a.negative { 0xbf } else { 0x3f }
                                    ]
                                );
                                assert_eq!(numeric.x87.status, 0x452f);
                            }
                            if a.exp == 0x3fff
                                && b.sig == 1 << 63
                                && b.exp == 0x3fff
                                && a.sig == (1 << 63) + 3 * half
                            {
                                assert_eq!(numeric.x87.status, 0x472f);
                            }
                        }
                        cases.push(format!("{{\"name\":\"{name}\",\"before\":{:?},\"after\":{:?},\"words\":{:?},\"width\":{width},\"fs\":{fs},\"status\":{},\"next\":{},\"quota\":{},\"retired\":{}}}",raw(before),raw(if admitted{numeric.x87}else{before}),words,if admitted{0}else{7},if admitted{decoded.next_pc().0}else{0x1000},u32::from(!admitted),u32::from(admitted)));
                    }
                }
            }
        }
    }
    for (name, a, b, cw, variant) in [
        ("reg", value(1 << 63, 0x3fff), one, 0x037f, 0),
        ("reg", one, one, 0x063f, 0),
        ("reg", one, one, 0x003e, 0),
        ("reg", value(1, 0), one, 0x023f, 0),
        ("reg", value(1, 0x3fff), one, 0x023f, 0),
        ("reg", value(1 << 63, 0x7fff), one, 0x003f, 0),
        ("reg", value(0xc000000000000001, 0x7fff), one, 0x023f, 0),
        ("reg", one, one, 0x023f, 1),
        ("m64", one, one, 0x023e, 2),
        ("m64", one, one, 0x023f, 3),
        ("m64", one, one, 0x023f, 4),
        ("m32", one, one, 0x003f, 5),
        (
            "m32",
            binary64(0x3ff0_0000_0000_0001),
            binary32(0x3f80_0001),
            0x023f,
            6,
        ),
    ] {
        let mut before = initial(cw, 0, 0x4220);
        put(&mut before, 0, a);
        put(&mut before, 1, b);
        if variant == 1 {
            before.tag |= 3 << 2;
        }
        if variant == 2 {
            before.status |= 1;
        }
        if variant == 6 {
            before.status = 0x470f;
        }
        let width = if name == "m64" {
            8
        } else if name == "m32" {
            4
        } else {
            0
        };
        let words = if width == 0 {
            [0, 0]
        } else {
            ieee(b, width).unwrap()
        };
        let fault = matches!(variant, 5 | 6);
        cases.push(format!("{{\"name\":\"{name}\",\"before\":{:?},\"after\":{:?},\"words\":{:?},\"width\":{width},\"fs\":false,\"status\":{},\"next\":4096,\"quota\":1,\"retired\":0,\"noRead\":{},\"cross\":{},\"unreadable\":{},\"fault\":{fault}}}",raw(before),raw(before),words,if fault{3}else{7},variant==2,variant==3,variant==4));
    }
    fs::write(scratch.join("cases.json"), format!("[{}]", cases.join(","))).unwrap();
    let result=Command::new("node").arg("--input-type=module").arg("-e").arg(r#"
import assert from 'node:assert/strict';import fs from 'node:fs';
const root=process.argv[1],memory=new WebAssembly.Memory({initial:4}),data=new DataView(memory.buffer),ram=new Uint8Array(memory.buffer),ptr=0x8000,bridge=0xa000,fp=0xb000,map=0x1000,perms=0x2000,backing=0x10000;
const word=(at,v)=>data.setUint32(ptr+at,v>>>0,true),get=at=>data.getUint32(ptr+at,true);let current,reads=0;
const imports={env:{memory},cpu:{read(p,address,width){reads++;assert(!current.noRead);assert.equal(width,4);assert.equal(address,current.fs?0x3001:0x3000);word(52,current.fault?3:0);if(current.fault){word(60,address);word(68,4);return 0;}return current.words[0];},write(){throw new Error('MUL cannot write guest RAM');}}};const functions=new Map();
for(const item of JSON.parse(fs.readFileSync(`${root}/cases.json`))){current=item;reads=0;ram.fill(0);word(0,1);word(4,160);[0x11223344,0x55667788,0x99aabbcc,item.cross?0x3ff9:item.fs?0x2001:0x3000,0x4800,0x12345678,0x87654321,0xaabbccdd].forEach((v,i)=>word(8+i*4,v));word(40,0x1000);word(44,0xcd7);word(48,1);word(124,bridge);word(152,63);if(item.fs)word(116,0x1000);data.setUint32(bridge,1,true);data.setUint32(bridge+4,40,true);data.setUint32(bridge+8,fp,true);[0x23,0x1b,0x23,0x23,0x3b,0x43].forEach((v,i)=>data.setUint32(bridge+12+i*4,v,true));ram.set(item.before,fp);if(item.width===8){word(76,map);word(80,perms);word(84,backing);word(88,4);data.setUint32(map+12,1,true);data.setUint8(perms+3,item.unreadable?0:1);data.setUint32(backing+(item.fs?1:0),item.words[0],true);data.setUint32(backing+(item.fs?5:4),item.words[1],true);}const regs=Array.from({length:8},(_,i)=>get(8+i*4)),beforeRAM=ram.slice(backing,backing+65536);if(!functions.has(item.name))functions.set(item.name,(await WebAssembly.instantiate(fs.readFileSync(`${root}/${item.name}.wasm`),imports)).instance.exports.run);assert.equal(functions.get(item.name)(ptr),item.status,`${item.name} raw80 normal-product domain`);assert.deepEqual([...ram.slice(fp,fp+100)],item.after,`${item.name} full raw80/ghost/TOP/tag/PE/C1/CW/metadata`);assert.deepEqual(Array.from({length:8},(_,i)=>get(8+i*4)),regs);assert.deepEqual([get(40),get(44),get(48),get(56)],[item.next,0xcd7,item.quota,item.retired]);assert.deepEqual(ram.slice(backing,backing+65536),beforeRAM);if(item.width===8||item.noRead)assert.equal(reads,0);if(item.fault){assert.equal(reads,1);assert.equal(get(60),0x3000);}}
console.log('raw80 MUL exact128 reference, PC24/PC53 signed ties/carries/PE/C1/allTOP/ghost/pop/metadata, IEEE memory and neutral control/range/special/fault boundaries passed');
"#).arg(&scratch).output().unwrap();
    assert!(
        result.status.success(),
        "fixture {} failed:\n{}\n{}",
        scratch.display(),
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    fs::remove_dir_all(scratch).unwrap();
}
