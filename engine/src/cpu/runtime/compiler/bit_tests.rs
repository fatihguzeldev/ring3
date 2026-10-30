use super::*;
use crate::memory::{PageRange, Permissions};

#[test]
fn generated_bit_operations_preserve_signed_addresses_flags_and_precise_effects() {
    use std::{
        fs,
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };
    let scratch = std::env::temp_dir().join(format!(
        "ring3-runtime-bit-{}-{}",
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
    for bits in [16, 32] {
        for (name, opcode, group) in [
            ("bt", 0xa3, 4),
            ("bts", 0xab, 5),
            ("btr", 0xb3, 6),
            ("btc", 0xbb, 7),
        ] {
            for form in ["reg", "mem"] {
                let mut code = if bits == 16 { vec![0x66] } else { Vec::new() };
                code.extend([0x0f, opcode, if form == "reg" { 0xc8 } else { 0x0b }]);
                memory.write(GuestAddress(0x1000), &code).unwrap();
                let unit = compile(&memory, GuestAddress(0x1000), 1).unwrap();
                fs::write(
                    scratch.join(format!("{name}{bits}-{form}.wasm")),
                    &unit.wasm,
                )
                .unwrap();
                for immediate in [32, 255] {
                    let mut code = if bits == 16 { vec![0x66] } else { Vec::new() };
                    code.extend([
                        0x0f,
                        0xba,
                        if form == "reg" {
                            0xc0 + group * 8
                        } else {
                            3 + group * 8
                        },
                        immediate,
                    ]);
                    memory.write(GuestAddress(0x1000), &code).unwrap();
                    let unit = compile(&memory, GuestAddress(0x1000), 1).unwrap();
                    fs::write(
                        scratch.join(format!("{name}{bits}-{form}-imm{immediate}.wasm")),
                        &unit.wasm,
                    )
                    .unwrap();
                }
            }
            if name != "bt" {
                let mut code = if bits == 16 {
                    vec![0xf0, 0x66]
                } else {
                    vec![0xf0]
                };
                code.extend([0x0f, opcode, 0x0b]);
                memory.write(GuestAddress(0x1000), &code).unwrap();
                let unit = compile(&memory, GuestAddress(0x1000), 1).unwrap();
                fs::write(
                    scratch.join(format!("locked-{name}{bits}.wasm")),
                    &unit.wasm,
                )
                .unwrap();
            }
        }
    }
    for (name, bytes) in [
        ("reached-stack", &[0x0f, 0xab, 0x04, 0x24][..]),
        ("alias-reg", &[0x0f, 0xab, 0xc0][..]),
        ("address16-dword", &[0x67, 0x0f, 0xab, 0x0f][..]),
        ("address16-word", &[0x67, 0x66, 0x0f, 0xab, 0x0f][..]),
        ("fs-offset", &[0x64, 0x0f, 0xab, 0x0b][..]),
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
const imports={env:{memory},cpu:{read(p,address,width){address>>>=0;reads.push([address,width]);word(52,0);if(readFault){word(52,3);word(60,address);return 0;}return(words.get(address)??0)&((width&255)===2?0xffff:-1);},write(p,address,width,value){address>>>=0;writes.push([address,width,value>>>0]);if(writeFault){word(52,3);word(60,address);return 3;}words.set(address,(value&(width===2?0xffff:-1))>>>0);word(52,smc?6:0);return smc?6:0;}}};
const functions=new Map(),load=async name=>{if(!functions.has(name))functions.set(name,(await WebAssembly.instantiate(fs.readFileSync(`${root}/${name}.wasm`),imports)).instance.exports.run);return functions.get(name);};
function init(eax,index){new Uint8Array(memory.buffer).fill(0);word(0,1);word(4,160);[eax,index,0xaabbccdd,0x5000,0x4800,0x11223344,0x55667788,0x99aabbcc].forEach((v,i)=>word(8+4*i,v));word(40,0x1000);word(44,0xcd7);word(48,1);word(152,0x1f);words=new Map();reads=[];writes=[];readFault=writeFault=smc=false;}
function model(kind,bits,value,index){const bit=BigInt.asUintN(bits,BigInt(value)),mask=1n<<BigInt(index&(bits-1)),carry=(bit&mask)!==0n,result=kind==='bt'?bit:kind==='bts'?bit|mask:kind==='btr'?bit&~mask:bit^mask;return[Number(BigInt.asUintN(bits,result)),0xcd6|Number(carry)];}
function displacement(bits,index){const n=BigInt.asIntN(bits,BigInt(index>>>0)),divisor=BigInt(bits),q=n>=0n?n/divisor:-((-n+divisor-1n)/divisor);return q*BigInt(bits/8);}
for(const bits of[16,32]){const max=2**bits-1,values=[0,1,2**(bits-1),max,0x55555555&max],indices=[0,1,15,16,31,32,33,255,32767,32768,65535,0xffffffff,0xffff0000,0x80000000];for(const kind of['bt','bts','btr','btc'])for(const form of['reg','mem']){const fn=await load(`${kind}${bits}-${form}`);for(const value of values)for(const index of indices){const eax=bits===16?(0x12340000|value)>>>0:value>>>0;init(eax,index);const addr=Number(BigInt.asUintN(32,0x5000n+displacement(bits,index)));words.set(addr,value>>>0);const before=regs(),[result,flags]=model(kind,bits,value,index);assert.equal(fn(ptr),0,`${kind}${bits}-${form}`);assert.equal(get(44),flags,`${kind}${bits}-${form} flags`);if(form==='reg'){assert.equal(get(8),bits===16?(0x12340000|result)>>>0:result);assert.deepEqual(regs().slice(1),before.slice(1));assert.deepEqual([reads,writes],[[],[]]);}else{assert.deepEqual(regs(),before);assert.deepEqual(reads,[[addr,bits/8|(kind==='bt'?0:0x100)]],`${kind}${bits} address ${index.toString(16)}`);assert.equal(words.get(addr),kind==='bt'?value>>>0:result);assert.equal(writes.length,kind==='bt'?0:1);}assert.equal(get(56),1);assert.equal(get(48),0);}
for(const immediate of[32,255]){const fn=await load(`${kind}${bits}-${form}-imm${immediate}`);for(const value of values){const eax=bits===16?(0x12340000|value)>>>0:value>>>0;init(eax,0xffffffff);words.set(0x5000,value>>>0);const[result,flags]=model(kind,bits,value,immediate);assert.equal(fn(ptr),0);assert.equal(get(44),flags);if(form==='reg')assert.equal(get(8),bits===16?(0x12340000|result)>>>0:result);else{assert.deepEqual(reads,[[0x5000,bits/8|(kind==='bt'?0:0x100)]]);assert.equal(words.get(0x5000),kind==='bt'?value>>>0:result);}}}
if(form==='mem'){init(0x12345678,0);words.set(0x5000,0);const before=regs();readFault=true;assert.equal(fn(ptr),3);assert.deepEqual(regs(),before);assert.equal(get(44),0xcd7);assert.equal(get(40),0x1000);assert.equal(get(56),0);assert.deepEqual(writes,[]);if(kind!=='bt'){init(0x12345678,0);words.set(0x5000,0);writeFault=true;assert.equal(fn(ptr),3);assert.equal(words.get(0x5000),0);assert.equal(get(44),0xcd7);assert.equal(get(40),0x1000);assert.equal(get(48),1);assert.equal(get(56),0);init(0x12345678,0);words.set(0x5000,1);smc=true;assert.equal(fn(ptr),6);assert.equal(words.get(0x5000),model(kind,bits,1,0)[0]);assert.equal(get(44),0xcd7);assert.equal(get(56),1);assert.equal(get(48),0);assert.equal(get(40),bits===16?0x1004:0x1003);assert.equal(writes.length,1);}}}}
for(const bits of[16,32])for(const kind of['bts','btr','btc']){const fn=await load(`locked-${kind}${bits}`);init(0x12345678,0xffffffff);const addr=0x5000-bits/8;words.set(addr,0x8001);assert.equal(fn(ptr),0);const[result,flags]=model(kind,bits,0x8001,0xffffffff);assert.equal(words.get(addr),result);assert.equal(get(44),flags);assert.deepEqual(reads,[[addr,bits/8|0x100]]);assert.equal(writes.length,1);}
init(31,0x12345678);words.set(0x4800,0);assert.equal((await load('reached-stack'))(ptr),0);assert.equal(words.get(0x4800),0x80000000);assert.equal(get(8),31);assert.equal(get(44),0xcd6);assert.deepEqual(reads,[[0x4800,0x104]]);init(31,0);assert.equal((await load('alias-reg'))(ptr),0);assert.equal(get(8),0x8000001f);assert.equal(get(44),0xcd6);
for(const[name,bits,addr]of[['address16-dword',32,0xfffe],['address16-word',16,0]]){init(0,0xffffffff);word(20,0xabcd0002);words.set(addr,0);assert.equal((await load(name))(ptr),0);assert.equal(words.get(addr),2**(bits-1));assert.deepEqual(reads,[[addr,bits/8|0x100]]);}
init(0,0xffffffff);word(116,0x1000);words.set(0x5ffc,0);assert.equal((await load('fs-offset'))(ptr),0);assert.deepEqual(reads,[[0x5ffc,0x104]]);assert.equal(words.get(0x5ffc),0x80000000);
init(0,0);word(24,0xfffffffe);const stack=await load('reached-stack');assert.equal(stack(ptr),9);assert.equal(get(68),12);assert.equal(get(40),0x1000);assert.equal(get(56),0);assert.deepEqual([reads,writes],[[],[]]);
console.log('BT/BTS/BTR/BTC16/32 signed dynamic and immediate indices, CF-only flags, source alias, address16/FS, lockedRMW, fault and completedSMC passed');
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
