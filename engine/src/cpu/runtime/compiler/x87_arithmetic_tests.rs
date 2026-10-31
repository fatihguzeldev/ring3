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
    for (offset, value) in [
        (0, state.control),
        (2, state.status),
        (4, state.tag),
        (6, state.opcode),
        (16, state.code_selector),
        (18, state.data_selector),
    ] {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    bytes[8..12].copy_from_slice(&state.instruction_pointer.to_le_bytes());
    bytes[12..16].copy_from_slice(&state.data_pointer.to_le_bytes());
    bytes[20..].copy_from_slice(state.registers.as_flattened());
    bytes
}

fn operand(state: &mut X87State, index: u8, bits: u32) {
    let physical = (state.top() + usize::from(index)) & 7;
    state.registers[physical] = Ext80::from_f64_bits(f64::from(f32::from_bits(bits)).to_bits()).0;
    state.tag = (state.tag & !(3 << (physical * 2)))
        | (u16::from(bits & 0x7fffffff == 0) << (physical * 2));
}

fn state(control: u16, top: u16) -> X87State {
    X87State {
        control,
        status: (top << 11) | 0x472f,
        opcode: 0x345,
        instruction_pointer: 0x11223344,
        data_pointer: 0x55667788,
        code_selector: 0x7b,
        data_selector: 0x83,
        registers: std::array::from_fn(|i| std::array::from_fn(|j| (i * 29 + j * 7) as u8)),
        ..X87State::default()
    }
}

