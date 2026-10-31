use super::*;
use crate::{
    cpu::{
        runtime::numeric::{NumericError, NumericState},
        x86::{SegmentContext, State32, X87Operation, X87State, decode::decode_one},
    },
    memory::{PageRange, Permissions},
};

#[derive(Clone, Copy)]
struct Raw {
    sig: u64,
    exp: u16,
}
fn value(sig: u64, exp: u16) -> Raw {
    Raw { sig, exp }
}
fn negative(v: Raw) -> Raw {
    Raw {
        exp: v.exp ^ 0x8000,
        ..v
    }
}
fn bytes(state: X87State) -> Vec<u8> {
    let mut out = vec![0; 100];
    for (at, word) in [
        (0, state.control),
        (2, state.status),
        (4, state.tag),
        (6, state.opcode),
        (16, state.code_selector),
        (18, state.data_selector),
    ] {
        out[at..at + 2].copy_from_slice(&word.to_le_bytes());
    }
    out[8..12].copy_from_slice(&state.instruction_pointer.to_le_bytes());
    out[12..16].copy_from_slice(&state.data_pointer.to_le_bytes());
    out[20..].copy_from_slice(state.registers.as_flattened());
    out
}
fn initial(cw: u16, top: u16, status: u16) -> X87State {
    X87State {
        control: cw,
        status: status | (top << 11),
        tag: 0xffff,
        opcode: 0x321,
        instruction_pointer: 0x11223344,
        data_pointer: 0x55667788,
        code_selector: 0x73,
        data_selector: 0x7b,
        registers: std::array::from_fn(|i| std::array::from_fn(|j| (i * 31 + j * 13) as u8)),
    }
}
fn put(state: &mut X87State, index: u8, v: Raw) {
    let at = (state.top() + usize::from(index)) & 7;
    state.registers[at][..8].copy_from_slice(&v.sig.to_le_bytes());
    state.registers[at][8..].copy_from_slice(&v.exp.to_le_bytes());
    state.tag = (state.tag & !(3 << (at * 2))) | (((at % 3) as u16) << (at * 2));
}
fn ieee(v: Raw, width: u32) -> [u32; 2] {
    let sign = u64::from(v.exp >> 15);
    let exponent = v.exp & 0x7fff;
    if v.sig == 0 {
        return if width == 4 {
            [(sign as u32) << 31, 0]
        } else {
            [0, (sign as u32) << 31]
        };
    }
    if width == 4 {
        assert!((16257..=16510).contains(&exponent) && v.sig & ((1_u64 << 40) - 1) == 0);
        [
            ((u32::from(exponent) - 16256) << 23)
                | ((v.sig >> 40) as u32 & 0x7fffff)
                | ((sign as u32) << 31),
            0,
        ]
    } else {
        assert!((15361..=17406).contains(&exponent) && v.sig & 0x7ff == 0);
        let bits = ((u64::from(exponent) - 15360) << 52)
            | ((v.sig >> 11) & 0x000fffffffffffff)
            | (sign << 63);
        [bits as u32, (bits >> 32) as u32]
    }
}
fn fixture() -> AddressSpace {
    let mut memory = AddressSpace::new(2).unwrap();
    for address in [0x1000, 0x3000] {
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(address), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
    }
    memory
}

