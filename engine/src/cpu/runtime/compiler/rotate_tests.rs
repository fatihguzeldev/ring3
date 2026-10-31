use super::*;
use crate::memory::{PageRange, Permissions};

#[test]
fn generated_rotates_and_xlat_preserve_aliases_flags_and_precise_memory_boundaries() {
    use std::{
        fs,
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };
    let scratch = std::env::temp_dir().join(format!(
        "ring3-runtime-rotate-{}-{}",
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
        for (name, group) in [("rol", 0), ("ror", 1)] {
            for form in ["reg", "mem"] {
                for mode in [
                    "one", "cl", "i0", "iwidth", "i9", "i17", "i31", "i32", "i33", "i255",
                ] {
                    let mut code = if bits == 16 { vec![0x66] } else { Vec::new() };
                    code.extend([
                        (if mode == "cl" {
                            0xd2
                        } else if mode == "one" {
                            0xd0
                        } else {
                            0xc0
                        }) + u8::from(bits != 8),
                        if form == "mem" {
                            3 + group * 8
                        } else {
                            0xc0 + group * 8 + if bits == 8 { 5 } else { 0 }
                        },
                    ]);
                    if mode.starts_with('i') {
                        code.push(match mode {
                            "i0" => 0,
                            "iwidth" => bits as u8,
                            "i9" => 9,
                            "i17" => 17,
                            "i31" => 31,
                            "i32" => 32,
                            "i33" => 33,
                            _ => 255,
                        });
                    }
                    memory.write(GuestAddress(0x1000), &code).unwrap();
                    let unit = compile(&memory, GuestAddress(0x1000), 1).unwrap();
                    fs::write(
                        scratch.join(format!("{name}{bits}-{form}-{mode}.wasm")),
                        unit.wasm,
                    )
                    .unwrap();
                }
            }
        }
    }
    for (name, code) in [
        ("cl-alias", vec![0xd2, 0xc1]),
        ("cx-alias", vec![0x66, 0xd3, 0xc9]),
        ("ecx-alias", vec![0xd3, 0xc9]),
        ("xlat", vec![0xd7]),
        ("fs-xlat", vec![0x64, 0xd7]),
        ("xlat16", vec![0x67, 0xd7]),
        ("fs-xlat16", vec![0x64, 0x67, 0xd7]),
        ("ss-xlat", vec![0x36, 0xd7]),
        ("rcl-outside", vec![0xd0, 0xd0]),
        ("rcr-outside", vec![0xd1, 0xd8]),
    ] {
        memory.write(GuestAddress(0x1000), &code).unwrap();
        let unit = compile(&memory, GuestAddress(0x1000), 1).unwrap();
        fs::write(scratch.join(format!("{name}.wasm")), unit.wasm).unwrap();
    }
    let result = Command::new("node").arg("--input-type=module").arg("-e").arg(r#"
import assert from 'node:assert/strict';
import fs from 'node:fs';
const root=process.argv[1],memory=new WebAssembly.Memory({initial:1}),data=new DataView(memory.buffer),ram=new Uint8Array(memory.buffer),ptr=0x8000;
const word=(at,value)=>data.setUint32(ptr+at,value>>>0,true),get=at=>data.getUint32(ptr+at,true),regs=()=>Array.from({length:8},(_,i)=>get(8+i*4));
let values=new Map(),reads=[],writes=[],readFault=false,writeFault=false,smc=false;
function fault(address,width,access){word(52,3);word(60,address);word(64,width|(access<<8)|(2<<16));word(68,width);}
const imports={env:{memory},cpu:{
  read(context,address,width){assert.equal(context,ptr);address>>>=0;reads.push([address,width]);if(readFault){fault(address,width&255,width&0x100?2:1);return 0;}word(52,0);return values.get(address)??0;},
  write(context,address,width,value){assert.equal(context,ptr);address>>>=0;value>>>=0;writes.push([address,width,value]);if(writeFault){fault(address,width,2);return 3;}values.set(address,value);word(52,smc?6:0);return smc?6:0;}
}};
const functions=new Map(),load=async name=>{if(!functions.has(name))functions.set(name,(await WebAssembly.instantiate(fs.readFileSync(`${root}/${name}.wasm`),imports)).instance.exports.run);return functions.get(name);};
function init(value=0,bits=8,count=0,flags=0xcd7){
  ram.fill(0);word(0,1);word(4,160);const eax=bits===8?0xaabbcc55:bits===16?(0xaabb0000|value)>>>0:value>>>0,ecx=bits===8?(0x11220000|(value<<8)|(count&255))>>>0:(0x11223300|(count&255))>>>0;
  [eax,ecx,0x55667788,0x5000,0x4800,0x99aabbcc,0x12345678,0x87654321].forEach((v,i)=>word(8+i*4,v));word(40,0x1000);word(44,flags);word(48,1);word(152,0x1f);
  for(let i=0;i<512;i++)ram[0xa000+i]=(i*17+23)&255;
  values=new Map([[0x5000,value>>>0]]);reads=[];writes=[];readFault=writeFault=smc=false;
}
function model(kind,bits,value,raw,flags){
  const mask=(1n<<BigInt(bits))-1n,old=BigInt(value) & mask,original=raw&31,n=BigInt(original%bits),width=BigInt(bits);
  const result=Number((kind==='rol'?(old<<n)|(old>>(width-n)):(old>>n)|(old<<(width-n)))&mask)>>>0;
  if(!original)return[result,flags];
  const carry=kind==='rol'?result&1:result>>>(bits-1),overflow=original===1?((result>>>(bits-1))^(kind==='rol'?carry:result>>>(bits-2)))&1:0;
  return[result,((flags&~0x801)|carry|(overflow<<11))>>>0];
}
function expected(before,{status=0,next=0x1002,flags=get(44),gpr=regs(),remaining=0,retired=1,address,width,access,vector}={}){
  const want=before.slice(),d=new DataView(want.buffer),set=(at,v)=>d.setUint32(ptr+at,v>>>0,true);
  gpr.forEach((v,i)=>set(8+i*4,v));for(const[at,v]of[[40,next],[44,flags],[48,remaining],[52,status],[56,retired]])set(at,v);
  if(address!==undefined){set(60,address);set(64,width|(access<<8)|(2<<16));set(68,width);}
  if(vector!==undefined){set(60,0);set(64,0);set(68,vector);}
  return want;
}
async function check(name,changes){const before=ram.slice(),want=expected(before,changes);assert.equal((await load(name))(ptr),changes.status??0,name);assert.deepEqual(ram,want,`${name}: complete scalar state and untouched numeric banks/RAM`);}
for(const bits of[8,16,32])for(const kind of['rol','ror'])for(const form of['reg','mem'])for(const mode of['one','cl','i0','iwidth','i9','i17','i31','i32','i33','i255']){
  const counts=mode==='cl'?[0,1,2,7,8,9,15,16,17,31,32,33,63,64,255]:[mode==='one'?1:mode==='i0'?0:mode==='iwidth'?bits:Number(mode.slice(1))],length=(bits===16?3:2)+Number(mode.startsWith('i'));
  for(const value of[0,1,2**(bits-1),2**bits-1,Number(0xa55ac381n&((1n<<BigInt(bits))-1n))])for(const count of counts)for(const flags of[2,0xcd7]){
    init(value,bits,count,flags);const gpr=regs(),[result,outFlags]=model(kind,bits,value,count,flags);
    if(form==='reg'){const index=bits===8?1:0;gpr[index]=bits===8?((gpr[index]&0xffff00ff)|(result<<8))>>>0:bits===16?((gpr[index]&0xffff0000)|result)>>>0:result;}
    await check(`${kind}${bits}-${form}-${mode}`,{next:0x1000+length,flags:outFlags,gpr});
    if(form==='reg')assert.deepEqual([reads,writes],[[],[]]);
    else{assert.equal(values.get(0x5000),result);assert.deepEqual(reads,[[0x5000,bits/8|0x100]]);assert.deepEqual(writes,[[0x5000,bits/8,result]]);}
  }
  if(form==='mem')for(const effect of['read-fault','write-fault','smc']){
    const count=counts[0];init(2**(bits-1),bits,count);const gpr=regs(),[result,flags]=model(kind,bits,2**(bits-1),count,0xcd7);readFault=effect==='read-fault';writeFault=effect==='write-fault';smc=effect==='smc';
    await check(`${kind}${bits}-${form}-${mode}`,smc?{status:6,next:0x1000+length,flags,gpr}:{status:3,next:0x1000,remaining:1,retired:0,gpr,address:0x5000,width:bits/8,access:2});
    assert.equal(values.get(0x5000),smc?result:2**(bits-1));assert.deepEqual(reads,[[0x5000,bits/8|0x100]]);assert.equal(writes.length,readFault?0:1);
  }
}
// Literal reached D0 C5: CH aliases ECX and must retain CL and the high word.
init(0x81,8,7,0x4d6);await check('rol8-reg-one',{next:0x1002,flags:0xcd7,gpr:[0xaabbcc55,0x11220307,0x55667788,0x5000,0x4800,0x99aabbcc,0x12345678,0x87654321]});
for(const[name,bits,kind]of[['cl-alias',8,'rol'],['cx-alias',16,'ror'],['ecx-alias',32,'ror']])for(const ecx of[0,1,8,16,32,33,0x88998007,0xffffffff]){
  init();word(12,ecx);const gpr=regs(),old=bits===8?ecx&255:bits===16?ecx&65535:ecx,[result,flags]=model(kind,bits,old,ecx&255,0xcd7);
  gpr[1]=bits===8?((ecx&0xffffff00)|result)>>>0:bits===16?((ecx&0xffff0000)|result)>>>0:result;
  await check(name,{next:bits===16?0x1003:0x1002,gpr,flags});
}
for(const[name,length,base,al,segmentBase,address]of[
  ['xlat',1,0x5000,0x35,0,0x5035],['xlat',1,0xfffffff0,0x30,0,0x20],
  ['fs-xlat',2,0x5000,0x35,0x1000,0x6035],
  ['xlat16',2,0x1234fff0,0x30,0,0x20],['fs-xlat16',3,0x1234fff0,0x30,0x4000,0x4020],
])for(const fail of[false,true]){
  init();word(8,(0xaabbcc00|al)>>>0);word(20,base);word(116,segmentBase);values.set(address,0x7e);const gpr=regs();if(!fail)gpr[0]=0xaabbcc7e;readFault=fail;
  await check(name,fail?{status:3,next:0x1000,remaining:1,retired:0,gpr,address,width:1,access:1}:{next:0x1000+length,gpr});
  assert.deepEqual(reads,[[address,1]]);assert.deepEqual(writes,[]);assert.equal(values.get(address),0x7e);
}
for(const[name,segment,vector]of[['xlat',3,13],['fs-xlat',4,13],['ss-xlat',2,12]]){
  init();word(152,0x1f&~(1<<segment));await check(name,{status:9,next:0x1000,remaining:1,retired:0,vector});assert.deepEqual([reads,writes],[[],[]]);
}
for(const name of['rcl-outside','rcr-outside']){init();await check(name,{status:7,next:0x1000,remaining:1,retired:0});assert.deepEqual([reads,writes],[[],[]]);}
console.log('ROL/ROR8/16/32 literal CH alias, normalized/full-cycle counts and FLAGS, MODIFYzero/fault/completedSMC, XLAT DS/FS/addr16/AL/fault passed');
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
