use super::*;
use crate::memory::{PageRange, Permissions};

#[test]
fn generated_strings_preserve_partial_quota_fault_and_smc_progress() {
    use std::{
        fs,
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };
    let scratch = std::env::temp_dir().join(format!(
        "ring3-runtime-string-{}-{}",
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
    for (name, bytes) in [
        ("stosd", &[0xf3, 0xab][..]),
        ("stosb", &[0xf3, 0xaa][..]),
        ("stosw", &[0x66, 0xf3, 0xab][..]),
        ("movsd", &[0xf3, 0xa5][..]),
        ("movsb", &[0xf3, 0xa4][..]),
        ("lodsd", &[0xf3, 0xad][..]),
        ("cmpsb", &[0xf3, 0xa6][..]),
        ("scasb", &[0xf2, 0xae][..]),
        ("stosd16", &[0x67, 0xf3, 0xab][..]),
        ("movsd-fs", &[0x64, 0xf3, 0xa5][..]),
        ("single-stosd", &[0xab][..]),
        ("single-movsd", &[0xa5][..]),
    ] {
        memory.write(GuestAddress(0x1000), bytes).unwrap();
        let unit = compile(&memory, GuestAddress(0x1000), 1).unwrap();
        fs::write(scratch.join(format!("{name}.wasm")), &unit.wasm).unwrap();
    }
    let result = Command::new("node").arg("--input-type=module").arg("-e").arg(r#"
import assert from 'node:assert/strict';
import fs from 'node:fs';
const root=process.argv[1],memory=new WebAssembly.Memory({initial:4}),data=new DataView(memory.buffer),ram=new Uint8Array(memory.buffer),ptr=0x20000;
const word=(at,value)=>data.setUint32(ptr+at,value>>>0,true),get=at=>data.getUint32(ptr+at,true);
let readFault=-1,writeFault=-1,smcAt=-1,reads=[],writes=[],attempts=[];
const imports={env:{memory},cpu:{read(p,address,width){width&=255;reads.push(address);word(52,0);if(address===readFault){word(52,3);word(60,address);return 0;}return width===1?data.getUint8(address):width===2?data.getUint16(address,true):data.getUint32(address,true);},write(p,address,width,value){attempts.push(address);if(address===writeFault){word(52,3);word(60,address);return 3;}if(width===1)data.setUint8(address,value);else if(width===2)data.setUint16(address,value,true);else data.setUint32(address,value,true);writes.push(address);const status=writes.length===smcAt?6:0;word(52,status);return status;}}};
const functions=new Map();for(const name of['stosd','stosb','stosw','movsd','movsb','lodsd','cmpsb','scasb','stosd16','movsd-fs','single-stosd','single-movsd'])functions.set(name,(await WebAssembly.instantiate(fs.readFileSync(`${root}/${name}.wasm`),imports)).instance.exports.run);
function init(count,quota,flags=0x8d7){ram.fill(0);word(0,1);word(4,160);word(8,0x11223344);word(12,count);word(32,0x4000);word(36,0x5000);word(40,0x1000);word(44,flags);word(48,quota);word(152,0x1f);readFault=writeFault=smcAt=-1;reads=[];writes=[];attempts=[];}
function state(){return[get(40),get(12),get(32),get(36),get(48),get(56),get(44)];}
const stos=functions.get('stosd');init(5,2);assert.equal(stos(ptr),2);assert.deepEqual(state(),[0x1000,3,0x4000,0x5008,0,0,0x8d7]);assert.deepEqual(writes,[0x5000,0x5004]);word(48,3);assert.equal(stos(ptr),0);assert.deepEqual(state(),[0x1002,0,0x4000,0x5014,0,1,0x8d7]);assert.deepEqual(writes,[0x5000,0x5004,0x5008,0x500c,0x5010]);for(let at=0x5000;at<0x5014;at+=4)assert.equal(data.getUint32(at,true),0x11223344);
init(4,10);writeFault=0x5004;assert.equal(stos(ptr),3);assert.deepEqual(state(),[0x1000,3,0x4000,0x5004,9,0,0x8d7]);assert.equal(data.getUint32(0x5004,true),0);assert.deepEqual(writes,[0x5000]);writeFault=-1;assert.equal(stos(ptr),0);assert.deepEqual(state(),[0x1002,0,0x4000,0x5010,6,1,0x8d7]);assert.deepEqual(writes,[0x5000,0x5004,0x5008,0x500c]);
init(0,1);word(36,0xffffffff);word(152,0);assert.equal(stos(ptr),0);assert.deepEqual(state(),[0x1002,0,0x4000,0xffffffff,0,1,0x8d7]);assert.deepEqual([reads,writes],[[],[]]);init(0,0);assert.equal(stos(ptr),2);assert.equal(get(56),0);assert.deepEqual([reads,writes],[[],[]]);
init(3,10);smcAt=1;assert.equal(stos(ptr),6);assert.deepEqual(state(),[0x1000,2,0x4000,0x5004,9,0,0x8d7]);smcAt=-1;assert.equal(stos(ptr),0);assert.deepEqual(state(),[0x1002,0,0x4000,0x500c,7,1,0x8d7]);assert.equal(writes.length,3);init(1,10);smcAt=1;assert.equal(stos(ptr),6);assert.deepEqual(state(),[0x1002,0,0x4000,0x5004,9,1,0x8d7]);assert.equal(writes.length,1);
const movs=functions.get('movsd');init(3,10);data.setUint32(0x4000,0xaaaaaaaa,true);data.setUint32(0x4004,0xbbbbbbbb,true);writeFault=0x5004;assert.equal(movs(ptr),3);assert.deepEqual(state(),[0x1000,2,0x4004,0x5004,9,0,0x8d7]);assert.deepEqual(reads,[0x4000,0x4004]);assert.deepEqual(attempts,[0x5000,0x5004]);assert.equal(data.getUint32(0x5000,true),0xaaaaaaaa);assert.equal(data.getUint32(0x5004,true),0);init(3,10);readFault=0x4004;assert.equal(movs(ptr),3);assert.deepEqual(state(),[0x1000,2,0x4004,0x5004,9,0,0x8d7]);assert.deepEqual(writes,[0x5000]);
init(4,10,2);word(36,0x4001);ram.set([1,2,3,4,5],0x4000);assert.equal(functions.get('movsb')(ptr),0);assert.deepEqual([...ram.slice(0x4000,0x4005)],[1,1,1,1,1]);assert.deepEqual(state(),[0x1002,0,0x4004,0x4005,6,1,2]);init(4,10,0x402);word(32,0x4003);word(36,0x4004);ram.set([1,2,3,4,5],0x4000);assert.equal(functions.get('movsb')(ptr),0);assert.deepEqual([...ram.slice(0x4000,0x4005)],[1,1,2,3,4]);assert.deepEqual(state(),[0x1002,0,0x3fff,0x4000,6,1,0x402]);
init(3,10,2);for(let i=0;i<3;i++)data.setUint32(0x4000+4*i,0x11111111*(i+1),true);assert.equal(functions.get('lodsd')(ptr),0);assert.deepEqual(state(),[0x1002,0,0x400c,0x5000,7,1,2]);assert.equal(get(8),0x33333333);assert.deepEqual(writes,[]);
init(3,10,2);ram.set([1,2,3],0x4000);ram.set([1,9,3],0x5000);assert.equal(functions.get('cmpsb')(ptr),0);assert.deepEqual(state(),[0x1002,1,0x4002,0x5002,8,1,0x97]);init(3,10,2);word(8,0x12345607);ram.set([1,7,9],0x5000);assert.equal(functions.get('scasb')(ptr),0);assert.deepEqual(state(),[0x1002,1,0x4000,0x5002,8,1,0x46]);assert.equal(get(8),0x12345607);
init(3,10,2);ram.set([1,2,3],0x4000);ram.set([1,9,3],0x5000);readFault=0x5001;assert.equal(functions.get('cmpsb')(ptr),3);assert.deepEqual(state(),[0x1000,2,0x4001,0x5001,9,0,0x46]);assert.deepEqual(reads,[0x4000,0x5000,0x4001,0x5001]);
init(0xabcd0003,10,2);word(36,0x1234fffe);assert.equal(functions.get('stosd16')(ptr),0);assert.deepEqual(state(),[0x1003,0xabcd0000,0x4000,0x1234000a,7,1,2]);assert.deepEqual(writes,[0xfffe,2,6]);for(const at of[0xfffe,2,6])assert.equal(data.getUint32(at,true),0x11223344);
init(1,10,2);word(32,0x3000);word(116,0x1000);data.setUint32(0x4000,0x87654321,true);assert.equal(functions.get('movsd-fs')(ptr),0);assert.deepEqual(state(),[0x1003,0,0x3004,0x5004,9,1,2]);assert.equal(data.getUint32(0x5000,true),0x87654321);assert.deepEqual(reads,[0x4000]);
for(const[name,width,pc]of[['stosb',1,0x1002],['stosw',2,0x1003]]){init(2,10,2);assert.equal(functions.get(name)(ptr),0);assert.deepEqual(state(),[pc,0,0x4000,0x5000+2*width,8,1,2]);assert.equal(width===1?data.getUint8(0x5000):data.getUint16(0x5000,true),width===1?0x44:0x3344);}
for(const name of['single-stosd','single-movsd']){init(7,1,2);data.setUint32(0x4000,0x87654321,true);assert.equal(functions.get(name)(ptr),0);assert.deepEqual(state(),[0x1001,7,name.includes('movs')?0x4004:0x4000,0x5004,0,1,2]);assert.equal(data.getUint32(0x5000,true),name.includes('movs')?0x87654321:0x11223344);}
// fixed baseline states for unsigned retirement/quota wrap. these assertions
// require the current eager implementation too; no optimization flag is used.
const retirementWrapRows = [
  {label:'ordinary initial sum wraps',name:'single-stosd',count:7,quota:3,retired:0xfffffffe,status:0,
    want:[0x1001,7,0x4000,0x5004,2,0xffffffff,2],writes:[0x5000]},
  {label:'ordinary retired wraps',name:'single-stosd',count:7,quota:1,retired:0xffffffff,status:0,
    want:[0x1001,7,0x4000,0x5004,0,0,2],writes:[0x5000]},
  {label:'large remaining sum wraps without a long loop',name:'single-stosd',count:7,quota:0xffffffff,retired:5,status:0,
    want:[0x1001,7,0x4000,0x5004,0xfffffffe,6,2],writes:[0x5000]},
  {label:'REP partial budget keeps nonzero retired',name:'stosd',count:5,quota:2,retired:0xfffffffe,status:2,
    want:[0x1000,3,0x4000,0x5008,0,0xfffffffe,2],writes:[0x5000,0x5004]},
  {label:'REP final iteration wraps retired once',name:'stosd',count:3,quota:5,retired:0xffffffff,status:0,
    want:[0x1002,0,0x4000,0x500c,2,0,2],writes:[0x5000,0x5004,0x5008]},
  {label:'zero count retires once without checking unusable operands',name:'stosd',count:0,quota:1,retired:0xffffffff,status:0,unusable:true,
    want:[0x1002,0,0x4000,0xffffffff,0,0,2],writes:[]},
  {label:'zero budget preserves unsigned retired before REP',name:'stosd',count:3,quota:0,retired:0xffffffff,status:2,
    want:[0x1000,3,0x4000,0x5000,0,0xffffffff,2],writes:[]},
  {label:'first destination fault consumes no work or retirement',name:'stosd',count:4,quota:3,retired:0xfffffffe,status:3,writeFault:0x5000,
    want:[0x1000,4,0x4000,0x5000,3,0xfffffffe,2],writes:[],attempts:[0x5000],fault:0x5000},
  {label:'late destination fault retains completed REP work only',name:'stosd',count:4,quota:5,retired:0xffffffff,status:3,writeFault:0x5004,
    want:[0x1000,3,0x4000,0x5004,4,0xffffffff,2],writes:[0x5000],attempts:[0x5000,0x5004],fault:0x5004},
  {label:'late source fault publishes prior work and nonzero retired',name:'movsd',count:3,quota:3,retired:0xfffffffe,status:3,readFault:0x4004,
    want:[0x1000,2,0x4004,0x5004,2,0xfffffffe,2],reads:[0x4000,0x4004],writes:[0x5000],fault:0x4004},
  {label:'late MOVS destination fault retains source-before-destination order',name:'movsd',count:3,quota:3,retired:0xffffffff,status:3,writeFault:0x5004,
    want:[0x1000,2,0x4004,0x5004,2,0xffffffff,2],reads:[0x4000,0x4004],writes:[0x5000],attempts:[0x5000,0x5004],fault:0x5004},
  {label:'inner SMC publishes partial work without retirement',name:'stosd',count:3,quota:5,retired:0xffffffff,status:6,smcAt:1,
    want:[0x1000,2,0x4000,0x5004,4,0xffffffff,2],writes:[0x5000]},
  {label:'final SMC wraps retired after the completed opcode',name:'stosd',count:1,quota:2,retired:0xffffffff,status:6,smcAt:1,
    want:[0x1002,0,0x4000,0x5004,1,0,2],writes:[0x5000]},
  {label:'early compare termination retires despite nonzero ECX',name:'cmpsb',count:3,quota:10,retired:0xffffffff,status:0,
    want:[0x1002,1,0x4002,0x5002,8,0,0x97],reads:[0x4000,0x5000,0x4001,0x5001],writes:[]},
];
for (const row of retirementWrapRows) {
  init(row.count,row.quota,2);word(56,row.retired);
  if(row.unusable){word(36,0xffffffff);word(152,0);}
  if(row.name==='movsd'){data.setUint32(0x4000,0x87654321,true);data.setUint32(0x4004,0xfedcba98,true);}
  if(row.name==='cmpsb'){ram.set([1,2,3],0x4000);ram.set([1,9,3],0x5000);}
  readFault=row.readFault??-1;writeFault=row.writeFault??-1;smcAt=row.smcAt??-1;
  const expected=ram.slice(),d=new DataView(expected.buffer);
  [40,12,32,36,48,56,44].forEach((at,i)=>d.setUint32(ptr+at,row.want[i]>>>0,true));
  d.setUint32(ptr+52,row.status,true);if(row.fault!==undefined)d.setUint32(ptr+60,row.fault,true);
  for(const address of row.writes)d.setUint32(address,row.name==='movsd'?0x87654321:0x11223344,true);
  assert.equal(functions.get(row.name)(ptr),row.status,row.label);
  assert.deepEqual(ram,expected,row.label+': entire RAM/slab, unsigned counters and committed memory prefix');
  assert.deepEqual(reads,row.reads??[],row.label+': exact reads');
  assert.deepEqual(writes,row.writes,row.label+': exact completed writes');
  assert.deepEqual(attempts,row.attempts??row.writes,row.label+': exact destination attempts');
}

console.log('generated REP/string quota, zero count, ordered fault, overlap/DF, early compare,16-bit address and partial/final SMC passed');
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
