use super::*;
use crate::memory::{PageRange, Permissions};

#[test]
fn generated_exchanges_capture_aliases_and_commit_memory_before_registers() {
    use std::{
        fs,
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };
    let scratch = std::env::temp_dir().join(format!(
        "ring3-runtime-exchange-{}-{}",
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
    for bits in [8, 16, 32] {
        for locked in [false, true] {
            let mut code = if locked { vec![0xf0] } else { Vec::new() };
            if bits == 16 {
                code.push(0x66);
            }
            code.extend([if bits == 8 { 0x86 } else { 0x87 }, 0x03]);
            memory.write(GuestAddress(0x1000), &code).unwrap();
            let unit = compile(&memory, GuestAddress(0x1000), 1).unwrap();
            fs::write(
                scratch.join(format!(
                    "{}mem{bits}.wasm",
                    if locked { "locked-" } else { "" }
                )),
                &unit.wasm,
            )
            .unwrap();
        }
    }
    for (name, bytes) in [
        ("al-ah", &[0x86, 0xe0][..]),
        ("ah-al", &[0x86, 0xc4][..]),
        ("al-al", &[0x86, 0xc0][..]),
        ("ah-bh", &[0x86, 0xfc][..]),
        ("al-dl", &[0x86, 0xd0][..]),
        ("ax-dx", &[0x66, 0x87, 0xd0][..]),
        ("ax-ax", &[0x66, 0x87, 0xc0][..]),
        ("word-short", &[0x66, 0x92][..]),
        ("eax-edx", &[0x87, 0xd0][..]),
        ("eax-eax", &[0x87, 0xc0][..]),
        ("dword-short", &[0x92][..]),
        ("eax-ah-memory", &[0x86, 0x20][..]),
        ("edx-dx-memory", &[0x66, 0x87, 0x12][..]),
        ("edx-edx-memory", &[0x87, 0x12][..]),
        ("fs-memory", &[0x64, 0x86, 0x23][..]),
        ("address16-word", &[0x67, 0x66, 0x87, 0x17][..]),
        ("esp-memory", &[0x87, 0x24, 0x24][..]),
    ] {
        memory.write(GuestAddress(0x1000), bytes).unwrap();
        let unit = compile(&memory, GuestAddress(0x1000), 1).unwrap();
        fs::write(scratch.join(format!("{name}.wasm")), &unit.wasm).unwrap();
    }
    let result = Command::new("node").arg("--input-type=module").arg("-e").arg(r#"
import assert from 'node:assert/strict';
import fs from 'node:fs';
const root=process.argv[1],memory=new WebAssembly.Memory({initial:1}),data=new DataView(memory.buffer),ptr=0x8000;
const word=(at,value)=>data.setUint32(ptr+at,value>>>0,true),get=at=>data.getUint32(ptr+at,true),regs=()=>Array.from({length:8},(_,i)=>get(8+4*i));
let words=new Map(),reads=[],writes=[],readFault=false,writeFault=false,smc=false;
const imports={env:{memory},cpu:{read(p,address,width){address>>>=0;reads.push([address,width]);word(52,0);if(readFault){word(52,3);word(60,address);return 0;}return(words.get(address)??0)&((width&255)===1?255:(width&255)===2?0xffff:-1);},write(p,address,width,value){address>>>=0;value>>>=0;writes.push([address,width,value]);if(writeFault){word(52,3);word(60,address);return 3;}words.set(address,(value&(width===1?255:width===2?0xffff:-1))>>>0);word(52,smc?6:0);return smc?6:0;}}};
const functions=new Map(),load=async name=>{if(!functions.has(name))functions.set(name,(await WebAssembly.instantiate(fs.readFileSync(`${root}/${name}.wasm`),imports)).instance.exports.run);return functions.get(name);};
function init(flags=0xcd7){new Uint8Array(memory.buffer).fill(0);word(0,1);word(4,160);[0x11223344,0x8899aabb,0xa1b2c3d4,0x55667788,0x4800,0x13579bdf,0x2468ace0,0x0f1e2d3c].forEach((v,i)=>word(8+4*i,v));word(40,0x1000);word(44,flags);word(48,1);word(152,0x1f);words=new Map();reads=[];writes=[];readFault=writeFault=smc=false;}
for(const[name,outEax,outEdx,outEbx]of[
['al-ah',0x11224433,0xa1b2c3d4,0x55667788],['ah-al',0x11224433,0xa1b2c3d4,0x55667788],['al-al',0x11223344,0xa1b2c3d4,0x55667788],
['ah-bh',0x11227744,0xa1b2c3d4,0x55663388],['al-dl',0x112233d4,0xa1b2c344,0x55667788],
['ax-dx',0x1122c3d4,0xa1b23344,0x55667788],['ax-ax',0x11223344,0xa1b2c3d4,0x55667788],['word-short',0x1122c3d4,0xa1b23344,0x55667788],
['eax-edx',0xa1b2c3d4,0x11223344,0x55667788],['eax-eax',0x11223344,0xa1b2c3d4,0x55667788],['dword-short',0xa1b2c3d4,0x11223344,0x55667788]]){const fn=await load(name);for(const flags of[2,0xcd7]){init(flags);const before=regs(),expected=[...before];expected[0]=outEax;expected[2]=outEdx;expected[3]=outEbx;assert.equal(fn(ptr),0,name);assert.deepEqual(regs(),expected,name);assert.equal(get(44),flags);assert.equal(get(56),1);assert.deepEqual([reads,writes],[[],[]]);}}
for(const bits of[8,16,32])for(const locked of[false,true]){const name=`${locked?'locked-':''}mem${bits}`,fn=await load(name),mask=2**bits-1,source=bits===8?0x44:bits===16?0x3344:0x11223344,bytes=bits/8,next=0x1000+(bits===16?3:2)+Number(locked);for(const value of[0,1,2**(bits-1),mask,source])for(const flags of[2,0xcd7]){init(flags);word(20,0x5000);words.set(0x5000,value);const before=regs(),expected=[...before];expected[0]=bits===8?(0x11223300|value)>>>0:bits===16?(0x11220000|value)>>>0:value;assert.equal(fn(ptr),0,name);assert.deepEqual(regs(),expected,name);assert.equal(get(44),flags);assert.equal(get(40),next);assert.equal(words.get(0x5000),source);assert.deepEqual(reads,[[0x5000,bytes|0x100]]);assert.deepEqual(writes,[[0x5000,bytes,source]]);assert.equal(get(56),1);assert.equal(get(48),0);}
for(const mode of['read','write']){init();word(20,0x5000);words.set(0x5000,0);const before=regs();readFault=mode==='read';writeFault=mode==='write';assert.equal(fn(ptr),3,`${name} ${mode}fault`);assert.deepEqual(regs(),before);assert.equal(words.get(0x5000),0);assert.equal(get(44),0xcd7);assert.equal(get(40),0x1000);assert.equal(get(56),0);assert.equal(get(48),1);assert.equal(writes.length,mode==='write'?1:0);}
for(const value of[2**(bits-1),source]){init();word(20,0x5000);words.set(0x5000,value);smc=true;assert.equal(fn(ptr),6,name);assert.equal(get(8),bits===8?(0x11223300|value)>>>0:bits===16?(0x11220000|value)>>>0:value);assert.equal(words.get(0x5000),source);assert.equal(get(44),0xcd7);assert.equal(get(40),next);assert.equal(get(56),1);assert.equal(get(48),0);assert.equal(writes.length,1);}}
init();word(8,0xaabbccdd);words.set(0xaabbccdd,0x11);assert.equal((await load('eax-ah-memory'))(ptr),0);assert.equal(get(8),0xaabb11dd);assert.deepEqual(reads,[[0xaabbccdd,0x101]]);assert.deepEqual(writes,[[0xaabbccdd,1,0xcc]]);assert.equal(words.get(0xaabbccdd),0xcc);
init();word(16,0x98765432);words.set(0x98765432,0xa1b2);assert.equal((await load('edx-dx-memory'))(ptr),0);assert.equal(get(16),0x9876a1b2);assert.deepEqual(reads,[[0x98765432,0x102]]);assert.deepEqual(writes,[[0x98765432,2,0x5432]]);
init();word(16,0x98765432);words.set(0x98765432,0x11223344);assert.equal((await load('edx-edx-memory'))(ptr),0);assert.equal(get(16),0x11223344);assert.deepEqual(reads,[[0x98765432,0x104]]);assert.deepEqual(writes,[[0x98765432,4,0x98765432]]);
init();word(20,0x3000);word(116,0x1000);words.set(0x4000,0xaa);assert.equal((await load('fs-memory'))(ptr),0);assert.equal(get(8),0x1122aa44);assert.deepEqual(reads,[[0x4000,0x101]]);assert.deepEqual(writes,[[0x4000,1,0x33]]);
init();word(20,0xabcdffff);words.set(0xffff,0xbeef);assert.equal((await load('address16-word'))(ptr),0);assert.equal(get(16),0xa1b2beef);assert.equal(get(20),0xabcdffff);assert.deepEqual(reads,[[0xffff,0x102]]);assert.deepEqual(writes,[[0xffff,2,0xc3d4]]);
init();words.set(0x4800,0x6000);assert.equal((await load('esp-memory'))(ptr),0);assert.equal(get(24),0x6000);assert.deepEqual(reads,[[0x4800,0x104]]);assert.deepEqual(writes,[[0x4800,4,0x4800]]);init();word(24,0xfffffffe);const before=regs();assert.equal((await load('esp-memory'))(ptr),9);assert.equal(get(68),12);assert.deepEqual(regs(),before);assert.equal(get(44),0xcd7);assert.equal(get(56),0);assert.deepEqual([reads,writes],[[],[]]);
console.log('XCHG8/16/32 register aliases, high halves, implicit/explicitlockmemory, oldEA/source capture, flags, fault and completedSMC passed');
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
