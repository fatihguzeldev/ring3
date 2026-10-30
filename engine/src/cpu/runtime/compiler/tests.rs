use super::*;
use crate::memory::{PageRange, Permissions};

#[test]
fn independent_compiler_owns_guest_source_dependencies() {
    let mut memory = AddressSpace::new(2).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x3000), 1).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    memory
        .write(GuestAddress(0x1000), &[0x83, 0xc0, 3, 0x0f, 0x0b])
        .unwrap();
    let unit = compile(&memory, GuestAddress(0x1000), 64).unwrap();
    assert!(unit.is_current(&memory));
    assert_eq!(unit.entries, [0x1000]);
    memory.write(GuestAddress(0x3000), &[1]).unwrap();
    assert!(unit.is_current(&memory));
    memory.write(GuestAddress(0x1002), &[4]).unwrap();
    assert!(!unit.is_current(&memory));
    assert_eq!(&unit.wasm[..4], b"\0asm");
}

#[test]
fn registered_gates_require_unique_metadata_and_exact_executable_stubs() {
    let mut memory = AddressSpace::new(1).unwrap();
    let page = PageRange::new(GuestAddress(0x1000), 1).unwrap();
    memory.map_zeroed(page, Permissions::ALL).unwrap();
    memory.write(GuestAddress(0x1000), &[0x0f, 0x0b]).unwrap();
    let gates = [(GuestAddress(0x1000), 17)];
    let unit = compile_with_gates(&memory, GuestAddress(0x1000), 16, &gates).unwrap();
    assert!(unit.is_current(&memory));
    assert!(matches!(
        compile_with_gates(
            &memory,
            GuestAddress(0x1000),
            16,
            &[(GuestAddress(0x1000), 0)]
        ),
        Err(CompileError::InvalidLimits)
    ));
    assert!(matches!(
        compile_with_gates(&memory, GuestAddress(0x1000), 16, &[gates[0], gates[0]]),
        Err(CompileError::InvalidLimits)
    ));
    memory.write(GuestAddress(0x1001), &[0x90]).unwrap();
    assert!(!unit.is_current(&memory));
    assert!(matches!(
        compile_with_gates(&memory, GuestAddress(0x1000), 16, &gates),
        Err(CompileError::InvalidLimits)
    ));
    memory.write(GuestAddress(0x1001), &[0x0b]).unwrap();
    memory.protect(page, Permissions::READ).unwrap();
    assert!(matches!(
        compile_with_gates(&memory, GuestAddress(0x1000), 16, &gates),
        Err(CompileError::Memory(_))
    ));
}

