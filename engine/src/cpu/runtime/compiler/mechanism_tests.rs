use super::*;
use crate::memory::{PageRange, Permissions};

#[test]
fn generated_structured_self_interior_and_forward_gate_preserve_exact_prefixes() {
    use std::{
        fs,
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };
    let scratch = std::env::temp_dir().join(format!(
        "ring3-runtime-structured-prefixes-{}-{}",
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
    memory
        .write(
            GuestAddress(0x1000),
            &[0x90, 0x40, 0x8b, 0x16, 0xe2, 0xfb, 0x0f, 0x0b],
        )
        .unwrap();
    let unit = compile_with_gates(
        &memory,
        GuestAddress(0x1000),
        8,
        &[(GuestAddress(0x1006), 23)],
    )
    .unwrap();
    assert_eq!(unit.entries, [0x1000, 0x1001, 0x1002, 0x1004, 0x1006]);
    fs::write(scratch.join("loop.wasm"), unit.wasm).unwrap();
    memory
        .write(
            GuestAddress(0x1000),
            &[0x40, 0x43, 0x49, 0x75, 0xfc, 0x0f, 0x0b],
        )
        .unwrap();
    let unit = compile_with_gates(
        &memory,
        GuestAddress(0x1000),
        8,
        &[(GuestAddress(0x1005), 23)],
    )
    .unwrap();
    assert_eq!(unit.entries, [0x1000, 0x1001, 0x1002, 0x1003, 0x1005]);
    fs::write(scratch.join("jcc.wasm"), unit.wasm).unwrap();
    let result = Command::new("node").arg("--input-type=module").arg("-e").arg(r#"
import assert from 'node:assert/strict';
import fs from 'node:fs';
const memory=new WebAssembly.Memory({initial:2}),ram=new Uint8Array(memory.buffer),d=new DataView(memory.buffer),ctx=0x8000;
const word=(at,v)=>d.setUint32(ctx+at,v>>>0,true);
let mode='',reads=[];
const imports={env:{memory},cpu:{
 read(p,a,w){assert.equal(p,ctx);assert.equal(a,0x6000);assert.equal(w,4);reads.push(ram.slice());
  if(reads.length===2&&mode==='throw')throw Error('authored-second-read-trap');
  if(reads.length===2&&mode==='fault'){word(52,3);word(60,a);word(64,0x10104);word(68,4);}else word(52,0);
  return 0x12345678;
 },write(){throw Error('this loop has no writes');}
}};
const run=(await WebAssembly.instantiate(fs.readFileSync(`${process.argv[1]}/loop.wasm`),imports)).instance.exports.run;
const runJcc=(await WebAssembly.instantiate(fs.readFileSync(`${process.argv[1]}/jcc.wasm`),imports)).instance.exports.run;
function init(pc,quota){
  for(let i=0;i<ram.length;i++)ram[i]=(i*13+29)&255;ram.fill(0,ctx,ctx+160);
  word(0,1);word(4,160);[5,2,7,8,0x4800,10,0x6000,12].forEach((v,i)=>word(8+4*i,v));
  word(40,pc);word(44,0x203);word(48,quota);word(56,0xfffffffc);word(152,63);reads=[];
}
function check(input,expect){
  init(input.pc??0x1000,input.quota);mode=input.mode??'';
  const before=ram.slice(),expected=before.slice(),view=new DataView(expected.buffer),set=(at,v)=>view.setUint32(ctx+at,v>>>0,true);
  for(const[at,v]of[[8,expect.eax],[12,expect.ecx],[16,expect.edx],[40,expect.pc],[44,expect.flags],[48,input.quota-expect.steps],[52,expect.status],[56,(0xfffffffc+expect.steps)>>>0],...(expect.fields??[])])set(at,v);
  assert.equal(run(ctx),expect.status);assert.deepEqual(ram,expected,'full state across structured self-interior and forward exit');
  assert.equal(reads.length,expect.reads);for(const observed of reads)assert.deepEqual(observed,before,'no intermediate scalar publication on a linked hop');
}
for(const[quota,pc,eax,ecx,edx,flags,count]of[
 [0,0x1000,5,2,7,0x203,0],[1,0x1001,5,2,7,0x203,0],
 [2,0x1002,6,2,7,0x207,0],[3,0x1004,6,2,0x12345678,0x207,1],
 [4,0x1001,6,1,0x12345678,0x207,1],[5,0x1002,7,1,0x12345678,0x203,1],
 [6,0x1004,7,1,0x12345678,0x203,2],[7,0x1006,7,0,0x12345678,0x203,2],
 [8,0x1006,7,0,0x12345678,0x203,2],
])check({quota},{pc,eax,ecx,edx,flags,reads:count,steps:Math.min(quota,7),status:quota===8?5:2,fields:quota===8?[[68,23]]:[]});
for(const quota of[6,7])check({pc:0x1001,quota},{pc:0x1006,eax:7,ecx:0,edx:0x12345678,flags:0x203,reads:2,steps:6,status:quota===7?5:2,fields:quota===7?[[68,23]]:[]});
for(const quota of[6,8])check({quota,mode:'fault'},{pc:0x1002,eax:7,ecx:1,edx:0x12345678,flags:0x203,reads:2,steps:5,status:3,fields:[[60,0x6000],[64,0x10104],[68,4]]});
init(0x1000,8);mode='throw';{const before=ram.slice();assert.throws(()=>run(ctx),/authored-second-read-trap/);assert.deepEqual(ram,before);assert.equal(reads.length,2);}
for(const[quota,pc,eax,ebx,ecx,flags]of[
 [0,0x1000,5,8,3,0x203],[1,0x1001,6,8,3,0x207],[2,0x1002,6,9,3,0x207],
 [3,0x1003,6,9,2,0x203],[4,0x1001,6,9,2,0x203],[5,0x1002,6,10,2,0x207],
 [6,0x1003,6,10,1,0x203],[7,0x1001,6,10,1,0x203],[8,0x1002,6,11,1,0x203],
 [9,0x1003,6,11,0,0x247],[10,0x1005,6,11,0,0x247],[11,0x1005,6,11,0,0x247],
]){
 init(0x1000,quota);word(12,3);mode='';const expected=ram.slice(),view=new DataView(expected.buffer),set=(at,v)=>view.setUint32(ctx+at,v>>>0,true),steps=Math.min(quota,10),status=quota===11?5:2;
 for(const[at,v]of[[8,eax],[12,ecx],[20,ebx],[40,pc],[44,flags],[48,quota-steps],[52,status],[56,(0xfffffffc+steps)>>>0]])set(at,v);
 if(status===5)set(68,23);assert.equal(runJcc(ctx),status);assert.deepEqual(ram,expected,'root instruction executes once before canonical interior Jcc loop');assert.deepEqual(reads,[]);
}
console.log('canonical self-interior ownership, forward gate, every quota prefix, later fault and import trap retain full state and helper observations');
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
fn generated_pending_flag_consumers_preserve_narrow_carry_overflow_and_partial_updates() {
    use std::{
        fs,
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };
    let scratch = std::env::temp_dir().join(format!(
        "ring3-runtime-pending-consumers-{}-{}",
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
    for (name, code, count, branch) in [
        ("adc32", &[0x83, 0xc0, 1, 0x83, 0xd2, 0][..], 2, false),
        ("sbb32", &[0x83, 0xe8, 1, 0x83, 0xda, 0][..], 2, false),
        (
            "adc16",
            &[0x66, 0x83, 0xc0, 1, 0x66, 0x83, 0xd2, 0][..],
            2,
            false,
        ),
        (
            "sbb16",
            &[0x66, 0x83, 0xe8, 1, 0x66, 0x83, 0xda, 0][..],
            2,
            false,
        ),
        ("adc8", &[0x80, 0xc0, 1, 0x80, 0xd2, 0][..], 2, false),
        ("sbb8", &[0x80, 0xe8, 1, 0x80, 0xda, 0][..], 2, false),
        (
            "inc-carry",
            &[0x83, 0xc0, 1, 0x43, 0x83, 0xd2, 0][..],
            3,
            false,
        ),
        (
            "logical",
            &[
                0xf9, 0x83, 0xd0, 0, 0x31, 0xd2, 0x0f, 0x92, 0xc3, 0x0f, 0x90, 0xc7,
            ][..],
            5,
            false,
        ),
        (
            "below",
            &[0x39, 0xd8, 0x72, 2, 0x0f, 0x0b, 0x0f, 0x0b][..],
            4,
            true,
        ),
        (
            "less",
            &[0x39, 0xd8, 0x7c, 2, 0x0f, 0x0b, 0x0f, 0x0b][..],
            4,
            true,
        ),
        (
            "logical-overflow",
            &[0x31, 0xc0, 0x70, 2, 0x0f, 0x0b, 0x0f, 0x0b][..],
            4,
            true,
        ),
        (
            "direction",
            &[0x83, 0xc0, 1, 0xfd, 0x9f, 0xfc, 0x9c][..],
            5,
            false,
        ),
    ] {
        memory.write(GuestAddress(0x1000), code).unwrap();
        let gates = if branch {
            vec![(GuestAddress(0x1004), 23), (GuestAddress(0x1006), 17)]
        } else {
            vec![]
        };
        let unit = compile_with_gates(&memory, GuestAddress(0x1000), count, &gates).unwrap();
        fs::write(scratch.join(format!("{name}.wasm")), unit.wasm).unwrap();
    }
    let result = Command::new("node").arg("--input-type=module").arg("-e").arg(r#"
import assert from 'node:assert/strict';
import fs from 'node:fs';
const memory=new WebAssembly.Memory({initial:2}),ram=new Uint8Array(memory.buffer),d=new DataView(memory.buffer),ctx=0x8000;
const word=(at,v)=>d.setUint32(ctx+at,v>>>0,true);
let fault=false,writes=[],observed=[];
const imports={env:{memory},cpu:{read(){throw Error('these authored consumers never read guest memory');},write(p,a,w,v){
  assert.equal(p,ctx);assert.equal(w,4);writes.push([a,v>>>0]);observed.push(ram.slice());
  if(fault){word(52,3);word(60,a);word(64,0x20204);word(68,4);return 3;}
  d.setUint32(a,v>>>0,true);word(52,0);return 0;
}}};
const units={};for(const name of['adc32','sbb32','adc16','sbb16','adc8','sbb8','inc-carry','logical','below','less','logical-overflow','direction'])units[name]=(await WebAssembly.instantiate(fs.readFileSync(`${process.argv[1]}/${name}.wasm`),imports)).instance.exports.run;
function scenario(name,input,expect){
  for(let i=0;i<ram.length;i++)ram[i]=(i*37+11)&255;ram.fill(0,ctx,ctx+160);
  word(0,1);word(4,160);[5,3,7,8,0x4800,10,11,12].forEach((v,i)=>word(8+4*i,v));
  for(const[index,v]of Object.entries(input.regs??{}))word(8+4*Number(index),v);
  word(40,0x1000);word(44,input.flags??0x200602);word(48,input.quota);word(56,0xfffffffe);word(152,63);
  const before=ram.slice(),expected=before.slice(),view=new DataView(expected.buffer),set=(at,v)=>view.setUint32(ctx+at,v>>>0,true);
  for(const[at,v]of[[40,expect.pc],[44,expect.flags],[48,input.quota-expect.steps],[52,expect.status??0],[56,(0xfffffffe+expect.steps)>>>0],...(expect.fields??[])])set(at,v);
  for(const[index,v]of Object.entries(expect.regs??{}))set(8+4*Number(index),v);
  for(const[a,v]of expect.stores??[])view.setUint32(a,v,true);
  fault=input.fault??false;writes=[];observed=[];
  assert.equal(units[name](ctx),expect.status??0,name);assert.deepEqual(ram,expected,`${name} full state`);assert.deepEqual(writes,expect.writes??[],name);
  if(writes.length)assert.deepEqual(observed,[before],`${name} retains the helper-visible entry publication`);else assert.deepEqual(observed,[]);
}
for(const[name,eax,edx,outEax,outEdx,flags,pc]of[
  ['adc32',0xffffffff,0x7fffffff,0,0x80000000,0x200e96,0x1006],
  ['sbb32',0,0x80000000,0xffffffff,0x7fffffff,0x200e16,0x1006],
  ['adc16',0x1234ffff,0x89ab7fff,0x12340000,0x89ab8000,0x200e96,0x1008],
  ['sbb16',0x12340000,0x89ab8000,0x1234ffff,0x89ab7fff,0x200e16,0x1008],
  ['adc8',0x1234ffff,0x89abcd7f,0x1234ff00,0x89abcd80,0x200e92,0x1006],
  ['sbb8',0x1234ff00,0x89abcd80,0x1234ffff,0x89abcd7f,0x200e12,0x1006],
])scenario(name,{quota:2,regs:{0:eax,2:edx}},{pc,steps:2,flags,regs:{0:outEax,2:outEdx}});
scenario('inc-carry',{quota:3,regs:{0:0xffffffff,2:0xffffffff,3:0xffffffff}},{pc:0x1007,steps:3,flags:0x200657,regs:{0:0,2:0,3:0}});
scenario('logical',{quota:5,regs:{0:0xffffffff,2:0x80000000,3:0x1234ffff}},{pc:0x100c,steps:5,flags:0x200646,regs:{0:0,2:0,3:0x12340000}});
for(const[name,a,b,taken,flags]of[
  ['below',0,1,true,0x200697],['below',0x80000000,1,false,0x200e16],['below',0x7fffffff,0xffffffff,true,0x200e87],
  ['less',0,1,true,0x200697],['less',0x80000000,1,true,0x200e16],['less',0x7fffffff,0xffffffff,false,0x200e87],
])scenario(name,{quota:3,regs:{0:a,3:b}},{pc:taken?0x1006:0x1004,steps:2,flags,status:5,fields:[[68,taken?17:23]]});
scenario('logical-overflow',{quota:3,regs:{0:0x80000000}},{pc:0x1004,steps:2,flags:0x200646,status:5,regs:{0:0},fields:[[68,23]]});
for(const[quota,pc,flags,eax]of[[2,0x1004,0x200657,0],[3,0x1005,0x200657,0x5700],[4,0x1006,0x200257,0x5700]])
  scenario('direction',{quota,flags:0x200202,regs:{0:0xffffffff}},{pc,steps:quota,flags,status:2,regs:{0:eax}});
scenario('direction',{quota:5,flags:0x200202,regs:{0:0xffffffff}},{pc:0x1007,steps:5,flags:0x200257,regs:{0:0x5700,4:0x47fc},stores:[[0x47fc,0x200257]],writes:[[0x47fc,0x200257]]});
scenario('direction',{quota:5,flags:0x200202,regs:{0:0xffffffff},fault:true},{pc:0x1006,steps:4,flags:0x200257,status:3,regs:{0:0x5700},writes:[[0x47fc,0x200257]],fields:[[60,0x47fc],[64,0x20204],[68,4]]});
console.log('literal narrow carry/overflow, signed and unsigned branch, preserved carry, dead logical recipe, DF/LAHF and PUSHF fault frontiers passed');
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
fn generated_optional_page_cache_rejects_legacy_reserved_bytes_without_dereferencing_them() {
    use std::{
        fs,
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };
    let scratch = std::env::temp_dir().join(format!(
        "ring3-runtime-cache-admission-{}-{}",
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
    memory.write(GuestAddress(0x1000), &[0x8b, 0x03]).unwrap();
    fs::write(
        scratch.join("read.wasm"),
        compile(&memory, GuestAddress(0x1000), 1).unwrap().wasm,
    )
    .unwrap();
    let result = Command::new("node").arg("--input-type=module").arg("-e").arg(r#"
import assert from 'node:assert/strict';
import fs from 'node:fs';
const memory=new WebAssembly.Memory({initial:225}),ram=new Uint8Array(memory.buffer),d=new DataView(memory.buffer);
const ctx=0x530000,cache=0x531000,map=0x1000,perm=0x410000,back=0x520000,rows=0x610000;
const put=(at,v)=>d.setUint32(at,v>>>0,true),word=(at,v)=>put(ctx+at,v);
let helpers=0;
const run=(await WebAssembly.instantiate(fs.readFileSync(`${process.argv[1]}/read.wasm`),{env:{memory},cpu:{read(){helpers++;throw Error('the unchanged direct mapping must remain usable');},write(){throw Error('unexpected write');}}})).instance.exports.run;
function init(pointer){
  for(let i=0;i<ram.length;i++)ram[i]=(i*31+7)&255;
  word(0,1);word(4,160);[1,2,3,0x3000,0x4800,6,7,8].forEach((v,i)=>word(8+4*i,v));
  for(const[at,v]of[[40,0x1000],[44,0x203],[48,1],[52,0],[56,0xffffffff],[72,pointer],[76,map],[80,perm],[84,back],[88,4],[152,0x1f]])word(at,v);
  put(map+3*4,1);ram[perm+3]=3;put(back,0x12345678);helpers=0;
}
function header(at,entries=rows){for(const[offset,value]of[[0,1],[4,0x434d3352],[8,32],[12,2],[16,entries],[20,1<<20],[24,0],[28,0]])put(at+offset,value);}
function check(label,edit){
  const expected=ram.slice(),view=new DataView(expected.buffer);
  for(const[at,v]of[[8,0x12345678],[40,0x1002],[48,0],[52,0],[56,0]])view.setUint32(ctx+at,v,true);
  edit?.(view);
  assert.equal(run(ctx),0,label);assert.deepEqual(ram,expected,label);assert.equal(helpers,0,label);
}
for(const pointer of[0,1,15,0x20,0x1000,ctx,cache+4,ram.length-32,ram.length-31,ram.length-16,ram.length,0xfffffff0,0xffffffff]){
  init(pointer);check(`legacy opaque word ${pointer.toString(16)}`);
}
for(const[field,value]of[[0,0],[4,0],[4,0x434d3353],[8,0],[8,4112],[8,0xffffffff],[12,0],[12,1],[16,0],[16,rows+1],[16,rows+8],[16,0xfffffff8],[20,0],[20,(1<<20)-1],[20,(1<<20)+1],[24,1],[28,1]]){
  init(cache);header(cache);put(cache+field,value);
  check(`malformed cache header field${field} value${value}`);
}
{
  init(cache);header(cache);word(20,0xfffff000);put(map+0xfffff*4,1);ram[perm+0xfffff]=3;
  check('the final flat row and exact table span remain usable',view=>view.setBigUint64(ram.length-8,0x100000003n,true));
}
{
  const tail=ram.length-32,entries=0x600000;
  init(tail);header(tail,entries);
  check('the final complete header span remains usable',view=>view.setBigUint64(entries+3*8,0x100000003n,true));
}
console.log('optional page-cache admission preserves opaque legacy field72 and rejects unaligned, truncated, wrapped and malformed cache pointers without guest effects');
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
