use super::*;
use crate::memory::{PageRange, Permissions};

fn compile_memory_case(code: &[u8], limit: usize) -> CompiledUnit {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(0x1000), code).unwrap();
    compile(&memory, GuestAddress(0x1000), limit).unwrap()
}

#[test]
fn generated_modify_write_preserves_fast_and_canonical_memory_contracts() {
    use std::{
        fs,
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };
    let scratch = std::env::temp_dir().join(format!(
        "ring3-runtime-memory-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&scratch).unwrap();
    for (name, opcode, immediate_group) in [
        ("add", 0x01, 0),
        ("or", 0x09, 1),
        ("adc", 0x11, 2),
        ("sbb", 0x19, 3),
        ("and", 0x21, 4),
        ("sub", 0x29, 5),
        ("xor", 0x31, 6),
    ] {
        for (form, code) in [
            ("reg", vec![opcode, 0x03]),
            ("imm", vec![0x83, 3 + immediate_group * 8, 1]),
        ] {
            let unit = compile_memory_case(&code, 1);
            fs::write(scratch.join(format!("{name}-{form}.wasm")), unit.wasm).unwrap();
        }
    }
    for (name, code, limit) in [
        ("twice", &[0x83, 0x03, 1, 0x83, 0x03, 1][..], 2),
        (
            "fast-then-cross",
            &[0x83, 0x03, 1, 0x83, 0x83, 0xfe, 0x0f, 0, 0, 1][..],
            2,
        ),
        ("view-read", &[0x8b, 0x03][..], 1),
        ("view-next-word", &[0x8b, 0x03, 0x8b, 0x4b, 4][..], 2),
        (
            "view-next-page",
            &[0x8b, 0x03, 0x8b, 0x8b, 0, 0x10, 0, 0][..],
            2,
        ),
    ] {
        let unit = compile_memory_case(code, limit);
        fs::write(scratch.join(format!("{name}.wasm")), unit.wasm).unwrap();
    }
    let result = Command::new("node").arg("--input-type=module").arg("-e").arg(r#"
import assert from 'node:assert/strict';
import fs from 'node:fs';
const root=process.argv[1],memory=new WebAssembly.Memory({initial:4}),data=new DataView(memory.buffer),ram=new Uint8Array(memory.buffer);
const ptr=0x8000,map=0x1000,permissions=0x2000,executable=0x3000,cow=0x4000,bits=0x5000,slots=0x6000,version=0x7000,backing=0x10000;
const word=(at,value)=>data.setUint32(ptr+at,value>>>0,true),get=at=>data.getUint32(ptr+at,true),regs=()=>Array.from({length:8},(_,i)=>get(8+i*4));
let reads=[],writes=[],forcedWriteFault=false;
function failure(address,width,access,reason,status=3){word(52,status);word(60,address);word(64,width|(access<<8)|(reason<<16));word(68,width);}
function span(address,width,modify){
  const locations=[];
  for(let i=0;i<width;i++){
    const at=address+i,page=at>>>12,slot=data.getUint32(map+page*4,true),permission=data.getUint8(permissions+page),access=modify?2:1;
    if(!slot){failure(at,width,access,1);return null;}
    if((permission&8)||(permission&(modify?3:1))!==(modify?3:1)){failure(at,width,access,2);return null;}
    locations.push(backing+(slot-1)*4096+(at&4095));
  }
  return locations;
}
function loadGuest(address){const locations=span(address,4,false);assert(locations);return locations.reduce((v,at,i)=>v+data.getUint8(at)*2**(8*i),0)>>>0;}
function seedGuest(address,value){for(let i=0;i<4;i++){const page=(address+i)>>>12,slot=data.getUint32(map+page*4,true);assert(slot);data.setUint8(backing+(slot-1)*4096+((address+i)&4095),value>>>i*8);}}
function flush(){for(let i=0;i<get(108);i++)data.setUint8(bits+data.getUint32(slots+i*4,true),0);if(get(108))data.setBigUint64(version,data.getBigUint64(version,true)+1n,true);word(108,0);}
const imports={env:{memory},cpu:{
  read(p,address,width){assert.equal(p,ptr);reads.push([address,width]);word(52,0);const locations=span(address,width&255,(width&0x100)!==0);return locations?locations.reduce((v,at,i)=>v+data.getUint8(at)*2**(8*i),0)>>>0:0;},
  write(p,address,width,value){
    assert.equal(p,ptr);writes.push([address,width,value>>>0]);word(52,0);
    let locations=span(address,width,true);if(!locations)return 3;
    if(forcedWriteFault){failure(address,width,2,2);return 3;}
    if(data.getBigUint64(version,true)===0xffffffffffffffffn){word(52,8);return 8;}
    const page=address>>>12,slot=data.getUint32(map+page*4,true)-1,isCow=data.getUint32(cow+page*4,true)!==0,isCode=data.getUint32(executable+slot*4,true)!==0,cross=(address&4095)+width>4096;
    if(isCow||isCode||cross){
      flush();
      if(isCow){ram.copyWithin(backing+2*4096,backing+slot*4096,backing+(slot+1)*4096);data.setUint32(map+page*4,3,true);data.setUint32(cow+page*4,0,true);locations=span(address,width,true);}
      locations.forEach((at,i)=>data.setUint8(at,value>>>i*8));
      data.setBigUint64(version,data.getBigUint64(version,true)+1n,true);
    }else{
      if(!data.getUint8(bits+slot)){data.setUint32(slots+get(108)*4,slot,true);data.setUint8(bits+slot,1);word(108,get(108)+1);}
      locations.forEach((at,i)=>data.setUint8(at,value>>>i*8));
    }
    word(52,isCode?6:0);return isCode?6:0;
  }
}};
const functions=new Map(),load=async name=>{if(!functions.has(name))functions.set(name,(await WebAssembly.instantiate(fs.readFileSync(`${root}/${name}.wasm`),imports)).instance.exports.run);return functions.get(name);};
function init(value=0,eax=1,flags=0xcd7,address=0x3000,quota=1){
  ram.fill(0);word(0,1);word(4,160);[eax,0x11223344,0x55667788,address,0x4800,0x99aabbcc,0x12345678,0x87654321].forEach((v,i)=>word(8+i*4,v));word(40,0x1000);word(44,flags);word(48,quota);word(152,0x1f);
  for(const[offset,value]of[[76,map],[80,permissions],[84,backing],[88,4],[92,executable],[96,cow],[100,bits],[104,slots],[112,4],[156,version]])word(offset,value);
  data.setUint32(map+3*4,1,true);data.setUint32(map+4*4,2,true);data.setUint8(permissions+3,3);data.setUint8(permissions+4,3);data.setBigUint64(version,10n,true);seedGuest(address,value);reads=[];writes=[];forcedWriteFault=false;
}
function model(kind,a,b,flags){
  const x=BigInt(a>>>0),y=BigInt(b>>>0),carry=BigInt((kind==='adc'||kind==='sbb')?(flags&1):0),subtract=kind==='sub'||kind==='sbb',logical=['or','and','xor'].includes(kind);
  const full=kind==='or'?x|y:kind==='and'?x&y:kind==='xor'?x^y:subtract?x-y-carry:x+y+carry,result=Number(BigInt.asUintN(32,full));
  let out=flags&~0x8d5,ones=0;for(let i=0;i<8;i++)ones+=(result>>>i)&1;
  if(result===0)out|=0x40;if(result>=0x80000000)out|=0x80;if(ones%2===0)out|=4;
  if(!logical){const signed= subtract?BigInt.asIntN(32,x)-BigInt.asIntN(32,y)-carry:BigInt.asIntN(32,x)+BigInt.asIntN(32,y)+carry;if(signed< -0x80000000n||signed>0x7fffffffn)out|=0x800;if(subtract?x<y+carry:full>0xffffffffn)out|=1;if(subtract?(x&15n)<(y&15n)+carry:(x&15n)+(y&15n)+carry>15n)out|=16;}
  return[result,(out|2)>>>0];
}
function complete(next,quota,retired,flags){assert.deepEqual([get(40),get(48),get(56),get(44)],[next,quota,retired,flags]);}
const values=[0,1,15,0x7fffffff,0x80000000,0xffffffff];
for(const kind of['add','or','adc','sbb','and','sub','xor'])for(const form of['reg','imm'])for(const a of values)for(const b of(form==='imm'?[1]:values))for(const flags of[0xcd6,0xcd7]){
  init(a,b,flags);const before=regs(),[expected,expectedFlags]=model(kind,a,b,flags),fn=await load(`${kind}-${form}`);assert.equal(fn(ptr),0,`${kind}-${form}`);assert.equal(loadGuest(0x3000),expected);assert.deepEqual(regs(),before);complete(form==='imm'?0x1003:0x1002,0,1,expectedFlags);assert.deepEqual([reads,writes],[[],[]],'ordinary RMW stays entirely in Wasm');assert.equal(get(108),1);assert.equal(data.getUint32(slots,true),0);assert.equal(data.getUint8(bits),1);assert.equal(data.getBigUint64(version,true),10n);
}
const add=await load('add-imm');
init(7);word(76,0);assert.equal(add(ptr),0);assert.equal(loadGuest(0x3000),8);assert.deepEqual(reads,[[0x3000,0x104]]);assert.deepEqual(writes,[[0x3000,4,8]]);complete(0x1003,0,1,model('add',7,1,0xcd7)[1]);
for(const[permission,mapped,access,reason]of[[1,true,2,2],[11,true,2,2],[3,false,2,1]]){
  init(0xabcdef01);data.setUint8(permissions+3,permission);if(!mapped)data.setUint32(map+3*4,0,true);const before=regs(),original=data.getUint32(backing,true);assert.equal(add(ptr),3);assert.deepEqual(regs(),before);complete(0x1000,1,0,0xcd7);assert.equal(data.getUint32(backing,true),original);assert.deepEqual(reads,[[0x3000,0x104]]);assert.deepEqual(writes,[]);assert.equal(get(108),0);assert.deepEqual([get(60),get(64),get(68)],[0x3000,4|(access<<8)|(reason<<16),4]);
}
init(0xffffffff,1,0xcd7,0x3ffe);assert.equal(add(ptr),0);assert.equal(loadGuest(0x3ffe),0);assert.deepEqual(reads,[[0x3ffe,0x104]]);assert.deepEqual(writes,[[0x3ffe,4,0]]);complete(0x1003,0,1,model('add',0xffffffff,1,0xcd7)[1]);
init(0x12345678,1,0xcd7,0x3ffe);data.setUint32(map+4*4,0,true);const untouched=ram.slice(backing+4094,backing+4098);assert.equal(add(ptr),3);assert.deepEqual(ram.slice(backing+4094,backing+4098),untouched);assert.deepEqual(writes,[]);assert.equal(get(60),0x4000);complete(0x1000,1,0,0xcd7);
init(7);data.setUint32(cow+3*4,3,true);assert.equal(add(ptr),0);assert.equal(data.getUint32(backing,true),7);assert.equal(loadGuest(0x3000),8);assert.equal(data.getUint32(map+3*4,true),3);assert.deepEqual(reads,[]);assert.deepEqual(writes,[[0x3000,4,8]]);assert.equal(get(108),0);complete(0x1003,0,1,model('add',7,1,0xcd7)[1]);
init(0xffffffff);data.setUint32(executable,1,true);assert.equal(add(ptr),6);assert.equal(loadGuest(0x3000),0);assert.deepEqual(reads,[]);assert.deepEqual(writes,[[0x3000,4,0]]);assert.equal(get(108),0);complete(0x1003,0,1,model('add',0xffffffff,1,0xcd7)[1]);
init(7);data.setUint32(executable,1,true);forcedWriteFault=true;assert.equal(add(ptr),3);assert.equal(loadGuest(0x3000),7);assert.deepEqual(reads,[]);assert.equal(writes.length,1);complete(0x1000,1,0,0xcd7);
init(7);data.setBigUint64(version,0xffffffffffffffffn,true);assert.equal(add(ptr),8);assert.equal(loadGuest(0x3000),7);assert.equal(get(108),0);assert.deepEqual(reads,[]);assert.equal(writes.length,1);complete(0x1000,1,0,0xcd7);
init(7,1,0xcd7,0x3000,2);assert.equal((await load('twice'))(ptr),0);assert.equal(loadGuest(0x3000),9);assert.equal(get(108),1);assert.deepEqual([reads,writes],[[],[]]);complete(0x1006,0,2,model('add',8,1,0xcd7)[1]);
init(7,1,0xcd7,0x3000,2);seedGuest(0x3ffe,0xffffffff);assert.equal((await load('fast-then-cross'))(ptr),0);assert.equal(loadGuest(0x3000),8);assert.equal(loadGuest(0x3ffe),0);assert.deepEqual(reads,[[0x3ffe,0x104]]);assert.deepEqual(writes,[[0x3ffe,4,0]]);complete(0x100a,0,2,model('add',0xffffffff,1,0xcd7)[1]);

// these private-view protocol negatives use a separate table from the trusted
// helper's canonical mappings. declining a malformed view must preserve that
// helper authority rather than inventing a guest mapping or fault.
const shadow=0x9000,slotValues=[0x10203040,0x50607080,0x90a0b0c0,0xd0e0f000];
function viewInit(slot,capacity,quota=1){
  init(7,1,0x602,0x3000,quota);word(76,shadow);word(88,capacity);data.setUint32(shadow+3*4,slot,true);data.setUint32(shadow+4*4,2,true);
  slotValues.forEach((value,index)=>data.setUint32(backing+index*4096,value,true));
}
function readExpected(value,next=0x1002,status=0,remaining=0){
  const expected=ram.slice(),d=new DataView(expected.buffer);
  for(const[at,v]of[[8,value],[40,next],[48,remaining],[52,status],[56,1]])d.setUint32(ptr+at,v>>>0,true);
  return expected;
}
const readView=await load('view-read');
for(const[slot,capacity,fast]of[
  [0,0,false],[0,4,false],[0,0xffffffff,false],
  [1,0,false],[1,1,true],[1,4,true],[1,0xffffffff,true],
  [4,3,false],[4,4,true],[5,4,false],[0xffffffff,4,false],[0xffffffff,0xfffffffe,false],
]){
  viewInit(slot,capacity);if(fast)data.setUint32(map+3*4,slot,true);const expected=readExpected(fast?slotValues[slot-1]:slotValues[0]);
  assert.equal(readView(ptr),0,`private slot ${slot} capacity ${capacity}`);assert.deepEqual(ram,expected);
  assert.deepEqual(reads,fast?[]:[[0x3000,4]]);assert.deepEqual(writes,[]);
}

// invalid slots must decline before dereferencing even a non-null permission
// pointer outside memory. valid slots still retain that permission load's trap.
for(const slot of[0,5,0xffffffff]){
  viewInit(slot,4);word(80,ram.length+1);const expected=readExpected(slotValues[0]);
  assert.equal(readView(ptr),0);assert.deepEqual(ram,expected);assert.deepEqual(reads,[[0x3000,4]]);assert.deepEqual(writes,[]);
}
viewInit(1,1);word(80,ram.length+1);{
  const expected=ram.slice();assert.throws(()=>readView(ptr),WebAssembly.RuntimeError);assert.deepEqual(ram,expected);assert.deepEqual([reads,writes],[[],[]]);
}

// MAX/MAX satisfies the slot range guard; the resulting malformed host view
// must still encounter the raw load's width bounds check after 32-bit wrapping.
viewInit(0xffffffff,0xffffffff);word(84,ram.length+8192);{
  const expected=ram.slice();assert.throws(()=>readView(ptr),WebAssembly.RuntimeError);assert.deepEqual(ram,expected);assert.deepEqual([reads,writes],[[],[]]);
}

// the last valid physical slot uses exactly one one-based-to-zero-based
// conversion, including the paired RMW store and dirty-slot identity.
viewInit(4,4);data.setUint32(map+3*4,4,true);{
  const expected=ram.slice(),d=new DataView(expected.buffer);
  d.setUint32(backing+3*4096,0xd0e0f001,true);d.setUint8(bits+3,1);d.setUint32(slots,3,true);
  for(const[at,value]of[[40,0x1003],[44,0x682],[48,0],[56,1],[108,1]])d.setUint32(ptr+at,value,true);
  assert.equal(add(ptr),0);assert.deepEqual(ram,expected);assert.deepEqual([reads,writes],[[],[]]);
}

// each first access fits, while the second access crosses a private mapping,
// permission, or host-backing boundary. no earlier local read may be published
// when the later raw Wasm access traps.
for(const kind of['mapping','permission','backing']){
  const run=await load(kind==='backing'?'view-next-word':'view-next-page');
  for(const quota of[1,2]){
    viewInit(1,1,quota);let first=slotValues[0];
    if(kind==='mapping'){word(76,ram.length-16);data.setUint32(ram.length-4,1,true);}
    if(kind==='permission'){word(88,4);word(80,ram.length-4);data.setUint8(ram.length-1,3);}
    if(kind==='backing'){word(84,ram.length-4);first=0xabcdef01;data.setUint32(ram.length-4,first,true);}
    const expected=quota===1?readExpected(first,0x1002,2):ram.slice();
    if(quota===1)assert.equal(run(ptr),2);else assert.throws(()=>run(ptr),WebAssembly.RuntimeError);
    assert.deepEqual(ram,expected,`${kind} boundary quota ${quota}`);assert.deepEqual([reads,writes],[[],[]]);
  }
}
console.log('paired RMW, exact FLAGS, dirty tracking, COW/SMC/exhaustion and private slot/permission/table/late host bounds passed');
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