#[test]
fn independent_generated_wasm_executes_integer_calls_and_precise_faults() {
    use std::{
        fs,
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };
    let scratch = std::env::temp_dir().join(format!(
        "ring3-runtime-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&scratch).unwrap();
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    let mut code = [0x90; 128];
    code[..10].copy_from_slice(&[0xe8, 0x3b, 0, 0, 0, 0x49, 0x75, 0xf8, 0x0f, 0x0b]);
    code[64..74].copy_from_slice(&[0x83, 0xc0, 3, 0x31, 0xc2, 0x83, 0x43, 12, 1, 0xc3]);
    memory.write(GuestAddress(0x1000), &code).unwrap();
    let unit = compile(&memory, GuestAddress(0x1000), 256).unwrap();
    fs::write(scratch.join("call.wasm"), &unit.wasm).unwrap();
    fs::write(
        scratch.join("dispatch.wasm"),
        crate::cpu::runtime::emit_dispatch(),
    )
    .unwrap();
    memory.write(GuestAddress(0x1040), &[0x0f, 0x0b]).unwrap();
    let unit = compile_with_gates(
        &memory,
        GuestAddress(0x1000),
        256,
        &[(GuestAddress(0x1040), 0x1234)],
    )
    .unwrap();
    fs::write(scratch.join("gate.wasm"), &unit.wasm).unwrap();
    memory
        .write(GuestAddress(0x1000), &[0x50, 0x89, 0x0b, 0xc3])
        .unwrap();
    for (name, pc, limit) in [("fault", 0x1000, 64), ("callee", 0x1040, 1)] {
        let unit = compile(&memory, GuestAddress(pc), limit).unwrap();
        fs::write(scratch.join(format!("{name}.wasm")), &unit.wasm).unwrap();
    }
    memory
        .write(GuestAddress(0x1000), &[0x40, 0xeb, 0x3d])
        .unwrap();
    memory
        .write(GuestAddress(0x1040), &[0x49, 0x75, 0xbd])
        .unwrap();
    for (name, pc) in [("unit-a", 0x1000), ("unit-b", 0x1040)] {
        let unit = compile(&memory, GuestAddress(pc), 2).unwrap();
        assert_eq!(unit.entries, [pc]);
        fs::write(scratch.join(format!("{name}.wasm")), &unit.wasm).unwrap();
    }
    for (name, code) in [
        ("gs-null", &[0x65, 0x8b, 0x03][..]),
        ("cs-store", &[0x2e, 0x89, 0x03][..]),
        ("span-wrap", &[0x8b, 0x03][..]),
    ] {
        memory.write(GuestAddress(0x1000), code).unwrap();
        let unit = compile(&memory, GuestAddress(0x1000), 1).unwrap();
        fs::write(scratch.join(format!("{name}.wasm")), &unit.wasm).unwrap();
    }
    for condition in 0..16 {
        memory
            .write(GuestAddress(0x1000), &[0x0f, 0x90 + condition, 0xc0])
            .unwrap();
        let unit = compile(&memory, GuestAddress(0x1000), 1).unwrap();
        fs::write(scratch.join(format!("setcc{condition}.wasm")), &unit.wasm).unwrap();
    }
    for (name, code) in [
        ("setcc-memory", &[0x0f, 0x94, 0x03][..]),
        ("cmov32", &[0x0f, 0x44, 0xc2][..]),
        ("cmov16", &[0x66, 0x0f, 0x44, 0xc2][..]),
        ("cmov-memory32", &[0x0f, 0x44, 0x03][..]),
        ("cmov-memory16", &[0x66, 0x0f, 0x44, 0x03][..]),
    ] {
        memory.write(GuestAddress(0x1000), code).unwrap();
        let unit = compile(&memory, GuestAddress(0x1000), 1).unwrap();
        fs::write(scratch.join(format!("{name}.wasm")), &unit.wasm).unwrap();
    }
    for (bits, prefix) in [(8, &[][..]), (16, &[0x66][..])] {
        for (name, opcode) in [
            ("add", 0x00),
            ("adc", 0x10),
            ("sub", 0x28),
            ("sbb", 0x18),
            ("cmp", 0x38),
            ("test", 0x84),
            ("and", 0x20),
            ("or", 0x08),
            ("xor", 0x30),
        ] {
            let mut code = prefix.to_vec();
            code.extend([opcode + u8::from(bits == 16), 0xd0]);
            memory.write(GuestAddress(0x1000), &code).unwrap();
            let unit = compile(&memory, GuestAddress(0x1000), 1).unwrap();
            fs::write(scratch.join(format!("flags{bits}-{name}.wasm")), &unit.wasm).unwrap();
        }
    }
    for (bits, prefix) in [(8, &[][..]), (16, &[0x66][..]), (32, &[][..])] {
        for (name, suffix) in [("inc", 0xc0), ("dec", 0xc8), ("neg", 0xd8), ("not", 0xd0)] {
            let mut code = prefix.to_vec();
            code.extend([
                if matches!(name, "inc" | "dec") {
                    if bits == 8 { 0xfe } else { 0xff }
                } else if bits == 8 {
                    0xf6
                } else {
                    0xf7
                },
                suffix,
            ]);
            memory.write(GuestAddress(0x1000), &code).unwrap();
            let unit = compile(&memory, GuestAddress(0x1000), 1).unwrap();
            fs::write(scratch.join(format!("unary{bits}-{name}.wasm")), &unit.wasm).unwrap();
        }
    }
    for (name, code) in [
        ("narrow-read8", &[0x02, 0x03][..]),
        ("narrow-rmw8", &[0x00, 0x13][..]),
        ("narrow-read16", &[0x66, 0x03, 0x03][..]),
        ("narrow-rmw16", &[0x66, 0x01, 0x13][..]),
        ("narrow-high8", &[0x00, 0xe6][..]),
    ] {
        memory.write(GuestAddress(0x1000), code).unwrap();
        let unit = compile(&memory, GuestAddress(0x1000), 1).unwrap();
        fs::write(scratch.join(format!("{name}.wasm")), &unit.wasm).unwrap();
    }
    for (name, opcode) in [
        ("add", 0x01),
        ("adc", 0x11),
        ("sub", 0x29),
        ("sbb", 0x19),
        ("cmp", 0x39),
        ("test", 0x85),
        ("and", 0x21),
        ("or", 0x09),
        ("xor", 0x31),
    ] {
        memory.write(GuestAddress(0x1000), &[opcode, 0xd0]).unwrap();
        let unit = compile(&memory, GuestAddress(0x1000), 1).unwrap();
        fs::write(scratch.join(format!("flags-{name}.wasm")), &unit.wasm).unwrap();
    }
    for (name, code) in [
        ("mov-ah-al", &[0x88, 0xc4][..]),
        ("mov-ax-dx", &[0x66, 0x89, 0xd0][..]),
        ("load-ah", &[0x8a, 0x23][..]),
        ("load-ax", &[0x66, 0x8b, 0x03][..]),
        ("store-ah", &[0x88, 0x23][..]),
        ("store-ax", &[0x66, 0x89, 0x03][..]),
        ("extend-ah", &[0x0f, 0xbe, 0xc4][..]),
        ("extend-dx", &[0x0f, 0xb7, 0xc2][..]),
        ("extend-mem", &[0x0f, 0xbe, 0x03][..]),
        ("extend-cx-ah", &[0x66, 0x0f, 0xbe, 0xcc][..]),
        ("cbw", &[0x66, 0x98][..]),
        ("cwd", &[0x66, 0x99][..]),
        ("cdq", &[0x99][..]),
        ("shift-shl", &[0xd3, 0xe0][..]),
        ("shift-shr", &[0xd3, 0xe8][..]),
        ("shift-sar", &[0xd3, 0xf8][..]),
        ("shift-mem", &[0xd3, 0x23][..]),
    ] {
        memory.write(GuestAddress(0x1000), code).unwrap();
        let unit = compile(&memory, GuestAddress(0x1000), 1).unwrap();
        fs::write(scratch.join(format!("{name}.wasm")), &unit.wasm).unwrap();
    }
    for bits in [16, 32] {
        let immediate = if bits == 16 {
            &[0x81, 0x80][..]
        } else {
            &[0x7f, 0, 0, 0x80][..]
        };
        for (name, opcode) in [
            ("rr", &[0x0f, 0xaf, 0xc2][..]),
            ("rm", &[0x0f, 0xaf, 0x03][..]),
            ("ri8", &[0x6b, 0xc2, 0xfd][..]),
            ("mi8", &[0x6b, 0x03, 0xfd][..]),
            ("ri", &[0x69, 0xc2][..]),
            ("mi", &[0x69, 0x03][..]),
        ] {
            let mut code = if bits == 16 { vec![0x66] } else { Vec::new() };
            code.extend_from_slice(opcode);
            if name == "ri" || name == "mi" {
                code.extend_from_slice(immediate);
            }
            memory.write(GuestAddress(0x1000), &code).unwrap();
            let unit = compile(&memory, GuestAddress(0x1000), 1).unwrap();
            fs::write(scratch.join(format!("imul{bits}-{name}.wasm")), &unit.wasm).unwrap();
        }
    }
    let result = Command::new("node").arg("--input-type=module").arg("-e").arg(r#"
import assert from 'node:assert/strict';
import fs from 'node:fs';
const root=process.argv[1], memory=new WebAssembly.Memory({initial:2}), data=new DataView(memory.buffer), ptr=0x8000;
const word=(offset,value)=>data.setUint32(ptr+offset,value>>>0,true), get=offset=>data.getUint32(ptr+offset,true);
let fault=false,readFault=false,writeStatus=0;
const imports={env:{memory},cpu:{read(p,address,width){width&=255;word(52,0);if(readFault&&address===0x1ffe){word(52,3);word(60,0x2000);return 0;}return width===1?data.getUint8(address):width===2?data.getUint16(address,true):data.getUint32(address,true);},write(p,address,width,value){if(fault&&address===0x1ffe){word(52,3);word(60,0x2000);return 3;}if(writeStatus===8){word(52,8);return 8;} if(width===1)data.setUint8(address,value);else if(width===2)data.setUint16(address,value,true);else data.setUint32(address,value,true);word(52,writeStatus);return writeStatus;}}};
function initialize(pc,quota){new Uint8Array(memory.buffer).fill(0);word(0,1);word(4,160);word(40,pc);word(44,2);word(48,quota);word(24,0x4800);word(152,0x1f);}
const load=async name=>(await WebAssembly.instantiate(fs.readFileSync(`${root}/${name}.wasm`),imports)).instance.exports.run;
const run=await load('call');initialize(0x1000,1000);[5,64,7,0x3000,0x4800,0,0,0].forEach((v,i)=>word(8+4*i,v));
assert.equal(run(ptr),7);let eax=5,edx=7;for(let i=0;i<64;i++){eax=(eax+3)>>>0;edx=(edx^eax)>>>0;}
assert.deepEqual(Array.from({length:8},(_,i)=>get(8+4*i)),[eax,0,edx,0x3000,0x4800,0,0,0]);assert.equal(get(40),0x1008);assert.equal(get(44),0x46);assert.equal(get(56),448);assert.equal(get(48),552);assert.equal(data.getUint32(0x300c,true),64);assert.equal(data.getUint32(0x47fc,true),0x1005);
initialize(0x1000,1000);const gate=await load('gate');assert.equal(gate(ptr),5);assert.equal(get(40),0x1040);assert.equal(get(68),0x1234);assert.equal(get(56),1);assert.equal(get(48),999);assert.equal(get(24),0x47fc);assert.equal(data.getUint32(0x47fc,true),0x1005);
const failed=await load('fault');initialize(0x1000,100);word(8,0x11223344);word(12,0x55667788);word(20,0x1ffe);fault=true;data.setUint16(0x1ffe,0xbbaa,true);assert.equal(failed(ptr),3);assert.equal(get(40),0x1001);assert.equal(get(56),1);assert.equal(get(48),99);assert.equal(get(24),0x47fc);assert.equal(data.getUint16(0x1ffe,true),0xbbaa);assert.equal(data.getUint32(0x47fc,true),0x11223344);assert.equal(get(60),0x2000);
fault=false;initialize(0x1000,1);word(8,9);assert.equal(failed(ptr),2);assert.equal(get(40),0x1001);assert.equal(get(56),1);assert.equal(get(24),0x47fc);
initialize(0x1000,100);word(8,9);writeStatus=8;assert.equal(failed(ptr),8);assert.equal(get(40),0x1000);assert.equal(get(56),0);assert.equal(get(24),0x4800);assert.equal(data.getUint32(0x47fc,true),0);
initialize(0x1000,100);word(8,9);writeStatus=6;assert.equal(failed(ptr),6);assert.equal(get(40),0x1001);assert.equal(get(56),1);assert.equal(get(48),99);assert.equal(get(24),0x47fc);assert.equal(data.getUint32(0x47fc,true),9);writeStatus=0;
for(const name of ['gs-null','cs-store','span-wrap']){initialize(0x1000,10);word(8,0x12345678);word(20,name==='span-wrap'?0xfffffffe:0x3000);const fn=await load(name);assert.equal(fn(ptr),9,name);assert.equal(get(40),0x1000);assert.equal(get(56),0);assert.equal(get(68),13);assert.equal(get(8),0x12345678);assert.equal(data.getUint32(0x3000,true),0);}
const functions=new WebAssembly.Table({initial:3,element:'anyfunc'});functions.set(1,run);const dispatch=(await WebAssembly.instantiate(fs.readFileSync(`${root}/dispatch.wasm`),{env:{memory,functions}})).instance.exports.run;
initialize(0xdead,20);word(144,0x9000);word(148,15);assert.equal(dispatch(ptr),1);assert.equal(get(56),0);
initialize(0x1000,1000);word(144,0x9000);word(148,15);[5,64,7,0x3000,0x4800,0,0,0].forEach((v,i)=>word(8+4*i,v));const hash=((0x1000>>>1)^(0x1000>>>12))&15;data.setUint32(0x9000+hash*8,0x1000,true);data.setUint32(0x9004+hash*8,1,true);assert.equal(dispatch(ptr),7);assert.equal(get(56),448);
functions.set(1,await load('unit-a'));functions.set(2,await load('unit-b'));initialize(0x1000,200001);word(12,50000);word(144,0x9000);word(148,15);for(const[pc,slot]of[[0x1000,1],[0x1040,2]]){let index=((pc>>>1)^(pc>>>12))&15;while(data.getUint32(0x9004+index*8,true))index=(index+1)&15;data.setUint32(0x9000+index*8,pc,true);data.setUint32(0x9004+index*8,slot,true);}assert.equal(dispatch(ptr),1);assert.equal(get(8),50000);assert.equal(get(12),0);assert.equal(get(40),0x1043);assert.equal(get(56),200000);assert.equal(get(48),1);
const values=[0,1,15,16,0x7fffffff,0x80000000,0xffffffff,0x55555555,0xaaaaaaaa],signed=x=>BigInt(x|0);
for(const name of ['add','adc','sub','sbb','cmp','test','and','or','xor']){const fn=await load(`flags-${name}`);for(const a of values)for(const b of values)for(const carry of [0,1]){const sub=['sub','sbb','cmp'].includes(name),logical=['test','and','or','xor'].includes(name),cin=['adc','sbb'].includes(name)?carry:0;let exact=logical?0n:sub?BigInt(a)-BigInt(b)-BigInt(cin):BigInt(a)+BigInt(b)+BigInt(cin);const result=logical?(name==='or'?(a|b):name==='xor'?(a^b):(a&b))>>>0:Number(BigInt.asUintN(32,exact)),signedExact=sub?signed(a)-signed(b)-BigInt(cin):signed(a)+signed(b)+BigInt(cin);let flags=0x402;if(!logical){if(sub?exact<0n:exact>0xffffffffn)flags|=1;if(sub?(a&15)<(b&15)+cin:(a&15)+(b&15)+cin>15)flags|=16;if(signedExact< -2147483648n||signedExact>2147483647n)flags|=0x800;}if(result===0)flags|=64;if(result>=0x80000000)flags|=128;if([...((result&255).toString(2))].filter(x=>x==='1').length%2===0)flags|=4;initialize(0x1000,1);word(8,a);word(16,b);word(44,0xc12|carry);assert.equal(fn(ptr),0,name);assert.equal(get(8),['cmp','test'].includes(name)?a:result,`${name} result`);assert.equal(get(44),flags,`${name} ${a.toString(16)} ${b.toString(16)} CF${carry}`);assert.equal(get(56),1);}}
for(const[name,eax,ecx,edx,outEax,outEcx,outEdx,ram]of[
['mov-ah-al',0x11223344,0xaabbccdd,0x556680ff,0x11224444,0xaabbccdd,0x556680ff,0x8180],
['mov-ax-dx',0x11223344,0xaabbccdd,0x556680ff,0x112280ff,0xaabbccdd,0x556680ff,0x8180],
['load-ah',0x11223344,0xaabbccdd,0x556680ff,0x11228044,0xaabbccdd,0x556680ff,0x8180],
['load-ax',0x11223344,0xaabbccdd,0x556680ff,0x11228180,0xaabbccdd,0x556680ff,0x8180],
['store-ah',0x11223344,0xaabbccdd,0x556680ff,0x11223344,0xaabbccdd,0x556680ff,0x8133],
['store-ax',0x11223344,0xaabbccdd,0x556680ff,0x11223344,0xaabbccdd,0x556680ff,0x3344],
['extend-ah',0x11228044,0xaabbccdd,0x556680ff,0xffffff80,0xaabbccdd,0x556680ff,0x8180],
['extend-dx',0x11223344,0xaabbccdd,0x556680ff,0x80ff,0xaabbccdd,0x556680ff,0x8180],
['extend-mem',0x11223344,0xaabbccdd,0x556680ff,0xffffff80,0xaabbccdd,0x556680ff,0x8180],
['extend-cx-ah',0x11228044,0xaabbccdd,0x556680ff,0x11228044,0xaabbff80,0x556680ff,0x8180],
['cbw',0x11223380,0xaabbccdd,0x556680ff,0x1122ff80,0xaabbccdd,0x556680ff,0x8180],
['cwd',0x11228000,0xaabbccdd,0x556680ff,0x11228000,0xaabbccdd,0x5566ffff,0x8180],
['cdq',0x81223344,0xaabbccdd,0x556680ff,0x81223344,0xaabbccdd,0xffffffff,0x8180]]){initialize(0x1000,1);[eax,ecx,edx,0x3000].forEach((v,i)=>word(8+4*i,v));word(44,0xcd7);data.setUint16(0x3000,0x8180,true);const fn=await load(name);assert.equal(fn(ptr),0,name);assert.deepEqual([get(8),get(12),get(16),get(44),data.getUint16(0x3000,true)],[outEax,outEcx,outEdx,0xcd7,ram],name);}
for(const kind of ['shl','shr','sar']){const fn=await load(`shift-${kind}`);for(const value of values)for(const raw of[0,1,2,15,31,32,33,63,255]){const count=raw&31;const exact=kind==='shl'?BigInt(value)<<BigInt(count):kind==='shr'?BigInt(value)>>BigInt(count):signed(value)>>BigInt(count);const result=Number(BigInt.asUintN(32,exact));let expected=0xc13,mask=-1;if(count){const cf=Number((BigInt(value)>>BigInt(kind==='shl'?32-count:count-1))&1n);expected&=~0x8d5;expected|=cf;if(result===0)expected|=64;if(result>=0x80000000)expected|=128;if([...((result&255).toString(2))].filter(x=>x==='1').length%2===0)expected|=4;if(count===1){const of=kind==='shl'?Number((result>>>31)^cf):kind==='shr'?(value>>>31):0;expected=(expected&~0x800)|(of<<11);}}initialize(0x1000,1);word(8,value);word(12,raw);word(44,0xc13);assert.equal(fn(ptr),0);assert.equal(get(8),result,`${kind} result${raw}`);assert.equal(get(44)&mask,expected&mask,`${kind} flags ${value.toString(16)} count${raw}`);}}
const shiftMemory=await load('shift-mem');initialize(0x1000,1);word(12,32);word(20,0x3000);word(44,0xcd7);data.setUint32(0x3000,0x87654321,true);assert.equal(shiftMemory(ptr),0);assert.equal(get(44),0xcd7);assert.equal(data.getUint32(0x3000,true),0x87654321);initialize(0x1000,1);word(12,0);word(20,0x1ffe);fault=true;assert.equal(shiftMemory(ptr),3);assert.equal(get(56),0);assert.equal(get(40),0x1000);fault=false;
for(const bits of[8,16]){const limit=2**bits,sign=limit/2,max=limit-1,operands=[0,1,15,16,sign-1,sign,max,0x55&max,0xaa&max],sx=x=>BigInt(x>=sign?x-limit:x),prefix=bits===8?0x12345600:0x12340000;for(const name of['add','adc','sub','sbb','cmp','test','and','or','xor']){const fn=await load(`flags${bits}-${name}`);for(const a of operands)for(const b of operands)for(const carry of[0,1]){const sub=['sub','sbb','cmp'].includes(name),logical=['test','and','or','xor'].includes(name),cin=['adc','sbb'].includes(name)?carry:0,exact=sub?BigInt(a)-BigInt(b)-BigInt(cin):BigInt(a)+BigInt(b)+BigInt(cin),result=logical?(name==='or'?(a|b):name==='xor'?(a^b):(a&b)):Number(BigInt.asUintN(bits,exact)),signedExact=sub?sx(a)-sx(b)-BigInt(cin):sx(a)+sx(b)+BigInt(cin);let flags=0x402;if(!logical){if(sub?exact<0n:exact>BigInt(max))flags|=1;if(sub?(a&15)<(b&15)+cin:(a&15)+(b&15)+cin>15)flags|=16;if(signedExact< -BigInt(sign)||signedExact>=BigInt(sign))flags|=0x800;}if(result===0)flags|=64;if(result>=sign)flags|=128;if([...((result&255).toString(2))].filter(x=>x==='1').length%2===0)flags|=4;initialize(0x1000,1);word(8,(prefix|a)>>>0);word(16,b);word(44,0xc12|carry);assert.equal(fn(ptr),0);assert.equal(get(8),(prefix|(['cmp','test'].includes(name)?a:result))>>>0,`${bits}${name} result`);assert.equal(get(44),flags,`${bits}${name} ${a.toString(16)} ${b.toString(16)} CF${carry}`);}}}
for(const bits of[8,16,32]){const limit=2**bits,sign=limit/2,max=limit-1,prefix=bits===8?0x12345600:bits===16?0x12340000:0;for(const name of['inc','dec','neg','not']){const fn=await load(`unary${bits}-${name}`);for(const value of[0,1,15,16,sign-1,sign,max])for(const carry of[0,1]){let result,flags=0xcd6|carry;if(name==='not')result=max-value;else{const sub=name!=='inc',left=name==='neg'?0:value,right=name==='neg'?value:1,exact=sub?BigInt(left)-BigInt(right):BigInt(left)+BigInt(right),sx=x=>BigInt(x>=sign?x-limit:x),signedExact=sub?sx(left)-sx(right):sx(left)+sx(right);result=Number(BigInt.asUintN(bits,exact));flags&=~0x8d5;if(name==='neg'&&value!==0||name!=='neg'&&carry)flags|=1;if(sub?(left&15)<(right&15):(left&15)+(right&15)>15)flags|=16;if(signedExact< -BigInt(sign)||signedExact>=BigInt(sign))flags|=0x800;if(result===0)flags|=64;if(result>=sign)flags|=128;if([...((result&255).toString(2))].filter(x=>x==='1').length%2===0)flags|=4;}initialize(0x1000,1);word(8,(prefix|value)>>>0);word(44,0xcd6|carry);assert.equal(fn(ptr),0);assert.equal(get(8),(prefix|result)>>>0,`unary${bits}${name} result`);assert.equal(get(44),flags,`unary${bits}${name} ${value.toString(16)} CF${carry}`);}}}
for(const[name,expectEax,expectRam]of[['narrow-read8',0x112233c4,0x8180],['narrow-read16',0x1122b4c4,0x8180],['narrow-rmw8',0x11223344,0x817f],['narrow-rmw16',0x11223344,0x027f]]){initialize(0x1000,1);word(8,0x11223344);word(16,0x80ff);word(20,0x3000);data.setUint16(0x3000,0x8180,true);const fn=await load(name);assert.equal(fn(ptr),0,name);assert.equal(get(8),expectEax,name);assert.equal(data.getUint16(0x3000,true),expectRam,name);if(name.includes('rmw')){initialize(0x1000,1);word(20,0x1ffe);word(44,0xcd7);fault=true;assert.equal(fn(ptr),3,name);assert.equal(get(40),0x1000);assert.equal(get(56),0);assert.equal(get(44),0xcd7);fault=false;}}
initialize(0x1000,1);word(8,0x11227f44);word(16,0x556680ff);const high=await load('narrow-high8');assert.equal(high(ptr),0);assert.equal(get(16),0x5566ffff);assert.equal(get(8),0x11227f44);assert.equal(get(44),0x86);
for(let condition=0;condition<16;condition++){const fn=await load(`setcc${condition}`);for(let row=0;row<32;row++){const[cf,pf,zf,sf,of]=[row&1,(row>>1)&1,(row>>2)&1,(row>>3)&1,(row>>4)&1],flags=0x412|cf|(pf<<2)|(zf<<6)|(sf<<7)|(of<<11),truth=[of,!of,cf,!cf,zf,!zf,cf||zf,!(cf||zf),sf,!sf,pf,!pf,sf!==of,sf===of,zf||sf!==of,!zf&&sf===of];initialize(0x1000,1);word(8,0x123456ab);word(44,flags);assert.equal(fn(ptr),0);assert.equal(get(8),0x12345600|Number(Boolean(truth[condition])),`setcc${condition} flags${flags.toString(16)}`);assert.equal(get(44),flags);assert.equal(get(56),1);}}
for(const name of['cmov32','cmov16','cmov-memory32','cmov-memory16']){const fn=await load(name);for(const zero of[0,64]){initialize(0x1000,1);word(8,0x11223344);word(16,0x556680ff);word(20,0x3000);word(44,0x412|zero);data.setUint32(0x3000,0x556680ff,true);assert.equal(fn(ptr),0);assert.equal(get(8),zero?(name.endsWith('16')?0x112280ff:0x556680ff):0x11223344,name);assert.equal(get(44),0x412|zero);}if(name.includes('memory')){initialize(0x1000,1);word(8,0x11223344);word(20,0x1ffe);word(44,2);readFault=true;assert.equal(fn(ptr),3,`${name} untaken memory still faults`);assert.equal(get(8),0x11223344);assert.equal(get(40),0x1000);assert.equal(get(56),0);readFault=false;}}
const setMemory=await load('setcc-memory');initialize(0x1000,1);word(20,0x3000);word(44,0x442);assert.equal(setMemory(ptr),0);assert.equal(data.getUint8(0x3000),1);assert.equal(get(44),0x442);initialize(0x1000,1);word(20,0x1ffe);word(44,0x442);fault=true;assert.equal(setMemory(ptr),3);assert.equal(get(56),0);assert.equal(get(40),0x1000);assert.equal(get(44),0x442);fault=false;
for(const bits of[16,32]){const modulus=2**bits,sign=modulus/2,max=modulus-1,operands=[0,1,2,sign-1,sign,max,0x7f,0x80,0xff],prefix=bits===16?0x12340000:0,sx=value=>BigInt.asIntN(bits,BigInt(value));for(const form of['rr','rm','ri8','mi8','ri','mi']){const fn=await load(`imul${bits}-${form}`),two=form==='rr'||form==='rm',imm=form.endsWith('8')?-3n:sx(bits===16?0x8081:0x8000007f);for(const a of operands)for(const b of(two?operands:[0])){const product=two?sx(a)*sx(b):sx(a)*imm,result=Number(BigInt.asUintN(bits,product)),overflow=product< -BigInt(sign)||product>=BigInt(sign);initialize(0x1000,1);word(8,(prefix|a)>>>0);word(16,two?b:a);word(20,0x3000);word(44,0xcd7);if(bits===16)data.setUint16(0x3000,two?b:a,true);else data.setUint32(0x3000,two?b:a,true);assert.equal(fn(ptr),0,`imul${bits}-${form}`);assert.equal(get(8),(prefix|result)>>>0,`imul${bits}-${form} ${a.toString(16)} ${b.toString(16)} result`);assert.equal(get(44),0x402|(overflow?0x801:0),`imul${bits}-${form} flags`);assert.equal(get(56),1);assert.equal(get(48),0);}if(form.includes('m')){initialize(0x1000,1);word(8,0x12345678);word(20,0x1ffe);word(44,0xcd7);readFault=true;assert.equal(fn(ptr),3,`imul${bits}-${form} sourcefault`);assert.equal(get(8),0x12345678);assert.equal(get(44),0xcd7);assert.equal(get(40),0x1000);assert.equal(get(56),0);readFault=false;}}}
console.log('independent integer/CALL/RET, quota, precise fault/SMC and 100000 Wasm unit handoffs passed');
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
