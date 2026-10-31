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
