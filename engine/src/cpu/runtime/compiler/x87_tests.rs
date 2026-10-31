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
