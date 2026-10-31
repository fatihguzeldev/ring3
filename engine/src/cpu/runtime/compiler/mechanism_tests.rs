use super::*;
use crate::memory::{PageRange, Permissions};

#[test]
fn generated_flag_elision_preserves_every_budget_fault_and_observation_frontier() {
    use std::{
        fs,
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };
    let scratch = std::env::temp_dir().join(format!(
        "ring3-runtime-flag-frontiers-{}-{}",
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
        (
            "overwrite",
            &[0x83, 0xc0, 1, 0x83, 0xc3, 1, 0x8b, 0x16, 0x31, 0xff][..],
            4,
        ),
        ("carry", &[0x83, 0xc0, 1, 0x43, 0x83, 0xd2, 0][..], 3),
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
const root=process.argv[1],memory=new WebAssembly.Memory({initial:2}),ram=new Uint8Array(memory.buffer),data=new DataView(memory.buffer),ptr=0x8000;
const word=(at,value)=>data.setUint32(ptr+at,value>>>0,true),get=at=>data.getUint32(ptr+at,true);
let mode=0,observed=[];
const imports={env:{memory},cpu:{read(context,address,width){
  assert.equal(context,ptr);assert.equal(address,0x6000);assert.equal(width,4);observed.push(ram.slice());
  if(mode==='throw')throw Error('authored-import-trap');
  word(52,mode);if(mode){word(60,0x6000);word(64,0x10104);word(68,4);}return 0x76543210;
},write(){throw Error('no write permitted');}}};
const units={};for(const name of['overwrite','carry'])units[name]=(await WebAssembly.instantiate(fs.readFileSync(`${root}/${name}.wasm`),imports)).instance.exports.run;
function init(quota,carry=false){
  for(let i=0;i<ram.length;i++)ram[i]=(i*43+19)&255;ram.fill(0,ptr,ptr+160);
  word(0,1);word(4,160);[carry?0xffffffff:0x7fffffff,3,10,carry?0x7fffffff:0xffffffff,0x4800,11,0x6000,12].forEach((v,i)=>word(8+4*i,v));
  word(40,0x1000);word(44,0x200602);word(48,quota);word(56,0xfffffffe);word(152,63);observed=[];
}
for(mode of[0,3,6,'throw'])for(const quota of[0,1,2,3,4]){
  init(quota);const before=ram.slice(),expected=before.slice(),d=new DataView(expected.buffer),set=(at,v)=>d.setUint32(ptr+at,v>>>0,true);
  if(mode==='throw'&&quota>=3){assert.throws(()=>units.overwrite(ptr),/authored-import-trap/);assert.deepEqual(ram,before,'an import throw must not publish deferred scalar state');assert.deepEqual(observed,[before]);continue;}
  const fault=mode!==0&&quota>=3,steps=fault?2:quota,status=fault?mode:quota===4?0:2;
  set(40,[0x1000,0x1003,0x1006,0x1008,0x100a][steps]);set(44,[0x200602,0x200e96,0x200657,0x200657,0x200646][steps]);
  set(48,quota-steps);set(52,status);set(56,(0xfffffffe+steps)>>>0);
  if(steps>=1)set(8,0x80000000);if(steps>=2)set(20,0);if(steps>=3)set(16,0x76543210);if(steps>=4)set(36,0);
  if(fault){set(60,0x6000);set(64,0x10104);set(68,4);}
  assert.equal(units.overwrite(ptr),status,`mode${mode}/quota${quota}`);assert.deepEqual(ram,expected,`full state mode${mode}/quota${quota}`);
  assert.deepEqual(observed,quota>=3?[before]:[],'helpers retain the invocation entry publication frontier');
}
mode=0;
for(const[quota,pc,flags,eax,ebx,edx]of[
  [0,0x1000,0x200602,0xffffffff,0x7fffffff,10],
  [1,0x1003,0x200657,0,0x7fffffff,10],
  [2,0x1004,0x200e97,0,0x80000000,10],
  [3,0x1007,0x200602,0,0x80000000,11],
]){
  init(quota,true);const expected=ram.slice(),d=new DataView(expected.buffer),set=(at,v)=>d.setUint32(ptr+at,v>>>0,true);
  for(const[at,v]of[[8,eax],[16,edx],[20,ebx],[40,pc],[44,flags],[48,0],[52,quota===3?0:2],[56,(0xfffffffe+quota)>>>0]])set(at,v);
  assert.equal(units.carry(ptr),quota===3?0:2);assert.deepEqual(ram,expected,'INC preserves carry for ADC while other arithmetic flags are replaced');assert.deepEqual(observed,[]);
}
console.log('FLAGS overwrite and partial-CF chains preserve quota, PRE read status6, faults, import-time slab visibility, throws and retirement wrap');
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
fn generated_direct_links_keep_return_guards_gate_budget_and_completed_store_exits() {
    use std::{
        fs,
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };
    let scratch = std::env::temp_dir().join(format!(
        "ring3-runtime-link-frontiers-{}-{}",
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
    let mut code = [0x90; 66];
    code[..8].copy_from_slice(&[0xe8, 0x1b, 0, 0, 0, 0x40, 0xeb, 0x38]);
    code[32..34].copy_from_slice(&[0x43, 0xc3]);
    code[64..66].copy_from_slice(&[0x0f, 0x0b]);
    memory.write(GuestAddress(0x1000), &code).unwrap();
    let unit = compile_with_gates(
        &memory,
        GuestAddress(0x1000),
        6,
        &[(GuestAddress(0x1040), 23)],
    )
    .unwrap();
    assert_eq!(
        unit.entries,
        [0x1000, 0x1005, 0x1006, 0x1020, 0x1021, 0x1040]
    );
    fs::write(scratch.join("links.wasm"), unit.wasm).unwrap();
    let result = Command::new("node").arg("--input-type=module").arg("-e").arg(r#"
import assert from 'node:assert/strict';
import fs from 'node:fs';
const root=process.argv[1],memory=new WebAssembly.Memory({initial:2}),ram=new Uint8Array(memory.buffer),data=new DataView(memory.buffer),ptr=0x8000;
const word=(at,value)=>data.setUint32(ptr+at,value>>>0,true);
let mode='',reads=[],writes=[];
const imports={env:{memory},cpu:{
  read(context,address,width){assert.equal(context,ptr);assert.equal(width,4);reads.push(address);if(mode==='read-fault'){word(52,3);word(60,address);word(64,0x10104);word(68,4);return 0;}word(52,0);return data.getUint32(address,true);},
  write(context,address,width,value){assert.equal(context,ptr);assert.equal(width,4);writes.push([address,value>>>0]);if(mode==='write-fault'){word(52,3);word(60,address);word(64,0x20204);word(68,4);return 3;}data.setUint32(address,value>>>0,true);word(52,mode==='smc'?6:0);return mode==='smc'?6:0;}
}};
const run=(await WebAssembly.instantiate(fs.readFileSync(`${root}/links.wasm`),imports)).instance.exports.run;
function init(quota,pc=0x1000,target=0x1005){
  for(let i=0;i<ram.length;i++)ram[i]=(i*17+23)&255;ram.fill(0,ptr,ptr+160);
  word(0,1);word(4,160);[5,3,7,8,0x4800,10,11,12].forEach((v,i)=>word(8+4*i,v));word(40,pc);word(44,0x203);word(48,quota);word(56,0xfffffffe);word(152,63);
  data.setUint32(0x4800,target,true);reads=[];writes=[];
}
function check(quota,expect,pc=0x1000,target=0x1005){
  init(quota,pc,target);const expected=ram.slice(),d=new DataView(expected.buffer),set=(at,v)=>d.setUint32(ptr+at,v>>>0,true);
  for(const[at,v]of[[40,expect.pc],[44,expect.flags??0x203],[48,quota-expect.steps],[52,expect.status],[56,(0xfffffffe+expect.steps)>>>0],...(expect.fields??[])])set(at,v);
  for(const[index,v]of Object.entries(expect.regs??{}))set(8+4*Number(index),v);
  for(const[address,value]of expect.stores??[])d.setUint32(address,value>>>0,true);
  assert.equal(run(ptr),expect.status);assert.deepEqual(ram,expected,`full link state quota${quota}/entry${pc.toString(16)}/mode${mode}/target${target.toString(16)}`);
  assert.deepEqual(reads,expect.reads??[]);assert.deepEqual(writes,expect.writes??[]);
}
mode='';
for(const[quota,pc,eax,ebx,esp,flags,status]of[
  [0,0x1000,5,8,0x4800,0x203,2],
  [1,0x1020,5,8,0x47fc,0x203,2],
  [2,0x1021,5,9,0x47fc,0x207,2],
  [3,0x1005,5,9,0x4800,0x207,2],
  [4,0x1006,6,9,0x4800,0x207,2],
  [5,0x1040,6,9,0x4800,0x207,2],
  [6,0x1040,6,9,0x4800,0x207,5],
])check(quota,{pc,status,steps:Math.min(quota,5),flags,regs:{0:eax,3:ebx,4:esp},stores:quota?[[0x47fc,0x1005]]:[],writes:quota?[[0x47fc,0x1005]]:[],reads:quota>=3?[0x47fc]:[],fields:quota===6?[[68,23]]:[]});
for(const target of[0x1000,0x1005,0x1006,0x1020,0x1021,0x1040,0x1007,0x7777,0xffffffff]){
  const known=[0x1000,0x1005,0x1006,0x1020,0x1021,0x1040].includes(target);
  check(1,{pc:target,status:known?2:0,steps:1,regs:{4:0x4804},reads:[0x4800]},0x1021,target);
}
check(2,{pc:0x1040,status:5,steps:1,regs:{4:0x4804},reads:[0x4800],fields:[[68,23]]},0x1021,0x1040);
mode='smc';check(6,{pc:0x1020,status:6,steps:1,regs:{4:0x47fc},stores:[[0x47fc,0x1005]],writes:[[0x47fc,0x1005]]});
mode='write-fault';check(6,{pc:0x1000,status:3,steps:0,writes:[[0x47fc,0x1005]],fields:[[60,0x47fc],[64,0x20204],[68,4]]});
mode='read-fault';check(3,{pc:0x1021,status:3,steps:2,flags:0x207,regs:{3:9,4:0x47fc},stores:[[0x47fc,0x1005]],writes:[[0x47fc,0x1005]],reads:[0x47fc],fields:[[60,0x47fc],[64,0x10104],[68,4]]});
console.log('direct CALL/RET continuation and guarded arbitrary return landings preserve gates, quota, retirement wrap, faults, full RAM and completed SMC');
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
