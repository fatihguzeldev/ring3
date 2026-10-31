use super::*;
use crate::{
    cpu::{
        runtime::numeric::NumericState,
        x86::{Ext80, SegmentContext, State32, X87Operation, X87State, decode::decode_one},
    },
    memory::{PageRange, Permissions},
};

fn raw(state: X87State) -> Vec<u8> {
    let mut bytes = vec![0; 100];
    for (at, value) in [
        (0, state.control),
        (2, state.status),
        (4, state.tag),
        (6, state.opcode),
        (16, state.code_selector),
        (18, state.data_selector),
    ] {
        bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
    }
    bytes[8..12].copy_from_slice(&state.instruction_pointer.to_le_bytes());
    bytes[12..16].copy_from_slice(&state.data_pointer.to_le_bytes());
    bytes[20..].copy_from_slice(state.registers.as_flattened());
    bytes
}

fn initial(control: u16, top: u16, status: u16) -> X87State {
    X87State {
        control,
        status: (top << 11) | status,
        opcode: 0x321,
        instruction_pointer: 0x11223344,
        data_pointer: 0x55667788,
        code_selector: 0x73,
        data_selector: 0x7b,
        registers: std::array::from_fn(|i| std::array::from_fn(|j| (i * 31 + j * 13) as u8)),
        ..X87State::default()
    }
}

fn put(state: &mut X87State, index: u8, bits: u64) {
    let physical = (state.top() + usize::from(index)) & 7;
    state.registers[physical] = Ext80::from_f64_bits(bits).0;
    // occupied tag classes do not constrain the separately stored raw payload.
    let tag = if physical % 3 == 0 {
        2
    } else {
        u16::from(bits << 1 == 0)
    };
    state.tag = (state.tag & !(3 << (physical * 2))) | (tag << (physical * 2));
}

fn memory_words(bits: u64, width: u32) -> Option<[u32; 2]> {
    if width == 4 {
        let value = f64::from_bits(bits);
        let narrow = value as f32;
        if f64::from(narrow).to_bits() != bits {
            return None;
        }
        Some([narrow.to_bits(), 0])
    } else {
        Some([bits as u32, (bits >> 32) as u32])
    }
}

