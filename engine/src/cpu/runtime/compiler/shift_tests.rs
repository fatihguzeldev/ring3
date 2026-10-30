use super::*;
use crate::memory::{PageRange, Permissions};

#[test]
fn generated_narrow_shifts_preserve_aliases_and_exact_count_fault_policy() {
    use std::{
        fs,
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };
    let scratch = std::env::temp_dir().join(format!(
        "ring3-runtime-shift-{}-{}",
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
    for bits in [8, 16] {
        for (name, group) in [("shl", 4), ("shr", 5), ("sar", 7)] {
            for form in ["reg", "mem"] {
                for mode in ["cl", "one", "i0", "iwidth", "i31", "i32", "i255"] {
                    let mut code = if bits == 16 { vec![0x66] } else { Vec::new() };
                    code.extend([
                        (if mode == "cl" {
                            0xd2
                        } else if mode == "one" {
                            0xd0
                        } else {
                            0xc0
                        }) + u8::from(bits == 16),
                        if form == "reg" {
                            0xc0 + group * 8
                        } else {
                            3 + group * 8
                        },
                    ]);
                    if mode.starts_with('i') {
                        code.push(match mode {
                            "i0" => 0,
                            "iwidth" => bits as u8,
                            "i31" => 31,
                            "i32" => 32,
                            _ => 255,
                        });
                    }
                    memory.write(GuestAddress(0x1000), &code).unwrap();
                    let unit = compile(&memory, GuestAddress(0x1000), 1).unwrap();
                    fs::write(
                        scratch.join(format!("{name}{bits}-{form}-{mode}.wasm")),
                        &unit.wasm,
                    )
                    .unwrap();
                }
            }
        }
    }
    for (name, bytes) in [
        ("reached-ch", &[0xc0, 0xed, 7][..]),
        ("ch-cl", &[0xd2, 0xed][..]),
        ("cl-cl", &[0xd2, 0xe9][..]),
        ("cx-cl", &[0x66, 0xd3, 0xe9][..]),
        ("memory-ecx", &[0x66, 0xd3, 0x29][..]),
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
const imports={env:{memory},cpu:{read(p,address,width){address>>>=0;reads.push([address,width]);word(52,0);if(readFault){word(52,3);word(60,address);return 0;}return words.get(address)??0;},write(p,address,width,value){address>>>=0;value>>>=0;writes.push([address,width,value]);if(writeFault){word(52,3);word(60,address);return 3;}words.set(address,value);word(52,smc?6:0);return smc?6:0;}}};
const functions=new Map(),load=async name=>{if(!functions.has(name))functions.set(name,(await WebAssembly.instantiate(fs.readFileSync(`${root}/${name}.wasm`),imports)).instance.exports.run);return functions.get(name);};
function init(value,bits,raw,flags=0xcd7){new Uint8Array(memory.buffer).fill(0);word(0,1);word(4,160);[bits===8?(0x1234ab00|value)>>>0:(0x12340000|value)>>>0,(0x55660000|(raw&0xffff))>>>0,0xaabbccdd,0x5000,0x4800,0x11223344,0x8899aabb,0x99aabbcc].forEach((v,i)=>word(8+4*i,v));word(40,0x1000);word(44,flags);word(48,1);word(152,0x1f);words=new Map([[0x5000,value]]);reads=[];writes=[];readFault=writeFault=smc=false;}
function model(kind,bits,value,raw,flags){const count=raw&31,old=value.toString(2).padStart(bits,'0');if(!count)return[value,flags];const n=Math.min(count,bits),fill=kind==='sar'?old[0]:'0',binary=kind==='shl'?old.slice(n)+'0'.repeat(n):fill.repeat(n)+old.slice(0,bits-n),result=parseInt(binary,2),carry=count<bits?Number(old[kind==='shl'?count-1:bits-count]):kind==='sar'?Number(old[0]):0;let out=(flags&~0x8d5)|carry;if(result===0)out|=64;if(binary[0]==='1')out|=128;if([...binary.slice(-8)].filter(bit=>bit==='1').length%2===0)out|=4;if(count===1){const overflow=kind==='shl'?(Number(binary[0])^carry):kind==='shr'?Number(old[0]):0;out|=overflow<<11;}return[result,out];}
for(const bits of[8,16]){const values=[0,1,2**(bits-1)-1,2**(bits-1),2**bits-1,bits===8?0x55:0x5555,bits===8?0xaa:0xaaaa];for(const kind of['shl','shr','sar'])for(const form of['reg','mem'])for(const mode of['cl','one','i0','iwidth','i31','i32','i255']){const fn=await load(`${kind}${bits}-${form}-${mode}`),counts=mode==='cl'?[0,1,2,7,8,9,15,16,17,31,32,33,255]:[mode==='one'?1:mode==='i0'?0:mode==='iwidth'?bits:mode==='i31'?31:mode==='i32'?32:255],length=(bits===16?3:2)+Number(mode.startsWith('i'));for(const value of values)for(const raw of counts)for(const flags of[2,0xcd7]){init(value,bits,raw,flags);const before=regs(),expected=[...before],[result,outFlags]=model(kind,bits,value,raw,flags);assert.equal(fn(ptr),0,`${kind}${bits}-${form}-${mode}`);if(form==='reg'){expected[0]=bits===8?(0x1234ab00|result)>>>0:(0x12340000|result)>>>0;assert.deepEqual([reads,writes],[[],[]]);}else{assert.equal(words.get(0x5000),result);assert.deepEqual(reads,[[0x5000,bits/8|0x100]]);assert.deepEqual(writes,[[0x5000,bits/8,result]]);}assert.deepEqual(regs(),expected);assert.equal(get(44),outFlags,`${kind}${bits} value${value.toString(16)} count${raw}`);assert.equal(get(40),0x1000+length);assert.equal(get(56),1);assert.equal(get(48),0);}
if(form==='mem')for(const mode2 of['read','write','smc']){const raw=mode==='one'?1:mode==='cl'?bits:mode==='i0'?0:mode==='iwidth'?bits:mode==='i31'?31:mode==='i32'?32:255;init(2**(bits-1),bits,raw);const before=regs();readFault=mode2==='read';writeFault=mode2==='write';smc=mode2==='smc';assert.equal(fn(ptr),smc?6:3);assert.deepEqual(regs(),before);if(smc){const[result,flags]=model(kind,bits,2**(bits-1),raw,0xcd7);assert.equal(words.get(0x5000),result);assert.equal(get(44),flags);assert.equal(get(40),0x1000+length);assert.equal(get(56),1);assert.equal(get(48),0);assert.equal(writes.length,1);}else{assert.equal(words.get(0x5000),2**(bits-1));assert.equal(get(44),0xcd7);assert.equal(get(40),0x1000);assert.equal(get(56),0);assert.equal(get(48),1);assert.equal(writes.length,writeFault?1:0);}}}}
init(0,8,0);word(12,0x89abcdef);assert.equal((await load('reached-ch'))(ptr),0);assert.equal(get(12),0x89ab01ef);assert.equal(get(44),0x403);assert.equal(get(40),0x1003);
for(const name of['ch-cl','cl-cl','cx-cl']){const fn=await load(name),bits=name==='cx-cl'?16:8;for(const raw of(name==='cx-cl'?[0]:[0,1,7,8,15,16,31,32,33,128,255]))for(const value of name==='cl-cl'?[raw]:name==='cx-cl'?[0,1,0x8007,0x8008,0x800f,0x8010,0x801f,0x8020,0xffff]:[0,1,2**(bits-1),2**bits-1]){init(0,8,0);const ecx=name==='ch-cl'?(0x77880000|(value<<8)|raw)>>>0:name==='cl-cl'?(0x7788aa00|raw)>>>0:(0x77880000|value)>>>0;word(12,ecx);const count=ecx&255,old=name==='ch-cl'?(ecx>>>8)&255:name==='cl-cl'?ecx&255:ecx&65535,[result,flags]=model('shr',bits,old,count,0xcd7),expected=name==='ch-cl'?((ecx&0xffff00ff)|(result<<8))>>>0:name==='cl-cl'?((ecx&0xffffff00)|result)>>>0:((ecx&0xffff0000)|result)>>>0;assert.equal(fn(ptr),0,name);assert.equal(get(12),expected,name);assert.equal(get(44),flags,name);}}
init(0,16,0);word(12,0xaabb4407);words.set(0xaabb4407,0x8001);const[result,flags]=model('shr',16,0x8001,7,0xcd7);assert.equal((await load('memory-ecx'))(ptr),0);assert.equal(get(12),0xaabb4407);assert.equal(words.get(0xaabb4407),result);assert.equal(get(44),flags);assert.deepEqual(reads,[[0xaabb4407,0x102]]);assert.deepEqual(writes,[[0xaabb4407,2,result]]);
console.log('SHL/SHR/SAR8/16 complete count policy, CH/CL/CX aliases, MODIFYsamevaluezero, faults and completedSMC passed');
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