#[test]
fn strict_x87_comparison_orders_memory_faults_before_stack_and_pending_before_operand() {
    for (code, width) in [([0xd8, 0x1b], 4), ([0xdc, 0x1b], 8)] {
        for pending in [false, true] {
            for empty in [false, true] {
                let mut memory = fixture();
                memory.write(GuestAddress(0x1000), &code).unwrap();
                let decoded = decode_one(&memory, GuestAddress(0x1000)).unwrap();
                memory
                    .protect(
                        PageRange::new(GuestAddress(0x3000), 1).unwrap(),
                        Permissions::NONE,
                    )
                    .unwrap();
                let mut scalar = State32 {
                    registers: [1, 2, 3, 0x3000, 5, 6, 7, 8],
                    eip: 0x1000,
                    eflags: 0xcd7,
                };
                let mut numeric = NumericState::default();
                numeric.x87 = initial(
                    if pending { 0x003e } else { 0x003f },
                    7,
                    if pending { 0x4201 } else { 0x4700 },
                );
                if !empty {
                    put(&mut numeric.x87, 0, value(1 << 63, 0x3fff));
                }
                let (before, scalar_before) = (numeric.x87, scalar);
                let result = numeric.execute(
                    &mut memory,
                    &mut scalar,
                    &SegmentContext::default(),
                    decoded.operation(),
                    decoded.next_pc().0,
                );
                let mut want = before;
                if pending {
                    want.status |= 0x8080;
                    assert_eq!(
                        result,
                        Err(NumericError::Cpu {
                            vector: 16,
                            detail: u32::from(want.status)
                        })
                    );
                } else {
                    assert!(matches!(result,Err(NumericError::Memory { width:w,.. }) if w==width));
                }
                assert_eq!(numeric.x87, want);
                assert_eq!(scalar, scalar_before);
            }
        }
    }
}