#[test]
fn generated_pc53_divide_matches_strict_quotient_flags_and_neutral_boundaries() {
    use std::{
        fs,
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };
    let scratch = std::env::temp_dir().join(format!(
        "ring3-runtime-x87-divide-{}-{}",
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
    let one = 0x3ff0_0000_0000_0000_u64;
    let forms = [
        ("reg", &[0xd8, 0xf1][..], 0),
        ("dest", &[0xdc, 0xf9][..], 0),
        ("pop", &[0xde, 0xf9][..], 0),
        ("self", &[0xd8, 0xf0][..], 0),
        ("m32", &[0xd8, 0x33][..], 4),
        ("m64", &[0xdc, 0x33][..], 8),
        ("fs32", &[0x64, 0xd8, 0x33][..], 4),
        ("fs64", &[0x64, 0xdc, 0x33][..], 8),
    ];
    // each admission decision is literal; expected raw state comes from the
    // established strict service, without a second copy of its rounding rule.
    let pairs = [
        (one, 0x4008_0000_0000_0000, true), // one third rounds down in magnitude
        (one, 0x4024_0000_0000_0000, true), // one tenth rounds up in magnitude
        (0x4014_0000_0000_0000, 0x4008_0000_0000_0000, true),
        (0x4008_0000_0000_0000, 0x4000_0000_0000_0000, true),
        (one + 1, one + 2, true),
        (0x3ff1_2345_6789_abcd, 0x3ffe_dcba_9876_5432, true),
        (0x3fef_ffff_ffff_ffff, 0x3ff0_0000_0000_0001, true),
        (0x0010_0000_0000_0000, one, true),
        (0x0020_0000_0000_0000, 0x4000_0000_0000_0000, true),
        (0x7fef_ffff_ffff_ffff, one, true),
        (0x7fef_ffff_ffff_ffff, 0x4000_0000_0000_0000, true),
        (0x0010_0000_0000_0000, 0x0010_0000_0000_0000, true),
        (0, 0x4008_0000_0000_0000, true),
    ];
    let boundaries = [
        (0x001f_ffff_ffff_ffff, 0x4000_0000_0000_0000, false),
        (0x001f_ffff_ffff_fffe, 0x4000_0000_0000_0000, false),
        (0x0010_0000_0000_0000, 0x4008_0000_0000_0000, false),
        (0x0010_0000_0000_0000, 0x7fef_ffff_ffff_ffff, false),
        (0x7fef_ffff_ffff_ffff, 0x0010_0000_0000_0000, false),
        (one, 0, false),
        (0, 0, false),
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
            panic!("expected x87 divide");
        };
        assert!(matches!(
            operation,
            X87Operation::Binary {
                kind: crate::cpu::x86::X87Binary::Divide,
                ..
            } | X87Operation::BinaryMemory {
                kind: crate::cpu::x86::X87Binary::Divide,
                ..
            }
        ));
        for (boundary, inputs) in [(false, pairs.as_slice()), (true, boundaries.as_slice())] {
            for &(a, b, admitted) in inputs {
                for sign in 0..4_u64 {
                    let a = a | ((sign & 1) << 63);
                    let mut b = b | ((sign >> 1) << 63);
                    let admitted = if name == "self" {
                        b = a;
                        a << 1 != 0
                    } else {
                        admitted
                    };
                    let Some(words) = memory_words(b, width) else {
                        continue;
                    };
                    for cw in [0x023f, 0x027f] {
                        for top in 0..8 {
                            if boundary && top != 0 {
                                continue;
                            }
                            for status in [0x450f, 0x472f] {
                                let mut before = initial(cw, top, status);
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
                                        0x11223344, 0x55667788, 0x99aabbcc, address, 0x4800,
                                        0x12345678, 0x87654321, 0xaabbccdd,
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
                                let cell = numeric.x87.registers
                                    [(usize::from(top) + usize::from(destination)) & 7];
                                if admitted && a << 1 != 0 {
                                    let exponent = u16::from_le_bytes([cell[8], cell[9]]) & 0x7fff;
                                    assert!((15361..=17406).contains(&exponent));
                                }
                                if name == "reg" && sign == 0 && top == 0 && status == 0x450f {
                                    if a == one && b == 0x4008_0000_0000_0000 {
                                        assert_eq!(
                                            cell,
                                            [0, 168, 170, 170, 170, 170, 170, 170, 253, 63]
                                        );
                                        assert_eq!(numeric.x87.status, 0x452f);
                                        assert_eq!((1.0_f64 / 3.0) * 3.0, 1.0);
                                    }
                                    if a == one && b == 0x4024_0000_0000_0000 {
                                        assert_eq!(
                                            cell,
                                            [0, 208, 204, 204, 204, 204, 204, 204, 251, 63]
                                        );
                                        assert_eq!(numeric.x87.status, 0x472f);
                                        assert_eq!((1.0_f64 / 10.0) * 10.0, 1.0);
                                    }
                                    if a == 0x001f_ffff_ffff_ffff && b == 0x4000_0000_0000_0000 {
                                        assert_eq!(
                                            cell,
                                            [0, 248, 255, 255, 255, 255, 255, 255, 0, 60]
                                        );
                                        assert_eq!(numeric.x87.status, 0x450f);
                                        assert_eq!(
                                            (f64::from_bits(a) / 2.0).to_bits(),
                                            0x0010_0000_0000_0000
                                        );
                                    }
                                }
                                cases.push(format!(
                                    "{{\"name\":\"{name}\",\"before\":{:?},\"after\":{:?},\"words\":{:?},\"width\":{width},\"fs\":{fs},\"status\":{},\"next\":{},\"quota\":{},\"retired\":{}}}",
                                    raw(before), raw(if admitted { numeric.x87 } else { before }), words,
                                    if admitted { 0 } else { 7 }, if admitted { decoded.next_pc().0 } else { 0x1000 },
                                    u32::from(!admitted), u32::from(admitted)
                                ));
                            }
                        }
                    }
                }
            }
        }
    }
    for (name, a, b, control, variant) in [
        ("reg", one + 1, one, 0x003f, 0),
        ("reg", one, one, 0x033f, 0),
        ("reg", one, one, 0x063f, 0),
        ("reg", one, one, 0x023e, 0),
        ("reg", 1, one, 0x023f, 0),
        ("reg", one, 1, 0x023f, 0),
        ("reg", 0x7ff0_0000_0000_0000, one, 0x023f, 0),
        ("reg", one, 0x7ff8_0000_0000_0001, 0x023f, 0),
        ("reg", one, one, 0x023f, 1),
        ("reg", one, one, 0x023f, 2),
        ("reg", one, one, 0x023f, 3),
        ("reg", one, one, 0x023f, 4),
        ("m64", one, one, 0x023e, 5),
        ("m64", one, one, 0x023f, 6),
        ("m64", one, one, 0x023f, 7),
        ("m32", one, one, 0x023f, 8),
        ("m64", one, 1, 0x023f, 0),
        ("m64", one, 0x7ff0_0000_0000_0000, 0x023f, 0),
    ] {
        let mut before = initial(control, 0, 0x4220);
        put(&mut before, 0, a);
        put(&mut before, 1, b);
        if variant == 1 {
            before.registers[0][0] |= 1;
        }
        if variant == 2 {
            before.registers[1][0] |= 1;
        }
        if variant == 3 {
            before.tag |= 3;
        }
        if variant == 4 {
            before.tag |= 3 << 2;
        }
        if variant == 5 {
            before.status |= 1;
        }
        let width = if name == "m64" {
            8
        } else if name == "m32" {
            4
        } else {
            0
        };
        let words = memory_words(b, width).unwrap();
        let fault = variant == 8;
        cases.push(format!(
            "{{\"name\":\"{name}\",\"before\":{:?},\"after\":{:?},\"words\":{:?},\"width\":{width},\"fs\":false,\"status\":{},\"next\":4096,\"quota\":1,\"retired\":0,\"noRead\":{},\"cross\":{},\"unreadable\":{},\"fault\":{fault}}}",
            raw(before), raw(before), words, if fault { 3 } else { 7 }, variant == 5, variant == 6, variant == 7
        ));
    }
    fs::write(scratch.join("cases.json"), format!("[{}]", cases.join(","))).unwrap();
    let result = Command::new("node").arg("--input-type=module").arg("-e").arg(r#"
import assert from 'node:assert/strict';
import fs from 'node:fs';
const root=process.argv[1],memory=new WebAssembly.Memory({initial:4}),data=new DataView(memory.buffer),ram=new Uint8Array(memory.buffer);
const ptr=0x8000,bridge=0xa000,fp=0xb000,map=0x1000,perms=0x2000,backing=0x10000;
const word=(at,v)=>data.setUint32(ptr+at,v>>>0,true),get=at=>data.getUint32(ptr+at,true);
let current,reads=0;
const imports={env:{memory},cpu:{read(p,address,width){
    reads++;assert(!current.noRead);assert.equal(width,4);assert.equal(address,current.fs?0x3001:0x3000);
    word(52,current.fault?3:0);if(current.fault){word(60,address);word(68,4);return 0;}return current.words[0];
},write(){throw new Error('DIV cannot write guest RAM');}}};
const functions=new Map();
for(const item of JSON.parse(fs.readFileSync(`${root}/cases.json`))){
    current=item;reads=0;ram.fill(0);word(0,1);word(4,160);
    [0x11223344,0x55667788,0x99aabbcc,item.cross?0x3ff9:item.fs?0x2001:0x3000,0x4800,0x12345678,0x87654321,0xaabbccdd].forEach((v,i)=>word(8+i*4,v));
    word(40,0x1000);word(44,0xcd7);word(48,1);word(124,bridge);word(152,63);if(item.fs)word(116,0x1000);
    data.setUint32(bridge,1,true);data.setUint32(bridge+4,40,true);data.setUint32(bridge+8,fp,true);
    [0x23,0x1b,0x23,0x23,0x3b,0x43].forEach((v,i)=>data.setUint32(bridge+12+i*4,v,true));ram.set(item.before,fp);
    if(item.width===8){word(76,map);word(80,perms);word(84,backing);word(88,4);data.setUint32(map+12,1,true);data.setUint8(perms+3,item.unreadable?0:1);data.setUint32(backing+(item.fs?1:0),item.words[0],true);data.setUint32(backing+(item.fs?5:4),item.words[1],true);}
    const regs=Array.from({length:8},(_,i)=>get(8+i*4)),beforeRAM=ram.slice(backing,backing+16384);
    if(!functions.has(item.name))functions.set(item.name,(await WebAssembly.instantiate(fs.readFileSync(`${root}/${item.name}.wasm`),imports)).instance.exports.run);
    assert.equal(functions.get(item.name)(ptr),item.status,`${item.name} strict PC53 DIV domain`);
    assert.deepEqual([...ram.slice(fp,fp+100)],item.after,`${item.name} raw80/ghost/TOP/tag/PE/C1/CW/metadata immediately after DIV`);
    assert.deepEqual(Array.from({length:8},(_,i)=>get(8+i*4)),regs);
    assert.deepEqual([get(40),get(44),get(48),get(56)],[item.next,0xcd7,item.quota,item.retired]);
    assert.deepEqual(ram.slice(backing,backing+16384),beforeRAM);
    if(item.width===8||item.noRead)assert.equal(reads,0);
    if(item.fault){assert.equal(reads,1);assert.equal(get(60),0x3000);assert.equal(get(68),4);}
}
console.log('PC53 DIV strict full-state oracle, exact-product PE/C1 anchors, lower-range tie, signs/zeros/allTOP/pop/FS and neutral control/fault boundaries passed');
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
