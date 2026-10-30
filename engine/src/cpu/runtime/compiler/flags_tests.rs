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
