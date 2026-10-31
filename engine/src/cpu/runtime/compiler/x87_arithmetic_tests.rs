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

fn operand64(state: &mut X87State, index: u8, bits: u64) {
    let physical = (state.top() + usize::from(index)) & 7;
    state.registers[physical] = Ext80::from_f64_bits(bits).0;
    state.tag = (state.tag & !(3 << (physical * 2)))
        | (u16::from(bits & 0x7fff_ffff_ffff_ffff == 0) << (physical * 2));
}

#[test]
fn generated_x87_control_changes_preserve_precision_and_fault_priority() {
    use std::{
        fs,
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };
    let scratch = std::env::temp_dir().join(format!(
        "ring3-runtime-x87-control-{}-{}",
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
    let programs: &[(&str, &[u8], u32)] = &[
        (
            "register",
            &[0xd8, 0xc1, 0xd9, 0x2d, 0x10, 0x30, 0, 0, 0xd8, 0xc1],
            3,
        ),
        (
            "memory",
            &[0xd8, 0xc1, 0xd9, 0x2d, 0x10, 0x30, 0, 0, 0xd8, 0x03],
            3,
        ),
        ("load-first", &[0xd9, 0x2d, 0x10, 0x30, 0, 0, 0xd8, 0x03], 2),
    ];
    for &(name, code, limit) in programs {
        memory.write(GuestAddress(0x1000), code).unwrap();
        fs::write(
            scratch.join(format!("{name}.wasm")),
            compile(&memory, GuestAddress(0x1000), limit as usize)
                .unwrap()
                .wasm,
        )
        .unwrap();
    }
    let mut cases = Vec::new();
    for (label, name, old_control, new_control, initial_status, retired, status, fault, blocked) in [
        (
            "pc24-to-pc53",
            "register",
            0x003f,
            0x023f,
            0x4020,
            3,
            0,
            0,
            0,
        ),
        (
            "reserved-bit-six",
            "register",
            0x007f,
            0x027f,
            0x4020,
            3,
            0,
            0,
            0,
        ),
        (
            "pc53-to-pc24",
            "register",
            0x023f,
            0x003f,
            0x4020,
            2,
            7,
            0,
            0,
        ),
        (
            "rounding-change",
            "register",
            0x023f,
            0x063f,
            0x4020,
            2,
            7,
            0,
            0,
        ),
        (
            "precision-change",
            "memory",
            0x003f,
            0x037f,
            0x4020,
            2,
            7,
            0,
            0x3020,
        ),
        (
            "reserved-high-bit",
            "register",
            0x003f,
            0x123f,
            0x4020,
            2,
            7,
            0,
            0,
        ),
        (
            "unmask-sticky",
            "memory",
            0x003f,
            0x021f,
            0x4020,
            2,
            7,
            0,
            0x3020,
        ),
        (
            "old-pending-before-load",
            "load-first",
            0x023e,
            0x023f,
            0x4001,
            0,
            7,
            0x3010,
            0x3010,
        ),
        (
            "faulting-control-load",
            "register",
            0x003f,
            0x023f,
            0x4020,
            1,
            3,
            0x3010,
            0,
        ),
    ] {
        let &(_, code, limit) = programs
            .iter()
            .find(|(program, _, _)| *program == name)
            .unwrap();
        memory.write(GuestAddress(0x1000), code).unwrap();
        memory
            .write(GuestAddress(0x3010), &u16::to_le_bytes(new_control))
            .unwrap();
        memory
            .write(GuestAddress(0x3020), &0x3080_0000_u32.to_le_bytes())
            .unwrap();
        let mut before = state(old_control, 0);
        before.status = initial_status;
        operand64(&mut before, 0, 1_f64.to_bits());
        operand64(&mut before, 1, (2_f64.powi(-30)).to_bits());
        let mut numeric = NumericState::default();
        numeric.x87 = before;
        let mut segments = SegmentContext::default();
        segments.state.selectors = [0x23, 0x1b, 0x23, 0x23, 0x3b, 0x43];
        let mut scalar = State32 {
            registers: [0x11223344, 2, 3, 0x3020, 0x4800, 6, 7, 8],
            eip: 0x1000,
            eflags: 0xcd7,
        };
        for _ in 0..retired {
            let decoded = decode_one(&memory, GuestAddress(scalar.eip)).unwrap();
            numeric
                .execute(
                    &mut memory,
                    &mut scalar,
                    &segments,
                    decoded.operation(),
                    decoded.next_pc().0,
                )
                .unwrap();
            scalar.eip = decoded.next_pc().0;
        }
        if label == "pc24-to-pc53" {
            assert_eq!(numeric.x87.control, 0x023f);
            assert_eq!(numeric.x87.status, 0x4020);
            assert_eq!(
                numeric.x87.registers[0],
                [0, 0, 0, 0, 2, 0, 0, 0x80, 0xff, 0x3f]
            );
        } else if label == "pc53-to-pc24" {
            assert_eq!(numeric.x87.control, 0x003f);
            assert_eq!(
                numeric.x87.registers[0],
                [0, 0, 0, 0, 2, 0, 0, 0x80, 0xff, 0x3f]
            );
        } else if label == "unmask-sticky" {
            assert_eq!(numeric.x87.status, 0xc0a0);
        }
        let reads: &[u32] = if retired == 0 { &[] } else { &[0x3010] };
        cases.push(format!(
            "{{\"label\":\"{label}\",\"name\":\"{name}\",\"before\":{:?},\"after\":{:?},\"control\":{new_control},\"limit\":{limit},\"retired\":{retired},\"status\":{status},\"next\":{},\"fault\":{fault},\"blocked\":{blocked},\"reads\":{reads:?}}}",
            raw(before), raw(numeric.x87), scalar.eip,
        ));
    }
    fs::write(scratch.join("cases.json"), format!("[{}]", cases.join(","))).unwrap();
    let result = Command::new("node").arg("--input-type=module").arg("-e").arg(r#"
import assert from 'node:assert/strict';import fs from 'node:fs';
const root=process.argv[1],memory=new WebAssembly.Memory({initial:1}),ram=new Uint8Array(memory.buffer),data=new DataView(memory.buffer),ptr=0x8000,view=0xa000,fp=0xb000;
const word=(at,value)=>data.setUint32(ptr+at,value>>>0,true);let item,reads=[];
const imports={env:{memory},cpu:{read(p,address,width){assert.equal(p,ptr);address>>>=0;assert.notEqual(address,item.blocked,'pending exception/profile miss precedes operand read');reads.push(address);if(address===item.fault){word(52,3);word(60,address);word(64,width|0x10100);word(68,width);return 0;}word(52,0);return width===2?data.getUint16(address,true):data.getUint32(address,true);},write(){throw new Error('no numerical guest stores');}}};
const functions=new Map();
for(item of JSON.parse(fs.readFileSync(`${root}/cases.json`))){
  for(let i=0;i<ram.length;i++)ram[i]=(i*29+7)&255;ram.fill(0,ptr,ptr+160);ram.fill(0,view,view+40);
  word(0,1);word(4,160);[0x11223344,2,3,0x3020,0x4800,6,7,8].forEach((v,i)=>word(8+4*i,v));word(40,0x1000);word(44,0xcd7);word(48,item.limit);word(124,view);word(152,0x1f);
  data.setUint32(view,1,true);data.setUint32(view+4,36,true);data.setUint32(view+8,fp,true);[0x23,0x1b,0x23,0x23,0x3b,0x43].forEach((v,i)=>data.setUint32(view+12+4*i,v,true));ram.set(item.before,fp);data.setUint16(0x3010,item.control,true);data.setUint32(0x3020,0x30800000,true);
  const expected=ram.slice(),dv=new DataView(expected.buffer);expected.set(item.after,fp);for(const[at,value]of[[40,item.next],[48,item.limit-item.retired],[52,item.status],[56,item.retired]])dv.setUint32(ptr+at,value,true);
  if(item.status===3)for(const[at,value]of[[60,item.fault],[64,0x10102],[68,2]])dv.setUint32(ptr+at,value,true);
  reads=[];if(!functions.has(item.name))functions.set(item.name,(await WebAssembly.instantiate(fs.readFileSync(`${root}/${item.name}.wasm`),imports)).instance.exports.run);
  assert.equal(functions.get(item.name)(ptr),item.status,item.label);assert.deepEqual(ram,expected,`${item.label}: full RAM, bank, ghost, CW/status, scalar and exact quota`);assert.deepEqual(reads,item.reads,`${item.label}: ordered operand reads`);
}
console.log('within-invocation x87 control changes: PC24/53 precision, RC/unsupported fallback, old/new pending priority and faulting FLDCW full-state baseline passed');
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
                (0x00800000, 0x3f000000),
                (0x00800000, 0x3f7fffff),
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
        let mut inputs: Vec<_> = pairs.iter().map(|&(a, b)| (a, b, 0x472f)).collect();
        let inexact: &[(u32, u32)] = if name.starts_with("add") {
            &[
                (0x3f800000, 0x0d800000),
                (0x3f800000, 0x8d800000),
                (0xbf800000, 0x0d800000),
                (0xbf800000, 0x8d800000),
                (0x3f800000, 0x33800000),
                (0x3f800001, 0x33800000),
                (0xbf800000, 0xb3800000),
                (0xbf800001, 0xb3800000),
            ]
        } else if name.starts_with("mul") {
            &[
                (0x3f800001, 0x3f800001),
                (0x3f800001, 0x3fc00000),
                (0xbf800001, 0x3fc00000),
            ]
        } else {
            &[
                (0x3f800000, 0x40400000),
                (0xbf800000, 0x40400000),
                (0x3f800000, 0x41c80000),
                (0xbf800000, 0x41c80000),
            ]
        };
        for &(a, b) in inexact {
            for status in [0x470f, 0x472f, 0x450f] {
                inputs.push((a, b, status));
            }
        }
        for control in [0x003f, 0x007f] {
            for top in [0, 3, 7] {
                for &(a, b, status) in &inputs {
                    let mut before = state(control, top);
                    before.status = (top << 11) | status;
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
                    if top == 0 && status == 0x450f {
                        if name == "add-reg" && a == 0x3f800000 && b == 0x33800000 {
                            assert_eq!(
                                numeric.x87.registers[0],
                                [0, 0, 0, 0, 0, 0, 0, 0x80, 0xff, 0x3f]
                            );
                            assert_eq!(numeric.x87.status, 0x452f); // halfway to even: PE, no increment
                        }
                        if name == "add-reg" && a == 0x3f800001 && b == 0x33800000 {
                            assert_eq!(
                                numeric.x87.registers[0],
                                [0, 0, 0, 0, 0, 2, 0, 0x80, 0xff, 0x3f]
                            );
                            assert_eq!(numeric.x87.status, 0x472f); // halfway from odd: PE and C1
                        }
                        if name == "mul-reg" && a == 0x3f800001 && b == 0x3fc00000 {
                            assert_eq!(
                                numeric.x87.registers[0],
                                [0, 0, 0, 0, 0, 2, 0, 0xc0, 0xff, 0x3f]
                            );
                            assert_eq!(numeric.x87.status, 0x472f);
                        }
                        if name == "div-reg" && a == 0x3f800000 && b == 0x40400000 {
                            assert_eq!(
                                numeric.x87.registers[0],
                                [0, 0, 0, 0, 0, 0xab, 0xaa, 0xaa, 0xfd, 0x3f]
                            );
                            assert_eq!(numeric.x87.status, 0x472f);
                        }
                        if name == "div-reg" && a == 0x3f800000 && b == 0x41c80000 {
                            assert_eq!(
                                numeric.x87.registers[0],
                                [0, 0, 0, 0, 0, 0x0a, 0xd7, 0xa3, 0xfa, 0x3f]
                            );
                            assert_eq!(numeric.x87.status, 0x452f);
                        }
                    }
                    cases.push(format!("{{\"name\":\"{name}\",\"before\":{:?},\"after\":{:?},\"memory\":{b},\"fs\":{},\"status\":0,\"next\":{},\"quota\":0,\"retired\":1}}", raw(before), raw(numeric.x87), name.ends_with("fs"), decoded.next_pc().0));
                }
            }
        }
    }
    for (name, a, b, control, variant) in [
        ("div-reg", 0, 0, 0x003f, 0),
        ("div-reg", 0x3f800000, 0, 0x007f, 0),
        ("add-reg", 0x7f7fffff, 0x7f7fffff, 0x007f, 0),
        ("div-reg", 0x3fffffff, 0x7f000000, 0x007f, 0), // same lower-bound distinction through division
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
console.log('PC24 arithmetic CW003f/007f full canonical raw-state and PE/C1 budget-boundary oracles, both signs/ties/losttiny/sticky flags and range/control/pending/fault fallback passed');
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

#[test]
fn generated_pc53_add_preserves_full_raw_state_and_exact_rounding_flags() {
    use std::{
        fs,
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };
    let scratch = std::env::temp_dir().join(format!(
        "ring3-runtime-x87-pc53-add-{}-{}",
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
        ("reg", &[0xd8, 0xc1][..], 0),
        ("pop", &[0xde, 0xc1][..], 0),
        ("m32", &[0xd8, 0x03][..], 4),
        ("m64", &[0xdc, 0x03][..], 8),
        ("fs32", &[0x64, 0xd8, 0x03][..], 4),
        ("fs64", &[0x64, 0xdc, 0x03][..], 8),
    ];
    for (name, code, width) in forms {
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
        for &(a, b) in &[
            (0x4008000000000000, 0x4000000000000000), // 3+2
            (0x3ff0000000000001, 0x3ff0000000000001), // wide53 exact inputs
            (0x3ff0000000000000, 0x3ca0000000000000),
            (0x3ff0000000000001, 0x3ca0000000000000), // even/odd ties
            (0xbff0000000000000, 0xbca0000000000000),
            (0xbff0000000000001, 0xbca0000000000000),
            (0x3ff0000000000000, 0x39b0000000000000),
            (0x3ff0000000000000, 0xb9b0000000000000), // lost tiny +/-
            (0xbff0000000000000, 0x39b0000000000000),
            (0xbff0000000000000, 0xb9b0000000000000),
            (0x3ff0000000000000, 0xbff0000000000000),
            (0x8000000000000000, 0x8000000000000000),
            (0, 0x8000000000000000),
            (0x0010000000000000, 0x0010000000000000),
            (0x0010000000000000, 0x8010000000000000),
            (0x0010000000000000, 0x0010000000000001),
            (0x8010000000000000, 0x8010000000000001), // subnormal TwoSum residual, rounded down
            (0x0010000000000001, 0x0010000000000002),
            (0x8010000000000001, 0x8010000000000002), // subnormal residual, rounded away
            (0x7fefffffffffffff, 0x0010000000000000),
            (0x7fefffffffffffff, 0x8010000000000000),
        ] {
            let input32 = (f64::from_bits(b) as f32).to_bits();
            if width == 4 && f64::from(f32::from_bits(input32)).to_bits() != b {
                continue;
            }
            for control in [0x023f, 0x027f] {
                for top in [0, 3, 7] {
                    for flags in [0x4000, 0x4200, 0x4020, 0x4220] {
                        let mut before = state(control, top);
                        before.status = (top << 11) | flags;
                        match operation {
                            X87Operation::Binary {
                                destination,
                                source,
                                ..
                            } => {
                                operand64(&mut before, destination, a);
                                operand64(&mut before, source, b);
                            }
                            _ => operand64(&mut before, 0, a),
                        }
                        let mut numeric = NumericState::default();
                        numeric.x87 = before;
                        let mut segments = SegmentContext::default();
                        segments.state.selectors = [0x23, 0x1b, 0x23, 0x23, 0x3b, 0x43];
                        let fs = name.starts_with("fs");
                        if fs {
                            segments.state.bases[4] = 0x1000;
                        }
                        let mut scalar = State32 {
                            registers: [
                                0x11223344,
                                0x55667788,
                                0x99aabbcc,
                                if fs { 0x2000 } else { 0x3000 },
                                0x4800,
                                0x12345678,
                                0x87654321,
                                0xaabbccdd,
                            ],
                            eip: 0x1000,
                            eflags: 0xcd7,
                        };
                        if width == 8 {
                            memory
                                .write(GuestAddress(0x3000), &b.to_le_bytes())
                                .unwrap();
                        } else {
                            memory
                                .write(GuestAddress(0x3000), &input32.to_le_bytes())
                                .unwrap();
                        }
                        numeric
                            .execute(
                                &mut memory,
                                &mut scalar,
                                &segments,
                                decoded.operation(),
                                decoded.next_pc().0,
                            )
                            .unwrap();
                        if name == "reg" && top == 0 && flags == 0x4000 {
                            if a == 0x3ff0000000000000 && b == 0x3ca0000000000000 {
                                assert_eq!(
                                    numeric.x87.registers[0],
                                    [0, 0, 0, 0, 0, 0, 0, 0x80, 0xff, 0x3f]
                                );
                                assert_eq!(numeric.x87.status, 0x4020);
                            }
                            if a == 0x3ff0000000000001 && b == 0x3ca0000000000000 {
                                assert_eq!(
                                    numeric.x87.registers[0],
                                    [0, 0x10, 0, 0, 0, 0, 0, 0x80, 0xff, 0x3f]
                                );
                                assert_eq!(numeric.x87.status, 0x4220);
                            }
                            if a == 0x0010000000000000 && b == 0x0010000000000001 {
                                assert_eq!(
                                    numeric.x87.registers[0],
                                    [0, 0, 0, 0, 0, 0, 0, 0x80, 2, 0x3c]
                                );
                                assert_eq!(numeric.x87.status, 0x4020);
                            }
                            if a == 0x0010000000000001 && b == 0x0010000000000002 {
                                assert_eq!(
                                    numeric.x87.registers[0],
                                    [0, 0x10, 0, 0, 0, 0, 0, 0x80, 2, 0x3c]
                                );
                                assert_eq!(numeric.x87.status, 0x4220);
                            }
                        }
                        cases.push(format!("{{\"name\":\"{name}\",\"before\":{:?},\"after\":{:?},\"words\":[{},{}],\"width\":{width},\"fs\":{fs},\"readonly\":{},\"status\":0,\"next\":{},\"quota\":0,\"retired\":1}}",raw(before),raw(numeric.x87),if width==8 {b as u32}else{input32},(b>>32) as u32,flags==0x4020,decoded.next_pc().0));
                    }
                }
            }
        }
    }
    for (name, a, b, control, variant) in [
        ("reg", 0x7fefffffffffffff, 0x7fefffffffffffff, 0x023f, 0), // raw80 result exceeds f64 normal range
        ("reg", 0x0010000000000000, 0x8010000000000001, 0x027f, 0), // cancellation into subnormal
        ("reg", 0x0020000000000000, 0x8010000000000001, 0x023f, 0), // largest f64 subnormal remains outside
        ("reg", 1, 0x3ff0000000000000, 0x023f, 0),
        ("reg", 0x7ff0000000000000, 0x3ff0000000000000, 0x023f, 0),
        ("reg", 0x7ff8000000000001, 0x3ff0000000000000, 0x023f, 0),
        ("reg", 0x3ff0000000000000, 0x4000000000000000, 0x037f, 0),
        ("reg", 0x3ff0000000000000, 0x4000000000000000, 0x063f, 0),
        ("reg", 0x3ff0000000000000, 0x4000000000000000, 0x023f, 1),
        ("reg", 0x3ff0000000000000, 0x4000000000000000, 0x023f, 2),
        ("m64", 0x3ff0000000000000, 0x4000000000000000, 0x023e, 3),
        ("m64", 0x3ff0000000000000, 0x4000000000000000, 0x023f, 4),
        ("m64", 0x3ff0000000000000, 0x4000000000000000, 0x023f, 5),
    ] {
        let mut before = state(control, 0);
        operand64(&mut before, 0, a);
        operand64(&mut before, 1, b);
        if variant == 1 {
            before.registers[0][0] |= 1;
        }
        if variant == 2 {
            before.tag |= 3 << 2;
        }
        if variant == 3 {
            before.status |= 1;
        }
        cases.push(format!("{{\"name\":\"{name}\",\"before\":{:?},\"after\":{:?},\"words\":[{},{}],\"width\":{},\"fs\":false,\"status\":7,\"next\":4096,\"quota\":1,\"retired\":0,\"cross\":{},\"unreadable\":{},\"noRead\":{}}}",raw(before),raw(before),b as u32,(b>>32)as u32,if name=="m64"{8}else{0},variant==4,variant==5,variant==3));
    }
    fs::write(scratch.join("cases.json"), format!("[{}]", cases.join(","))).unwrap();
    let result=Command::new("node").arg("--input-type=module").arg("-e").arg(r#"
import assert from 'node:assert/strict';import fs from 'node:fs';
const root=process.argv[1],memory=new WebAssembly.Memory({initial:4}),data=new DataView(memory.buffer),ram=new Uint8Array(memory.buffer),ptr=0x8000,bridge=0xa000,fp=0xb000,map=0x1000,permissions=0x2000,backing=0x10000;
const word=(at,value)=>data.setUint32(ptr+at,value>>>0,true),get=at=>data.getUint32(ptr+at,true);let current,reads=0;
const imports={env:{memory},cpu:{read(p,address,width){reads++;assert(!current.noRead);assert.equal(width,4);assert.equal(address,0x3000);word(52,0);return current.words[0];},write(){throw new Error('ADD cannot write guest RAM');}}};const functions=new Map();
for(const item of JSON.parse(fs.readFileSync(`${root}/cases.json`))){current=item;reads=0;ram.fill(0);word(0,1);word(4,160);[0x11223344,0x55667788,0x99aabbcc,item.cross?0x3ff9:item.fs?0x2000:0x3000,0x4800,0x12345678,0x87654321,0xaabbccdd].forEach((v,i)=>word(8+i*4,v));word(40,0x1000);word(44,0xcd7);word(48,1);word(124,bridge);word(152,63);if(item.fs)word(116,0x1000);data.setUint32(bridge,1,true);data.setUint32(bridge+4,40,true);data.setUint32(bridge+8,fp,true);[0x23,0x1b,0x23,0x23,0x3b,0x43].forEach((v,i)=>data.setUint32(bridge+12+i*4,v,true));ram.set(item.before,fp);if(item.width===8){word(76,map);word(80,permissions);word(84,backing);word(88,4);data.setUint32(map+12,1,true);data.setUint8(permissions+3,item.unreadable?0:item.readonly?1:3);data.setUint32(backing,item.words[0],true);data.setUint32(backing+4,item.words[1],true);}const regs=Array.from({length:8},(_,i)=>get(8+i*4));if(!functions.has(item.name))functions.set(item.name,(await WebAssembly.instantiate(fs.readFileSync(`${root}/${item.name}.wasm`),imports)).instance.exports.run);assert.equal(functions.get(item.name)(ptr),item.status,`${item.name} CW${new DataView(Uint8Array.from(item.before).buffer).getUint16(0,true).toString(16)}`);assert.deepEqual([...ram.slice(fp,fp+100)],item.after,`${item.name} all raw80/ghost/tags/TOP/PE/C1/CW/metadata`);assert.deepEqual(Array.from({length:8},(_,i)=>get(8+i*4)),regs);assert.deepEqual([get(40),get(44),get(48),get(56)],[item.next,0xcd7,item.quota,item.retired]);if(item.noRead||item.width===8)assert.equal(reads,0);}
console.log('PC53 ADD wide53 reg/pop/m32/m64/FS, CW023f/027f immediate PE/C1 full-state oracles, signed ties/losttiny/zero and range/control/memory fallback passed');
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

#[test]
fn generated_x87_subtract_and_reverse_subtract_preserve_exact_rounding_state() {
    use crate::cpu::x86::X87Binary;
    use std::{
        fs,
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };
    let scratch = std::env::temp_dir().join(format!(
        "ring3-runtime-x87-subtract-{}-{}",
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
    let forms = [
        ("s-reg", &[0xd8, 0xe1][..], 0, false),
        ("r-reg", &[0xd8, 0xe9][..], 0, true),
        ("s-dest", &[0xdc, 0xe9][..], 0, false),
        ("r-dest", &[0xdc, 0xe1][..], 0, true),
        ("s-pop", &[0xde, 0xe9][..], 0, false),
        ("r-pop", &[0xde, 0xe1][..], 0, true),
        ("s-self", &[0xd8, 0xe0][..], 0, false),
        ("r-self", &[0xd8, 0xe8][..], 0, true),
        ("s-m32", &[0xd8, 0x23][..], 4, false),
        ("r-m32", &[0xd8, 0x2b][..], 4, true),
        ("s-m64", &[0xdc, 0x23][..], 8, false),
        ("r-m64", &[0xdc, 0x2b][..], 8, true),
        ("fs-s32", &[0x64, 0xd8, 0x23][..], 4, false),
        ("fs-r32", &[0x64, 0xd8, 0x2b][..], 4, true),
        ("fs-s64", &[0x64, 0xdc, 0x23][..], 8, false),
        ("fs-r64", &[0x64, 0xdc, 0x2b][..], 8, true),
    ];
    let pc24: Vec<_> = [
        (0x40400000, 0x40000000),
        (0xc0400000, 0x40000000),
        (0x3f800000, 0x21800000),
        (0x3f800000, 0xa1800000),
        (0x3f800000, 0x33000000),
        (0x3f7fffff, 0x33000000),
        (0x3f800001, 0x33800000),
        (0xbf800001, 0xb3800000),
        (0, 0),
        (0, 0x80000000),
        (0x80000000, 0),
        (0x80000000, 0x80000000),
        (0x00800000, 0x80800001),
        (0x00800001, 0x80800002),
        (0x00800000, 0x00800001),
        (0x7f7fffff, 0xff7fffff),
    ]
    .into_iter()
    .map(|(a, b)| {
        (
            f64::from(f32::from_bits(a)).to_bits(),
            f64::from(f32::from_bits(b)).to_bits(),
        )
    })
    .collect();
    let pc53 = [
        (0x4008000000000000, 0x4000000000000000),
        (0xc008000000000000, 0x4000000000000000),
        (0x3ff0000000000000, 0x39b0000000000000),
        (0x3ff0000000000000, 0xb9b0000000000000),
        (0x3ff0000000000000, 0x3c90000000000000),
        (0x3fefffffffffffff, 0x3c90000000000000),
        (0x3ff0000000000001, 0x3ca0000000000000),
        (0xbff0000000000001, 0xbca0000000000000),
        (0x3ff123456789abcd, 0x3ff123456789abcc),
        (0, 0),
        (0, 0x8000000000000000),
        (0x8000000000000000, 0),
        (0x8000000000000000, 0x8000000000000000),
        (0x0010000000000000, 0x8010000000000001),
        (0x0010000000000001, 0x8010000000000002),
        (0x0010000000000000, 0x0010000000000001),
        (0x7fefffffffffffff, 0xffefffffffffffff),
    ];
    let mut cases = Vec::new();
    for (name, code, width, reverse) in forms {
        memory.write(GuestAddress(0x1000), code).unwrap();
        fs::write(
            scratch.join(format!("{name}.wasm")),
            compile(&memory, GuestAddress(0x1000), 1).unwrap().wasm,
        )
        .unwrap();
        let decoded = decode_one(&memory, GuestAddress(0x1000)).unwrap();
        let Operation::X87 { operation, .. } = *decoded.operation() else {
            panic!("expected x87 subtraction");
        };
        let kind = match operation {
            X87Operation::Binary { kind, .. } | X87Operation::BinaryMemory { kind, .. } => kind,
            _ => panic!("expected binary x87"),
        };
        assert_eq!(
            kind,
            if reverse {
                X87Binary::ReverseSubtract
            } else {
                X87Binary::Subtract
            }
        );
        for (precision, pairs) in [(24, pc24.as_slice()), (53, pc53.as_slice())] {
            if precision == 24 && width == 8 {
                continue;
            }
            for &(a, mut b) in pairs {
                if name.ends_with("self") {
                    b = a;
                }
                let bits32 = (f64::from_bits(b) as f32).to_bits();
                if width == 4 && f64::from(f32::from_bits(bits32)).to_bits() != b {
                    continue;
                }
                let words = if width == 8 {
                    [b as u32, (b >> 32) as u32]
                } else {
                    [bits32, 0]
                };
                for control in if precision == 24 {
                    [0x003f, 0x007f]
                } else {
                    [0x023f, 0x027f]
                } {
                    for top in 0..8 {
                        for status in [0x450f, 0x472f] {
                            let mut before = state(control, top);
                            before.status = (top << 11) | status;
                            let destination = match operation {
                                X87Operation::Binary {
                                    destination,
                                    source,
                                    ..
                                } => {
                                    operand64(&mut before, destination, a);
                                    operand64(&mut before, source, b);
                                    destination
                                }
                                _ => {
                                    operand64(&mut before, 0, a);
                                    0
                                }
                            };
                            let fs = name.starts_with("fs");
                            let mut segments = SegmentContext::default();
                            segments.state.selectors = [0x23, 0x1b, 0x23, 0x23, 0x3b, 0x43];
                            if fs {
                                segments.state.bases[4] = 0x1000;
                            }
                            let mut scalar = State32 {
                                registers: [
                                    0x11223344,
                                    0x55667788,
                                    0x99aabbcc,
                                    if fs { 0x2001 } else { 0x3000 },
                                    0x4800,
                                    0x12345678,
                                    0x87654321,
                                    0xaabbccdd,
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
                                    .write(
                                        GuestAddress(if fs { 0x3001 } else { 0x3000 }),
                                        &input[..width as usize],
                                    )
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
                            let result = numeric.x87.registers
                                [(usize::from(top) + usize::from(destination)) & 7];
                            let significand = u64::from_le_bytes(result[..8].try_into().unwrap());
                            let exponent = u16::from_le_bytes([result[8], result[9]]) & 0x7fff;
                            let range = if precision == 24 {
                                16257..=16510
                            } else {
                                15361..=17406
                            };
                            let admitted =
                                significand == 0 && exponent == 0 || range.contains(&exponent);
                            if (name == "s-reg" || name == "r-reg") && top == 0 && status == 0x450f
                            {
                                if a == 0x4008000000000000 && b == 0x4000000000000000 {
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
                                            if reverse { 0xbf } else { 0x3f }
                                        ]
                                    );
                                    assert_eq!(numeric.x87.status, 0x450f);
                                }
                                if a == 0x3ff0000000000000
                                    && b == (if precision == 24 {
                                        0x3c30000000000000
                                    } else {
                                        0x39b0000000000000
                                    })
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
                                            if reverse { 0xbf } else { 0x3f }
                                        ]
                                    );
                                    assert_eq!(numeric.x87.status, 0x472f);
                                }
                                if a << 1 == 0 && b << 1 == 0 {
                                    let negative = if reverse {
                                        b >> 63 != 0 && a >> 63 == 0
                                    } else {
                                        a >> 63 != 0 && b >> 63 == 0
                                    };
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
                                            0,
                                            0,
                                            if negative { 0x80 } else { 0 }
                                        ]
                                    );
                                    assert_eq!(numeric.x87.status, 0x450f);
                                }
                            }
                            cases.push(format!("{{\"name\":\"{name}\",\"before\":{:?},\"after\":{:?},\"words\":{:?},\"width\":{width},\"fs\":{fs},\"status\":{},\"next\":{},\"quota\":{},\"retired\":{}}}", raw(before), raw(if admitted {numeric.x87} else {before}), words, if admitted {0} else {7}, if admitted {decoded.next_pc().0} else {0x1000}, u32::from(!admitted), u32::from(admitted)));
                        }
                    }
                }
            }
        }
    }
    for (name, a, b, control, variant) in [
        ("s-reg", 0x3ff0000000000000, 0x4000000000000000, 0x037f, 0),
        ("r-reg", 0x3ff0000000000000, 0x4000000000000000, 0x063f, 0),
        ("s-reg", 0x3ff0000000000001, 0x4000000000000000, 0x003f, 0),
        ("r-reg", 0x3ff0000000000000, 0x4000000000000000, 0x023f, 1),
        ("s-reg", 1, 0x3ff0000000000000, 0x023f, 0),
        ("r-reg", 0x7ff0000000000000, 0x3ff0000000000000, 0x023f, 0),
        ("s-reg", 0x7ff8000000000001, 0x3ff0000000000000, 0x023f, 0),
        ("s-reg", 0x3ff0000000000000, 0x4000000000000000, 0x023f, 2),
        ("r-reg", 0x3ff0000000000000, 0x4000000000000000, 0x023f, 3),
        ("s-m64", 0x3ff0000000000000, 0x4000000000000000, 0x003f, 0),
        ("fs-r64", 0x3ff0000000000000, 0x4000000000000000, 0x023f, 4),
        ("fs-r64", 0x3ff0000000000000, 0x4000000000000000, 0x023f, 5),
        ("s-m32", 0x3ff0000000000000, 0x4000000000000000, 0x023e, 6),
        ("r-m32", 0x3ff0000000000000, 0x4000000000000000, 0x003f, 7),
    ] {
        let mut before = state(control, 0);
        before.status = 0x4220;
        operand64(&mut before, 0, a);
        operand64(&mut before, 1, b);
        if variant == 1 {
            before.registers[0][0] |= 1;
        }
        if variant == 2 {
            before.tag |= 3 << 2;
        }
        if variant == 3 {
            before.tag |= 3;
        }
        if variant == 6 {
            before.status |= 1;
        }
        let width = if name.ends_with("64") {
            8
        } else if name.ends_with("32") {
            4
        } else {
            0
        };
        let words = if width == 8 {
            [b as u32, (b >> 32) as u32]
        } else {
            [(f64::from_bits(b) as f32).to_bits(), 0]
        };
        cases.push(format!("{{\"name\":\"{name}\",\"before\":{:?},\"after\":{:?},\"words\":{:?},\"width\":{width},\"fs\":{},\"status\":{},\"next\":4096,\"quota\":1,\"retired\":0,\"cross\":{},\"unreadable\":{},\"noRead\":{},\"fault\":{}}}", raw(before), raw(before), words, name.starts_with("fs"), if variant == 7 {3} else {7}, variant == 4, variant == 5, variant == 6, variant == 7));
    }
    fs::write(scratch.join("cases.json"), format!("[{}]", cases.join(","))).unwrap();
    let result = Command::new("node").arg("--input-type=module").arg("-e").arg(r#"
import assert from 'node:assert/strict';import fs from 'node:fs';
const root=process.argv[1],memory=new WebAssembly.Memory({initial:4}),data=new DataView(memory.buffer),ram=new Uint8Array(memory.buffer),ptr=0x8000,bridge=0xa000,fp=0xb000,map=0x1000,permissions=0x2000,backing=0x10000;
const word=(at,value)=>data.setUint32(ptr+at,value>>>0,true),get=at=>data.getUint32(ptr+at,true);let current,reads=0;
const imports={env:{memory},cpu:{read(p,address,width){reads++;assert(!current.noRead);assert.equal(width,4);assert.equal(address,current.fs?0x3001:0x3000);word(52,current.fault?3:0);if(current.fault){word(60,address);word(68,4);return 0;}return current.words[0];},write(){throw Error('subtraction cannot write guest RAM');}}};const functions=new Map();
for(const item of JSON.parse(fs.readFileSync(`${root}/cases.json`))){
  current=item;reads=0;ram.fill(0);word(0,1);word(4,160);[0x11223344,0x55667788,0x99aabbcc,item.cross?0x3ff9:item.fs?0x2001:0x3000,0x4800,0x12345678,0x87654321,0xaabbccdd].forEach((v,i)=>word(8+i*4,v));
  word(40,0x1000);word(44,0xcd7);word(48,1);word(124,bridge);word(152,63);if(item.fs)word(116,0x1000);data.setUint32(bridge,1,true);data.setUint32(bridge+4,40,true);data.setUint32(bridge+8,fp,true);[0x23,0x1b,0x23,0x23,0x3b,0x43].forEach((v,i)=>data.setUint32(bridge+12+i*4,v,true));ram.set(item.before,fp);
  if(item.width===8){word(76,map);word(80,permissions);word(84,backing);word(88,4);data.setUint32(map+12,1,true);data.setUint8(permissions+3,item.unreadable?0:1);data.setUint32(backing+(item.fs?1:0),item.words[0],true);data.setUint32(backing+(item.fs?5:4),item.words[1],true);}
  const regs=Array.from({length:8},(_,i)=>get(8+i*4)),beforeRAM=ram.slice(backing,backing+16384);
  if(!functions.has(item.name))functions.set(item.name,(await WebAssembly.instantiate(fs.readFileSync(`${root}/${item.name}.wasm`),imports)).instance.exports.run);
  assert.equal(functions.get(item.name)(ptr),item.status,`${item.name} subtraction precision/domain`);assert.deepEqual([...ram.slice(fp,fp+100)],item.after,`${item.name} full raw80/ghost/TOP/tag/PE/C1/CW/metadata immediately after math`);
  assert.deepEqual(Array.from({length:8},(_,i)=>get(8+i*4)),regs);assert.deepEqual([get(40),get(44),get(48),get(56)],[item.next,0xcd7,item.quota,item.retired]);assert.deepEqual(ram.slice(backing,backing+16384),beforeRAM);
  if(item.width===8||item.noRead)assert.equal(reads,0);if(item.fault){assert.equal(reads,1);assert.equal(get(60),0x3000);assert.equal(get(68),4);}
}
console.log('PC24/PC53 SUB/RSUB strict full-state oracle, signed zero/cancellation/ties/losttiny/subnormal residual, register orientations/pop/FS and neutral domain/fault boundaries passed');
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

#[test]
fn generated_x87_f32_stores_round_f64_normals_without_changing_raw_operands() {
    use std::{
        fs,
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };
    let scratch = std::env::temp_dir().join(format!(
        "ring3-runtime-f32-stores-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&scratch).unwrap();
    let mut memory = AddressSpace::new(3).unwrap();
    for address in [0x1000, 0x3000, 0x4000] {
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(address), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
    }
    let forms: &[(&str, &[u8], bool)] = &[
        ("fst", &[0xd9, 0x13], false),
        ("fstp", &[0xd9, 0x1b], false),
        ("fs-fstp", &[0x64, 0xd9, 0x1b], true),
    ];
    for &(name, code, _) in forms {
        memory.write(GuestAddress(0x1000), code).unwrap();
        fs::write(
            scratch.join(format!("{name}.wasm")),
            compile(&memory, GuestAddress(0x1000), 1).unwrap().wasm,
        )
        .unwrap();
    }
    let mut cases = Vec::new();
    let mut add = |label: &str,
                   bits: u64,
                   control: u16,
                   old_status: u16,
                   top: u16,
                   form: &str,
                   mode: u8,
                   wide: bool,
                   eligible: bool,
                   literal: Option<(u32, bool, bool)>| {
        let &(_, code, fs_prefix) = forms.iter().find(|(name, _, _)| *name == form).unwrap();
        let linear = if fs_prefix { 0x4000 } else { 0x3000 };
        let range = PageRange::new(GuestAddress(linear), 1).unwrap();
        memory.protect(range, Permissions::READ_WRITE).unwrap();
        memory
            .write(GuestAddress(linear), &0xdead_beef_u32.to_le_bytes())
            .unwrap();
        memory
            .protect(
                range,
                if mode == 1 || mode == 3 {
                    Permissions::READ
                } else if mode == 2 {
                    Permissions::ALL
                } else {
                    Permissions::READ_WRITE
                },
            )
            .unwrap();
        memory.write(GuestAddress(0x1000), code).unwrap();
        let decoded = decode_one(&memory, GuestAddress(0x1000)).unwrap();
        let mut before = state(control, top);
        before.status = (top << 11) | old_status;
        operand64(&mut before, 0, bits);
        let physical = usize::from(top);
        if wide {
            before.registers[physical][0] |= 1;
        }
        let mut numeric = NumericState::default();
        numeric.x87 = before;
        let mut segments = SegmentContext::default();
        segments.state.selectors = [0x23, 0x1b, 0x23, 0x23, 0x3b, 0x43];
        if fs_prefix {
            segments.state.bases[4] = 0x1000;
        }
        let mut scalar = State32 {
            registers: [0x11223344, 2, 3, 0x3000, 0x4800, 6, 7, 8],
            eip: 0x1000,
            eflags: 0xcd7,
        };
        let result = numeric.execute(
            &mut memory,
            &mut scalar,
            &segments,
            decoded.operation(),
            decoded.next_pc().0,
        );
        let mut output = [0; 4];
        memory.read(GuestAddress(linear), &mut output).unwrap();
        let output = u32::from_le_bytes(output);
        if mode == 3 {
            assert!(
                matches!(
                    result,
                    Err(crate::cpu::runtime::numeric::NumericError::Cpu { vector: 16, .. })
                ),
                "{label}: pending MF precedes readonly destination"
            );
            assert_eq!(numeric.x87.status & 0x8080, 0x8080);
            assert_eq!(output, 0xdead_beef);
        } else if mode == 1 {
            assert!(matches!(
                result,
                Err(crate::cpu::runtime::numeric::NumericError::Memory { width: 4, .. })
            ));
            assert_eq!(
                numeric.x87, before,
                "faulting store retains the entire original bank"
            );
            assert_eq!(output, 0xdead_beef);
        } else {
            let outcome = result.unwrap();
            assert_eq!(outcome.code_invalidated, mode == 2);
            if let Some((expected, inexact, incremented)) = literal {
                assert_eq!(output, expected, "{label}: literal binary32 output");
                assert_eq!(
                    numeric.x87.status & 0x20 != 0,
                    inexact || old_status & 0x20 != 0,
                    "{label}: sticky PE"
                );
                assert_eq!(
                    numeric.x87.status & 0x200 != 0,
                    incremented,
                    "{label}: magnitude C1"
                );
            }
            assert_eq!(
                numeric.x87.registers, before.registers,
                "FST/FSTP never replace their raw source payload"
            );
            assert_eq!(numeric.x87.control, control);
            assert_eq!(numeric.x87.instruction_pointer, 0x1000);
            assert_eq!(numeric.x87.data_pointer, 0x3000);
            if label == "unmasked-PM" {
                assert_eq!(output, 0x3f80_0002);
                assert_eq!(
                    numeric.x87.status & 0x82a0,
                    0x82a0,
                    "unmasked PE commits the store and summary before a later waiting fault"
                );
                assert_eq!(numeric.x87.top(), 1);
            }
        }
        assert_eq!(scalar.eflags, 0xcd7);
        let status = if mode == 3 || !eligible {
            7
        } else if mode == 1 {
            3
        } else if mode == 2 {
            6
        } else {
            0
        };
        let completed = status == 0 || status == 6;
        let after = if completed { numeric.x87 } else { before };
        let case_label =
            format!("{label}-cw{control:04x}-sw{old_status:04x}-top{top}-mode{mode}-wide{wide}");
        cases.push(format!("{{\"label\":\"{case_label}\",\"form\":\"{form}\",\"before\":{:?},\"after\":{:?},\"status\":{status},\"next\":{},\"remaining\":{},\"retired\":{},\"linear\":{linear},\"fs\":{fs_prefix},\"mode\":{mode},\"output\":{},\"writes\":{}}}",
            raw(before), raw(after), if completed { decoded.next_pc().0 } else { 0x1000 }, u32::from(!completed), u32::from(completed), if completed { output } else { 0xdead_beef }, u32::from(status != 7)));
    };
    // nearest-only expansion: each sign, every TOP, FST/FSTP/FS, fresh and sticky PE.
    let inexact = [
        ("tie-even", 0x3ff0_0000_1000_0000_u64, 0x3f80_0000, false),
        ("tie-odd", 0x3ff0_0000_3000_0000, 0x3f80_0002, true),
        ("carry", 0x3fff_ffff_f000_0000, 0x4000_0000, true),
        ("minnormal-half", 0x3810_0000_1000_0000, 0x0080_0000, false),
        ("next-above-one", 0x3ff0_0000_0000_0001, 0x3f80_0000, false),
        ("next-below-one", 0x3fef_ffff_ffff_ffff, 0x3f80_0000, true),
    ];
    for &(label, input, output, incremented) in &inexact {
        for negative in [false, true] {
            let bits = input | if negative { 1_u64 << 63 } else { 0 };
            let output = output | if negative { 1_u32 << 31 } else { 0 };
            for top in 0..8 {
                for &(form, _, _) in forms {
                    for old_status in [0x4700, 0x472f] {
                        add(
                            label,
                            bits,
                            0x023f,
                            old_status,
                            top,
                            form,
                            0,
                            false,
                            true,
                            Some((output, true, incremented)),
                        );
                    }
                }
            }
            for control in [0x003f, 0x007f, 0x027f, 0x033f, 0x037f, 0x013f, 0x133f] {
                add(
                    label,
                    bits,
                    control,
                    0x4700,
                    0,
                    "fstp",
                    0,
                    false,
                    true,
                    Some((output, true, incremented)),
                );
            }
        }
    }
    // exact transport keeps its existing CW independence, including unmasked PM.
    for (label, bits, output) in [
        ("zero", 0_u64, 0_u32),
        ("one", 0x3ff0_0000_0000_0000, 0x3f80_0000),
        ("minnormal", 0x3810_0000_0000_0000, 0x0080_0000),
        ("maxfinite", 0x47ef_ffff_e000_0000, 0x7f7f_ffff),
    ] {
        for negative in [false, true] {
            for control in [
                0x0000, 0x003e, 0x001f, 0x043f, 0x083f, 0x0c3f, 0xffff, 0x023f,
            ] {
                add(
                    label,
                    bits | if negative { 1_u64 << 63 } else { 0 },
                    control,
                    0x4700,
                    7,
                    "fstp",
                    0,
                    false,
                    true,
                    Some((
                        output | if negative { 1_u32 << 31 } else { 0 },
                        false,
                        false,
                    )),
                );
            }
        }
    }
    // outside the bounded domain: canonical conversion stays authoritative.
    for (label, bits, control, wide) in [
        ("f32-subnormal", 0x36a0_0000_0000_0000, 0x023f, false),
        ("underflow-to-zero", 0x3690_0000_0000_0000, 0x023f, false),
        ("below-minnormal", 0x380f_ffff_ffff_ffff, 0x023f, false),
        ("above-maxfinite", 0x47ef_ffff_e000_0001, 0x023f, false),
        ("overflow", 0x47f0_0000_0000_0000, 0x023f, false),
        ("infinity", 0x7ff0_0000_0000_0000, 0x023f, false),
        ("quiet-nan", 0x7ff8_0000_0000_1234, 0x023f, false),
        ("signaling-nan", 0x7ff0_0000_0000_1234, 0x023f, false),
        ("extended-precision", 0x3ff0_0000_0000_0000, 0x023f, true),
        ("round-down", 0x3ff0_0000_1000_0000, 0x043f, false),
        ("round-up", 0x3ff0_0000_1000_0000, 0x083f, false),
        ("round-zero", 0x3ff0_0000_1000_0000, 0x0c3f, false),
        ("unmasked-IV", 0x3ff0_0000_1000_0000, 0x003e, false),
        ("unmasked-PM", 0x3ff0_0000_3000_0000, 0x001f, false),
    ] {
        add(
            label, bits, control, 0x4700, 0, "fstp", 0, wide, false, None,
        );
    }
    for &(form, _, _) in forms {
        for negative in [false, true] {
            for &(label, input, _, _) in &inexact[..2] {
                for mode in [1, 2] {
                    add(
                        label,
                        input | if negative { 1_u64 << 63 } else { 0 },
                        0x023f,
                        0x4700,
                        7,
                        form,
                        mode,
                        false,
                        true,
                        None,
                    );
                }
            }
        }
        add(
            "pending-before-store-fault",
            0x3ff0_0000_3000_0000,
            0x003e,
            1,
            7,
            form,
            3,
            false,
            false,
            None,
        );
    }
    drop(add);
    fs::write(scratch.join("cases.json"), format!("[{}]", cases.join(","))).unwrap();
    let result = Command::new("node").arg("--input-type=module").arg("-e").arg(r#"
import assert from 'node:assert/strict';import fs from 'node:fs';
const root=process.argv[1],memory=new WebAssembly.Memory({initial:1}),data=new DataView(memory.buffer),ram=new Uint8Array(memory.buffer),state=0x8000,view=0x9800,fp=0xa000;
const word=(at,value)=>data.setUint32(state+at,value>>>0,true);let current,writes=0;
const imports={env:{memory},cpu:{read(){throw Error('FST/FSTP must not read guest RAM');},write(context,address,width,value){assert.equal(context,state);assert.equal(address,current.linear);assert.equal(width,4);writes++;assert.notEqual(current.mode,3,'pending MF precedes any destination access');if(current.mode===1){word(52,3);word(60,address);word(64,0x20204);word(68,4);return 3;}data.setUint32(address,value>>>0,true);const status=current.mode===2?6:0;word(52,status);return status;}}};
const functions=new Map();
for(current of JSON.parse(fs.readFileSync(`${root}/cases.json`))){
  for(let i=0;i<ram.length;i++)ram[i]=(i*37+11)&255;ram.fill(0,state,state+160);ram.fill(0,view,view+36);
  word(0,1);word(4,160);[0x11223344,2,3,0x3000,0x4800,6,7,8].forEach((v,i)=>word(8+4*i,v));word(40,0x1000);word(44,0xcd7);word(48,1);word(124,view);word(152,0x1f);if(current.fs)word(116,0x1000);
  data.setUint32(view,1,true);data.setUint32(view+4,36,true);data.setUint32(view+8,fp,true);[0x23,0x1b,0x23,0x23,0x3b,0x43].forEach((v,i)=>data.setUint32(view+12+4*i,v,true));ram.set(current.before,fp);data.setUint32(current.linear,0xdeadbeef,true);
  const before=ram.slice(),expected=ram.slice(),d=new DataView(expected.buffer);expected.set(current.after,fp);d.setUint32(current.linear,current.output,true);
  for(const[at,value]of[[40,current.next],[48,current.remaining],[52,current.status],[56,current.retired]])d.setUint32(state+at,value,true);
  if(current.status===3)for(const[at,value]of[[60,current.linear],[64,0x20204],[68,4]])d.setUint32(state+at,value,true);
  if(!functions.has(current.form))functions.set(current.form,(await WebAssembly.instantiate(fs.readFileSync(`${root}/${current.form}.wasm`),imports)).instance.exports.run);
  writes=0;const status=functions.get(current.form)(state);
  if(status===7){const neutral=before.slice();new DataView(neutral.buffer).setUint32(state+52,7,true);assert.deepEqual(ram,neutral,'service fallback is neutral before effects');assert.equal(writes,0);}
  assert.equal(status,current.status,`${current.label}/${current.form}: required compiled admission, not an architectural failure claim about the baseline`);
  assert.deepEqual(ram,expected,`${current.label}/${current.form}: complete bank/raw ghosts/CW/PE/C1/pop/meta, scalar/RAM and exact budget immediately after store`);assert.equal(writes,current.writes);
}
console.log('bounded f64-to-f32 FST/FSTP: strict/literal rounding and C1, all TOP/PC/signs, exact-CW compatibility, fault/SMC/pending priority and full RAM passed');
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
