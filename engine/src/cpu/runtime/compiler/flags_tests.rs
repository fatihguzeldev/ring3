use super::*;
use crate::memory::{PageRange, Permissions};

#[test]
fn generated_static_flag_recipes_preserve_clobbers_entries_and_fault_frontiers() {
    use std::{
        fs,
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };
    let scratch = std::env::temp_dir().join(format!(
        "ring3-runtime-static-flags-{}-{}",
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
    for (kind, opcode) in [("add", 0x01), ("sub", 0x29), ("adc", 0x11), ("sbb", 0x19)] {
        for (form, operand) in [("clobber", 0xc8), ("alias", 0xc0)] {
            let mut code = vec![opcode, operand, 0xb9, 0x44, 0x33, 0x22, 0x11];
            code.extend_from_slice(&[0x0f, 0x90, 0xc2, 0x0f, 0x92, 0xc6, 0x9f]);
            memory.write(GuestAddress(0x1000), &code).unwrap();
            fs::write(
                scratch.join(format!("{kind}-{form}.wasm")),
                compile(&memory, GuestAddress(0x1000), 5).unwrap().wasm,
            )
            .unwrap();
        }
    }
    for (name, code, count, gates) in [
        (
            "partial",
            &[0x01, 0xc8, 0xb1, 0x44, 0x0f, 0x90, 0xc2, 0x9f][..],
            4,
            vec![],
        ),
        (
            "cross",
            &[
                0x01, 0xc8, 0xeb, 4, 0x90, 0x90, 0x90, 0x90, 0x31, 0xdb, 0x83, 0xd2, 0, 0x46, 0x0f,
                0x0b,
            ][..],
            6,
            vec![(GuestAddress(0x100e), 23)],
        ),
        (
            "fault",
            &[0x01, 0xc8, 0x03, 0x1e, 0x31, 0xd2][..],
            3,
            vec![],
        ),
    ] {
        memory.write(GuestAddress(0x1000), code).unwrap();
        fs::write(
            scratch.join(format!("{name}.wasm")),
            compile_with_gates(&memory, GuestAddress(0x1000), count, &gates)
                .unwrap()
                .wasm,
        )
        .unwrap();
    }
    let result = Command::new("node").arg("--input-type=module").arg("-e").arg(r#"
import assert from 'node:assert/strict';
import fs from 'node:fs';
const memory=new WebAssembly.Memory({initial:2}),ram=new Uint8Array(memory.buffer),d=new DataView(memory.buffer),ctx=0x8000;
const word=(at,v)=>d.setUint32(ctx+at,v>>>0,true);
let mode='',reads=[];
const imports={env:{memory},cpu:{read(p,a,w){
 assert.deepEqual([p,a>>>0,w],[ctx,0x6000,4]);reads.push(ram.slice());if(mode==='throw')throw Error('authored-flag-producer-trap');
 word(52,3);word(60,a);word(64,0x10104);word(68,4);return 0;
},write(){throw Error('no guest write in static flag recipe fixtures');}}};
const runs={};for(const name of['add-clobber','sub-clobber','adc-clobber','sbb-clobber','add-alias','sub-alias','adc-alias','sbb-alias','partial','cross','fault'])runs[name]=(await WebAssembly.instantiate(fs.readFileSync(`${process.argv[1]}/${name}.wasm`),imports)).instance.exports.run;
function scenario(name,input,expect){
 for(let i=0;i<ram.length;i++)ram[i]=(i*47+5)&255;ram.fill(0,ctx,ctx+160);
 word(0,1);word(4,160);[0x7fffffff,1,0x1234abcd,0x12345678,0x4800,10,0x6000,12].forEach((v,i)=>word(8+4*i,v));
 for(const[index,v]of Object.entries(input.regs??{}))word(8+4*Number(index),v);
 word(40,input.pc??0x1000);word(44,0x200603);word(48,input.quota);word(56,0xfffffffe);word(60,0x11223344);word(64,0x55667788);word(68,0x12345678);word(152,63);
 const before=ram.slice();mode=input.mode??'';reads=[];
 if(mode==='throw'){assert.throws(()=>runs[name](ctx),/authored-flag-producer-trap/);assert.deepEqual(ram,before);assert.deepEqual(reads,[before]);return;}
 const expected=before.slice(),view=new DataView(expected.buffer),set=(at,v)=>view.setUint32(ctx+at,v>>>0,true);
 for(const[at,v]of[[40,expect.pc],[44,expect.flags],[48,input.quota-expect.steps],[52,expect.status??0],[56,(0xfffffffe+expect.steps)>>>0],...(expect.fields??[])])set(at,v);
 for(const[index,v]of Object.entries(expect.regs??{}))set(8+4*Number(index),v);
 assert.equal(runs[name](ctx),expect.status??0,name);assert.deepEqual(ram,expected,`${name}/entry${(input.pc??0x1000).toString(16)}/quota${input.quota}: full state`);
 assert.equal(reads.length,expect.reads??0);for(const observed of reads)assert.deepEqual(observed,before,'faulting consumers do not publish scalar state before the helper');
}
for(const[name,eax,ecx,outEax,outEdx,flags]of[
 ['add-clobber',0x7fffffff,1,0x80009600,0x12340001,0x200e96],
 ['sub-clobber',0x80000000,1,0x7fff16ff,0x12340001,0x200e16],
 ['adc-clobber',0x7fffffff,0,0x80009600,0x12340001,0x200e96],
 ['sbb-clobber',0x80000000,0,0x7fff16ff,0x12340001,0x200e16],
 ['add-alias',0x40000000,1,0x80008600,0x12340001,0x200e86],
 ['sub-alias',0x80000000,1,0x00004600,0x12340000,0x200646],
 ['adc-alias',0x80000000,1,0x00000301,0x12340101,0x200e03],
 ['sbb-alias',0x80000000,1,0xffff97ff,0x12340100,0x200697],
])scenario(name,{quota:5,regs:{0:eax,1:ecx}},{pc:0x100e,steps:5,flags,regs:{0:outEax,1:0x11223344,2:outEdx}});
scenario('partial',{quota:4},{pc:0x1008,steps:4,flags:0x200e96,regs:{0:0x80009600,1:0x44,2:0x1234ab01}});
scenario('add-clobber',{pc:0x1002,quota:4,regs:{0:0x12345678,1:0xdeadbeef}},{pc:0x100e,steps:4,flags:0x200603,regs:{0:0x12340378,1:0x11223344,2:0x12340100}});
scenario('add-clobber',{pc:0x1002,quota:0},{pc:0x1002,steps:0,flags:0x200603,status:2});
scenario('add-clobber',{pc:0x1007,quota:1},{pc:0x100a,steps:1,flags:0x200603,status:2,regs:{2:0x1234ab00}});
for(const[quota,pc,flags,eax,edx,ebx,esi]of[
 [0,0x1000,0x200603,0x7fffffff,0xffffffff,0x12345678,0x7fffffff],
 [1,0x1002,0x200e96,0x80000000,0xffffffff,0x12345678,0x7fffffff],
 [2,0x1008,0x200e96,0x80000000,0xffffffff,0x12345678,0x7fffffff],
 [3,0x100a,0x200646,0x80000000,0xffffffff,0,0x7fffffff],
 [4,0x100d,0x200686,0x80000000,0xffffffff,0,0x7fffffff],
 [5,0x100e,0x200e96,0x80000000,0xffffffff,0,0x80000000],
 [6,0x100e,0x200e96,0x80000000,0xffffffff,0,0x80000000],
])scenario('cross',{quota,regs:{2:0xffffffff,6:0x7fffffff}},{pc,steps:Math.min(quota,5),flags,status:quota===6?5:2,regs:{0:eax,2:edx,3:ebx,6:esi},fields:quota===6?[[68,23]]:[]});
scenario('cross',{pc:0x1008,quota:0},{pc:0x1008,steps:0,flags:0x200603,status:2});
scenario('cross',{pc:0x1008,quota:1},{pc:0x100a,steps:1,flags:0x200646,status:2,regs:{3:0}});
scenario('cross',{pc:0x100a,quota:1,regs:{2:0xffffffff}},{pc:0x100d,steps:1,flags:0x200657,status:2,regs:{2:0}});
scenario('fault',{quota:3},{pc:0x1002,steps:1,flags:0x200e96,status:3,regs:{0:0x80000000},reads:1,fields:[[60,0x6000],[64,0x10104],[68,4]]});
scenario('fault',{quota:3,mode:'throw'});
console.log('literal static FLAGS: stable RHS clobbers and aliases, partial writes, incoming landings, cross-block zero/one quotas, carry and PRE memory faults passed');
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
fn generated_ah_flag_transfers_preserve_reserved_bits_and_partial_registers() {
    use std::{
        fs,
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };
    let scratch = std::env::temp_dir().join(format!(
        "ring3-runtime-flags-{}-{}",
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
    for (name, bytes, count) in [
        ("sahf", &[0x9e][..], 1),
        ("lahf", &[0x9f][..], 1),
        ("salc", &[0xd6][..], 1),
        ("roundtrip", &[0x9e, 0x9f][..], 2),
        ("condition", &[0x9e, 0x0f, 0x96, 0xc0][..], 2),
    ] {
        memory.write(GuestAddress(0x1000), bytes).unwrap();
        let unit = compile(&memory, GuestAddress(0x1000), count).unwrap();
        fs::write(scratch.join(format!("{name}.wasm")), &unit.wasm).unwrap();
    }
    let result = Command::new("node").arg("--input-type=module").arg("-e").arg(r#"
import assert from 'node:assert/strict';
import fs from 'node:fs';
const root=process.argv[1],memory=new WebAssembly.Memory({initial:1}),data=new DataView(memory.buffer),ptr=0x8000;
const word=(at,value)=>data.setUint32(ptr+at,value>>>0,true),get=at=>data.getUint32(ptr+at,true),regs=()=>Array.from({length:8},(_,i)=>get(8+4*i));
const imports={env:{memory},cpu:{read(){throw new Error('flag transport unexpectedly read guest memory');},write(){throw new Error('flag transport unexpectedly wrote guest memory');}}};
const functions=new Map();for(const name of['sahf','lahf','salc','roundtrip','condition'])functions.set(name,(await WebAssembly.instantiate(fs.readFileSync(`${root}/${name}.wasm`),imports)).instance.exports.run);
function init(eax,flags,quota=1){new Uint8Array(memory.buffer).fill(0);word(0,1);word(4,160);[eax,0x8899aabb,0xa1b2c3d4,0x55667788,0x4800,0x13579bdf,0x2468ace0,0x0f1e2d3c].forEach((v,i)=>word(8+4*i,v));word(40,0x1000);word(44,flags);word(48,quota);word(152,0x1f);}
function transported(flags,ah){let result=flags;for(const bit of[0,2,4,6,7])result=(result&~(1<<bit))|(((ah>>bit)&1)<<bit);return(result|2)>>>0;}
for(const flags of[2,0x202,0xcd7,0x200602])for(let ah=0;ah<256;ah++){const eax=(0xa5a5005a|(ah<<8))>>>0;init(eax,flags);const before=regs();assert.equal(functions.get('sahf')(ptr),0);assert.deepEqual(regs(),before);assert.equal(get(44),transported(flags,ah),`SAHF AH${ah.toString(16)} FLAGS${flags.toString(16)}`);assert.equal(get(56),1);assert.equal(get(48),0);assert.equal(get(40),0x1001);}
for(const[ah,expected]of[[0,0xc02],[255,0xcd7],[0x28,0xc02],[2,0xc02],[0x80,0xc82],[0x40,0xc42]]){init((0x11220044|(ah<<8))>>>0,0xcd7);assert.equal(functions.get('sahf')(ptr),0);assert.equal(get(44),expected);}
for(let row=0;row<32;row++)for(const preserved of[0,0x800,0x400,0x200e00]){let ah=2;for(const[i,bit]of[0,2,4,6,7].entries())ah|=((row>>i)&1)<<bit;const flags=ah|preserved,eax=0xa5a5ff5a;init(eax,flags);const before=regs(),expected=[...before];expected[0]=(0xa5a5005a|(ah<<8))>>>0;assert.equal(functions.get('lahf')(ptr),0);assert.deepEqual(regs(),expected);assert.equal(get(44),flags);assert.equal(get(56),1);assert.equal(get(40),0x1001);}
for(const flags of[2,3,0xcd6,0xcd7,0x200602,0x200603]){init(0xa5a5ff5a,flags);const before=regs(),expected=[...before];expected[0]=(0xa5a5ff00|(flags&1?255:0))>>>0;assert.equal(functions.get('salc')(ptr),0);assert.deepEqual(regs(),expected);assert.equal(get(44),flags);assert.equal(get(56),1);assert.equal(get(48),0);}
for(let ah=0;ah<256;ah++){init((0xa5a5005a|(ah<<8))>>>0,0x200e02,2);assert.equal(functions.get('roundtrip')(ptr),0);assert.equal(get(8),(0xa5a5005a|(((ah&0xd5)|2)<<8))>>>0);assert.equal(get(44),transported(0x200e02,ah));assert.equal(get(56),2);assert.equal(get(48),0);assert.equal(get(40),0x1002);init((0xa5a5005a|(ah<<8))>>>0,0xc02,2);assert.equal(functions.get('condition')(ptr),0);assert.equal(get(8),(0xa5a50000|(ah<<8)|Number(Boolean(ah&0x41)))>>>0);assert.equal(get(44),transported(0xc02,ah));assert.equal(get(56),2);assert.equal(get(40),0x1004);}
console.log('SAHF/LAHF/SALC reserved-bit and partial-register transport, preservedOF/DF/ID, fullAHdomain and condition composition passed');
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
fn generated_deferred_flag_observers_preserve_prefix_fault_smc_and_interior_state() {
    use std::{
        fs,
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };
    let scratch = std::env::temp_dir().join(format!(
        "ring3-runtime-deferred-flags-{}-{}",
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
    for (name, code, count) in [
        ("overwrite", &[0x01, 0xc8, 0x31, 0xd2][..], 2),
        ("read", &[0x01, 0xc8, 0x8b, 0x13][..], 2),
        ("store", &[0x01, 0xc8, 0x89, 0x03][..], 2),
        ("condition", &[0x01, 0xc8, 0x0f, 0x90, 0xc2][..], 2),
        ("zero", &[0x31, 0xc0, 0x0f, 0x94, 0xc2][..], 2),
        ("carry", &[0x01, 0xc8, 0x11, 0xd2][..], 2),
        ("partial", &[0x01, 0xc8, 0xf9][..], 2),
        ("lahf", &[0x01, 0xc8, 0x9f][..], 2),
    ] {
        memory.write(GuestAddress(0x1000), code).unwrap();
        let unit = compile(&memory, GuestAddress(0x1000), count).unwrap();
        fs::write(scratch.join(format!("{name}.wasm")), unit.wasm).unwrap();
    }
    let result = Command::new("node").arg("--input-type=module").arg("-e").arg(r#"
import assert from 'node:assert/strict';
import fs from 'node:fs';
const root=process.argv[1],memory=new WebAssembly.Memory({initial:2}),ram=new Uint8Array(memory.buffer),data=new DataView(memory.buffer),ptr=0x8000;
const word=(at,value)=>data.setUint32(ptr+at,value>>>0,true);let mode='',reads=0,writes=0;
const imports={env:{memory},cpu:{
 read(context,address,width){assert.deepEqual([context,address>>>0,width],[ptr,0x3000,4]);reads++;word(52,3);word(60,0x3000);word(64,0x10104);word(68,4);return 0;},
 write(context,address,width,value){assert.deepEqual([context,address>>>0,width,value>>>0],[ptr,0x3000,4,0x80000000]);writes++;data.setUint32(0x3000,value,true);word(52,6);return 6;}
}};
const runs=new Map();for(const name of['overwrite','read','store','condition','zero','carry','partial','lahf'])runs.set(name,(await WebAssembly.instantiate(fs.readFileSync(`${root}/${name}.wasm`),imports)).instance.exports.run);
function scenario(name,{quota=2,pc=0x1000,initialEax:eax=0x7fffffff,initialEcx:ecx=1,initialEdx:edx=0x12345678,...expected}={}){
 for(let i=0;i<ram.length;i++)ram[i]=(i*31+9)&255;ram.fill(0,ptr,ptr+160);word(0,1);word(4,160);[eax,ecx,edx,0x3000,0x4800,10,11,12].forEach((value,index)=>word(8+4*index,value));word(40,pc);word(44,0x603);word(48,quota);word(52,0xabcdef01);word(56,0xffffffff);word(60,0x11223344);word(64,0x55667788);word(68,0x12345678);word(152,63);reads=writes=0;
 const want=ram.slice(),view=new DataView(want.buffer),put=(at,value)=>view.setUint32(ptr+at,value>>>0,true);
 for(const[at,value]of[[8,expected.eax??eax],[16,expected.edx??edx],[40,expected.next],[44,expected.flags],[48,quota-expected.work],[52,expected.status],[56,(0xffffffff+expected.work)>>>0]])put(at,value);
 if(name==='read'){put(60,0x3000);put(64,0x10104);put(68,4);}if(name==='store')view.setUint32(0x3000,0x80000000,true);
 assert.equal(runs.get(name)(ptr),expected.status,name);assert.deepEqual(ram,want,`${name}: complete scalar, banks, RAM and wrap`);assert.deepEqual([reads,writes],[name==='read'?1:0,name==='store'?1:0]);
}
scenario('overwrite',{quota:0,next:0x1000,flags:0x603,work:0,status:2});
scenario('overwrite',{quota:1,next:0x1002,eax:0x80000000,flags:0xe96,work:1,status:2});
scenario('overwrite',{next:0x1004,eax:0x80000000,edx:0,flags:0x646,work:2,status:0});
scenario('overwrite',{pc:0x1002,next:0x1004,edx:0,flags:0x646,work:1,status:0});
scenario('read',{next:0x1002,eax:0x80000000,flags:0xe96,work:1,status:3});
scenario('store',{next:0x1004,eax:0x80000000,flags:0xe96,work:2,status:6});
scenario('condition',{next:0x1005,eax:0x80000000,edx:0x12345601,flags:0xe96,work:2,status:0});
scenario('zero',{next:0x1005,eax:0,edx:0x12345601,flags:0x646,work:2,status:0});
scenario('carry',{initialEdx:0,edx:0,next:0x1004,eax:0x80000000,flags:0x646,work:2,status:0});
scenario('partial',{next:0x1003,eax:0x80000000,flags:0xe97,work:2,status:0});
scenario('lahf',{next:0x1003,eax:0x80009600,flags:0xe96,work:2,status:0});
console.log('deferred FLAGS complete-state literals: overwrite, prefix quota, interior entry, fault, SMC, conditions, carry, partial write, LAHF and retirement wrap passed');
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
fn generated_pending_flags_survive_numeric_success_faults_and_status_observers() {
    use std::{
        fs,
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };
    let scratch = std::env::temp_dir().join(format!(
        "ring3-runtime-numeric-flags-{}-{}",
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
    for (name, code, count) in [
        ("constant", &[0x01, 0xc8, 0xd9, 0xe8, 0x31, 0xd2][..], 3),
        ("simd", &[0x01, 0xc8, 0x0f, 0x28, 0xc1, 0x31, 0xd2][..], 3),
        ("status", &[0x01, 0xc8, 0xdf, 0xe0, 0x9e][..], 3),
        (
            "status-rhs",
            &[0x01, 0xc3, 0xdf, 0xe0, 0x0f, 0x90, 0xc2][..],
            3,
        ),
        ("wait", &[0x01, 0xc8, 0x9b][..], 2),
        ("load", &[0x01, 0xc8, 0xd9, 0x03][..], 2),
        ("integer-compare", &[0x01, 0xc8, 0xdb, 0xf1][..], 2),
    ] {
        memory.write(GuestAddress(0x1000), code).unwrap();
        fs::write(
            scratch.join(format!("{name}.wasm")),
            compile(&memory, GuestAddress(0x1000), count).unwrap().wasm,
        )
        .unwrap();
    }
    let result = Command::new("node").arg("--input-type=module").arg("-e").arg(r#"
import assert from 'node:assert/strict';
import fs from 'node:fs';
const memory=new WebAssembly.Memory({initial:2}),ram=new Uint8Array(memory.buffer),d=new DataView(memory.buffer),ctx=0x8000,view=0x9000,fp=0xa000,simd=0xa100;
const word=(at,value)=>d.setUint32(ctx+at,value>>>0,true);let reads=0;
const imports={env:{memory},cpu:{
 read(p,a,w){assert.deepEqual([p,a>>>0,w],[ctx,0x3000,4]);reads++;word(52,3);word(60,0x3000);word(64,0x10104);word(68,4);return 0;},
 write(){throw Error('no guest store in these numeric consumers');}
}};
const units={};for(const name of['constant','simd','status','status-rhs','wait','load','integer-compare'])units[name]=(await WebAssembly.instantiate(fs.readFileSync(`${process.argv[1]}/${name}.wasm`),imports)).instance.exports.run;
function scenario(name,quota,expect,{pending=false,rhs=false}={}){
 for(let i=0;i<ram.length;i++)ram[i]=(i*41+13)&255;ram.fill(0,ctx,ctx+160);ram.fill(0,view,view+40);
 word(0,1);word(4,160);[0x7fffffff,1,0x12345678,rhs?1:0x3000,0x4800,10,11,12].forEach((v,i)=>word(8+4*i,v));word(40,0x1000);word(44,0x603);word(48,quota);word(52,0xabcdef01);word(56,0xffffffff);word(60,0x11223344);word(64,0x55667788);word(68,0x12345678);word(124,view);word(152,63);
 d.setUint32(view,1,true);d.setUint32(view+4,40,true);d.setUint32(view+8,fp,true);d.setUint32(view+36,simd,true);[0x23,0x1b,0x23,0x23,0x3b,0x43].forEach((v,i)=>d.setUint32(view+12+4*i,v,true));
 for(const[at,v]of[[0,pending?0x003e:0x003f],[2,pending?0x4501:0x4500],[4,0xffff],[6,0x321],[16,0x71],[18,0x72]])d.setUint16(fp+at,v,true);d.setUint32(fp+8,0x12345678,true);d.setUint32(fp+12,0x87654321,true);d.setUint32(simd,0x1f80,true);
 const initial=ram.slice(),expected=initial.slice(),out=new DataView(expected.buffer),set=(at,v)=>out.setUint32(ctx+at,v>>>0,true);reads=0;
 for(const[at,v]of[[8,expect.eax??0x80000000],[16,expect.edx??0x12345678],[40,expect.pc],[44,expect.flags??0xe96],[48,quota-expect.steps],[52,expect.status],[56,(0xffffffff+expect.steps)>>>0]])set(at,v);
 if(expect.ebx!==undefined)set(20,expect.ebx);
 if(expect.loaded){out.setUint16(fp+2,0x7d00,true);out.setUint16(fp+4,0x3fff,true);out.setUint16(fp+6,0x1e8,true);out.setUint32(fp+8,0x1002,true);out.setUint16(fp+16,0x1b,true);out.setBigUint64(fp+90,0x8000000000000000n,true);out.setUint16(fp+98,0x3fff,true);}
 if(expect.simd)expected.set(initial.slice(simd+20,simd+36),simd+4);
 if(name==='load'){set(60,0x3000);set(64,0x10104);set(68,4);}
 assert.equal(units[name](ctx),expect.status,name);assert.deepEqual(ram,expected,`${name}/quota${quota}: full numeric, scalar, RAM and retirement state`);assert.equal(reads,name==='load'?1:0);
}
scenario('constant',1,{pc:0x1002,steps:1,status:2});
scenario('constant',2,{pc:0x1004,steps:2,status:2,loaded:true});
scenario('constant',3,{pc:0x1006,steps:3,status:0,flags:0x646,edx:0,loaded:true});
scenario('simd',2,{pc:0x1005,steps:2,status:2,simd:true});
scenario('simd',3,{pc:0x1007,steps:3,status:0,flags:0x646,edx:0,simd:true});
scenario('status',2,{pc:0x1004,steps:2,status:2,eax:0x80004500});
scenario('status',3,{pc:0x1005,steps:3,status:0,eax:0x80004500,flags:0xe47});
scenario('status-rhs',3,{pc:0x1007,steps:3,status:0,eax:0x7fff4500,ebx:0x80000000,edx:0x12345601},{rhs:true});
scenario('wait',2,{pc:0x1003,steps:2,status:0});
scenario('wait',2,{pc:0x1002,steps:1,status:7},{pending:true});
scenario('load',2,{pc:0x1002,steps:1,status:3});
scenario('integer-compare',2,{pc:0x1002,steps:1,status:7});
console.log('pending integer FLAGS survive exact x87/SIMD effects, later overwrite, short quotas, FNSTSW/SAHF observation, waiting fallback, operand fault and FCOMI service');
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
fn generated_sahf_and_narrow_producers_preserve_only_the_required_old_flags() {
    use std::{
        fs,
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };
    let scratch = std::env::temp_dir().join(format!(
        "ring3-runtime-partial-flag-observers-{}-{}",
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
    for (name, code, count) in [
        ("sahf-one", &[0x01, 0xcb, 0x9e][..], 2),
        ("sahf-zero", &[0x31, 0xd2, 0x9e][..], 2),
        (
            "adc-byte",
            &[0x01, 0xc2, 0x10, 0xe0, 0x0f, 0x92, 0xc3, 0x0f, 0x90, 0xc7][..],
            4,
        ),
        (
            "sbb-word",
            &[
                0x29, 0xc2, 0x66, 0x19, 0xc0, 0x0f, 0x92, 0xc3, 0x0f, 0x90, 0xc7,
            ][..],
            4,
        ),
        ("read-fault", &[0x01, 0xca, 0x12, 0x06][..], 2),
        ("write-fault", &[0x01, 0xca, 0x00, 0x06][..], 2),
    ] {
        memory.write(GuestAddress(0x1000), code).unwrap();
        fs::write(
            scratch.join(format!("{name}.wasm")),
            compile(&memory, GuestAddress(0x1000), count).unwrap().wasm,
        )
        .unwrap();
    }
    let result = Command::new("node").arg("--input-type=module").arg("-e").arg(r#"
import assert from 'node:assert/strict';
import fs from 'node:fs';
const memory=new WebAssembly.Memory({initial:2}),ram=new Uint8Array(memory.buffer),d=new DataView(memory.buffer),ctx=0x8000;
const word=(at,v)=>d.setUint32(ctx+at,v>>>0,true);
let mode='',reads=[],writes=[];
const imports={env:{memory},cpu:{
 read(p,a,w){assert.equal(p,ctx);assert.equal(a>>>0,0x6000);assert.equal(w,mode==='write-fault'?0x101:1);reads.push(ram.slice());
  if(mode==='write-fault'){word(52,0);return 1;}
  word(52,3);word(60,a);word(64,0x10101);word(68,1);return 0;
 },
 write(p,a,w,v){assert.deepEqual([p,a>>>0,w,v>>>0],[ctx,0x6000,1,0x80]);writes.push(ram.slice());
  word(52,3);word(60,a);word(64,0x20201);word(68,1);return 3;
 }
}};
const runs={};for(const name of['sahf-one','sahf-zero','adc-byte','sbb-word','read-fault','write-fault'])runs[name]=(await WebAssembly.instantiate(fs.readFileSync(`${process.argv[1]}/${name}.wasm`),imports)).instance.exports.run;
function scenario(name,input,expect){
 for(let i=0;i<ram.length;i++)ram[i]=(i*53+17)&255;ram.fill(0,ctx,ctx+160);
 word(0,1);word(4,160);[0x12340056,1,0xffffffff,0x7fffffff,0x4800,10,0x6000,12].forEach((v,i)=>word(8+4*i,v));
 for(const[index,v]of Object.entries(input.regs??{}))word(8+4*Number(index),v);
 word(40,input.pc??0x1000);word(44,input.flags??0x200603);word(48,input.quota);word(56,0xfffffffe);word(60,0x11223344);word(64,0x55667788);word(68,0x12345678);word(152,63);
 const before=ram.slice(),expected=before.slice(),view=new DataView(expected.buffer),set=(at,v)=>view.setUint32(ctx+at,v>>>0,true);mode=name;reads=[];writes=[];
 for(const[at,v]of[[40,expect.pc],[44,expect.flags],[48,input.quota-expect.steps],[52,expect.status??0],[56,(0xfffffffe+expect.steps)>>>0],...(expect.fields??[])])set(at,v);
 for(const[index,v]of Object.entries(expect.regs??{}))set(8+4*Number(index),v);
 assert.equal(runs[name](ctx),expect.status??0,name);assert.deepEqual(ram,expected,`${name} full state`);
 assert.equal(reads.length,expect.reads??0);assert.equal(writes.length,expect.writes??0);
 for(const observed of [...reads,...writes])assert.deepEqual(observed,before,'a faultable narrow producer retains the old helper-visible publication');
}
scenario('sahf-one',{quota:1},{pc:0x1002,steps:1,status:2,flags:0x200e96,regs:{3:0x80000000}});
scenario('sahf-one',{quota:2},{pc:0x1003,steps:2,flags:0x200e02,regs:{3:0x80000000}});
scenario('sahf-zero',{quota:1,flags:0x200e03,regs:{0:0x1234ff56}},{pc:0x1002,steps:1,status:2,flags:0x200646,regs:{2:0}});
scenario('sahf-zero',{quota:2,flags:0x200e03,regs:{0:0x1234ff56}},{pc:0x1003,steps:2,flags:0x2006d7,regs:{2:0}});
scenario('sahf-one',{pc:0x1002,quota:1,flags:0x200e03},{pc:0x1003,steps:1,flags:0x200e02});
scenario('sahf-zero',{pc:0x1002,quota:1,flags:0x200e03,regs:{0:0x1234ff56}},{pc:0x1003,steps:1,flags:0x200ed7});
scenario('adc-byte',{quota:1,flags:0x200602,regs:{0:0x1234aa7f,3:0x89abccdd}},{pc:0x1002,steps:1,status:2,flags:0x200617,regs:{2:0x1234aa7e}});
scenario('adc-byte',{quota:4,flags:0x200602,regs:{0:0x1234aa7f,3:0x89abccdd}},{pc:0x100a,steps:4,flags:0x200613,regs:{0:0x1234aa2a,2:0x1234aa7e,3:0x89ab0001}});
scenario('sbb-word',{quota:1,flags:0x200602,regs:{0:0x12348000,2:0,3:0x89abccdd}},{pc:0x1002,steps:1,status:2,flags:0x200687,regs:{2:0xedcb8000}});
scenario('sbb-word',{quota:4,flags:0x200602,regs:{0:0x12348000,2:0,3:0x89abccdd}},{pc:0x100b,steps:4,flags:0x200697,regs:{0:0x1234ffff,2:0xedcb8000,3:0x89ab0001}});
scenario('read-fault',{quota:2,regs:{0:0x1234ff7f,2:0x7fffffff}},{pc:0x1002,steps:1,status:3,flags:0x200e96,regs:{2:0x80000000},reads:1,fields:[[60,0x6000],[64,0x10101],[68,1]]});
scenario('write-fault',{quota:2,regs:{0:0x1234ff7f,2:0x7fffffff}},{pc:0x1002,steps:1,status:3,flags:0x200e96,regs:{2:0x80000000},reads:1,writes:1,fields:[[60,0x6000],[64,0x20201],[68,1]]});
console.log('SAHF preserves only old OF; narrow carry aliases, incoming landings, partial registers and read/write fault frontiers retain complete state');
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
fn generated_flag_recipe_joins_preserve_values_cold_entries_and_loop_frontiers() {
    use std::{
        fs,
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };
    let scratch = std::env::temp_dir().join(format!(
        "ring3-runtime-flag-joins-{}-{}",
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
    for (name, opcode) in [("same", 0xc0), ("conflict", 0xe8)] {
        let mut code = [0x90; 42];
        code[..7].copy_from_slice(&[0x74, 0x0e, 0x83, 0xc0, 1, 0xeb, 0x19]);
        code[16..21].copy_from_slice(&[0x83, opcode, 1, 0xeb, 0x0b]);
        code[32..42].copy_from_slice(&[0x8b, 0x16, 0x0f, 0x92, 0xc3, 0x0f, 0x90, 0xc7, 0x0f, 0x0b]);
        memory.write(GuestAddress(0x1000), &code).unwrap();
        let unit = compile_with_gates(
            &memory,
            GuestAddress(0x1000),
            9,
            &[(GuestAddress(0x1028), 23)],
        )
        .unwrap();
        assert_eq!(
            unit.entries,
            [
                0x1000, 0x1002, 0x1005, 0x1010, 0x1013, 0x1020, 0x1022, 0x1025, 0x1028
            ]
        );
        fs::write(scratch.join(format!("{name}.wasm")), unit.wasm).unwrap();
    }
    let mut code = [0x90; 25];
    code[..2].copy_from_slice(&[0xeb, 0x0e]);
    code[16..25].copy_from_slice(&[0x83, 0xc0, 1, 0x8b, 0x16, 0xe2, 0xf9, 0x0f, 0x0b]);
    memory.write(GuestAddress(0x1000), &code).unwrap();
    let unit = compile_with_gates(
        &memory,
        GuestAddress(0x1000),
        5,
        &[(GuestAddress(0x1017), 23)],
    )
    .unwrap();
    assert_eq!(unit.entries, [0x1000, 0x1010, 0x1013, 0x1015, 0x1017]);
    fs::write(scratch.join("loop.wasm"), unit.wasm).unwrap();
    let mut code = [0x90; 29];
    code[..7].copy_from_slice(&[0x74, 0x0e, 0x83, 0xe8, 2, 0xeb, 0x0c]);
    code[16..29].copy_from_slice(&[
        0x83, 0xc0, 1, 0x8b, 0x16, 0x0f, 0x92, 0xc3, 0x0f, 0x90, 0xc7, 0x0f, 0x0b,
    ]);
    memory.write(GuestAddress(0x1000), &code).unwrap();
    let unit = compile_with_gates(
        &memory,
        GuestAddress(0x1000),
        8,
        &[(GuestAddress(0x101b), 23)],
    )
    .unwrap();
    assert_eq!(
        unit.entries,
        [
            0x1000, 0x1002, 0x1005, 0x1010, 0x1013, 0x1015, 0x1018, 0x101b
        ]
    );
    fs::write(scratch.join("interior-conflict.wasm"), unit.wasm).unwrap();
    let result = Command::new("node").arg("--input-type=module").arg("-e").arg(r#"
import assert from 'node:assert/strict';
import fs from 'node:fs';
const memory=new WebAssembly.Memory({initial:2}),ram=new Uint8Array(memory.buffer),d=new DataView(memory.buffer),ctx=0x8000;
const word=(at,v)=>d.setUint32(ctx+at,v>>>0,true);
let reads=[],faultAt=0,throwAt=0;
const imports={env:{memory},cpu:{read(p,a,w){
 assert.deepEqual([p,a>>>0,w],[ctx,0x6000,4]);reads.push(ram.slice());
 if(reads.length===throwAt)throw Error('authored-join-import-trap');
 if(reads.length===faultAt){word(52,3);word(60,a);word(64,0x10104);word(68,4);return 0;}
 word(52,0);return 0x76543210;
},write(){throw Error('the join fixtures never write guest memory');}}};
const runs={};for(const name of['same','conflict','loop','interior-conflict'])runs[name]=(await WebAssembly.instantiate(fs.readFileSync(`${process.argv[1]}/${name}.wasm`),imports)).instance.exports.run;
function scenario(name,input,expect){
 for(let i=0;i<ram.length;i++)ram[i]=(i*59+23)&255;ram.fill(0,ctx,ctx+160);
 word(0,1);word(4,160);[0x7fffffff,2,0xdeadbeef,0xa5a5ff7e,0x4800,10,0x6000,12].forEach((v,i)=>word(8+4*i,v));
 for(const[index,v]of Object.entries(input.regs??{}))word(8+4*Number(index),v);
 word(40,input.pc??0x1000);word(44,input.flags??0x200603);word(48,input.quota);word(56,0xfffffffe);word(60,0x11223344);word(64,0x55667788);word(68,0x12345678);word(152,63);
 const before=ram.slice();reads=[];faultAt=input.faultAt??0;throwAt=input.throwAt??0;
 if(throwAt){assert.throws(()=>runs[name](ctx),/authored-join-import-trap/);assert.deepEqual(ram,before);assert.equal(reads.length,throwAt);return;}
 const expected=before.slice(),view=new DataView(expected.buffer),set=(at,v)=>view.setUint32(ctx+at,v>>>0,true);
 for(const[at,v]of[[40,expect.pc],[44,expect.flags],[48,input.quota-expect.steps],[52,expect.status],[56,(0xfffffffe+expect.steps)>>>0],...(expect.fields??[])])set(at,v);
 for(const[index,v]of Object.entries(expect.regs??{}))set(8+4*Number(index),v);
 assert.equal(runs[name](ctx),expect.status);assert.deepEqual(ram,expected,`${name}/entry${(input.pc??0x1000).toString(16)}/quota${input.quota}: full state`);assert.equal(reads.length,expect.reads??0);
 for(const observed of reads)assert.deepEqual(observed,before,'a recipe edge does not publish scalar state before a read helper');
}
scenario('same',{quota:7,flags:0x200643,regs:{0:0xffffffff}},{pc:0x1028,steps:6,status:5,flags:0x200657,regs:{0:0,2:0x76543210,3:0xa5a50001},reads:1,fields:[[68,23]]});
scenario('same',{quota:7},{pc:0x1028,steps:6,status:5,flags:0x200e96,regs:{0:0x80000000,2:0x76543210,3:0xa5a50100},reads:1,fields:[[68,23]]});
scenario('same',{quota:3,flags:0x200643,regs:{0:0xffffffff}},{pc:0x1020,steps:3,status:2,flags:0x200657,regs:{0:0}});
scenario('same',{quota:3},{pc:0x1020,steps:3,status:2,flags:0x200e96,regs:{0:0x80000000}});
scenario('same',{quota:4},{pc:0x1022,steps:4,status:2,flags:0x200e96,regs:{0:0x80000000,2:0x76543210},reads:1});
scenario('same',{quota:4,faultAt:1},{pc:0x1020,steps:3,status:3,flags:0x200e96,regs:{0:0x80000000},reads:1,fields:[[60,0x6000],[64,0x10104],[68,4]]});
scenario('same',{quota:7,throwAt:1});
scenario('same',{pc:0x1020,quota:4,flags:0x200e03,regs:{0:0x11223344}},{pc:0x1028,steps:3,status:5,flags:0x200e03,regs:{2:0x76543210,3:0xa5a50101},reads:1,fields:[[68,23]]});
scenario('same',{pc:0x1020,quota:0,flags:0x200e03,regs:{0:0x11223344}},{pc:0x1020,steps:0,status:2,flags:0x200e03});
scenario('conflict',{quota:7,flags:0x200643,regs:{0:0}},{pc:0x1028,steps:6,status:5,flags:0x200697,regs:{0:0xffffffff,2:0x76543210,3:0xa5a50001},reads:1,fields:[[68,23]]});
scenario('conflict',{quota:7},{pc:0x1028,steps:6,status:5,flags:0x200e96,regs:{0:0x80000000,2:0x76543210,3:0xa5a50100},reads:1,fields:[[68,23]]});
scenario('conflict',{quota:3,flags:0x200643,regs:{0:0}},{pc:0x1020,steps:3,status:2,flags:0x200697,regs:{0:0xffffffff}});
scenario('loop',{quota:1,regs:{0:0xfffffffe}},{pc:0x1010,steps:1,status:2,flags:0x200603});
scenario('loop',{quota:4,regs:{0:0xfffffffe}},{pc:0x1010,steps:4,status:2,flags:0x200686,regs:{0:0xffffffff,1:1,2:0x76543210},reads:1});
scenario('loop',{quota:5,regs:{0:0xfffffffe}},{pc:0x1013,steps:5,status:2,flags:0x200657,regs:{0:0,1:1,2:0x76543210},reads:1});
scenario('loop',{quota:8,regs:{0:0xfffffffe}},{pc:0x1017,steps:7,status:5,flags:0x200657,regs:{0:0,1:0,2:0x76543210},reads:2,fields:[[68,23]]});
scenario('loop',{quota:6,faultAt:2,regs:{0:0xfffffffe}},{pc:0x1013,steps:5,status:3,flags:0x200657,regs:{0:0,1:1,2:0x76543210},reads:2,fields:[[60,0x6000],[64,0x10104],[68,4]]});
scenario('interior-conflict',{quota:6,flags:0x200643},{pc:0x101b,steps:5,status:5,flags:0x200e96,regs:{0:0x80000000,2:0x76543210,3:0xa5a50100},reads:1,fields:[[68,23]]});
scenario('interior-conflict',{quota:7,regs:{0:0}},{pc:0x101b,steps:6,status:5,flags:0x200693,regs:{0:0xfffffffe,2:0x76543210,3:0xa5a50001},reads:1,fields:[[68,23]]});
scenario('interior-conflict',{quota:2,flags:0x200643},{pc:0x1013,steps:2,status:2,flags:0x200e96,regs:{0:0x80000000}});
scenario('interior-conflict',{quota:3,regs:{0:0}},{pc:0x1013,steps:3,status:2,flags:0x200693,regs:{0:0xfffffffe}});
scenario('interior-conflict',{quota:3,flags:0x200643,faultAt:1},{pc:0x1013,steps:2,status:3,flags:0x200e96,regs:{0:0x80000000},reads:1,fields:[[60,0x6000],[64,0x10104],[68,4]]});
scenario('interior-conflict',{quota:4,faultAt:1,regs:{0:0}},{pc:0x1013,steps:3,status:3,flags:0x200693,regs:{0:0xfffffffe},reads:1,fields:[[60,0x6000],[64,0x10104],[68,4]]});
scenario('interior-conflict',{pc:0x1013,quota:4,flags:0x200e03,regs:{0:0x11223344}},{pc:0x101b,steps:3,status:5,flags:0x200e03,regs:{2:0x76543210,3:0xa5a50101},reads:1,fields:[[68,23]]});
console.log('equal and conflicting flag recipes, canonical cold interiors, loop fixed points, next-entry budget/fault guards and unchanged helper publication passed');
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
fn generated_register_flag_dependencies_preserve_clobbers_raw_bits_and_conditions() {
    use std::{
        fs,
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };
    let scratch = std::env::temp_dir().join(format!(
        "ring3-runtime-register-flags-{}-{}",
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
    for (name, code, count) in [
        (
            "result-full",
            &[
                0x01, 0xc8, 0xb8, 0x44, 0x33, 0x22, 0x11, 0x0f, 0x90, 0xc2, 0x0f, 0x92, 0xc6, 0x9f,
            ][..],
            5,
        ),
        (
            "result-byte",
            &[0x01, 0xc8, 0xb0, 0x44, 0x0f, 0x90, 0xc2, 0x9f][..],
            4,
        ),
        (
            "cmp-left",
            &[
                0x39, 0xc8, 0xb8, 0x44, 0x33, 0x22, 0x11, 0x0f, 0x9c, 0xc2, 0x0f, 0x92, 0xc6, 0x9f,
            ][..],
            5,
        ),
        (
            "cmp-right",
            &[
                0x39, 0xc8, 0xb1, 0x44, 0x0f, 0x9c, 0xc2, 0x0f, 0x92, 0xc6, 0x9f,
            ][..],
            5,
        ),
        (
            "test-left",
            &[
                0x85, 0xc8, 0xb8, 0x44, 0x33, 0x22, 0x11, 0x0f, 0x98, 0xc2, 0x0f, 0x94, 0xc6, 0x9f,
            ][..],
            5,
        ),
        (
            "carry-df",
            &[
                0x11, 0xc8, 0xfd, 0xba, 0, 0, 0, 0, 0x0f, 0x92, 0xc2, 0x9f, 0xfc,
            ][..],
            6,
        ),
        ("cmp-inc", &[0x39, 0xc8, 0x42][..], 2),
        ("cmp-dec", &[0x39, 0xc8, 0x4a][..], 2),
        ("opaque", &[0x90, 0xfd, 0xfc][..], 3),
    ] {
        memory.write(GuestAddress(0x1000), code).unwrap();
        fs::write(
            scratch.join(format!("{name}.wasm")),
            compile(&memory, GuestAddress(0x1000), count).unwrap().wasm,
        )
        .unwrap();
    }
    for (name, jump) in [("unsigned", 0x72), ("signed", 0x7c)] {
        let mut code = [0x90; 23];
        code[..11].copy_from_slice(&[0x39, 0xc8, jump, 0x0c, 0xba, 0, 0, 0, 0, 0xeb, 0x0a]);
        code[16..].copy_from_slice(&[0xba, 1, 0, 0, 0, 0x0f, 0x0b]);
        memory.write(GuestAddress(0x1000), &code).unwrap();
        fs::write(
            scratch.join(format!("{name}.wasm")),
            compile_with_gates(
                &memory,
                GuestAddress(0x1000),
                6,
                &[(GuestAddress(0x1015), 23)],
            )
            .unwrap()
            .wasm,
        )
        .unwrap();
    }
    let result = Command::new("node").arg("--input-type=module").arg("-e").arg(r#"
import assert from 'node:assert/strict';
import fs from 'node:fs';
const memory=new WebAssembly.Memory({initial:2}),ram=new Uint8Array(memory.buffer),d=new DataView(memory.buffer),ctx=0x8000;
const word=(at,v)=>d.setUint32(ctx+at,v>>>0,true);
const imports={env:{memory},cpu:{read(){throw Error('unexpected read in register dependency fixture');},write(){throw Error('unexpected write in register dependency fixture');}}};
const runs={};for(const name of['result-full','result-byte','cmp-left','cmp-right','test-left','carry-df','cmp-inc','cmp-dec','opaque','unsigned','signed'])runs[name]=(await WebAssembly.instantiate(fs.readFileSync(`${process.argv[1]}/${name}.wasm`),imports)).instance.exports.run;
function scenario(name,input,expect){
 for(let i=0;i<ram.length;i++)ram[i]=(i*47+5)&255;ram.fill(0,ctx,ctx+160);
 word(0,1);word(4,160);[0x7fffffff,1,0x1234abcd,0x12345678,0x4800,10,0x6000,12].forEach((v,i)=>word(8+i*4,v));
 for(const[index,v]of Object.entries(input.regs??{}))word(8+4*Number(index),v);
 word(40,input.pc??0x1000);word(44,input.flags??0x200603);word(48,input.quota);word(52,0xabcdef01);word(56,0xfffffffe);word(60,0x11223344);word(64,0x55667788);word(68,0x12345678);word(152,63);
 const expected=ram.slice(),view=new DataView(expected.buffer),set=(at,v)=>view.setUint32(ctx+at,v>>>0,true);
 for(const[at,v]of[[40,expect.pc],[44,expect.flags],[48,input.quota-expect.steps],[52,expect.status??0],[56,(0xfffffffe+expect.steps)>>>0],...(expect.fields??[])])set(at,v);
 for(const[index,v]of Object.entries(expect.regs??{}))set(8+4*Number(index),v);
 assert.equal(runs[name](ctx),expect.status??0,name);assert.deepEqual(ram,expected,`${name}/entry${(input.pc??0x1000).toString(16)}/quota${input.quota}: full state`);
}
scenario('result-full',{quota:5},{pc:0x100e,steps:5,flags:0x200e96,regs:{0:0x11229644,2:0x12340001}});
scenario('result-full',{pc:0x1002,quota:0,flags:0x80000800},{pc:0x1002,steps:0,flags:0x80000800,status:2});
scenario('result-full',{pc:0x1002,quota:1,flags:0x80000800},{pc:0x1007,steps:1,flags:0x80000800,status:2,regs:{0:0x11223344}});
scenario('result-byte',{quota:4},{pc:0x1008,steps:4,flags:0x200e96,regs:{0:0x80009644,2:0x1234ab01}});
scenario('cmp-left',{quota:5,regs:{0:0x80000000}},{pc:0x100e,steps:5,flags:0x200e16,regs:{0:0x11221644,2:0x12340001}});
scenario('cmp-right',{quota:5,regs:{1:0xffffffff}},{pc:0x100b,steps:5,flags:0x200e87,regs:{0:0x7fff87ff,1:0xffffff44,2:0x12340100}});
scenario('test-left',{quota:5,regs:{0:0x80000000,1:0xffffffff}},{pc:0x100e,steps:5,flags:0x200686,regs:{0:0x11228644,2:0x12340001}});
scenario('cmp-inc',{quota:2,flags:0x200602,regs:{0:0,1:1,2:0x7fffffff}},{pc:0x1003,steps:2,flags:0x200e97,regs:{2:0x80000000}});
scenario('cmp-inc',{quota:2,flags:0x200603,regs:{0:1,1:0,2:0xffffffff}},{pc:0x1003,steps:2,flags:0x200656,regs:{2:0}});
scenario('cmp-dec',{quota:2,flags:0x200602,regs:{0:0,1:1,2:0x80000000}},{pc:0x1003,steps:2,flags:0x200e17,regs:{2:0x7fffffff}});
scenario('cmp-dec',{quota:2,flags:0x200603,regs:{0:1,1:0,2:0}},{pc:0x1003,steps:2,flags:0x200696,regs:{2:0xffffffff}});
scenario('carry-df',{quota:1,flags:0x200203,regs:{0:0xffffffff,1:0}},{pc:0x1002,steps:1,flags:0x200257,status:2,regs:{0:0}});
scenario('carry-df',{quota:2,flags:0x200203,regs:{0:0xffffffff,1:0}},{pc:0x1003,steps:2,flags:0x200657,status:2,regs:{0:0}});
scenario('carry-df',{quota:6,flags:0x200203,regs:{0:0xffffffff,1:0}},{pc:0x100d,steps:6,flags:0x200257,regs:{0:0x5700,2:1}});
for(const[quota,pc,flags,status]of[
 [0,0x1000,0x80000800,2],[1,0x1001,0x80000800,2],[2,0x1002,0x80000c00,2],[3,0x1003,0x80000800,0]
])scenario('opaque',{quota,flags:0x80000800},{pc,flags,steps:quota,status});
scenario('opaque',{pc:0x1001,quota:1,flags:0x80000800},{pc:0x1002,flags:0x80000c00,steps:1,status:2});
for(const[name,eax,ecx,edx,steps,flags]of[
 ['unsigned',0x80000000,1,0,4,0x200e16],['signed',0x80000000,1,1,3,0x200e16],
 ['unsigned',0x7fffffff,0xffffffff,1,3,0x200e87],['signed',0x7fffffff,0xffffffff,0,4,0x200e87]
])scenario(name,{quota:5,regs:{0:eax,1:ecx}},{pc:0x1015,steps,flags,status:5,regs:{2:edx},fields:[[68,23]]});
scenario('unsigned',{pc:0x1002,quota:1,flags:0x80000801,regs:{0:0x80000000}},{pc:0x1010,steps:1,flags:0x80000801,status:2});
scenario('signed',{pc:0x1002,quota:1,flags:0x80000000,regs:{0:0x80000000}},{pc:0x1004,steps:1,flags:0x80000000,status:2});
console.log('literal register FLAGS: result/CMP/TEST dependencies, partial clobbers, carry through DF, opaque raw bits and signed/unsigned branch edges passed');
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
