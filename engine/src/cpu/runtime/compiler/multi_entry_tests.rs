use super::*;
use crate::memory::{PageRange, Permissions};

#[test]
fn every_decoded_instruction_and_gate_has_an_exact_resume_entry() {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory
        .write(GuestAddress(0x1000), &[0x40, 0x43, 0xd9, 0xe8, 0x0f, 0x0b])
        .unwrap();
    let unit = compile_with_gates(
        &memory,
        GuestAddress(0x1000),
        4,
        &[(GuestAddress(0x1004), 17)],
    )
    .unwrap();
    assert_eq!(unit.entries, [0x1000, 0x1001, 0x1002, 0x1004]);
    assert!(unit.is_current(&memory));
    memory.write(GuestAddress(0x1001), &[0x42]).unwrap();
    assert!(!unit.is_current(&memory));
}

#[test]
fn generated_multi_entry_landings_preserve_budget_service_gate_and_shared_paths() {
    use std::{
        fs,
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };
    let scratch = std::env::temp_dir().join(format!(
        "ring3-runtime-multi-entry-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&scratch).unwrap();
    let mut memory = AddressSpace::new(3).unwrap();
    for pc in [0, 0x1000, 0xfffff000] {
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(pc), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
    }
    let mut records = Vec::new();
    for (name, bytes, count, gates) in [
        ("plain", &[0x40, 0x43, 0x42, 0x90][..], 4, vec![]),
        (
            "service",
            &[0x40, 0x64, 0xd9, 0x05, 0x00, 0x30, 0, 0, 0x43, 0x90][..],
            4,
            vec![],
        ),
        (
            "shared",
            &[0x75, 0x01, 0x40, 0x43, 0x90, 0xc3][..],
            8,
            vec![],
        ),
        (
            "gate",
            &[0x40, 0x43, 0x0f, 0x0b][..],
            3,
            vec![(GuestAddress(0x1002), 17)],
        ),
    ] {
        memory.write(GuestAddress(0x1000), bytes).unwrap();
        let unit = compile_with_gates(&memory, GuestAddress(0x1000), count, &gates).unwrap();
        fs::write(scratch.join(format!("{name}.wasm")), &unit.wasm).unwrap();
        records.push(format!(
            "{{\"name\":\"{name}\",\"entries\":{:?}}}",
            unit.entries
        ));
    }
    memory.write(GuestAddress(u32::MAX), &[0x90]).unwrap();
    memory.write(GuestAddress(0), &[0x43, 0x90]).unwrap();
    let unit = compile(&memory, GuestAddress(u32::MAX), 3).unwrap();
    fs::write(scratch.join("wrap.wasm"), &unit.wasm).unwrap();
    records.push(format!(
        "{{\"name\":\"wrap\",\"entries\":{:?}}}",
        unit.entries
    ));
    let mut fanout = [0; 4096];
    for block in 0..128 {
        let at = block * 32;
        fanout[at..at + if block == 127 { 14 } else { 15 }].fill(0x40);
        if block == 127 {
            fanout[at + 14] = 0x49;
            fanout[at + 15..at + 17].copy_from_slice(&[0x0f, 0x85]);
            fanout[at + 17..at + 21].copy_from_slice(&(-(at as i32 + 21)).to_le_bytes());
            fanout[at + 21..at + 23].copy_from_slice(&[0x0f, 0x0b]);
        } else {
            fanout[at + 15] = 0xe9;
            fanout[at + 16..at + 20].copy_from_slice(&12_i32.to_le_bytes());
        }
    }
    memory.write(GuestAddress(0x1000), &fanout).unwrap();
    let unit = compile(&memory, GuestAddress(0x1000), 2048).unwrap();
    fs::write(scratch.join("fanout.wasm"), &unit.wasm).unwrap();
    records.push(format!(
        "{{\"name\":\"fanout\",\"entries\":{:?}}}",
        unit.entries
    ));
    fs::write(
        scratch.join("entries.json"),
        format!("[{}]", records.join(",")),
    )
    .unwrap();
    fs::write(
        scratch.join("dispatch.wasm"),
        crate::cpu::runtime::emit_dispatch_with_service(),
    )
    .unwrap();
    let result = Command::new("node").arg("--input-type=module").arg("-e").arg(r#"
import assert from 'node:assert/strict';
import fs from 'node:fs';
const root=process.argv[1],memory=new WebAssembly.Memory({initial:4}),ram=new Uint8Array(memory.buffer),data=new DataView(memory.buffer),ptr=0x8000,directory=0x10000,mask=8191;
const word=(at,value)=>data.setUint32(ptr+at,value>>>0,true),get=at=>data.getUint32(ptr+at,true),regs=()=>Array.from({length:8},(_,i)=>get(8+4*i));
const metadata=new Map(JSON.parse(fs.readFileSync(`${root}/entries.json`)).map(item=>[item.name,item.entries]));
function directoryHash(pc){let value=pc>>>0;value=Math.imul(value^(value>>>16),0x7feb352d);value=Math.imul(value^(value>>>15),0x846ca68b);return(value^(value>>>16))>>>0;}
const table=new WebAssembly.Table({initial:2,element:'anyfunc'}),units=new Map();
const imports={env:{memory},cpu:{read(){throw Error('no scalar memory access expected');},write(){throw Error('no scalar memory write expected');}}};
for(const name of metadata.keys())units.set(name,(await WebAssembly.instantiate(fs.readFileSync(`${root}/${name}.wasm`),imports)).instance.exports.run);
let services=0;
const service=context=>{assert.equal(context,ptr);assert.equal(get(40),0x1001);assert.equal(get(52),7);assert(get(48)>0);services++;word(40,0x1008);word(48,get(48)-1);word(56,get(56)+1);word(52,0);return 0;};
const dispatch=(await WebAssembly.instantiate(fs.readFileSync(`${root}/dispatch.wasm`),{env:{memory,functions:table},cpu:{instruction:service}})).instance.exports.run;
function init(name,pc,quota,flags=0x203){ram.fill(0);word(0,1);word(4,160);[5,6,7,8,0x4800,10,11,12].forEach((v,i)=>word(8+i*4,v));word(40,pc);word(44,flags);word(48,quota);word(144,directory);word(148,mask);word(152,63);table.set(1,units.get(name));for(const entry of metadata.get(name)){let index=directoryHash(entry)&mask;while(data.getUint32(directory+index*8+4,true))index=(index+1)&mask;data.setUint32(directory+index*8,entry,true);data.setUint32(directory+index*8+4,1,true);}services=0;}
function complete(pc,remaining,retired,status=2){assert.deepEqual([get(40),get(48),get(56),get(52)],[pc>>>0,remaining,retired,status]);}
assert.deepEqual(metadata.get('plain'),[0x1000,0x1001,0x1002,0x1003]);
for(const[pc,index,result,flags]of[[0x1000,0,6,0x207],[0x1001,3,9,0x207],[0x1002,2,8,0x203],[0x1003,null,null,0x203]]){init('plain',pc,1);const expected=regs();if(index!==null)expected[index]=result;assert.equal(dispatch(ptr),2);assert.deepEqual(regs(),expected);assert.equal(get(44),flags);complete(pc+1,0,1);assert.equal(services,0);}
init('plain',0x1000,1);for(const pc of[0x1001,0x1002,0x1003,0x1004]){assert.equal(dispatch(ptr),2);assert.equal(get(40),pc);assert.equal(get(48),0);word(48,1);word(56,0);}assert.deepEqual(regs(),[6,6,8,9,0x4800,10,11,12]);const before=regs();assert.equal(dispatch(ptr),1);assert.deepEqual(regs(),before);complete(0x1004,1,0,1);
init('plain',0x1005,1);const unknown=regs();assert.equal(units.get('plain')(ptr),0);assert.deepEqual(regs(),unknown);complete(0x1005,1,0,0);
assert.deepEqual(metadata.get('service'),[0x1000,0x1001,0x1008,0x1009]);init('service',0x1000,4);assert.equal(dispatch(ptr),2);assert.deepEqual(regs(),[6,6,7,9,0x4800,10,11,12]);complete(0x100a,0,4);assert.equal(services,1,'one exact service followed by an existing interior landing, with no prefix replay');
assert.deepEqual(metadata.get('gate'),[0x1000,0x1001,0x1002]);init('gate',0x1001,1);assert.equal(dispatch(ptr),2);complete(0x1002,0,1);assert.deepEqual(regs(),[5,6,7,9,0x4800,10,11,12]);word(48,1);word(56,0);assert.equal(dispatch(ptr),5);complete(0x1002,1,0,5);assert.equal(get(68),17);assert.deepEqual(regs(),[5,6,7,9,0x4800,10,11,12]);
assert.deepEqual(metadata.get('shared'),[0x1000,0x1002,0x1003,0x1004,0x1005]);for(const[flags,eax,ebx,pc]of[[0x203,5,9,0x1004],[0x243,6,8,0x1003]]){init('shared',0x1000,2,flags);assert.equal(dispatch(ptr),2);assert.deepEqual(regs(),[eax,6,7,ebx,0x4800,10,11,12]);complete(pc,0,2);}init('shared',0x1002,2);assert.equal(dispatch(ptr),2);assert.deepEqual(regs(),[6,6,7,9,0x4800,10,11,12]);complete(0x1004,0,2);
assert.deepEqual(metadata.get('wrap'),[0,1,0xffffffff]);init('wrap',0xffffffff,2);assert.equal(dispatch(ptr),2);assert.deepEqual(regs(),[5,6,7,9,0x4800,10,11,12]);complete(1,0,2);init('wrap',0,1);assert.equal(dispatch(ptr),2);complete(1,0,1);
assert.equal(metadata.get('fanout').length,2048);init('fanout',0x1000,4096,2);word(12,2);assert.equal(dispatch(ptr),2);assert.deepEqual(regs(),[3843,0,7,8,0x4800,10,11,12]);assert.equal(get(44),0x46);complete(0x1ff5,0,4096);assert.equal(services,0);
console.log('all instruction/gate landings, budget restart without prefix replay, exact-service continuation, shared branch paths, unsigned wrap, unknown PC and128-block/2048-instruction dispatch passed');
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
fn generated_routing_edges_preserve_call_smc_fault_and_zero_quota_priority() {
    use std::{
        fs,
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };
    let scratch = std::env::temp_dir().join(format!(
        "ring3-runtime-route-boundaries-{}-{}",
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
    let mut call = [0x90; 32];
    call[..5].copy_from_slice(&[0xe8, 0x0b, 0, 0, 0]);
    call[16..18].copy_from_slice(&[0x43, 0xc3]);
    let mut records = Vec::new();
    for (name, code, limit, covered) in [
        ("call-inside", &call[..], 3, None),
        ("call-outside", &call[..], 1, None),
        (
            "jcc-outside",
            &[0x75, 2, 0x90, 0xc3, 0xeb, 0xfe][..],
            1,
            None,
        ),
        (
            "jcc-taken-inside",
            &[0x75, 2, 0x90, 0xc3, 0xeb, 0xfe][..],
            2,
            None,
        ),
        (
            "jcc-fallthrough-inside",
            &[0x75, 2, 0x90, 0xc3, 0xeb, 0xfe][..],
            2,
            Some(0x1004),
        ),
        (
            "jcc-both-inside",
            &[0x75, 2, 0x90, 0xc3, 0xeb, 0xfe][..],
            3,
            None,
        ),
        ("loop-same-destination", &[0xe2, 0, 0x90, 0xc3][..], 2, None),
    ] {
        memory.write(GuestAddress(0x1000), code).unwrap();
        let unit = compile_with_gates_until(&memory, GuestAddress(0x1000), limit, &[], |pc| {
            covered == Some(pc.0)
        })
        .unwrap();
        fs::write(scratch.join(format!("{name}.wasm")), &unit.wasm).unwrap();
        records.push(format!(
            "{{\"name\":\"{name}\",\"entries\":{:?}}}",
            unit.entries
        ));
    }
    fs::write(
        scratch.join("entries.json"),
        format!("[{}]", records.join(",")),
    )
    .unwrap();
    let result = Command::new("node").arg("--input-type=module").arg("-e").arg(r#"
import assert from 'node:assert/strict';
import fs from 'node:fs';
const root=process.argv[1],memory=new WebAssembly.Memory({initial:2}),ram=new Uint8Array(memory.buffer),data=new DataView(memory.buffer),ptr=0x8000;
const word=(at,value)=>data.setUint32(ptr+at,value>>>0,true);
let mode='',reads=[],writes=[];
function fault(address,access,reason){word(52,3);word(60,address);word(64,4|(access<<8)|(reason<<16));word(68,4);}
const imports={env:{memory},cpu:{
  read(context,address,width){assert.equal(context,ptr);assert.equal(width,4);address>>>=0;reads.push([address,width]);if(mode==='read-fault'){fault(address,1,1);return 0;}word(52,0);return data.getUint32(address,true);},
  write(context,address,width,value){assert.equal(context,ptr);assert.equal(width,4);address>>>=0;writes.push([address,width,value>>>0]);if(mode==='write-fault'){fault(address,2,2);return 3;}data.setUint32(address,value>>>0,true);word(52,mode==='smc'?6:0);return mode==='smc'?6:0;}
}};
const entries=new Map(JSON.parse(fs.readFileSync(`${root}/entries.json`)).map(item=>[item.name,item.entries])),units=new Map();
for(const name of entries.keys())units.set(name,(await WebAssembly.instantiate(fs.readFileSync(`${root}/${name}.wasm`),imports)).instance.exports.run);
assert.deepEqual(entries.get('call-inside'),[0x1000,0x1010,0x1011]);assert.deepEqual(entries.get('call-outside'),[0x1000]);
assert.deepEqual(entries.get('jcc-outside'),[0x1000]);assert.deepEqual(entries.get('jcc-taken-inside'),[0x1000,0x1004]);assert.deepEqual(entries.get('jcc-fallthrough-inside'),[0x1000,0x1002]);assert.deepEqual(entries.get('jcc-both-inside'),[0x1000,0x1002,0x1004]);assert.deepEqual(entries.get('loop-same-destination'),[0x1000,0x1002]);
function init(options){
  for(let i=0;i<ram.length;i++)ram[i]=(i*29+7)&255;ram.fill(0,ptr,ptr+160);
  word(0,1);word(4,160);[5,3,7,8,0x4800,10,11,12].forEach((v,i)=>word(8+4*i,v));word(40,options.pc??0x1000);word(44,options.flags??0x203);word(48,options.quota??1);word(52,0xabcdef01);word(56,11);word(60,0x11223344);word(64,0x55667788);word(68,0x12345678);word(124,0x9000);word(152,63);
  data.setUint32(0x9000,1,true);data.setUint32(0x9004,40,true);data.setUint32(0x9008,0x9100,true);data.setUint32(0x9024,0x9200,true);
  for(const[index,value]of Object.entries(options.registers??{}))word(8+4*Number(index),value);
  if(options.stackTarget!==undefined)data.setUint32(0x4800,options.stackTarget,true);mode=options.mode??'';reads=[];writes=[];
}
function scenario(name,options,expect){
  init(options);const expected=ram.slice(),d=new DataView(expected.buffer),set=(at,value)=>d.setUint32(ptr+at,value>>>0,true);
  for(const[at,value]of[[40,expect.pc??options.pc??0x1000],[44,expect.flags??options.flags??0x203],[48,(options.quota??1)-(expect.work??0)],[52,expect.status],[56,11+(expect.retired??0)]])set(at,value);
  for(const[index,value]of Object.entries(expect.registers??{}))set(8+4*Number(index),value);
  for(const[at,value]of Object.entries(expect.fields??{}))set(Number(at),value);
  for(const[address,value]of expect.stores??[])d.setUint32(address,value>>>0,true);
  assert.equal(units.get(name)(ptr),expect.status,`${name}: return`);assert.deepEqual(ram,expected,`${name}: full GPR/FLAGS/numeric banks/RAM/work/retirement`);assert.deepEqual(reads,expect.reads??[],`${name}: reads`);assert.deepEqual(writes,expect.writes??[],`${name}: writes`);
}
for(const[name,inside]of[['call-inside',true],['call-outside',false]]){
  scenario(name,{mode:'smc',quota:4},{status:6,pc:0x1010,work:1,retired:1,registers:{4:0x47fc},stores:[[0x47fc,0x1005]],writes:[[0x47fc,4,0x1005]]});
  scenario(name,{}, {status:inside?2:0,pc:0x1010,work:1,retired:1,registers:{4:0x47fc},stores:[[0x47fc,0x1005]],writes:[[0x47fc,4,0x1005]]});
  scenario(name,{mode:'write-fault',quota:4},{status:3,fields:{60:0x47fc,64:0x20204,68:4},writes:[[0x47fc,4,0x1005]]});
  scenario(name,{quota:0,mode:'write-fault'},{status:2});
}
for(const[pc,status]of[[0x1000,2],[0x1010,2],[0x1011,2],[0x1005,0],[0x7777,0]])scenario('call-inside',{pc,quota:0},{status});

for(const[name,takenInside,fallInside]of[['jcc-outside',false,false],['jcc-taken-inside',true,false],['jcc-fallthrough-inside',false,true],['jcc-both-inside',true,true]]){
  for(const[taken,flags,pc]of[[true,0x203,0x1004],[false,0x243,0x1002]])scenario(name,{flags},{status:(taken?takenInside:fallInside)?2:0,pc,work:1,retired:1});
}
scenario('jcc-both-inside',{quota:2},{status:2,pc:0x1004,work:2,retired:2});
scenario('jcc-both-inside',{quota:2,flags:0x243},{status:0,pc:0x1003,work:2,retired:2});
for(const count of[0,1,2])scenario('loop-same-destination',{registers:{1:count}},{status:2,pc:0x1002,work:1,retired:1,registers:{1:(count-1)>>>0}});

// the callee return resolves through guest memory, including a destination
// outside this unit after its quota has reached zero.
scenario('call-inside',{quota:3},{status:0,pc:0x1005,work:3,retired:3,flags:0x207,registers:{3:9},stores:[[0x47fc,0x1005]],reads:[[0x47fc,4]],writes:[[0x47fc,4,0x1005]]});
for(const[target,status]of[[0x1010,2],[0x7777,0]])scenario('call-inside',{pc:0x1011,stackTarget:target},{status,pc:target,work:1,retired:1,registers:{4:0x4804},reads:[[0x4800,4]]});
scenario('call-inside',{pc:0x1011,stackTarget:0x1010,mode:'read-fault'},{status:3,fields:{60:0x4800,64:0x10104,68:4},reads:[[0x4800,4]]});
console.log('generated route boundaries: CALL inside/outside SMC and neutral fault, zero-quota PC priority, all Jcc destinations, single LOOP decrement, RET, full state/RAM/banks and helper effects passed');
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
