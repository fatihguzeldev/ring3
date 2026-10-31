use super::*;
use crate::memory::{PageRange, Permissions};

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