#[test]
fn generated_x87_comparisons_match_strict_raw_order_stack_metadata_and_neutral_boundaries() {
    use std::{
        fs,
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };
    let scratch = std::env::temp_dir().join(format!(
        "ring3-runtime-x87-compare-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&scratch).unwrap();
    let mut memory = fixture();
    let one = value(1 << 63, 0x3fff);
    let zero = value(0, 0);
    let forms = [
        ("m32", vec![0xd8, 0x13], 4, 0),
        ("m32p", vec![0xd8, 0x1b], 4, 0),
        ("m64", vec![0xdc, 0x13], 8, 0),
        ("m64p", vec![0xdc, 0x1b], 8, 0),
        ("fs32", vec![0x64, 0xd8, 0x13], 4, 4),
        ("gs64", vec![0x65, 0xdc, 0x1b], 8, 5),
        ("ss32", vec![0xd8, 0x55, 0], 4, 2),
        ("a16", vec![0x67, 0xd8, 0x17], 4, 3),
        ("reg", vec![0xd8, 0xd1], 0, 0),
        ("regp", vec![0xd8, 0xd9], 0, 0),
        ("quiet", vec![0xdd, 0xe1], 0, 0),
        ("quietp", vec![0xdd, 0xe9], 0, 0),
        ("double", vec![0xde, 0xd9], 0, 0),
        ("qdouble", vec![0xda, 0xe9], 0, 0),
        ("self", vec![0xd8, 0xd0], 0, 0),
        (
            "prefix",
            vec![0xb8, 0x78, 0x56, 0x34, 0x12, 0xd8, 0x13],
            4,
            0,
        ),
    ];
    let mut cases = Vec::new();
    for (name, code, width, segment) in forms {
        memory.write(GuestAddress(0x1000), &code).unwrap();
        let prefix = u32::from(name == "prefix");
        let pc = 0x1000 + 5 * prefix;
        let unit = compile(&memory, GuestAddress(0x1000), 1 + prefix as usize).unwrap();
        assert_eq!(
            unit.entries,
            if prefix == 1 {
                vec![0x1000, 0x1005]
            } else {
                vec![0x1000]
            }
        );
        fs::write(scratch.join(format!("{name}.wasm")), unit.wasm).unwrap();
        let decoded = decode_one(&memory, GuestAddress(pc)).unwrap();
        let Operation::X87 { operation, .. } = *decoded.operation() else {
            panic!("comparison decode");
        };
        let mut pairs = vec![
            (value(0x8000000000000001, 0x3fff), one),
            (value(u64::MAX, 0x3ffe), one),
            (negative(value(0x8000000000000001, 0x3fff)), negative(one)),
            (negative(value(u64::MAX, 0x3ffe)), negative(one)),
            (zero, zero),
            (negative(zero), zero),
            (zero, negative(zero)),
            (negative(zero), negative(zero)),
            (value(1 << 63, 1), zero),
            (negative(value(1 << 63, 1)), zero),
            (value(u64::MAX, 0x7ffe), one),
            (negative(value(u64::MAX, 0x7ffe)), negative(one)),
        ];
        if width != 0 {
            pairs.push((
                one,
                value(1 << 63, if width == 4 { 0x3f81 } else { 0x3c01 }),
            ));
            pairs.push((
                one,
                value(
                    if width == 4 {
                        0xffffff0000000000
                    } else {
                        0xfffffffffffff800
                    },
                    if width == 4 { 0x407e } else { 0x43fe },
                ),
            ));
        } else {
            pairs.push((value(u64::MAX, 1), value(1 << 63, 1)));
            pairs.push((
                negative(value(u64::MAX, 0x7ffe)),
                negative(value(1 << 63, 0x7ffe)),
            ));
        }
        for (pair, (a, b)) in pairs.into_iter().enumerate() {
            for top in 0..8 {
                let controls = [0x003f, 0x023f, 0x037f, 0x0f7f, 0x077f, 0x0b7f, 0xffff, 0];
                let cw = if top == 0 {
                    0x003f
                } else {
                    controls[(pair + top as usize) % controls.len()]
                };
                let status = if top == 0 {
                    0x4720
                } else if cw & 0x20 != 0 && pair % 3 == 0 {
                    0x4720
                } else {
                    0xc780
                };
                let mut before = initial(cw, top, status);
                put(&mut before, 0, a);
                if let X87Operation::Compare { index, .. } = operation {
                    if index != 0 {
                        put(&mut before, index, b);
                    }
                }
                let words = if width == 0 { [0, 0] } else { ieee(b, width) };
                let offset = if segment == 4 || segment == 5 {
                    0x2001
                } else {
                    0x3000
                };
                let linear = if segment == 4 || segment == 5 {
                    0x3001
                } else {
                    0x3000
                };
                let mut segments = SegmentContext::default();
                segments.state.selectors = [0x23, 0x1b, 0x2b, 0x23, 0x3b, 0x43];
                if segment == 4 || segment == 5 {
                    segments.state.bases[segment as usize] = 0x1000;
                }
                let segments = SegmentContext::legacy(segments.state);
                let mut registers = [
                    0x11223344,
                    0x55667788,
                    0x99aabbcc,
                    if name == "a16" { 0x12343000 } else { offset },
                    0x4800,
                    offset,
                    0x87654321,
                    0xaabbccdd,
                ];
                if prefix == 1 {
                    registers[0] = 0x12345678;
                }
                let mut scalar = State32 {
                    registers,
                    eip: pc,
                    eflags: 0xcd7,
                };
                if width != 0 {
                    let input = [words[0].to_le_bytes(), words[1].to_le_bytes()].concat();
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
                    .unwrap_or_else(|error| {
                        panic!("{name}/pair{pair}/top{top}/cw{cw:04x}: {error:?}")
                    });
                if name == "m32" && top == 0 && pair < 6 {
                    assert_eq!(
                        numeric.x87.status,
                        [0x0020, 0x0120, 0x0120, 0x0020, 0x4020, 0x4020][pair],
                        "literal raw80-bit0/negative/signed-zero anchor"
                    );
                }
                assert_eq!(scalar.registers, registers);
                assert_eq!(scalar.eflags, 0xcd7);
                cases.push(format!(r#"{{"name":"{name}","before":{:?},"after":{:?},"words":{:?},"width":{width},"segment":{segment},"offset":{offset},"prefix":{prefix},"next":{},"required":0}}"#,bytes(before),bytes(numeric.x87),words,decoded.next_pc().0));
            }
        }
    }
    for (name, code) in [("flags", [0xdb, 0xf1]), ("integer", [0xda, 0x13])] {
        memory.write(GuestAddress(0x1000), &code).unwrap();
        fs::write(
            scratch.join(format!("{name}.wasm")),
            compile(&memory, GuestAddress(0x1000), 1).unwrap().wasm,
        )
        .unwrap();
    }
    fs::write(scratch.join("cases.json"), format!("[{}]", cases.join(","))).unwrap();
    let result=Command::new("node").arg("--input-type=module").arg("-e").arg(r#"
import assert from 'node:assert/strict';import fs from 'node:fs';
const root=process.argv[1],memory=new WebAssembly.Memory({initial:1}),ram=new Uint8Array(memory.buffer),d=new DataView(memory.buffer),ptr=0x8000,view=0x9800,fp=0xa000,map=0x1000,perms=0x2000,backing=0x4000;
const word=(at,v)=>d.setUint32(ptr+at,v>>>0,true),get=at=>d.getUint32(ptr+at,true),put=(at,v)=>d.setUint32(at,v>>>0,true),functions=new Map(),missing=[];
let current,reads=0,writes=0;
const imports={env:{memory},cpu:{read(context,address,width){assert.equal(context,ptr);assert.equal(width,4);reads++;address>>>=0;if(current.fault){word(52,3);word(60,address);word(64,0);word(68,14);return 0;}word(52,0);return current.words[0]|0;},write(){writes++;throw Error('comparison must not write guest memory');}}};
for(const name of['m32','m32p','m64','m64p','fs32','gs64','ss32','a16','reg','regp','quiet','quietp','double','qdouble','self','prefix','flags','integer'])functions.set(name,(await WebAssembly.instantiate(fs.readFileSync(root+'/'+name+'.wasm'),imports)).instance.exports.run);
function init(item){current=item;for(let i=0;i<ram.length;i++)ram[i]=(i*13+0x59)&255;
 word(0,1);word(4,160);[0x11223344,0x55667788,0x99aabbcc,item.name==='a16'?0x12343000:item.offset,0x4800,item.offset,0x87654321,0xaabbccdd].forEach((v,i)=>word(8+4*i,v));word(40,0x1000);word(44,0xcd7);word(48,item.quota??(1+item.prefix));word(52,0);word(56,0);word(124,view);word(152,63);word(116,item.segment===4?0x1000:0);word(120,item.segment===5?0x1000:0);word(76,0);word(80,0);word(84,0);
 put(view,1);put(view+4,36);put(view+8,fp);[0x23,0x1b,0x2b,0x23,0x3b,0x43].forEach((v,i)=>put(view+12+4*i,v));ram.set(item.before,fp);
 if(item.width){word(76,map);word(80,perms);word(84,backing);word(88,4);const linear=(item.offset+(item.segment===4||item.segment===5?0x1000:0))>>>0;put(map+(linear>>>12)*4,1);d.setUint8(perms+(linear>>>12),item.fault?0:1);if((linear&4095)<=4096-item.width){put(backing+(linear&4095),item.words[0]);if(item.width===8)put(backing+(linear&4095)+4,item.words[1]);}}
 reads=writes=0;
}
function check(item){init(item);if(item.mutate)item.mutate();const before=ram.slice(),actual=functions.get(item.name)(ptr),want=before.slice(),w=new DataView(want.buffer),prefix=item.prefix||0,completedPrefix=item.quota===0?0:prefix;
 assert.ok(actual===item.required||(!item.budget&&actual===7),item.name+' exit '+actual);
 if(actual===0){want.set(item.after,fp);w.setUint32(ptr+40,item.next,true);w.setUint32(ptr+48,0,true);w.setUint32(ptr+56,1+prefix,true);}
 else{w.setUint32(ptr+40,0x1000+completedPrefix*5,true);w.setUint32(ptr+48,actual===2?item.quota-completedPrefix:1,true);w.setUint32(ptr+56,completedPrefix,true);}
 if(completedPrefix)w.setUint32(ptr+8,0x12345678,true);w.setUint32(ptr+52,actual,true);
 if(actual===3){w.setUint32(ptr+60,item.offset,true);w.setUint32(ptr+64,0,true);w.setUint32(ptr+68,14,true);}
 assert.deepEqual(ram,want,item.name+' full raw80/ghost/TOP/tag/C1/C0C2C3/CW/selectors/FLAGS/scalar/RAM');assert.equal(writes,0);assert.equal(reads,actual===3?1:0,item.name+' read calls');
 if(actual!==item.required)missing.push(item.name+':'+item.required+'→'+actual);
}
const cases=JSON.parse(fs.readFileSync(root+'/cases.json'));for(const item of cases)check(item);
const template=name=>JSON.parse(JSON.stringify(cases.find(x=>x.name===name)));
for(const name of['m32','m32p','m64','m64p','reg','regp','quiet','quietp','double','qdouble','self','prefix'])for(const kind of['empty','denormal','unnormal','infinity','nan','pending','view']){
 const item=template(name);item.required=7;item.after=item.before.slice();const b=new DataView(Uint8Array.from(item.before).buffer),top=(b.getUint16(2,true)>>>11)&7,cell=20+top*10;
 if(kind==='empty')b.setUint16(4,b.getUint16(4,true)|(3<<(top*2)),true);
 if(kind==='denormal'){b.setBigUint64(cell,1n,true);b.setUint16(cell+8,0,true);}
 if(kind==='unnormal'){b.setBigUint64(cell,1n,true);b.setUint16(cell+8,0x3fff,true);}
 if(kind==='infinity'||kind==='nan'){b.setBigUint64(cell,kind==='nan'?0xc000000000000001n:0x8000000000000000n,true);b.setUint16(cell+8,0x7fff,true);}
 if(kind==='pending'){b.setUint16(0,0x003e,true);b.setUint16(2,b.getUint16(2,true)|1,true);item.fault=true;}
 if(kind==='view')item.mutate=()=>word(124,0);
 item.before=Array.from(new Uint8Array(b.buffer));check(item);
}
for(const name of['m32','m32p','m64','m64p','prefix'])for(const kind of['permission','empty-permission','cross']){
 const item=template(name);item.fault=true;item.required=item.width===4?3:7;
 if(kind==='empty-permission'){const b=new DataView(Uint8Array.from(item.before).buffer),top=(b.getUint16(2,true)>>>11)&7;b.setUint16(4,b.getUint16(4,true)|(3<<(top*2)),true);item.before=Array.from(new Uint8Array(b.buffer));}
 if(kind==='cross'){item.offset=0x4000-item.width+2;item.required=item.width===4?3:7;}
 check(item);
}
for(const name of['m32','m32p','m64','m64p'])for(const kind of['subnormal','nan','infinity']){
 const item=template(name);item.required=7;item.words=item.width===4?[kind==='subnormal'?1:kind==='nan'?0x7fc00001:0x7f800000,0]:kind==='subnormal'?[1,0]:[1,kind==='nan'?0x7ff80000:0x7ff00000];if(kind==='infinity'&&item.width===8)item.words[0]=0;check(item);
}
for(const name of['reg','regp','quiet','quietp','double','qdouble']){
 const item=template(name);item.required=7;const b=new DataView(Uint8Array.from(item.before).buffer),top=(b.getUint16(2,true)>>>11)&7,right=(top+1)&7;
 assert.notEqual((b.getUint16(4,true)>>>(top*2))&3,3,'empty-right fixture keeps ST0 occupied');b.setUint16(4,b.getUint16(4,true)|(3<<(right*2)),true);
 item.before=Array.from(new Uint8Array(b.buffer));check(item);
}
for(const name of['m32','m64','reg','prefix'])for(const kind of['view','operand'])for(const quota of(name==='prefix'?[0,1]:[0])){
 const item=template(name);item.required=2;item.budget=true;item.quota=quota;
 if(kind==='view')item.mutate=()=>word(124,0);
 else if(item.width)item.fault=true;
 else{const b=new DataView(Uint8Array.from(item.before).buffer),right=(((b.getUint16(2,true)>>>11)&7)+1)&7;b.setUint16(4,b.getUint16(4,true)|(3<<(right*2)),true);item.before=Array.from(new Uint8Array(b.buffer));}
 check(item);
}
for(const name of['flags','integer']){const item=template(name==='flags'?'reg':'m32');item.name=name;item.required=7;check(item);}
console.log('strict comparison/literal anchors and full-memory architectural success or neutral-service/fault paths passed; cases='+cases.length);
assert.equal(missing.length,0,'mechanism: ordered normal/zero compare must inline and narrow canonical faults must remain direct: '+missing.slice(0,8).join(', '));
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
