use super::*;
use crate::memory::{PageRange, Permissions};

#[test]
fn generated_simd_exact_add_and_transport_preserve_banks_and_atomic_memory() {
    use std::{
        fs,
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };
    let scratch = std::env::temp_dir().join(format!(
        "ring3-runtime-simd-{}-{}",
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
    for (name, bytes, limit) in [
        ("load", vec![0x0f, 0x28, 0x03], 1),
        ("store", vec![0x0f, 0x29, 0x03], 1),
        ("copy", vec![0x0f, 0x28, 0xc1], 1),
        ("self-copy", vec![0x0f, 0x28, 0xc0], 1),
        ("unaligned-load", vec![0x0f, 0x10, 0x03], 1),
        ("unaligned-store", vec![0x0f, 0x11, 0x03], 1),
        ("add-memory", vec![0x0f, 0x58, 0x03], 1),
        ("add-register", vec![0x0f, 0x58, 0xc1], 1),
        ("add-self", vec![0x0f, 0x58, 0xc0], 1),
        (
            "kernel",
            vec![
                0x0f, 0x28, 0x03, 0x0f, 0x58, 0x43, 0x10, 0x0f, 0x29, 0x43, 0x20,
            ],
            3,
        ),
    ] {
        memory.write(GuestAddress(0x1000), &bytes).unwrap();
        let unit = compile(&memory, GuestAddress(0x1000), limit).unwrap();
        fs::write(scratch.join(format!("{name}.wasm")), unit.wasm).unwrap();
    }
    let result = Command::new("node").arg("--input-type=module").arg("-e").arg(r#"
import assert from 'node:assert/strict';
import fs from 'node:fs';
const root=process.argv[1],memory=new WebAssembly.Memory({initial:4}),data=new DataView(memory.buffer),ram=new Uint8Array(memory.buffer);
const state=0x8000,view=0x9000,x87=0xa000,simd=0xa100,ymm=0xa200,map=0x1000,permissions=0x2000,executable=0x3000,cow=0x4000,bits=0x5000,slots=0x6000,version=0x7000,backing=0x10000;
const word=(at,value)=>data.setUint32(state+at,value>>>0,true),put=(at,value)=>data.setUint32(at,value>>>0,true),get=at=>data.getUint32(state+at,true);
let reads=0,writes=0;
const imports={env:{memory},cpu:{read(){reads++;throw Error('SIMD must use one complete wide proof or return to its service');},write(){writes++;throw Error('SIMD must never split a wide store into scalar helpers');}}};
const functions=new Map();
for(const name of['load','store','copy','self-copy','unaligned-load','unaligned-store','add-memory','add-register','add-self','kernel'])functions.set(name,(await WebAssembly.instantiate(fs.readFileSync(`${root}/${name}.wasm`),imports)).instance.exports.run);
const host=address=>{const slot=data.getUint32(map+(address>>>12)*4,true);assert(slot);return backing+(slot-1)*4096+(address&4095);};
function setMemory(address,words){words.forEach((v,i)=>put(host(address+i*4),v));}
function setXmm(index,words){words.forEach((v,i)=>put(simd+4+index*16+i*4,v));}
const a=[0x3f800000,0x40000000,0x40400000,0x40800000],b=[0x3f000000,0x3f800000,0x40000000,0x40800000],sum=[0x3fc00000,0x40400000,0x40a00000,0x41000000];
const payload=[0x7f801234,0xffc54321,0x80000000,0x00000001];
function init(address=0x3000,quota=1,mxcsr=0x1fbf){
  ram.fill(0);word(0,1);word(4,160);[0xaabbccdd,0x12345678,0x87654321,address,0x4800,0x99112233,0x44556677,0xabcdef01].forEach((v,i)=>word(8+i*4,v));word(40,0x1000);word(44,0xcd7);word(48,quota);word(124,view);word(152,0x1f);
  for(const[offset,value]of[[76,map],[80,permissions],[84,backing],[88,4],[92,executable],[96,cow],[100,bits],[104,slots],[112,4],[156,version]])word(offset,value);
  for(let page=3;page<=4;page++){put(map+page*4,page-2);data.setUint8(permissions+page,3);}
  data.setBigUint64(version,10n,true);put(view,1);put(view+4,40);put(view+8,x87);put(view+36,simd);[0x23,0x1b,0x23,0x23,0x3b,0x43].forEach((v,i)=>put(view+12+i*4,v));
  for(let i=0;i<100;i++)ram[x87+i]=(i*37+11)&255;
  // Pending, unmasked x87 invalid must not turn an SSE operation into #MF.
  data.setUint16(x87,0x037e,true);data.setUint16(x87+2,1,true);
  for(let i=0;i<256;i++)ram[ymm+i]=(i*29+7)&255;
  for(let i=0;i<128;i++)ram[simd+4+i]=(i*17+5)&255;
  for(let i=0;i<4*4096;i++)ram[backing+i]=(i*13+19)&255;
  put(simd,mxcsr);setXmm(0,a);setXmm(1,b);setMemory(0x3000,b);setMemory(0x3010,b);reads=writes=0;
}
function expected(before,{status=0,next=0x1003,remaining=0,retired=1,xmm,memory:stored,dirty=false,vector}={}){
  const out=before.slice(),d=new DataView(out.buffer),set=(at,v)=>d.setUint32(at,v>>>0,true);
  for(const[at,v]of[[40,next],[48,remaining],[52,status],[56,retired]])set(state+at,v);
  if(vector!==undefined){set(state+60,0);set(state+64,0);set(state+68,vector);}
  if(xmm)xmm.forEach((v,i)=>set(simd+4+i*4,v));
  if(stored)stored[1].forEach((v,i)=>set(host(stored[0]+i*4),v));
  if(dirty){set(state+108,1);set(slots,0);out[bits]=1;}
  return out;
}
function run(name,changes={}){
  const before=ram.slice(),want=expected(before,changes),status=changes.status??0;
  assert.equal(functions.get(name)(state),status,name);
  assert.deepEqual(ram,want,`${name}: complete scalar state, numeric banks and RAM`);
  assert.deepEqual([reads,writes],[0,0],`${name}: no scalar memory helper`);
}
function miss(name){run(name,{status:7,next:0x1000,remaining:get(48),retired:0});}

// Raw transport does not inspect floating payloads or the x87 pending state.
for(const name of['load','copy','self-copy']){
  init();if(name==='load')setMemory(0x3000,payload);else setXmm(name==='copy'?1:0,payload);
  run(name,{xmm:payload});
}
init();setXmm(0,payload);run('store',{memory:[0x3000,payload],dirty:true});
init(0x3003);setMemory(0x3003,payload);run('unaligned-load',{xmm:payload});
init(0x3003);setXmm(0,payload);run('unaligned-store',{memory:[0x3003,payload],dirty:true});
for(const name of['add-memory','add-register'])for(const sticky of[0,1,2,4,8,16,32,63]){init(0x3000,1,0x1f80|sticky);run(name,{xmm:sum});}
init();run('add-self',{xmm:[0x40000000,0x40800000,0x40c00000,0x41000000]});
for(const[left,right,result]of[
  [[0,0x80000000,0x00800000,0xbf800000],[0x80000000,0x80000000,0x80800000,0x3f800000],[0,0x80000000,0,0]],
  [[0x7f7fffff,0x00800000,0x71800000,0x3f800000],[0xff7fffff,0x00800000,0x71800000,0x34000000],[0,0x01000000,0x72000000,0x3f800001]],
]){init();setXmm(0,left);setXmm(1,right);run('add-register',{xmm:result});}
// Each budget boundary observes exactly the completed prefix of the existing kernel.
for(const quota of[1,2,3]){
  init(0x3000,quota);setMemory(0x3000,a);setMemory(0x3010,b);
  run('kernel',{status:quota<3?2:0,next:[0x1003,0x1007,0x100b][quota-1],remaining:0,retired:quota,xmm:quota===1?a:sum,...(quota===3?{memory:[0x3020,sum],dirty:true}:{})});
}
// Every lane must pass before any destination lane or MXCSR changes.
for(const[left,right]of[
  [0x3f800000,0x21800000], // addend lost even in f64: TwoSum must reject it
  [0x3f800000,0x33800000], // nearest f32 rounds and would set PE
  [0x00800000,0x80800001], // exact but subnormal result
  [0x7f7fffff,0x7f7fffff], // overflow
  [0x00000001,0x3f800000], // denormal operand
  [0x7f801234,0x3f800000], // signaling NaN
  [0x7fc01234,0x3f800000], // quiet NaN payload must use the pure owner
  [0x7f800000,0x3f800000],
]){init();setXmm(0,[a[0],a[1],a[2],left]);setXmm(1,[b[0],b[1],b[2],right]);miss('add-register');}
for(const mxcsr of[0x1f00,0x3f80,0x5f80,0x7f80,0x1fc0,0x9f80,0x10000]){init(0x3000,1,mxcsr);miss('add-register');}
// Move128 accepts every defined MXCSR bit but reserved high bits still defer.
init(0x3000,1,0xffff);run('copy',{xmm:b});
init(0x3000,1,0x10000);miss('copy');
for(const change of[
  ()=>word(124,0),()=>word(124,0xfffffff0),()=>word(124,view+1),()=>put(view,2),()=>put(view+4,36),
  ()=>put(view+36,0),()=>put(view+36,0xfffffffc),()=>put(view+36,simd+1),()=>put(view+36,0x3ff80),
]){init();change();miss('copy');}
// Complete16-byte proof failures must defer without partial scalar reads/stores.
for(const name of['load','store','add-memory'])for(const change of[
  ()=>put(map+3*4,0),()=>data.setUint8(permissions+3,8),()=>word(76,0),
]){init();change();miss(name);}
init();data.setUint8(permissions+3,1);miss('store');
for(const change of[
  ()=>put(cow+3*4,3),()=>put(executable,1),()=>data.setBigUint64(version,0xffffffffffffffffn,true),
  ()=>word(108,5),()=>{word(108,4);ram[bits+1]=1;for(let i=0;i<4;i++)put(slots+i*4,1);},
]){init();change();miss('store');}
for(const name of['load','store','add-memory']){init(0x3004);miss(name);}
for(const name of['unaligned-load','unaligned-store']){
  init(0x3ff8);put(map+4*4,0);miss(name);
  init(0x3ff8);miss(name);
}
// Segment-span rejection precedes paging and retains the original banks/RAM.
init();word(152,0x1f&~(1<<3));run('load',{status:9,next:0x1000,remaining:1,retired:0,vector:13});
init(0xfffffff8);run('unaligned-store',{status:9,next:0x1000,remaining:1,retired:0,vector:13});
console.log('inline SIMD literal ADDPS, raw128 transport, full banks/RAM, sticky MXCSR, pending x87 independence, budget prefixes and whole-span neutral fallback passed');
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