#[test]
fn generated_exact_x87_arithmetic_matches_strict_state_and_falls_back_before_effects() {
    use std::{
        fs,
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };
    let scratch = std::env::temp_dir().join(format!(
        "ring3-runtime-x87-arithmetic-{}-{}",
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
    let mut cases = Vec::new();
    let forms = [
        ("add-reg", &[0xd8, 0xc1][..]),
        ("mul-reg", &[0xd8, 0xc9][..]),
        ("div-reg", &[0xd8, 0xf1][..]),
        ("add-pop", &[0xde, 0xc1][..]),
        ("mul-pop", &[0xde, 0xc9][..]),
        ("div-pop", &[0xde, 0xf9][..]),
        ("add-mem", &[0xd8, 0x03][..]),
        ("mul-mem", &[0xd8, 0x0b][..]),
        ("div-mem", &[0xd8, 0x33][..]),
        ("add-fs", &[0x64, 0xd8, 0x03][..]),
    ];
    for (name, code) in forms {
        memory.write(GuestAddress(0x1000), code).unwrap();
        fs::write(
            scratch.join(format!("{name}.wasm")),
            compile(&memory, GuestAddress(0x1000), 1).unwrap().wasm,
        )
        .unwrap();
        let decoded = decode_one(&memory, GuestAddress(0x1000)).unwrap();
        let Operation::X87 { operation, .. } = *decoded.operation() else {
            panic!("expected numerical operation")
        };
        let pairs: &[(u32, u32)] = if name.starts_with("add") {
            &[
                (0x40400000, 0x40000000),
                (0xc0400000, 0x40000000),
                (0x3f800000, 0xbf800000),
                (0x80000000, 0x80000000),
                (0, 0x80000000),
                (0x00800000, 0x00800000),
                (0x7f7fffff, 0),
                (0x3f800001, 0xbf800000),
            ]
        } else if name.starts_with("mul") {
            &[
                (0x40400000, 0x40000000),
                (0xc0400000, 0x40000000),
                (0x00800000, 0x3f800000),
                (0x7f7fffff, 0x3f800000),
                (0, 0xbf800000),
                (0x80000000, 0xbf800000),
                (0x3fc00000, 0x40000000),
                (0x3f800001, 0x3f800000),
            ]
        } else {
            &[
                (0x40400000, 0x40000000),
                (0xc0400000, 0x40000000),
                (0x00800000, 0x3f800000),
                (0x7f7fffff, 0x3f800000),
                (0, 0xbf800000),
                (0x80000000, 0xbf800000),
                (0x40c00000, 0x40400000),
                (0x00800000, 0x00800000),
            ]
        };
        for control in [0x003f, 0x007f] {
            for top in [0, 3, 7] {
                for &(a, b) in pairs {
                    let mut before = state(control, top);
                    match operation {
                        X87Operation::Binary {
                            destination,
                            source,
                            ..
                        } => {
                            operand(&mut before, destination, a);
                            operand(&mut before, source, b);
                        }
                        _ => operand(&mut before, 0, a),
                    }
                    let mut numeric = NumericState::default();
                    numeric.x87 = before;
                    let mut segments = SegmentContext::default();
                    segments.state.selectors = [0x23, 0x1b, 0x23, 0x23, 0x3b, 0x43];
                    if name.ends_with("fs") {
                        segments.state.bases[4] = 0x1000;
                    }
                    let mut scalar = State32 {
                        registers: [
                            0x11223344,
                            0x55667788,
                            0x99aabbcc,
                            if name.ends_with("fs") { 0x2000 } else { 0x3000 },
                            0x4800,
                            0x12345678,
                            0x87654321,
                            0xaabbccdd,
                        ],
                        eip: 0x1000,
                        eflags: 0xcd7,
                    };
                    memory
                        .write(GuestAddress(0x3000), &b.to_le_bytes())
                        .unwrap();
                    numeric
                        .execute(
                            &mut memory,
                            &mut scalar,
                            &segments,
                            decoded.operation(),
                            decoded.next_pc().0,
                        )
                        .unwrap();
                    if name == "add-reg" && a == 0x40400000 && b == 0x40000000 && top == 0 {
                        assert_eq!(
                            numeric.x87.registers[0],
                            [0, 0, 0, 0, 0, 0, 0, 0xa0, 1, 0x40]
                        );
                        assert_eq!(numeric.x87.status, 0x452f);
                        assert_eq!(numeric.x87.control, control);
                    }
                    cases.push(format!("{{\"name\":\"{name}\",\"before\":{:?},\"after\":{:?},\"memory\":{b},\"fs\":{},\"status\":0,\"next\":{},\"quota\":0,\"retired\":1}}", raw(before), raw(numeric.x87), name.ends_with("fs"), decoded.next_pc().0));
                }
            }
        }
    }
    for (name, a, b, control, variant) in [
        ("add-reg", 0x3f800000, 0x0d800000, 0x007f, 0), // 1 + 2^-100: binary64 loses the addend
        ("add-reg", 0x3f800000, 0x33800000, 0x003f, 0), // midpoint
        ("mul-reg", 0x3f800001, 0x3f800001, 0x003f, 0),
        ("div-reg", 0x3f800000, 0x40400000, 0x007f, 0),
        ("div-reg", 0, 0, 0x003f, 0),
        ("div-reg", 0x3f800000, 0, 0x007f, 0),
        ("add-reg", 0x7f7fffff, 0x7f7fffff, 0x007f, 0),
        ("mul-reg", 0x00800000, 0x3f000000, 0x003f, 0),
        ("add-reg", 1, 0x3f800000, 0x007f, 0),
        ("add-reg", 0x7f800000, 0x3f800000, 0x003f, 0),
        ("add-reg", 0x7fc00001, 0x3f800000, 0x007f, 0),
        ("add-reg", 0x3f800000, 0x40000000, 0x037f, 0),
        ("add-reg", 0x3f800000, 0x40000000, 0x0c7f, 0),
        ("add-reg", 0x3f800000, 0x40000000, 0x007f, 1), // extra raw80 precision
        ("add-reg", 0x3f800000, 0x40000000, 0x007f, 2), // empty source
        ("add-mem", 0x3f800000, 1, 0x003f, 0),
        ("add-mem", 0x3f800000, 0x40000000, 0x007e, 3), // waiting #MF precedes operand access
    ] {
        let mut before = state(control, 0);
        operand(&mut before, 0, a);
        operand(&mut before, 1, b);
        if variant == 1 {
            before.registers[0][0] = 1;
        }
        if variant == 2 {
            before.tag |= 3 << 2;
        }
        if variant == 3 {
            before.status |= 1;
        }
        cases.push(format!("{{\"name\":\"{name}\",\"before\":{:?},\"after\":{:?},\"memory\":{b},\"fs\":false,\"status\":7,\"next\":4096,\"quota\":1,\"retired\":0,\"noRead\":{}}}", raw(before), raw(before), variant == 3));
    }
    let mut before = state(0x003f, 7);
    operand(&mut before, 0, 0x40400000);
    cases.push(format!("{{\"name\":\"add-mem\",\"before\":{:?},\"after\":{:?},\"memory\":1073741824,\"fs\":false,\"status\":3,\"next\":4096,\"quota\":1,\"retired\":0,\"fault\":true}}", raw(before), raw(before)));
    fs::write(scratch.join("cases.json"), format!("[{}]", cases.join(","))).unwrap();
    let result = Command::new("node").arg("--input-type=module").arg("-e").arg(r#"
import assert from 'node:assert/strict';import fs from 'node:fs';
const root=process.argv[1],memory=new WebAssembly.Memory({initial:2}),data=new DataView(memory.buffer),ram=new Uint8Array(memory.buffer),ptr=0x8000,bridge=0xa000,fp=0xb000;
const word=(at,value)=>data.setUint32(ptr+at,value>>>0,true),get=at=>data.getUint32(ptr+at,true);let current,reads=0;
const imports={env:{memory},cpu:{read(p,address,width){reads++;assert(!current.noRead,'pending #MF must precede memory');assert.equal(address,0x3000);assert.equal(width,4);word(52,current.fault?3:0);if(current.fault){word(60,address);word(68,4);return 0;}return current.memory;},write(){throw new Error('arithmetic cannot write guest RAM');}}};
const functions=new Map();
for(const item of JSON.parse(fs.readFileSync(`${root}/cases.json`))){current=item;reads=0;ram.fill(0);word(0,1);word(4,160);[0x11223344,0x55667788,0x99aabbcc,item.fs?0x2000:0x3000,0x4800,0x12345678,0x87654321,0xaabbccdd].forEach((v,i)=>word(8+i*4,v));word(40,0x1000);word(44,0xcd7);word(48,1);word(124,bridge);word(152,0x1f);if(item.fs)word(116,0x1000);data.setUint32(bridge,1,true);data.setUint32(bridge+4,36,true);data.setUint32(bridge+8,fp,true);[0x23,0x1b,0x23,0x23,0x3b,0x43].forEach((v,i)=>data.setUint32(bridge+12+i*4,v,true));ram.set(item.before,fp);const regs=Array.from({length:8},(_,i)=>get(8+i*4));if(!functions.has(item.name))functions.set(item.name,(await WebAssembly.instantiate(fs.readFileSync(`${root}/${item.name}.wasm`),imports)).instance.exports.run);assert.equal(functions.get(item.name)(ptr),item.status,item.name);assert.deepEqual([...ram.slice(fp,fp+100)],item.after,`${item.name} full raw80/ghost/tags/TOP/status/CW/metadata`);assert.deepEqual(Array.from({length:8},(_,i)=>get(8+i*4)),regs);assert.deepEqual([get(40),get(44),get(48),get(56)],[item.next,0xcd7,item.quota,item.retired]);if(item.noRead)assert.equal(reads,0);if(item.fault){assert.equal(reads,1);assert.equal(get(60),0x3000);}}
console.log('exact PC24 arithmetic CW003f/007f full canonical raw-state oracles, cancellation/zero/boundary literals and inexact/special/control/pending/fault fallback passed');
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
