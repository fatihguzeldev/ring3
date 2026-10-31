use super::*;
use crate::memory::{PageRange, Permissions};

#[test]
fn generated_x87_transport_preserves_raw80_stack_metadata_and_fault_boundaries() {
    use std::{
        fs,
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };
    let scratch = std::env::temp_dir().join(format!(
        "ring3-runtime-x87-{}-{}",
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
    let mut names = Vec::new();
    for (name, bytes) in [
        ("one", vec![0xd9, 0xe8]),
        ("zero", vec![0xd9, 0xee]),
        ("load", vec![0xd9, 0x05, 0x00, 0x30, 0x00, 0x00]),
        ("fs-load", vec![0x64, 0xd9, 0x05, 0x00, 0x30, 0x00, 0x00]),
        ("store", vec![0xd9, 0x15, 0x04, 0x30, 0x00, 0x00]),
        ("pop-store", vec![0xd9, 0x1d, 0x04, 0x30, 0x00, 0x00]),
        (
            "fs-pop-store",
            vec![0x64, 0xd9, 0x1d, 0x04, 0x30, 0x00, 0x00],
        ),
        ("status", vec![0xdf, 0xe0]),
        ("control-store", vec![0xd9, 0x3d, 0x08, 0x30, 0x00, 0x00]),
        ("control-load", vec![0xd9, 0x2d, 0x0a, 0x30, 0x00, 0x00]),
        ("status-store", vec![0xdd, 0x3d, 0x0c, 0x30, 0x00, 0x00]),
    ] {
        memory.write(GuestAddress(0x1000), &bytes).unwrap();
        let unit = compile(&memory, GuestAddress(0x1000), 1).unwrap();
        fs::write(scratch.join(format!("{name}.wasm")), unit.wasm).unwrap();
        names.push(name.to_string());
    }
    for (name, bytes, limit) in [
        (
            "integer-prefix",
            vec![0xb8, 0x78, 0x56, 0x34, 0x12, 0xd9, 0xe8],
            2,
        ),
        (
            "control-prefix",
            vec![0xd9, 0xe8, 0xd9, 0x2d, 0x0a, 0x30, 0, 0, 0xd9, 0xe8],
            3,
        ),
    ] {
        memory.write(GuestAddress(0x1000), &bytes).unwrap();
        let unit = compile(&memory, GuestAddress(0x1000), limit).unwrap();
        fs::write(scratch.join(format!("{name}.wasm")), unit.wasm).unwrap();
        names.push(name.to_string());
    }
    for index in 0..8_u8 {
        for (prefix, opcode) in [
            ("exchange", [0xd9, 0xc8 + index]),
            ("copy", [0xdd, 0xd0 + index]),
            ("pop", [0xdd, 0xd8 + index]),
            ("dup", [0xd9, 0xc0 + index]),
        ] {
            let name = format!("{prefix}-{index}");
            memory.write(GuestAddress(0x1000), &opcode).unwrap();
            let unit = compile(&memory, GuestAddress(0x1000), 1).unwrap();
            fs::write(scratch.join(format!("{name}.wasm")), unit.wasm).unwrap();
            names.push(name);
        }
    }
    fs::write(scratch.join("names.txt"), names.join("\n")).unwrap();
    let result = Command::new("node").arg("--input-type=module").arg("-e").arg(r#"
import assert from 'node:assert/strict';
import fs from 'node:fs';
const root=process.argv[1],memory=new WebAssembly.Memory({initial:1}),data=new DataView(memory.buffer),state=0x8000,view=0x9800,fp=0xa000;
const word=(at,v)=>data.setUint32(state+at,v>>>0,true),get=at=>data.getUint32(state+at,true),put=(at,v)=>data.setUint32(at,v>>>0,true);
const u16=(at,v)=>data.setUint16(fp+at,v,true),get16=at=>data.getUint16(fp+at,true),bytes=at=>Array.from(new Uint8Array(memory.buffer,at,100));
let reads=0,writes=0,readFault=false,writeFault=false,smc=false;
const imports={env:{memory},cpu:{read(context,address,width){assert.equal(context,state);reads++;address>>>=0;if(readFault){word(52,3);return 0;}word(52,0);return width===2?data.getUint16(address,true):data.getUint32(address,true);},write(context,address,width,value){assert.equal(context,state);writes++;address>>>=0;if(writeFault){word(52,3);return 3;}if(width===2)data.setUint16(address,value,true);else data.setUint32(address,value>>>0,true);word(52,smc?6:0);return smc?6:0;}}};
const run=new Map();for(const name of fs.readFileSync(`${root}/names.txt`,'utf8').split('\n'))run.set(name,(await WebAssembly.instantiate(fs.readFileSync(`${root}/${name}.wasm`),imports)).instance.exports.run);
function raw(significand,exponent){const b=new Uint8Array(10),d=new DataView(b.buffer);d.setBigUint64(0,significand,true);d.setUint16(8,exponent,true);return Array.from(b);}
const ONE=raw(0x8000000000000000n,0x3fff),ZERO=raw(0n,0),NEGZERO=raw(0n,0x8000),EPS=raw(0x8000010000000000n,0x3fff),WIDE=raw(0xabcdef0123456789n,0x4123);
function init(top=0,cw=0x37f,status=0x4700){new Uint8Array(memory.buffer).fill(0);word(0,1);word(4,160);[0xaabbccdd,2,3,4,0x4800,6,7,8].forEach((v,i)=>word(8+i*4,v));word(40,0x1000);word(44,0xcd7);word(48,1);word(124,view);word(116,0x1000);word(152,63);put(view,1);put(view+4,36);put(view+8,fp);[0x23,0x1b,0x23,0x23,0x3b,0x43].forEach((v,i)=>put(view+12+i*4,v));u16(0,cw);u16(2,(status&~0x3800)|(top<<11));u16(4,0xffff);u16(6,0x4321);data.setUint32(fp+8,0x12345678,true);data.setUint32(fp+12,0x87654321,true);u16(16,0x71);u16(18,0x72);for(let i=0;i<80;i++)data.setUint8(fp+20+i,0x80+i);reads=writes=0;readFault=writeFault=smc=false;}
function setSlot(physical,payload,tag=payload.slice(0,8).every(x=>x===0)?1:0){new Uint8Array(memory.buffer,fp+20+physical*10,10).set(payload);u16(4,(get16(4)&~(3<<(physical*2)))|(tag<<(physical*2)));}
function expected(before,{status,tag,control,opcode,fip,fdp,cs,ds,slots=[]}={}){const b=Uint8Array.from(before),d=new DataView(b.buffer);for(const[at,v]of[[0,control],[2,status],[4,tag],[6,opcode],[16,cs],[18,ds]])if(v!==undefined)d.setUint16(at,v,true);if(fip!==undefined)d.setUint32(8,fip,true);if(fdp!==undefined)d.setUint32(12,fdp,true);for(const[payload,index]of slots)b.set(payload,20+index*10);return Array.from(b);}
function assertFP(value,label){assert.deepEqual(bytes(fp),value,label);}
function finish(name,next,stop=0){const flags=get(44),gpr=Array.from({length:8},(_,i)=>get(8+4*i));assert.equal(run.get(name)(state),stop,name);assert.equal(get(52),stop);assert.equal(get(40),next);assert.equal(get(48),0);assert.equal(get(56),1);assert.equal(get(44),flags);if(name!=='status')assert.deepEqual(Array.from({length:8},(_,i)=>get(8+4*i)),gpr);}
function miss(name){const before=bytes(fp),gpr=Array.from({length:8},(_,i)=>get(8+4*i));assert.equal(run.get(name)(state),7,name);assert.equal(get(52),7);assert.equal(get(40),0x1000);assert.equal(get(48),1);assert.equal(get(56),0);assert.equal(get(44),0xcd7);assert.deepEqual(Array.from({length:8},(_,i)=>get(8+4*i)),gpr);assertFP(before,name);}
for(const cw of[0x003f,0x007f,0x037f,0x077f,0x0b7f,0x0f7f])for(let top=0;top<8;top++)for(const[name,payload,zero]of[['one',ONE,0],['zero',ZERO,1]]){init(top,cw,0x4763);const before=bytes(fp),dest=(top+7)&7,tag=(0xffff&~(3<<(dest*2)))|(zero<<(dest*2));finish(name,0x1002);assertFP(expected(before,{status:((0x4763&~0x3a00)|(dest<<11)),tag,opcode:name==='one'?0x1e8:0x1ee,fip:0x1000,cs:0x1b,slots:[[payload,dest]]}),`${name}top${top}CW${cw}`);}
for(const[bits,payload]of[[0x3f800001,EPS],[0, ZERO],[0x80000000,NEGZERO],[0x00800000,raw(0x8000000000000000n,0x3f81)],[0x7f7fffff,raw(0xffffff0000000000n,0x407e)]])for(const fsPrefix of[false,true]){init(0);put(fsPrefix?0x4000:0x3000,bits);const before=bytes(fp),dest=7,tag=(0xffff&~(3<<14))|((bits&0x7fffffff?0:1)<<14);finish(fsPrefix?'fs-load':'load',fsPrefix?0x1007:0x1006);assertFP(expected(before,{status:0x7d00,tag,opcode:0x105,fip:0x1000,cs:0x1b,fdp:0x3000,ds:fsPrefix?0x3b:0x23,slots:[[payload,dest]]}));assert.equal(reads,1);assert.equal(writes,0);}
for(let top=0;top<8;top++)for(let index=0;index<8;index++){init(top);const other=(top+index)&7;setSlot(top,WIDE,2);if(index)setSlot(other,NEGZERO,2);const before=bytes(fp),oldStatus=get16(2),oldTag=get16(4),a=index?NEGZERO:WIDE,b=WIDE;finish(`exchange-${index}`,0x1002);let tag=oldTag;tag=(tag&~(3<<(top*2)))|((index?1:0)<<(top*2));tag=(tag&~(3<<(other*2)));assertFP(expected(before,{status:oldStatus&~0x200,tag,opcode:0x1c8+index,fip:0x1000,cs:0x1b,slots:[[a,top],[b,other]]}));}
for(let top=0;top<8;top++)for(let index=0;index<8;index++)for(const popping of[false,true]){init(top);setSlot(top,WIDE,2);const before=bytes(fp),dest=(top+index)&7,oldTag=get16(4),oldStatus=get16(2);let tag=oldTag&~(3<<(dest*2)),status=oldStatus;if(popping){tag|=3<<(top*2);status=(status&~0x3800)|(((top+1)&7)<<11);}finish(`${popping?'pop':'copy'}-${index}`,0x1002);assertFP(expected(before,{tag,status,opcode:(popping?0x5d8:0x5d0)+index,fip:0x1000,cs:0x1b,slots:[[WIDE,dest]]}));}
for(let top=0;top<8;top++)for(let index=0;index<7;index++){init(top);const source=(top+index)&7,dest=(top+7)&7;setSlot(source,WIDE,2);const before=bytes(fp),oldStatus=get16(2),tag=get16(4)&~(3<<(dest*2));finish(`dup-${index}`,0x1002);assertFP(expected(before,{tag,status:(oldStatus&~0x3800)|(dest<<11),opcode:0x1c0+index,fip:0x1000,cs:0x1b,slots:[[WIDE,dest]]}));}
for(const[payload,bits]of[[EPS,0x3f800001],[NEGZERO,0x80000000],[ZERO,0]])for(const[name,popping,fsPrefix]of[['store',false,false],['pop-store',true,false],['fs-pop-store',true,true]]){init(7);setSlot(7,payload);const before=bytes(fp),tag=get16(4),status=get16(2);finish(name,fsPrefix?0x1007:0x1006);assert.equal(data.getUint32(fsPrefix?0x4004:0x3004,true),bits);assertFP(expected(before,{status:popping?status&~0x3a00:status&~0x200,tag:popping?tag|0xc000:tag,opcode:popping?0x11d:0x115,fip:0x1000,cs:0x1b,fdp:0x3004,ds:fsPrefix?0x3b:0x23}));assert.equal(writes,1);}
init(3);let before=bytes(fp),status=get16(2);finish('status',0x1002);assert.equal(get(8),(0xaabb0000|status)>>>0);assertFP(before);assert.equal(reads+writes,0);
for(const[name,at,wordValue]of[['control-store',0x3008,0x37f],['status-store',0x300c,0x4700]]){init();const before=bytes(fp);finish(name,0x1006);assert.equal(data.getUint16(at,true),wordValue);assertFP(before);assert.equal(writes,1);}
for(const[oldStatus,newControl,newStatus]of[[0x4763,0x035f,0xc7e3],[0x8080,0x007f,0]]){init(0,0x37f,oldStatus);data.setUint16(0x300a,newControl,true);const before=bytes(fp);finish('control-load',0x1006);assertFP(expected(before,{control:newControl,status:newStatus}));assert.equal(reads,1);}
for(const bits of[1,0x007fffff,0x80000001,0x7f800000,0xff800000,0x7fc01234,0x7f801234]){init();put(0x3000,bits);miss('load');assert.equal(reads,1);assert.equal(writes,0);}
for(const payload of[raw(0x8000000000000001n,0x3fff),raw(0x8000000000000000n,0x3f80),raw(0x8000000000000000n,0x407f),raw(0x8000000000000000n,0x7fff),raw(1n,0)]){init();setSlot(0,payload);miss('pop-store');assert.equal(writes,0);}
init();miss('pop-store');assert.equal(writes,0);
init();u16(4,0);miss('one');
init();setSlot(0,ONE);miss('exchange-1');
init(0,0x37e,1);readFault=true;miss('load');assert.equal(reads,0);
for(const mutate of[()=>word(124,0),()=>word(124,0xfffffff0),()=>word(124,view+1),()=>put(view,2),()=>put(view+4,32),()=>put(view+8,0),()=>put(view+8,0xffd0),()=>put(view+8,fp+1)]){init();mutate();miss('one');assert.equal(reads+writes,0);}
// Invalid numerical pointers belong to the first reached numeric instruction,
// after the completed integer prefix and its exact budget boundary.
for(const mutate of[()=>word(124,0),()=>word(124,0xfffffff0),()=>word(124,view+1),()=>put(view,2),()=>put(view+4,32),()=>put(view+8,0),()=>put(view+8,0xffd0),()=>put(view+8,fp+1)])for(const quota of[0,1,2]){
  init();word(48,quota);mutate();const want=new Uint8Array(memory.buffer).slice(),d=new DataView(want.buffer),completed=quota===0?0:1;
  for(const[at,value]of[[8,completed?0x12345678:get(8)],[40,0x1000+completed*5],[48,quota-completed],[52,quota===2?7:2],[56,completed]])d.setUint32(state+at,value,true);
  assert.equal(run.get('integer-prefix')(state),quota===2?7:2);
  assert.deepEqual(new Uint8Array(memory.buffer),want,'integer prefix and unreached malformed view preserve exact full-memory state');
  assert.equal(reads+writes,0);
}
// A control load inside one invocation must affect the next pending-MF check.
init(0,0x037f,1);word(48,3);data.setUint16(0x300a,0x037e,true);before=bytes(fp);
assert.equal(run.get('control-prefix')(state),7);
assert.deepEqual([get(40),get(48),get(56)],[0x1008,1,2]);
assertFP(expected(before,{control:0x037e,status:0xb881,tag:0x3fff,opcode:0x1e8,fip:0x1000,cs:0x1b,slots:[[ONE,7]]}));
assert.deepEqual([reads,writes],[1,0]);
// The same exported function starts with fresh pointer proofs on every call.
init();const original=bytes(fp);finish('one',0x1002);const firstBank=bytes(fp),otherView=0x9900,otherFP=0xa100;
new Uint8Array(memory.buffer,otherView,36).set(new Uint8Array(memory.buffer,view,36));put(otherView+8,otherFP);new Uint8Array(memory.buffer,otherFP,100).set(original);
word(124,otherView);word(40,0x1000);word(48,1);word(52,0);word(56,0);finish('one',0x1002);
assertFP(firstBank,'a subsequent invocation must not reuse the previous bank pointer');
assert.deepEqual(bytes(otherFP),expected(original,{status:0x7d00,tag:0x3fff,opcode:0x1e8,fip:0x1000,cs:0x1b,slots:[[ONE,7]]}));
const secondBank=bytes(otherFP);word(124,0);word(40,0x1000);word(48,1);word(52,0);word(56,0);miss('one');assert.deepEqual(bytes(otherFP),secondBank);
for(const name of['load','pop-store','control-load']){init();setSlot(0,ONE);const before=bytes(fp);readFault=name!=='pop-store';writeFault=name==='pop-store';assert.equal(run.get(name)(state),3);assert.equal(get(40),0x1000);assert.equal(get(48),1);assert.equal(get(56),0);assertFP(before);}
init(7);setSlot(7,ONE);before=bytes(fp);smc=true;finish('pop-store',0x1006,6);assert.equal(writes,1);assert.equal(data.getUint32(0x3004,true),0x3f800000);assertFP(expected(before,{status:get16(2),tag:0xffff,opcode:0x11d,fip:0x1000,cs:0x1b,fdp:0x3004,ds:0x23}));assert.equal(get16(2),0x4500);
console.log('inline x87 raw80 normal/zero transport, allTOP/index stack ghost/tag/C1, metadata/selectors/control/status, failclosed view/special/pending/fault andcompletedSMC passed');
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
fn generated_x87_binary64_transport_uses_whole_canonical_spans_before_fp_commit() {
    use std::{
        fs,
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };
    let scratch = std::env::temp_dir().join(format!(
        "ring3-runtime-x87-f64-{}-{}",
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
        ("load", &[0xdd, 0x03][..], 1),
        ("fs-load", &[0x64, 0xdd, 0x03][..], 1),
        ("store", &[0xdd, 0x13][..], 1),
        ("pop-store", &[0xdd, 0x1b][..], 1),
        ("fs-pop-store", &[0x64, 0xdd, 0x1b][..], 1),
        ("twice", &[0xdd, 0x13, 0xdd, 0x13][..], 2),
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
const root=process.argv[1],memory=new WebAssembly.Memory({initial:4}),ram=new Uint8Array(memory.buffer),data=new DataView(memory.buffer);
const ptr=0x8000,bridge=0xa000,fp=0xb000,map=0x1000,permissions=0x2000,executable=0x3000,cow=0x4000,dirty=0x5000,slots=0x6000,version=0x7000,backing=0x10000;
const word=(at,value)=>data.setUint32(ptr+at,value>>>0,true),get=at=>data.getUint32(ptr+at,true),put=(at,value)=>data.setUint32(at,value>>>0,true),half=(at,value)=>data.setUint16(fp+at,value,true),short=at=>data.getUint16(fp+at,true);
const bank=()=>[...ram.slice(fp,fp+100)],guest=()=>[...ram.slice(backing,backing+4*4096)],regs=()=>Array.from({length:8},(_,i)=>get(8+i*4));
let imports=0;
const functions=new Map();for(const name of['load','fs-load','store','pop-store','fs-pop-store','twice'])functions.set(name,(await WebAssembly.instantiate(fs.readFileSync(`${root}/${name}.wasm`),{env:{memory},cpu:{read(){imports++;throw new Error('wide x87 cannot split or delegate a read');},write(){imports++;throw new Error('wide x87 cannot split or delegate a store');}}})).instance.exports.run);
function raw(significand,exponent){const bytes=new Uint8Array(10),d=new DataView(bytes.buffer);d.setBigUint64(0,significand,true);d.setUint16(8,exponent,true);return[...bytes];}
// Binary64 boundary literals are independent of the emitted pack/unpack code.
const values=[
 [0n,raw(0n,0)], [0x8000000000000000n,raw(0n,0x8000)],
 [0x3ff0000000000000n,raw(0x8000000000000000n,0x3fff)],
 [0x3ff0000000000001n,raw(0x8000000000000800n,0x3fff)],
 [0xc02a000000000000n,raw(0xd000000000000000n,0xc002)],
 [0x0010000000000000n,raw(0x8000000000000000n,0x3c01)],
 [0x7fefffffffffffffn,raw(0xfffffffffffff800n,0x43fe)],
];
function init(top=0,cw=0x023f,address=0x3000,fsPrefix=false,quota=1){ram.fill(0);word(0,1);word(4,160);[0xaabbccdd,0x12345678,0x87654321,address,0x4800,6,7,8].forEach((v,i)=>word(8+4*i,v));word(40,0x1000);word(44,0xcd7);word(48,quota);word(124,bridge);word(152,63);if(fsPrefix)word(116,0x1000);put(bridge,1);put(bridge+4,36);put(bridge+8,fp);[0x23,0x1b,0x23,0x23,0x3b,0x43].forEach((v,i)=>put(bridge+12+i*4,v));half(0,cw);half(2,(top<<11)|0x4220);half(4,0xffff);half(6,0x321);put(fp+8,0x11223344);put(fp+12,0x55667788);half(16,0x71);half(18,0x73);for(let i=0;i<80;i++)data.setUint8(fp+20+i,0x80+i);for(const[at,v]of[[76,map],[80,permissions],[84,backing],[88,4],[92,executable],[96,cow],[100,dirty],[104,slots],[112,4],[156,version]])word(at,v);put(map+3*4,1);put(map+4*4,2);data.setUint8(permissions+3,3);data.setUint8(permissions+4,3);data.setBigUint64(version,10n,true);for(let i=0;i<4*4096;i++)ram[backing+i]=(i*13+0x51)&255;imports=0;}
function slot(index,payload){ram.set(payload,fp+20+10*index);half(4,(short(4)&~(3<<(index*2)))|((payload.slice(0,8).every(x=>x===0)?1:0)<<(index*2)));}
function expected(before,{status,tag,opcode,fdp,ds,fip=0x1000,slots=[]}){const bytes=Uint8Array.from(before),d=new DataView(bytes.buffer);for(const[at,value]of[[2,status],[4,tag],[6,opcode],[16,0x1b],[18,ds]])if(value!==undefined)d.setUint16(at,value,true);d.setUint32(8,fip,true);if(fdp!==undefined)d.setUint32(12,fdp,true);for(const[payload,index]of slots)bytes.set(payload,20+10*index);return[...bytes];}
function done(name,next=0x1002,retired=1){const beforeRegs=regs();assert.equal(functions.get(name)(ptr),0,name);assert.deepEqual(regs(),beforeRegs);assert.deepEqual([get(40),get(44),get(48),get(52),get(56)],[next,0xcd7,0,0,retired]);assert.equal(imports,0);}
function miss(name){const before=bank(),beforeGuest=guest(),beforeRegs=regs(),length=get(108),bits=[...ram.slice(dirty,dirty+4)],queue=[...ram.slice(slots,slots+16)];assert.equal(functions.get(name)(ptr),7,name);assert.deepEqual(bank(),before);assert.deepEqual(guest(),beforeGuest);assert.deepEqual(regs(),beforeRegs);assert.deepEqual([get(40),get(44),get(48),get(52),get(56)],[0x1000,0xcd7,1,7,0]);assert.equal(get(108),length);assert.deepEqual([...ram.slice(dirty,dirty+4)],bits);assert.deepEqual([...ram.slice(slots,slots+16)],queue);assert.equal(imports,0);}
for(const cw of[0x003f,0x007f,0x023f,0x027f,0x037f,0x0b7f])for(const[bits,payload]of values)for(const[top,address,fsPrefix]of[[0,0x3000,false],[7,0x3001,false],[3,0x3ff8,false],[4,0x2001,true]]){init(top,cw,address,fsPrefix);const linear=address+(fsPrefix?0x1000:0),host=backing+(linear-0x3000);data.setBigUint64(host,bits,true);const before=bank(),beforeGuest=guest(),dest=(top+7)&7,newTag=(0xffff&~(3<<(dest*2)))|((bits&0x7fffffffffffffffn?0:1)<<(dest*2));done(fsPrefix?'fs-load':'load',fsPrefix?0x1003:0x1002);assert.deepEqual(bank(),expected(before,{status:0x4020|(dest<<11),tag:newTag,opcode:0x503,fdp:address,ds:fsPrefix?0x3b:0x23,slots:[[payload,dest]]}));assert.deepEqual(guest(),beforeGuest);assert.equal(get(108),0);}
for(const[bits,payload]of values)for(const[name,pop,fsPrefix]of[['store',false,false],['pop-store',true,false],['fs-pop-store',true,true]])for(const[top,address]of[[0,0x3000],[7,0x3001],[3,0x3ff8]]){const logical=fsPrefix?address-0x1000:address;init(top,0x023f,logical,fsPrefix);slot(top,payload);const before=bank(),beforeGuest=guest(),tag=short(4),expectedGuest=Uint8Array.from(beforeGuest);new DataView(expectedGuest.buffer).setBigUint64(address-0x3000,bits,true);done(name,fsPrefix?0x1003:0x1002);assert.deepEqual(guest(),[...expectedGuest]);assert.deepEqual(bank(),expected(before,{status:0x4020|(((top+(pop?1:0))&7)<<11),tag:pop?tag|(3<<(top*2)):tag,opcode:pop?0x51b:0x513,fdp:logical,ds:fsPrefix?0x3b:0x23}));assert.equal(get(108),1);assert.equal(data.getUint8(dirty),1);assert.equal(data.getUint32(slots,true),0);assert.equal(data.getBigUint64(version,true),10n);}
init(7,0x023f,0x3001,false,2);slot(7,values[3][1]);const twiceBefore=bank();done('twice',0x1004,2);assert.deepEqual(bank(),expected(twiceBefore,{status:0x7820,tag:short(4),opcode:0x513,fip:0x1002,fdp:0x3001,ds:0x23}));assert.equal(get(108),1);
for(const bits of[1n,0x000fffffffffffffn,0x8000000000000001n,0x7ff0000000000000n,0xfff0000000000000n,0x7ff8000000000123n,0x7ff0000000000123n]){init();data.setBigUint64(backing,bits,true);miss('load');}
for(const payload of[raw(0x8000000000000001n,0x3fff),raw(0x8000000000000000n,0x3c00),raw(0x8000000000000000n,0x43ff),raw(0x8000000000000000n,0x7fff),raw(1n,0)]){init();slot(0,payload);miss('pop-store');}
for(const[name,mutate]of[
 ['load',()=>word(76,0)], ['load',()=>put(map+3*4,0)], ['load',()=>data.setUint8(permissions+3,0)], ['load',()=>data.setUint8(permissions+3,11)],
 ['load',()=>word(20,0x3ff9)], ['pop-store',()=>word(20,0x3ff9)],
 ['pop-store',()=>data.setUint8(permissions+3,1)], ['pop-store',()=>put(executable,1)], ['pop-store',()=>put(cow+3*4,1)],
 ['pop-store',()=>data.setBigUint64(version,0xffffffffffffffffn,true)], ['pop-store',()=>word(112,0)], ['pop-store',()=>word(108,5)],
]){init();slot(0,values[2][1]);mutate();miss(name);}
init();miss('pop-store');
init();half(4,0);miss('load');
init();half(0,0x023e);half(2,1);put(map+3*4,0);miss('load');
init();slot(0,values[2][1]);word(20,0xfffffffc);const beforeSegment=bank(),beforeRAM=guest();assert.equal(functions.get('pop-store')(ptr),9);assert.deepEqual(bank(),beforeSegment);assert.deepEqual(guest(),beforeRAM);assert.deepEqual([get(40),get(48),get(56),get(68)],[0x1000,1,0,13]);assert.equal(imports,0);
console.log('binary64 canonical whole-span normal/zero loads/stores, PC53/CW preservation, raw80 boundaries/metadata/TOP/ghost, dirty dedup and preeffect service misses passed');
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
fn x87_wait_owner_changes_only_pending_status_before_precise_mf() {
    use crate::cpu::{
        runtime::numeric::{NumericError, NumericState},
        x86::{SegmentContext, State32, X87Operation},
    };
    let mut memory = AddressSpace::new(1).unwrap();
    let segments = SegmentContext::default();
    let operation = Operation::X87 {
        operation: X87Operation::Wait,
        address: None,
        waiting: true,
        opcode: 0,
    };
    for control in [
        0x003f_u16, 0x007f, 0x023f, 0x037f, 0xffff, 0x003e, 0, 0xffc0,
    ] {
        for exceptions in 0..64_u16 {
            let mut numeric = NumericState::default();
            numeric.x87.control = control;
            numeric.x87.status = 0x7f00 | exceptions;
            numeric.x87.tag = 0xa5a5;
            numeric.x87.opcode = 0x4321;
            numeric.x87.instruction_pointer = 0x12345678;
            numeric.x87.data_pointer = 0x87654321;
            numeric.x87.code_selector = 0x71;
            numeric.x87.data_selector = 0x72;
            for (index, cell) in numeric.x87.registers.iter_mut().enumerate() {
                cell.fill(0x80 + index as u8);
            }
            let mut scalar = State32 {
                registers: [0x12345678, 2, 3, 4, 5, 6, 7, 8],
                eip: 0x1000,
                eflags: 0xcd7,
            };
            let (before, scalar_before) = (numeric.x87, scalar);
            let pending =
                (0..6).any(|bit| exceptions & (1 << bit) != 0 && control & (1 << bit) == 0);
            let result = numeric.execute(&mut memory, &mut scalar, &segments, &operation, 0x1001);
            let mut expected = before;
            if pending {
                expected.status |= 0x8080;
                assert_eq!(
                    result,
                    Err(NumericError::Cpu {
                        vector: 16,
                        detail: u32::from(expected.status)
                    })
                );
                assert_eq!(scalar, scalar_before);
            } else {
                assert!(!result.unwrap().code_invalidated);
                assert_eq!(
                    scalar,
                    State32 {
                        eip: 0x1001,
                        ..scalar_before
                    }
                );
            }
            assert_eq!(
                numeric.x87, expected,
                "control={control:04x} status={:04x}",
                before.status
            );
        }
    }
}

#[test]
fn generated_x87_wait_preserves_banks_metadata_prefixes_and_pending_fault_ownership() {
    use std::{
        fs,
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };
    let scratch = std::env::temp_dir().join(format!(
        "ring3-runtime-x87-wait-{}-{}",
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
    let mut missing_entries = Vec::new();
    for (name, bytes, limit, entries) in [
        ("wait", &[0x9b][..], 1, vec![0x1000]),
        (
            "prefix",
            &[0xb8, 0x78, 0x56, 0x34, 0x12, 0x9b][..],
            2,
            vec![0x1000, 0x1005],
        ),
        ("twice", &[0x9b, 0x9b][..], 2, vec![0x1000, 0x1001]),
        (
            "control",
            &[0xd9, 0x2d, 0x00, 0x30, 0, 0, 0x9b][..],
            2,
            vec![0x1000, 0x1006],
        ),
    ] {
        memory.write(GuestAddress(0x1000), bytes).unwrap();
        let unit = compile(&memory, GuestAddress(0x1000), limit).unwrap();
        if name == "twice" && unit.entries == [0x1000] {
            // unsupported baseline FWAIT terminates decoding before the second WAIT.
            missing_entries.push(name);
        } else {
            assert_eq!(unit.entries, entries, "{name} actual decoder boundaries");
        }
        fs::write(scratch.join(format!("{name}.wasm")), unit.wasm).unwrap();
    }
    let result = Command::new("node").arg("--input-type=module").arg("-e").arg(r#"
import assert from 'node:assert/strict';
import fs from 'node:fs';
const root=process.argv[1],memory=new WebAssembly.Memory({initial:1}),ram=new Uint8Array(memory.buffer),data=new DataView(memory.buffer);
const state=0x8000,view=0x9800,fp=0xa000,word=(at,value)=>data.setUint32(state+at,value>>>0,true),put=(at,value)=>data.setUint32(at,value>>>0,true);
const get=at=>data.getUint32(state+at,true),fp16=(at,value)=>data.setUint16(fp+at,value,true);
let reads=0,writes=0;
const imports={env:{memory},cpu:{read(context,address,width){assert.equal(context,state);assert.equal(address>>>0,0x3000);assert.equal(width,2);reads++;word(52,0);return data.getUint16(0x3000,true);},write(){writes++;throw Error('FWAIT must not write guest memory');}}};
const run=new Map();for(const name of['wait','prefix','twice','control'])run.set(name,(await WebAssembly.instantiate(fs.readFileSync(root+'/'+name+'.wasm'),imports)).instance.exports.run);
const missing=[];
function init(control=0x003f,status=0x4200,tag=0xffff){
  for(let i=0;i<ram.length;i++)ram[i]=(i*13+0x59)&255;
  word(0,1);word(4,160);[0xaabbccdd,2,3,4,0x4800,6,7,8].forEach((v,i)=>word(8+4*i,v));
  word(40,0x1000);word(44,0xcd7);word(48,1);word(52,0);word(56,7);word(124,view);word(152,63);
  word(76,0);word(80,0);word(84,0);
  put(view,1);put(view+4,36);put(view+8,fp);[0x23,0x1b,0x23,0x23,0x3b,0x43].forEach((v,i)=>put(view+12+4*i,v));
  fp16(0,control);fp16(2,status);fp16(4,tag);fp16(6,0x4321);put(fp+8,0x12345678);put(fp+12,0x87654321);fp16(16,0x71);fp16(18,0x72);
  reads=writes=0;
}
function pending(control,status){for(let bit=0;bit<6;bit++)if((status&(1<<bit))&&!(control&(1<<bit)))return true;return false;}
// each expected path names the completed prefix explicitly. before the mechanism
// gate, a baseline's service exit must also preserve the exact whole memory image.
function check(name,paths,label,requireInline=false,expectedReads=0){
  const before=ram.slice(),actual=run.get(name)(state),path=paths[actual];
  assert.ok(path,label+' unexpected exit '+actual);
  const wanted=before.slice(),d=new DataView(wanted.buffer);
  for(const[at,value]of[[40,path.pc],[48,path.remaining],[52,actual],[56,path.retired],...(path.eax===undefined?[]:[[8,path.eax]])])d.setUint32(state+at,value>>>0,true);
  if(path.control!==undefined)d.setUint16(fp,path.control,true);
  if(path.status!==undefined)d.setUint16(fp+2,path.status,true);
  assert.deepEqual(ram,wanted,label+' full scalar/slab/bank/RAM state');
  assert.deepEqual([reads,writes],[expectedReads,0],label+' trusted memory calls');
  if(requireInline&&actual!==0)missing.push(label);
  return actual;
}
const success={pc:0x1001,remaining:0,retired:8},miss={pc:0x1000,remaining:1,retired:7};
for(const control of[0x003f,0x007f,0x023f,0x037f,0xffff,0x003e,0])for(const low of[0,1,0x3f,0x8080,0x8081,0x4200])for(let top=0;top<8;top++){
  const status=(low&~0x3800)|(top<<11),tag=[0xffff,0,0xaaaa,0x5555][top&3];init(control,status,tag);
  const fault=pending(control,status),paths=fault?{7:miss}:{0:success,7:miss};
  check('wait',paths,'CW'+control.toString(16)+' SW'+status.toString(16)+' tag'+tag.toString(16),!fault);
}
for(const quota of[0,1,2]){
  init();word(48,quota);
  const paths=quota===0?{2:{pc:0x1000,remaining:0,retired:7}}:quota===1?{2:{pc:0x1005,remaining:0,retired:8,eax:0x12345678}}:
    {0:{pc:0x1006,remaining:0,retired:9,eax:0x12345678},7:{pc:0x1005,remaining:1,retired:8,eax:0x12345678}};
  check('prefix',paths,'completed integer prefix quota'+quota,quota===2);
}
for(const mutate of[
  ()=>word(124,0),()=>word(124,0xfffffff0),()=>word(124,view+1),()=>put(view,0),()=>put(view,2),
  ()=>put(view+4,0),()=>put(view+4,35),()=>put(view+8,0),()=>put(view+8,fp+1),
  ()=>put(view+8,0xfffffff0),()=>put(view+8,ram.length-96),
])for(const quota of[0,1,2]){
  init();word(48,quota);mutate();
  const paths=quota===0?{2:{pc:0x1000,remaining:0,retired:7}}:quota===1?{2:{pc:0x1005,remaining:0,retired:8,eax:0x12345678}}:
    {7:{pc:0x1005,remaining:1,retired:8,eax:0x12345678}};
  check('prefix',paths,'malformed unreached/reached view quota'+quota);
}
init(0x003e,1);word(48,2);
check('prefix',{7:{pc:0x1005,remaining:1,retired:8,eax:0x12345678}},'pending FWAIT preserves completed integer prefix without publishing8080');
init();word(40,0x1005);
check('prefix',{0:{pc:0x1006,remaining:0,retired:8},7:{pc:0x1005,remaining:1,retired:7}},'arbitrary all-PC landing at FWAIT',true);
init();word(48,1);
const twice=check('twice',{2:{pc:0x1001,remaining:0,retired:8},7:miss},'budget between repeated FWAIT');
if(twice!==2)missing.push('first FWAIT before repeated budget');
init();word(48,2);
check('twice',{0:{pc:0x1002,remaining:0,retired:9},7:{pc:0x1000,remaining:2,retired:7}},'repeated FWAIT one invocation',true);
init(0x003f,0x4201);word(48,2);data.setUint16(0x3000,0x003e,true);
check('control',{7:{pc:0x1006,remaining:1,retired:8,control:0x003e,status:0xc281}},'same-invocation FLDCW changes live pending check',false,1);
for(const prefix of[36,40]){
  init();put(view+4,prefix);
  check('wait',{0:success,7:miss},'view prefix'+prefix,true);
}
init();const lastBank=ram.length-100;ram.copyWithin(lastBank,fp,fp+100);put(view+8,lastBank);
check('wait',{0:success,7:miss},'bank ending exactly at memory boundary',true);
init();const lastView=ram.length-36;ram.copyWithin(lastView,view,view+36);word(124,lastView);
check('wait',{0:success,7:miss},'view36 ending exactly at memory boundary',true);
console.log('FWAIT full-memory architectural paths, pending no-write ownership, masks/TOP/tags/raw ghosts, quota/prefix, malformed pointers, FLDCW and exact bounds passed');
assert.equal(missing.length,0,'mechanism: valid no-pending FWAIT must inline instead of Instruction7; missing '+missing.slice(0,6).join(', '));
"#).arg(&scratch).output().unwrap();
    assert!(
        result.status.success(),
        "fixture {} failed:\n{}\n{}",
        scratch.display(),
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(
        missing_entries.is_empty(),
        "mechanism: inline FWAIT must retain every repeated-WAIT entry: {missing_entries:?}"
    );
    fs::remove_dir_all(scratch).unwrap();
}

// explicit old-compiler baseline; promote the expectation only after separate
// authorization of inline FCHS.
const FCHS_EXPECT_INLINE: bool = true;

#[test]
fn x87_change_sign_owner_preserves_raw_precision_tags_and_pending_priority() {
    use crate::cpu::{
        runtime::numeric::{NumericError, NumericState},
        x86::{SegmentContext, State32, X87Operation, X87State},
    };
    let mut memory = AddressSpace::new(1).unwrap();
    let mut segments = SegmentContext::default();
    segments.state.selectors = [0x23, 0x1b, 0x23, 0x23, 0x3b, 0x43];
    let segments = SegmentContext::legacy(segments.state);
    let operation = Operation::X87 {
        operation: X87Operation::ChangeSign,
        address: None,
        waiting: true,
        opcode: 0x1e0,
    };
    for control in [
        0_u16, 0x40, 0x003f, 0x007f, 0x013f, 0x023f, 0x037f, 0x077f, 0x0b7f, 0x0f7f, 0x003e, 0xffff,
    ] {
        for (sig, exponent) in [
            (0_u64, 0_u16),
            (1 << 63, 0x3fff),
            (0xabcdef0123456789, 0x4123),
            (0x8000000000000001, 1),
            (u64::MAX, 0x7ffe),
        ] {
            for sign in [0_u16, 0x8000] {
                for top in 0..8_u16 {
                    for tag in 0..3_u16 {
                        for status in [0x4500, 0xc7c0] {
                            let mut before = X87State {
                                control,
                                status: status | (top << 11) | (control & 63),
                                tag: 0xffff,
                                opcode: 0x4321,
                                instruction_pointer: 0x12345678,
                                data_pointer: 0x87654321,
                                code_selector: 0x71,
                                data_selector: 0x72,
                                registers: std::array::from_fn(|i| {
                                    std::array::from_fn(|j| (i * 29 + j * 17) as u8)
                                }),
                            };
                            let physical = usize::from(top);
                            before.tag =
                                (before.tag & !(3 << (physical * 2))) | (tag << (physical * 2));
                            before.registers[physical][..8].copy_from_slice(&sig.to_le_bytes());
                            before.registers[physical][8..]
                                .copy_from_slice(&(exponent | sign).to_le_bytes());
                            let mut expected = before;
                            expected.registers[physical][9] ^= 0x80;
                            expected.tag = (before.tag & !(3 << (physical * 2)))
                                | (u16::from(sig == 0) << (physical * 2));
                            expected.status &= !0x200;
                            expected.opcode = 0x1e0;
                            expected.instruction_pointer = 0x1000;
                            expected.code_selector = 0x1b;
                            let mut scalar = State32 {
                                registers: [0x12345678, 2, 3, 4, 0x4800, 6, 7, 8],
                                eip: 0x1000,
                                eflags: 0xcd7,
                            };
                            let scalar_before = scalar;
                            let mut numeric = NumericState::default();
                            numeric.x87 = before;
                            assert!(
                                !numeric
                                    .execute(
                                        &mut memory,
                                        &mut scalar,
                                        &segments,
                                        &operation,
                                        0x1002
                                    )
                                    .unwrap()
                                    .code_invalidated
                            );
                            assert_eq!(
                                numeric.x87, expected,
                                "CW{control:x} sig{sig:x} exp{exponent:x} sign{sign:x} TOP{top} tag{tag}"
                            );
                            assert_eq!(
                                scalar,
                                State32 {
                                    eip: 0x1002,
                                    ..scalar_before
                                }
                            );
                        }
                    }
                }
            }
        }
    }
    for bit in 0..6 {
        for base_control in [0x003f_u16, 0x0f7f] {
            for empty in [false, true] {
                let mut numeric = NumericState::default();
                numeric.x87.control = base_control & !(1 << bit);
                numeric.x87.status = 0x7f00 | (1 << bit);
                numeric.x87.tag = if empty { 0xffff } else { 0x3fff };
                numeric.x87.opcode = 0x4321;
                numeric.x87.instruction_pointer = 0x12345678;
                numeric.x87.data_pointer = 0x87654321;
                numeric.x87.code_selector = 0x71;
                numeric.x87.data_selector = 0x72;
                numeric.x87.registers =
                    std::array::from_fn(|i| std::array::from_fn(|j| (i * 29 + j * 17) as u8));
                let mut scalar = State32 {
                    registers: [1, 2, 3, 4, 0x4800, 6, 7, 8],
                    eip: 0x1000,
                    eflags: 0xcd7,
                };
                let scalar_before = scalar;
                let mut expected = numeric.x87;
                expected.status |= 0x8080;
                assert_eq!(
                    numeric.execute(&mut memory, &mut scalar, &segments, &operation, 0x1002),
                    Err(NumericError::Cpu {
                        vector: 16,
                        detail: u32::from(expected.status)
                    })
                );
                assert_eq!(numeric.x87, expected);
                assert_eq!(scalar, scalar_before);
            }
        }
    }
}

#[test]
fn generated_x87_change_sign_preserves_full_memory_and_service_frontiers() {
    use std::{
        fs,
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };
    let scratch = std::env::temp_dir().join(format!(
        "ring3-runtime-x87-fchs-{}-{}",
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
        ("fchs", &[0xd9, 0xe0][..], 1),
        ("prefix", &[0xb8, 0x78, 0x56, 0x34, 0x12, 0xd9, 0xe0][..], 2),
        ("twice", &[0xd9, 0xe0, 0xd9, 0xe0][..], 2),
    ] {
        memory.write(GuestAddress(0x1000), bytes).unwrap();
        fs::write(
            scratch.join(format!("{name}.wasm")),
            compile(&memory, GuestAddress(0x1000), limit).unwrap().wasm,
        )
        .unwrap();
    }
    let result = Command::new("node").arg("--input-type=module").arg("-e").arg(r#"
import assert from 'node:assert/strict';import fs from 'node:fs';
const root=process.argv[1],inline=process.argv[2]==='1',memory=new WebAssembly.Memory({initial:2}),ram=new Uint8Array(memory.buffer),d=new DataView(memory.buffer),state=0x8000,view=0xa000,fp=0xb000;
const seed=Uint8Array.from({length:ram.length},(_,i)=>(i*13+0x59)&255),word=(at,v)=>d.setUint32(state+at,v>>>0,true),put=(at,v)=>d.setUint32(at,v>>>0,true),u16=(at,v)=>d.setUint16(fp+at,v,true),get=at=>d.getUint32(state+at,true);
let reads=0,writes=0;const imports={env:{memory},cpu:{read(){reads++;throw Error('FCHS has no memory operand');},write(){writes++;throw Error('FCHS cannot write guest RAM');}}},run=new Map();
for(const name of['fchs','prefix','twice'])run.set(name,(await WebAssembly.instantiate(fs.readFileSync(`${root}/${name}.wasm`),imports)).instance.exports.run);
function init(cw=0x003f,top=0,tag=2,sig=0xabcdef0123456789n,exp=0x4123,status=0x4700){ram.set(seed);ram.fill(0,state,state+160);ram.fill(0,view,view+36);ram.fill(0,fp,fp+100);word(0,1);word(4,160);[0xaabbccdd,2,3,4,0x4800,6,7,8].forEach((v,i)=>word(8+i*4,v));word(40,0x1000);word(44,0xcd7);word(48,1);word(56,7);word(124,view);word(152,63);put(view,1);put(view+4,36);put(view+8,fp);[0x23,0x1b,0x23,0x23,0x3b,0x43].forEach((v,i)=>put(view+12+i*4,v));u16(0,cw);u16(2,(status&~0x3800)|(top<<11));u16(4,(0xffff&~(3<<(top*2)))|(tag<<(top*2)));u16(6,0x4321);put(fp+8,0x12345678);put(fp+12,0x87654321);u16(16,0x71);u16(18,0x72);for(let i=0;i<80;i++)ram[fp+20+i]=(i*29+17)&255;d.setBigUint64(fp+20+top*10,sig,true);d.setUint16(fp+28+top*10,exp,true);reads=writes=0;}
function check(name,{status,pc,remaining,retired,eax,steps=0,start=0x1000},label){const wanted=ram.slice(),v=new DataView(wanted.buffer);for(const[at,value]of[[40,pc],[48,remaining],[52,status],[56,retired],...(eax===undefined?[]:[[8,eax]])])v.setUint32(state+at,value>>>0,true);if(steps){const sw=v.getUint16(fp+2,true),top=sw>>>11&7,at=fp+20+top*10,zero=v.getBigUint64(at,true)===0n&&(v.getUint16(at+8,true)&0x7fff)===0;if(steps&1)wanted[at+9]^=0x80;v.setUint16(fp+2,sw&~0x200,true);v.setUint16(fp+4,(v.getUint16(fp+4,true)&~(3<<(top*2)))|((zero?1:0)<<(top*2)),true);v.setUint16(fp+6,0x1e0,true);v.setUint32(fp+8,start+(steps-1)*2,true);v.setUint16(fp+16,0x1b,true);}assert.equal(run.get(name)(state),status,label);assert.deepEqual(ram,wanted,label+' full slab/bank/RAM');assert.deepEqual([reads,writes],[0,0]);}
for(const cw of[0,0x40,0x003f,0x007f,0x013f,0x023f,0x037f,0x077f,0x0b7f,0x0f7f,0x003e,0xffff])for(const[sig,exp]of[[0n,0],[0x8000000000000000n,0x3fff],[0xabcdef0123456789n,0x4123],[0x8000000000000001n,1],[0xffffffffffffffffn,0x7ffe]])for(const sign of[0,0x8000])for(let top=0;top<8;top++)for(const tag of[0,1,2])for(const sw of[0x4500,0xc7c0]){init(cw,top,tag,sig,exp|sign,sw|(cw&63));check('fchs',inline?{status:0,pc:0x1002,remaining:0,retired:8,steps:1}:{status:7,pc:0x1000,remaining:1,retired:7},'CW'+cw.toString(16)+' TOP'+top+' tag'+tag+' normal/zero');}
for(const[sig,exp]of[[0x8000000000000000n,0x7fff],[0xc000000000000001n,0x7fff],[0x8000000000000001n,0x7fff],[1n,0],[0x8000000000000000n,0],[1n,0x3fff],[0n,0x3fff]]){init(0x037f,7,2,sig,exp);check('fchs',{status:7,pc:0x1000,remaining:1,retired:7},'conservative special/unsupported fallback');}
init(0x037f,7,3);check('fchs',{status:7,pc:0x1000,remaining:1,retired:7},'empty fallback');
for(let bit=0;bit<6;bit++)for(const tag of[2,3]){init(0x037f&~(1<<bit),7,tag,0xabcdef0123456789n,0x4123,0x4700|(1<<bit));check('fchs',{status:7,pc:0x1000,remaining:1,retired:7},'pending before payload/domain');}
for(const quota of[0,1,2]){init();word(48,quota);const path=quota===0?{status:2,pc:0x1000,remaining:0,retired:7}:quota===1?{status:2,pc:0x1005,remaining:0,retired:8,eax:0x12345678}:inline?{status:0,pc:0x1007,remaining:0,retired:9,eax:0x12345678,steps:1,start:0x1005}:{status:7,pc:0x1005,remaining:1,retired:8,eax:0x12345678};check('prefix',path,'integer prefix quota'+quota);}
for(const mutate of[()=>word(124,0),()=>word(124,view+1),()=>put(view,2),()=>put(view+4,35),()=>put(view+8,0),()=>put(view+8,fp+1),()=>put(view+8,ram.length-96)])for(const quota of[0,1,2]){init();word(48,quota);mutate();check('prefix',quota===0?{status:2,pc:0x1000,remaining:0,retired:7}:quota===1?{status:2,pc:0x1005,remaining:0,retired:8,eax:0x12345678}:{status:7,pc:0x1005,remaining:1,retired:8,eax:0x12345678},'malformed/unreached numeric view quota'+quota);}
init();word(48,2);check('twice',inline?{status:0,pc:0x1004,remaining:0,retired:9,steps:2}:{status:7,pc:0x1000,remaining:2,retired:7},'two FCHS restore payload sign with canonical retag');
init();word(40,0x1005);check('prefix',inline?{status:0,pc:0x1007,remaining:0,retired:8,steps:1,start:0x1005}:{status:7,pc:0x1005,remaining:1,retired:7},'direct numeric landing');
console.log('FCHS full-memory architecture/frontiers passed; explicit inline expectation='+inline);
"#).arg(&scratch).arg(if FCHS_EXPECT_INLINE { "1" } else { "0" }).output().unwrap();
    assert!(
        result.status.success(),
        "fixture {} failed:\n{}\n{}",
        scratch.display(),
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    fs::remove_dir_all(scratch).unwrap();
}

// ignored baseline fragment for the existing compiler::x87_tests module.
// root must explicitly promote the expectation after authorizing inline FIST/P.
const FIST32_EXPECT_INLINE: bool = false;

fn fist32_bank_bytes(state: crate::cpu::x86::X87State) -> Vec<u8> {
    let mut bytes = vec![0; 100];
    for (at, value) in [
        (0, state.control),
        (2, state.status),
        (4, state.tag),
        (6, state.opcode),
        (16, state.code_selector),
        (18, state.data_selector),
    ] {
        bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
    }
    bytes[8..12].copy_from_slice(&state.instruction_pointer.to_le_bytes());
    bytes[12..16].copy_from_slice(&state.data_pointer.to_le_bytes());
    bytes[20..].copy_from_slice(state.registers.as_flattened());
    bytes
}

fn fist32_bank(
    control: u16,
    status: u16,
    top: u16,
    tag: u16,
    bits: u64,
) -> crate::cpu::x86::X87State {
    use crate::cpu::x86::{Ext80, X87State};
    let mut state = X87State {
        control,
        status: (status & !0x3800) | (top << 11),
        tag: (0xffff & !(3 << (top * 2))) | (tag << (top * 2)),
        opcode: 0x4321,
        instruction_pointer: 0x12345678,
        data_pointer: 0x87654321,
        code_selector: 0x71,
        data_selector: 0x72,
        registers: std::array::from_fn(|i| std::array::from_fn(|j| (i * 29 + j * 17) as u8)),
    };
    state.registers[usize::from(top)] = Ext80::from_f64_bits(bits).0;
    state
}

// literal outputs and new exception/C1 bits, independently fixed at exact dyadic boundaries.
fn fist32_literals() -> Vec<(&'static str, u64, u32, u16, bool)> {
    vec![
        ("positive-zero", 0, 0, 0, true),
        ("negative-zero", 0x8000000000000000, 0, 0, true),
        ("positive-one", 0x3ff0000000000000, 1, 0, true),
        ("negative-one", 0xbff0000000000000, 0xffffffff, 0, true),
        ("positive-half", 0x3fe0000000000000, 0, 0x20, true),
        ("negative-half", 0xbfe0000000000000, 0, 0x20, true),
        ("positive-odd-tie", 0x3ff8000000000000, 2, 0x220, true),
        (
            "negative-odd-tie",
            0xbff8000000000000,
            0xfffffffe,
            0x220,
            true,
        ),
        ("positive-even-tie", 0x4004000000000000, 2, 0x20, true),
        (
            "negative-even-tie",
            0xc004000000000000,
            0xfffffffe,
            0x20,
            true,
        ),
        ("below-odd-tie", 0x3ff7ffffffffffff, 1, 0x20, true),
        ("above-odd-tie", 0x3ff8000000000001, 2, 0x220, true),
        ("above-even-tie", 0x4004000000000001, 3, 0x220, true),
        ("positive-endpoint", 0x41dfffffffc00000, 0x7fffffff, 0, true),
        ("negative-endpoint", 0xc1e0000000000000, 0x80000000, 0, true),
        (
            "above-positive-endpoint-valid",
            0x41dfffffffd00000,
            0x7fffffff,
            0x20,
            true,
        ),
        (
            "positive-endpoint-half-invalid",
            0x41dfffffffe00000,
            0x80000000,
            1,
            false,
        ),
        (
            "negative-endpoint-inner-half",
            0xc1dfffffffe00000,
            0x80000000,
            0x220,
            true,
        ),
        (
            "negative-endpoint-outer-half",
            0xc1e0000000100000,
            0x80000000,
            0x20,
            true,
        ),
        (
            "negative-endpoint-invalid",
            0xc1e0000000180000,
            0x80000000,
            1,
            false,
        ),
        (
            "negative-endpoint-before-half",
            0xc1e00000000fffff,
            0x80000000,
            0x20,
            true,
        ),
        (
            "negative-endpoint-after-half",
            0xc1e0000000100001,
            0x80000000,
            1,
            false,
        ),
        ("smallest-normal-f64", 0x0010000000000000, 0, 0x20, true),
    ]
}

#[test]
fn x87_int32_store_owner_preserves_literal_rounding_source_tags_and_fault_priority() {
    use crate::cpu::{
        runtime::numeric::{NumericError, NumericState},
        x86::{SegmentContext, State32, decode::decode_one},
    };
    let mut memory = AddressSpace::new(3).unwrap();
    for address in [0x1000, 0x3000, 0x4000] {
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(address), 1).unwrap(),
                if address == 0x1000 {
                    Permissions::ALL
                } else {
                    Permissions::READ_WRITE
                },
            )
            .unwrap();
    }
    let mut context = SegmentContext::default();
    context.state.selectors = [0x23, 0x1b, 0x23, 0x23, 0x3b, 0x43];
    let segments = SegmentContext::legacy(context.state);
    let scalar_seed = State32 {
        registers: [0x11223344, 2, 3, 0x3000, 0x4800, 6, 7, 8],
        eip: 0x1000,
        eflags: 0xcd7,
    };
    for pop in [false, true] {
        memory
            .write(GuestAddress(0x1000), &[0xdb, if pop { 0x1b } else { 0x13 }])
            .unwrap();
        let decoded = decode_one(&memory, GuestAddress(0x1000)).unwrap();
        for (label, bits, output, flags, _) in fist32_literals() {
            for control in [0x003f, 0x007f, 0x013f, 0x023f, 0x037f, 0x133f] {
                for top in 0..8_u16 {
                    for tag in 0..3_u16 {
                        for old in [0x4700_u16, 0x472f] {
                            let before = fist32_bank(control, old, top, tag, bits);
                            let mut numeric = NumericState::default();
                            numeric.x87 = before;
                            let vectors = numeric.vectors;
                            let mut scalar = scalar_seed;
                            memory
                                .write(GuestAddress(0x3000), &0xdeadbeef_u32.to_le_bytes())
                                .unwrap();
                            let mut ram = vec![0; 4096];
                            memory.read(GuestAddress(0x3000), &mut ram).unwrap();
                            ram[..4].copy_from_slice(&output.to_le_bytes());
                            assert!(
                                !numeric
                                    .execute(
                                        &mut memory,
                                        &mut scalar,
                                        &segments,
                                        decoded.operation(),
                                        decoded.next_pc().0
                                    )
                                    .unwrap()
                                    .code_invalidated
                            );
                            let mut expected = before;
                            expected.status = (before.status & !0x200) | flags;
                            if pop {
                                expected.tag |= 3 << (top * 2);
                                expected.status =
                                    (expected.status & !0x3800) | (((top + 1) & 7) << 11);
                            }
                            expected.instruction_pointer = 0x1000;
                            expected.data_pointer = 0x3000;
                            expected.opcode = if pop { 0x31b } else { 0x313 };
                            expected.code_selector = 0x1b;
                            expected.data_selector = 0x23;
                            assert_eq!(
                                numeric.x87, expected,
                                "{label} CW{control:x} TOP{top} tag{tag} pop{pop} SW{old:x}"
                            );
                            assert_eq!(numeric.vectors, vectors);
                            assert_eq!(
                                scalar,
                                State32 {
                                    eip: decoded.next_pc().0,
                                    ..scalar_seed
                                }
                            );
                            let mut actual = vec![0; 4096];
                            memory.read(GuestAddress(0x3000), &mut actual).unwrap();
                            assert_eq!(
                                actual, ram,
                                "{label}: only the four destination bytes change"
                            );
                        }
                    }
                }
            }
        }
    }
    // exact integers are independent of precision, rounding mode and exception masks.
    for control in [0_u16, 0x40, 0x003e, 0x001f, 0x043f, 0x083f, 0x0c3f, 0xffff] {
        for (bits, output) in [
            (0_u64, 0_u32),
            (0x8000000000000000, 0),
            (0xbff0000000000000, 0xffffffff),
            (0x41dfffffffc00000, 0x7fffffff),
            (0xc1e0000000000000, 0x80000000),
        ] {
            memory.write(GuestAddress(0x1000), &[0xdb, 0x1b]).unwrap();
            let decoded = decode_one(&memory, GuestAddress(0x1000)).unwrap();
            let before = fist32_bank(control, 0x4700, 7, 2, bits);
            let mut numeric = NumericState::default();
            numeric.x87 = before;
            let mut scalar = scalar_seed;
            numeric
                .execute(
                    &mut memory,
                    &mut scalar,
                    &segments,
                    decoded.operation(),
                    decoded.next_pc().0,
                )
                .unwrap();
            let mut actual = [0; 4];
            memory.read(GuestAddress(0x3000), &mut actual).unwrap();
            assert_eq!(u32::from_le_bytes(actual), output);
            assert_eq!(numeric.x87.status, 0x4500);
            assert_eq!(numeric.x87.control, control);
            assert_eq!(numeric.x87.registers, before.registers);
        }
    }
    // a newly unmasked precision result commits and pops before a later waiting MF.
    memory.write(GuestAddress(0x1000), &[0xdb, 0x1b]).unwrap();
    let decoded = decode_one(&memory, GuestAddress(0x1000)).unwrap();
    let mut numeric = NumericState::default();
    numeric.x87 = fist32_bank(0x001f, 0, 7, 2, 0xbff8000000000000);
    let mut scalar = scalar_seed;
    numeric
        .execute(
            &mut memory,
            &mut scalar,
            &segments,
            decoded.operation(),
            decoded.next_pc().0,
        )
        .unwrap();
    let mut actual = [0; 4];
    memory.read(GuestAddress(0x3000), &mut actual).unwrap();
    assert_eq!(u32::from_le_bytes(actual), 0xfffffffe);
    assert_eq!(numeric.x87.status, 0x82a0);
    assert_eq!(numeric.x87.tag, 0xffff);
    let before = numeric.x87;
    let scalar_before = scalar;
    let wait = Operation::X87 {
        operation: crate::cpu::x86::X87Operation::Wait,
        address: None,
        waiting: true,
        opcode: 0,
    };
    assert_eq!(
        numeric.execute(&mut memory, &mut scalar, &segments, &wait, 0x1003),
        Err(NumericError::Cpu {
            vector: 16,
            detail: 0x82a0
        })
    );
    assert_eq!(numeric.x87, before);
    assert_eq!(scalar, scalar_before);
    // full destination preflight precedes empty/range conversion, but old MF precedes it.
    for empty in [false, true] {
        for pending in [false, true] {
            let mut numeric = NumericState::default();
            numeric.x87 = fist32_bank(
                if pending { 0x003e } else { 0x003f },
                if pending { 1 } else { 0x4700 },
                7,
                if empty { 3 } else { 2 },
                0x41e0000000000000,
            );
            let before = numeric.x87;
            let mut scalar = State32 {
                registers: [0x11223344, 2, 3, 0x3ffe, 0x4800, 6, 7, 8],
                ..scalar_seed
            };
            let scalar_before = scalar;
            memory
                .write(GuestAddress(0x3ffe), &0xdeadbeef_u32.to_le_bytes())
                .unwrap();
            memory
                .protect(
                    PageRange::new(GuestAddress(0x4000), 1).unwrap(),
                    Permissions::READ,
                )
                .unwrap();
            let result = numeric.execute(
                &mut memory,
                &mut scalar,
                &segments,
                decoded.operation(),
                decoded.next_pc().0,
            );
            if pending {
                let mut expected = before;
                expected.status |= 0x8080;
                assert_eq!(
                    result,
                    Err(NumericError::Cpu {
                        vector: 16,
                        detail: u32::from(expected.status)
                    })
                );
                assert_eq!(numeric.x87, expected);
            } else {
                assert!(matches!(result, Err(NumericError::Memory { width: 4, .. })));
                assert_eq!(numeric.x87, before);
            }
            assert_eq!(scalar, scalar_before);
            let mut actual = [0; 4];
            memory.read(GuestAddress(0x3ffe), &mut actual).unwrap();
            assert_eq!(actual, 0xdeadbeef_u32.to_le_bytes());
            memory
                .protect(
                    PageRange::new(GuestAddress(0x4000), 1).unwrap(),
                    Permissions::READ_WRITE,
                )
                .unwrap();
        }
    }
}

#[test]
fn generated_x87_int32_stores_preserve_full_memory_and_service_frontiers() {
    use crate::cpu::{
        runtime::numeric::{NumericError, NumericState},
        x86::{SegmentContext, State32, decode::decode_one},
    };
    use std::{
        fs,
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };
    let scratch = std::env::temp_dir().join(format!(
        "ring3-runtime-fist32-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&scratch).unwrap();
    let mut memory = AddressSpace::new(3).unwrap();
    for address in [0x1000, 0x3000, 0x4000] {
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(address), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
    }
    let forms: &[(&str, &[u8], bool, bool, bool)] = &[
        ("fist", &[0xdb, 0x13], false, false, false),
        ("fistp", &[0xdb, 0x1b], true, false, false),
        ("fs-fistp", &[0x64, 0xdb, 0x1b], true, true, false),
        ("fisttp", &[0xdb, 0x0b], true, false, true),
    ];
    for &(name, code, _, _, _) in forms {
        memory.write(GuestAddress(0x1000), code).unwrap();
        fs::write(
            scratch.join(format!("{name}.wasm")),
            compile(&memory, GuestAddress(0x1000), 1).unwrap().wasm,
        )
        .unwrap();
    }
    memory
        .write(
            GuestAddress(0x1000),
            &[0xb8, 0x78, 0x56, 0x34, 0x12, 0xdb, 0x1b],
        )
        .unwrap();
    fs::write(
        scratch.join("prefix.wasm"),
        compile(&memory, GuestAddress(0x1000), 2).unwrap().wasm,
    )
    .unwrap();
    let mut cases = Vec::new();
    let mut add = |label: &str,
                   bits: u64,
                   control: u16,
                   old_status: u16,
                   top: u16,
                   tag: u16,
                   form: &str,
                   mode: u8,
                   wide: bool,
                   eligible: bool| {
        let &(_, code, _, fs_prefix, _) = forms
            .iter()
            .find(|(name, _, _, _, _)| *name == form)
            .unwrap();
        let linear = if mode == 4 {
            0x3ffe
        } else if fs_prefix {
            0x4000
        } else {
            0x3000
        };
        for address in [0x3000, 0x4000] {
            memory
                .protect(
                    PageRange::new(GuestAddress(address), 1).unwrap(),
                    Permissions::READ_WRITE,
                )
                .unwrap();
        }
        memory
            .write(GuestAddress(linear), &0xdeadbeef_u32.to_le_bytes())
            .unwrap();
        if mode == 1 || mode == 3 {
            memory
                .protect(
                    PageRange::new(GuestAddress(linear & !4095), 1).unwrap(),
                    Permissions::READ,
                )
                .unwrap();
        }
        if mode == 4 {
            memory
                .protect(
                    PageRange::new(GuestAddress(0x4000), 1).unwrap(),
                    Permissions::READ,
                )
                .unwrap();
        }
        if mode == 2 {
            memory
                .protect(
                    PageRange::new(GuestAddress(linear), 1).unwrap(),
                    Permissions::ALL,
                )
                .unwrap();
        }
        memory.write(GuestAddress(0x1000), code).unwrap();
        let decoded = decode_one(&memory, GuestAddress(0x1000)).unwrap();
        let mut before = fist32_bank(control, old_status, top, tag, bits);
        if wide {
            before.registers[usize::from(top)][0] |= 1;
        }
        let mut numeric = NumericState::default();
        numeric.x87 = before;
        let vectors = numeric.vectors;
        let mut segments = SegmentContext::default();
        segments.state.selectors = [0x23, 0x1b, 0x23, 0x23, 0x3b, 0x43];
        if fs_prefix {
            segments.state.bases[4] = 0x1000;
        }
        let segments = SegmentContext::legacy(segments.state);
        let mut scalar = State32 {
            registers: [
                0x11223344,
                2,
                3,
                if mode == 4 { 0x3ffe } else { 0x3000 },
                0x4800,
                6,
                7,
                8,
            ],
            eip: 0x1000,
            eflags: 0xcd7,
        };
        let scalar_before = scalar;
        let result = numeric.execute(
            &mut memory,
            &mut scalar,
            &segments,
            decoded.operation(),
            decoded.next_pc().0,
        );
        let mut bytes = [0; 4];
        memory.read(GuestAddress(linear), &mut bytes).unwrap();
        let output = u32::from_le_bytes(bytes);
        if mode == 3 {
            let mut expected = before;
            expected.status |= 0x8080;
            assert_eq!(
                result,
                Err(NumericError::Cpu {
                    vector: 16,
                    detail: u32::from(expected.status)
                }),
                "{label}"
            );
            assert_eq!(numeric.x87, expected);
            assert_eq!(scalar, scalar_before);
            assert_eq!(output, 0xdeadbeef);
        } else if mode == 1 || mode == 4 {
            assert!(
                matches!(result, Err(NumericError::Memory { width: 4, .. })),
                "{label}"
            );
            assert_eq!(numeric.x87, before);
            assert_eq!(scalar, scalar_before);
            assert_eq!(output, 0xdeadbeef);
        } else {
            assert_eq!(result.unwrap().code_invalidated, mode == 2, "{label}");
            assert_eq!(numeric.x87.control, control);
            assert_eq!(
                numeric.x87.registers, before.registers,
                "{label}: source and ghost payloads"
            );
            assert_eq!(
                scalar,
                State32 {
                    eip: decoded.next_pc().0,
                    ..scalar_before
                }
            );
        }
        assert_eq!(numeric.vectors, vectors, "{label}: SIMD/AVX bank unchanged");
        let status = if !FIST32_EXPECT_INLINE || !eligible || mode == 3 {
            7
        } else if mode == 1 || mode == 4 {
            3
        } else if mode == 2 {
            6
        } else {
            0
        };
        let completed = status == 0 || status == 6;
        cases.push(format!("{{\"label\":\"{label}-cw{control:04x}-sw{old_status:04x}-top{top}-tag{tag}-mode{mode}\",\"form\":\"{form}\",\"before\":{:?},\"after\":{:?},\"ownerAfter\":{:?},\"ownerOutput\":{output},\"status\":{status},\"next\":{},\"remaining\":{},\"retired\":{},\"linear\":{linear},\"fs\":{fs_prefix},\"mode\":{mode},\"output\":{},\"writes\":{}}}", fist32_bank_bytes(before), fist32_bank_bytes(if completed { numeric.x87 } else { before }), fist32_bank_bytes(numeric.x87), if completed { decoded.next_pc().0 } else { 0x1000 }, u32::from(!completed), u32::from(completed), if completed { output } else { 0xdeadbeef }, u32::from(status == 0 || status == 3 || status == 6)));
    };
    for (label, bits, _, _, eligible) in fist32_literals() {
        for top in 0..8_u16 {
            for tag in 0..3_u16 {
                for &(form, _, _, _, truncate) in forms {
                    for old_status in [0x4700, 0x472f] {
                        add(
                            label,
                            bits,
                            0x023f,
                            old_status,
                            top,
                            tag,
                            form,
                            0,
                            false,
                            eligible && !truncate,
                        );
                    }
                }
            }
        }
    }
    for control in [
        0_u16, 0x40, 0x003e, 0x001f, 0x043f, 0x083f, 0x0c3f, 0xffff, 0x013f, 0x037f,
    ] {
        for bits in [
            0_u64,
            0x8000000000000000,
            0xbff0000000000000,
            0x41dfffffffc00000,
            0xc1e0000000000000,
        ] {
            add(
                "exact-any-CW",
                bits,
                control,
                0x4700,
                7,
                2,
                "fistp",
                0,
                false,
                true,
            );
        }
    }
    for control in [0x003f_u16, 0x007f, 0x013f, 0x037f, 0x133f] {
        add(
            "inexact-PC-independent",
            0xbff8000000000000,
            control,
            0x4700,
            0,
            2,
            "fist",
            0,
            false,
            true,
        );
    }
    for control in [0x043f_u16, 0x083f, 0x0c3f, 0x003e, 0x001f] {
        add(
            "inexact-CW-service",
            0xbff8000000000000,
            control,
            0x4700,
            7,
            2,
            "fistp",
            0,
            false,
            false,
        );
    }
    for (label, bits, wide, tag) in [
        ("quiet-nan", 0x7ff8000000001234, false, 2),
        ("infinity", 0x7ff0000000000000, false, 2),
        ("f64-subnormal", 1, false, 2),
        ("raw-extra-bit", 0x3ff0000000000000, true, 2),
        ("empty", 0x3ff8000000000000, false, 3),
    ] {
        add(label, bits, 0x003f, 0x4700, 7, tag, "fistp", 0, wide, false);
        add(label, bits, 0x003f, 0x4700, 7, tag, "fistp", 4, wide, false);
    }
    for form in ["fist", "fistp", "fs-fistp"] {
        for mode in [1, 2] {
            for old_status in [0x4500, 0x4700] {
                add(
                    "store-fault-or-SMC",
                    0xbff8000000000000,
                    0x023f,
                    old_status,
                    7,
                    2,
                    form,
                    mode,
                    false,
                    true,
                );
            }
        }
        add(
            "pending-before-destination",
            0x3ff8000000000000,
            0x003e,
            1,
            7,
            2,
            form,
            3,
            false,
            false,
        );
    }
    for bit in 0..6 {
        for tag in [2, 3] {
            add(
                "each-pending-bit-before-destination",
                0x3ff8000000000000,
                0x003f & !(1 << bit),
                1 << bit,
                7,
                tag,
                "fistp",
                3,
                false,
                false,
            );
        }
    }
    add(
        "cross-page-store-fault",
        0xbff8000000000000,
        0x023f,
        0x4700,
        7,
        2,
        "fistp",
        4,
        false,
        true,
    );
    drop(add);
    fs::write(scratch.join("cases.json"), format!("[{}]", cases.join(","))).unwrap();
    let result = Command::new("node").arg("--input-type=module").arg("-e").arg(r#"
import assert from 'node:assert/strict';import fs from 'node:fs';
const root=process.argv[1],inline=process.argv[2]==='1',memory=new WebAssembly.Memory({initial:1}),ram=new Uint8Array(memory.buffer),d=new DataView(memory.buffer),state=0x8000,view=0x9800,fp=0xa000;
const seed=Uint8Array.from({length:ram.length},(_,i)=>(i*37+11)&255),word=(at,v)=>d.setUint32(state+at,v>>>0,true);let current,writes=0;
const imports={env:{memory},cpu:{read(){throw Error('FIST/P cannot read guest RAM');},write(context,address,width,value){assert.equal(context,state);assert.equal(address,current.linear);assert.equal(width,4);assert.notEqual(current.mode,3,'pending before destination access');writes++;if(current.mode===1||current.mode===4){word(52,3);word(60,address+(current.mode===4?2:0));word(64,0x20204);word(68,4);return 3;}d.setUint32(address,value>>>0,true);word(52,current.mode===2?6:0);return current.mode===2?6:0;}}};
const functions=new Map();for(const name of['fist','fistp','fs-fistp','fisttp','prefix'])functions.set(name,(await WebAssembly.instantiate(fs.readFileSync(`${root}/${name}.wasm`),imports)).instance.exports.run);
function init(item){current=item;ram.set(seed);ram.fill(0,state,state+160);ram.fill(0,view,view+36);word(0,1);word(4,160);[0x11223344,2,3,item.mode===4?0x3ffe:0x3000,0x4800,6,7,8].forEach((v,i)=>word(8+4*i,v));word(40,0x1000);word(44,0xcd7);word(48,1);word(56,7);word(124,view);word(152,63);if(item.fs)word(116,0x1000);d.setUint32(view,1,true);d.setUint32(view+4,36,true);d.setUint32(view+8,fp,true);[0x23,0x1b,0x23,0x23,0x3b,0x43].forEach((v,i)=>d.setUint32(view+12+4*i,v,true));ram.set(item.before,fp);d.setUint32(item.linear,0xdeadbeef,true);writes=0;}
function check(item,name=item.form){const expected=ram.slice(),v=new DataView(expected.buffer);expected.set(item.after,fp);v.setUint32(item.linear,item.output,true);for(const[at,value]of[[40,item.next],[48,item.remaining],[52,item.status],[56,7+item.retired]])v.setUint32(state+at,value,true);if(item.status===3)for(const[at,value]of[[60,item.linear+(item.mode===4?2:0)],[64,0x20204],[68,4]])v.setUint32(state+at,value,true);assert.equal(functions.get(name)(state),item.status,item.label);assert.deepEqual(ram,expected,item.label+' full bank/raw ghosts/scalar/RAM/quota/status');assert.equal(writes,item.writes,item.label+' exact guest store attempts');}
const cases=JSON.parse(fs.readFileSync(`${root}/cases.json`));for(const item of cases){init(item);check(item);}
const base=cases.find(item=>item.form==='fistp'&&item.label.startsWith('positive-odd-tie-')&&item.label.includes('-top0-tag2-mode0'));
assert(base);for(const quota of[0,1,2]){init(base);word(48,quota);const before=ram.slice(),expected=ram.slice(),v=new DataView(expected.buffer),prefix=quota!==0,completed=inline&&quota===2;for(const[at,value]of[[8,prefix?0x12345678:0x11223344],[40,prefix?(completed?0x1007:0x1005):0x1000],[48,completed?0:quota-(prefix?1:0)],[52,quota<2?2:inline?0:7],[56,7+(prefix?1:0)+(completed?1:0)]])v.setUint32(state+at,value>>>0,true);if(completed){expected.set(base.ownerAfter,fp);v.setUint32(fp+8,0x1005,true);v.setUint32(base.linear,base.ownerOutput,true);}assert.equal(functions.get('prefix')(state),quota<2?2:inline?0:7);assert.deepEqual(ram,expected,'integer prefix precise FISTP quota '+quota);assert.equal(writes,completed?1:0);}
init(base);word(40,0x1005);const expected=ram.slice(),v=new DataView(expected.buffer);if(inline){expected.set(base.ownerAfter,fp);v.setUint32(fp+8,0x1005,true);v.setUint32(base.linear,base.ownerOutput,true);}for(const[at,value]of[[40,inline?0x1007:0x1005],[48,inline?0:1],[52,inline?0:7],[56,inline?8:7]])v.setUint32(state+at,value,true);assert.equal(functions.get('prefix')(state),inline?0:7);assert.deepEqual(ram,expected,'arbitrary FISTP entry has no inherited numeric/operand proof');assert.equal(writes,inline?1:0);
for(const mutate of[()=>word(124,0),()=>word(124,view+1),()=>d.setUint32(view,2,true),()=>d.setUint32(view+4,35,true),()=>d.setUint32(view+8,0,true),()=>d.setUint32(view+8,fp+1,true)])for(const quota of[0,1,2]){init(base);word(48,quota);mutate();const expected=ram.slice(),v=new DataView(expected.buffer),prefix=quota!==0;for(const[at,value]of[[8,prefix?0x12345678:0x11223344],[40,prefix?0x1005:0x1000],[48,quota-(prefix?1:0)],[52,quota<2?2:7],[56,7+(prefix?1:0)]])v.setUint32(state+at,value>>>0,true);assert.equal(functions.get('prefix')(state),quota<2?2:7);assert.deepEqual(ram,expected,'unreached or malformed view retains prefix/quota and complete RAM');assert.equal(writes,0);}
console.log('FIST/P Int32 strict/literal owner controls and full-RAM compiled frontiers passed; explicit inline expectation='+inline);
"#).arg(&scratch).arg(if FIST32_EXPECT_INLINE { "1" } else { "0" }).output().unwrap();
    assert!(
        result.status.success(),
        "fixture {} failed:\n{}\n{}",
        scratch.display(),
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    fs::remove_dir_all(scratch).unwrap();
}
