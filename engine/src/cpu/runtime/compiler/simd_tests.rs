use super::*;
use crate::memory::{PageRange, Permissions};

#[test]
fn generated_numeric_view_prefixes_preserve_lazy_bank_order_and_exact_bounds() {
    use std::{
        fs,
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };
    let scratch = std::env::temp_dir().join(format!(
        "ring3-runtime-numeric-views-{}-{}",
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
        ("integer", vec![0xb8, 0x78, 0x56, 0x34, 0x12, 0x90], 2),
        ("x87", vec![0xb8, 0x78, 0x56, 0x34, 0x12, 0xd9, 0xe8], 2),
        (
            "simd",
            vec![0xb8, 0x78, 0x56, 0x34, 0x12, 0x0f, 0x28, 0xc1],
            2,
        ),
        (
            "x87-simd",
            vec![0xb8, 0x78, 0x56, 0x34, 0x12, 0xd9, 0xe8, 0x0f, 0x28, 0xc1],
            3,
        ),
        (
            "simd-x87",
            vec![0xb8, 0x78, 0x56, 0x34, 0x12, 0x0f, 0x28, 0xc1, 0xd9, 0xe8],
            3,
        ),
    ] {
        memory.write(GuestAddress(0x1000), &bytes).unwrap();
        fs::write(
            scratch.join(format!("{name}.wasm")),
            compile(&memory, GuestAddress(0x1000), limit).unwrap().wasm,
        )
        .unwrap();
    }
    let result = Command::new("node").arg("--input-type=module").arg("-e").arg(r#"
import assert from 'node:assert/strict';import fs from 'node:fs';
const root=process.argv[1],memory=new WebAssembly.Memory({initial:1}),ram=new Uint8Array(memory.buffer),data=new DataView(memory.buffer),state=0x8000,view=0x9000,x87=0xa000,simd=0xa100;
const word=(at,value)=>data.setUint32(state+at,value>>>0,true),put=(at,value)=>data.setUint32(at,value>>>0,true);
let fpAt=x87,simdAt=simd;
const imports={env:{memory},cpu:{read(){throw Error('register-only numeric fixture must not call cpu.read');},write(){throw Error('register-only numeric fixture must not call cpu.write');}}};
const programs=new Map([['integer',[['mov',5],['nop',1]]],['x87',[['mov',5],['x87',2]]],['simd',[['mov',5],['simd',3]]],['x87-simd',[['mov',5],['x87',2],['simd',3]]],['simd-x87',[['mov',5],['simd',3],['x87',2]]]]),functions=new Map(),modules=new Map();
for(const name of programs.keys()){
  const module=new WebAssembly.Module(fs.readFileSync(`${root}/${name}.wasm`));
  assert.deepEqual(WebAssembly.Module.imports(module).map(x=>`${x.module}.${x.name}:${x.kind}`),['env.memory:memory','cpu.read:function','cpu.write:function']);
  assert.deepEqual(WebAssembly.Module.exports(module).map(x=>[x.name,x.kind]),[['run','function']]);
  modules.set(name,module);
  functions.set(name,new WebAssembly.Instance(module,imports).exports.run);
}
function init(quota){
  for(let i=0;i<ram.length;i++)ram[i]=(i*37+11)&255;ram.fill(0,state,state+160);ram.fill(0,view,view+40);
  word(0,1);word(4,160);[0xaabbccdd,2,3,4,0x4800,6,7,8].forEach((v,i)=>word(8+4*i,v));word(40,0x1000);word(44,0xcd7);word(48,quota);word(124,view);word(152,0x1f);
  put(view,1);put(view+4,40);put(view+8,x87);put(view+36,simd);[0x23,0x1b,0x23,0x23,0x3b,0x43].forEach((v,i)=>put(view+12+4*i,v));
  data.setUint16(x87,0x037f,true);data.setUint16(x87+2,0x4700,true);data.setUint16(x87+4,0xffff,true);put(simd,0x1fbf);fpAt=x87;simdAt=simd;
  [0x7f801234,0xffc54321,0x80000000,1].forEach((v,i)=>put(simd+20+4*i,v));
}
function edgeView(size){const at=ram.length-36;ram.set(ram.slice(view,view+36),at);put(at+4,size);word(124,at);}
function edgeX87(){fpAt=ram.length-100;ram.set(ram.slice(x87,x87+100),fpAt);put(view+8,fpAt);}
function edgeSimd(){simdAt=ram.length-132;ram.set(ram.slice(simd,simd+132),simdAt);put(view+36,simdAt);}
const cases=[
  ['valid',()=>{},true,true],
  ['36-byte-prefix',()=>put(view+4,36),true,false],
  ['maximum-reported-size',()=>put(view+4,0xffffffff),true,true],
  ['physical-36-reported-36',()=>edgeView(36),true,false],
  ['physical-36-reported-40',()=>edgeView(40),true,false],
  ['physical-36-reported-maximum',()=>edgeView(0xffffffff),true,false],
  ['zero-view',()=>word(124,0),false,false],
  ['overflow-view',()=>word(124,0xfffffff0),false,false],
  ['misaligned-view',()=>word(124,view+1),false,false],
  ['unknown-view-version',()=>put(view,2),false,false],
  ['short-view-header',()=>put(view+4,35),false,false],
  ['zero-x87-bank',()=>put(view+8,0),false,true],
  ['misaligned-x87-bank',()=>put(view+8,x87+1),false,true],
  ['x87-span-96',()=>put(view+8,ram.length-96),false,true],
  ['overflow-x87-bank',()=>put(view+8,0xfffffffc),false,true],
  ['exact-x87-span-100',edgeX87,true,true],
  ['zero-simd-bank',()=>put(view+36,0),true,false],
  ['misaligned-simd-bank',()=>put(view+36,simd+1),true,false],
  ['simd-span-128',()=>put(view+36,ram.length-128),true,false],
  ['overflow-simd-bank',()=>put(view+36,0xfffffffc),true,false],
  ['exact-simd-span-132',edgeSimd,true,true],
  ['pending-unmasked-x87',()=>{data.setUint16(x87,0x037e,true);data.setUint16(x87+2,1,true);},false,true],
  ['reserved-mxcsr',()=>put(simd,0x10000),true,false],
];
for(const[label,mutate,x87OK,simdOK]of cases)for(const[name,steps]of programs)for(const quota of[0,1,2,3]){
  init(quota);mutate();const expected=ram.slice(),d=new DataView(expected.buffer);let remaining=quota,retired=0,pc=0x1000,status=0;
  for(const[kind,length]of steps){
    if(!remaining){status=2;break;}
    if(kind==='x87'&&!x87OK||kind==='simd'&&!simdOK){status=7;break;}
    if(kind==='mov')d.setUint32(state+8,0x12345678,true);
    if(kind==='x87'){
      for(const[at,value]of[[2,0x7d00],[4,0x3fff],[6,0x1e8],[16,0x1b]])d.setUint16(fpAt+at,value,true);
      d.setUint32(fpAt+8,pc,true);expected.set([0,0,0,0,0,0,0,0x80,0xff,0x3f],fpAt+90);
    }
    if(kind==='simd')expected.set(expected.slice(simdAt+20,simdAt+36),simdAt+4);
    remaining--;retired++;pc+=length;
  }
  for(const[at,value]of[[40,pc],[48,remaining],[52,status],[56,retired]])d.setUint32(state+at,value,true);
  assert.equal(functions.get(name)(state),status,`${label}/${name}/quota${quota}`);
  assert.deepEqual(ram,expected,`${label}/${name}/quota${quota}: full RAM, independent banks, exact first reached guard and prefix`);
}
function completedMixed(bytes,context,fp,vector){
  const expected=bytes.slice(),d=new DataView(expected.buffer);
  for(const[at,value]of[[8,0x12345678],[40,0x100a],[48,0],[52,0],[56,3]])d.setUint32(context+at,value,true);
  for(const[at,value]of[[2,0x7d00],[4,0x3fff],[6,0x1e8],[16,0x1b]])d.setUint16(fp+at,value,true);
  d.setUint32(fp+8,0x1005,true);expected.set([0,0,0,0,0,0,0,0x80,0xff,0x3f],fp+90);
  expected.set(expected.slice(vector+20,vector+36),vector+4);return expected;
}
// one compiled module, two imported memories and repeated fresh invocations.
const otherMemory=new WebAssembly.Memory({initial:1}),otherRAM=new Uint8Array(otherMemory.buffer),otherData=new DataView(otherMemory.buffer),otherState=0x8100,otherView=0x9200,otherFP=0xa400,otherSIMD=0xa500;
const otherRun=new WebAssembly.Instance(modules.get('x87-simd'),{env:{memory:otherMemory},cpu:imports.cpu}).exports.run;
for(let round=0;round<3;round++){
  init(3);
  if(round===1){
    const changedView=0x9400;fpAt=0xa800;simdAt=0xa900;
    ram.set(ram.slice(view,view+40),changedView);ram.set(ram.slice(x87,x87+100),fpAt);ram.set(ram.slice(simd,simd+132),simdAt);
    put(changedView+8,fpAt);put(changedView+36,simdAt);word(124,changedView);put(simdAt+20,0xdeadbeef);
  }
  const expected=completedMixed(ram,state,fpAt,simdAt);
  assert.equal(functions.get('x87-simd')(state),0);assert.deepEqual(ram,expected,'same instance must rebind both bank proofs between invocations');
  otherRAM.fill(0x6b);otherRAM.fill(0,otherState,otherState+160);otherRAM.fill(0,otherView,otherView+40);
  for(const[at,value]of[[0,1],[4,160],[40,0x1000],[44,0xcd7],[48,3],[124,otherView],[152,0x1f]])otherData.setUint32(otherState+at,value,true);
  for(const[at,value]of[[0,1],[4,40],[8,otherFP],[16,0x1b],[36,otherSIMD]])otherData.setUint32(otherView+at,value,true);
  otherData.setUint16(otherFP,0x007f,true);otherData.setUint16(otherFP+2,0x4700,true);otherData.setUint16(otherFP+4,0xffff,true);otherData.setUint32(otherSIMD,0xffff,true);
  [0xcafebabe,round,0x7fc01234,0x80000000].forEach((v,i)=>otherData.setUint32(otherSIMD+20+i*4,v,true));
  const otherExpected=completedMixed(otherRAM,otherState,otherFP,otherSIMD);
  assert.equal(otherRun(otherState),0);assert.deepEqual(otherRAM,otherExpected,'private proof helpers must use their own imported memory and context');
  assert.deepEqual(ram,expected,'the other instance must not change the first memory');
}
console.log('numeric-view baseline: both bank orders, integer prefix/quota, physical36/40, minimum headers, exact100/132 bounds, pointer overflow/alignment, pendingMF and MXCSR full-RAM passed');
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
            "integer-prefix",
            vec![0xb8, 0x78, 0x56, 0x34, 0x12, 0x0f, 0x28, 0xc1],
            2,
        ),
        ("x87-then-simd", vec![0xd9, 0xe8, 0x0f, 0x28, 0xc1], 2),
        ("simd-then-x87", vec![0x0f, 0x28, 0xc1, 0xd9, 0xe8], 2),
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
for(const name of['load','store','copy','self-copy','unaligned-load','unaligned-store','add-memory','add-register','add-self','integer-prefix','x87-then-simd','simd-then-x87','kernel'])functions.set(name,(await WebAssembly.instantiate(fs.readFileSync(`${root}/${name}.wasm`),imports)).instance.exports.run);
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
function expected(before,{status=0,next=0x1003,remaining=0,retired=1,xmm,xmmAt=simd+4,x87Bank,memory:stored,dirty=false,vector}={}){
  const out=before.slice(),d=new DataView(out.buffer),set=(at,v)=>d.setUint32(at,v>>>0,true);
  for(const[at,v]of[[40,next],[48,remaining],[52,status],[56,retired]])set(state+at,v);
  if(vector!==undefined){set(state+60,0);set(state+64,0);set(state+68,vector);}
  if(xmm)xmm.forEach((v,i)=>set(xmmAt+i*4,v));
  if(x87Bank)out.set(x87Bank,x87);
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
// An independent integer oracle uses exact units of 2^-149. It does not use
// floating addition, TwoSum, or the emitter's exponent-gap admission shortcut.
function exactAdd(a,b){
  function units(bits){
    bits>>>=0;const exponent=(bits>>>23)&255,fraction=bits&0x7fffff;
    if(exponent===255||exponent===0&&fraction!==0)return null;
    const magnitude=exponent===0?0n:BigInt(0x800000|fraction)<<BigInt(exponent-1);
    return bits>>>31?-magnitude:magnitude;
  }
  const left=units(a),right=units(b);if(left===null||right===null)return null;
  let sum=left+right;
  if(sum===0n)return a===0x80000000&&b===0x80000000?0x80000000:0;
  const negative=sum<0n;if(negative)sum=-sum;
  const top=sum.toString(2).length-1;if(top<23||top>276)return null;
  const shift=BigInt(top-23);if((sum&((1n<<shift)-1n))!==0n)return null;
  return (((negative?0x80000000:0)|((top-22)<<23))|Number((sum>>shift)&0x7fffffn))>>>0;
}
assert.equal(exactAdd(0x3f800000,0xb3800000),0x3f7fffff);
assert.equal(exactAdd(0x00800001,0x80800000),null);
assert.equal(exactAdd(0x80000000,0x80000000),0x80000000);
assert.equal(exactAdd(0x3f800000,0x21800000),null);
const gapPairs=[0,1,23,24,27,28,29,30,52,53,54,60,126].flatMap(gap=>{
  const small=(127-gap)<<23;
  return gap===0?[[0x3f800000,small]]:[[0x3f800000,small],[small,0x3f800000]];
});
const boundaryPairs=[
  ...gapPairs,[0x3f800001,0xbf800000],[0x00800001,0x80800000],
  [0x00800000,0x80800000],[0x00800000,0x00800000],
  [0,0x7f7fffff],[0x80000000,0],[0x80000000,0x80000000],
  [0x7f7fffff,0xff7fffff],[0x7f7fffff,0x7f7fffff],
];
for(const[p,q]of boundaryPairs)for(let signs=0;signs<4;signs++)for(let lane=0;lane<4;lane++){
  const left=[...a],right=[...b];left[lane]=(p^((signs&1)?0x80000000:0))>>>0;right[lane]=(q^((signs&2)?0x80000000:0))>>>0;
  const result=left.map((v,i)=>exactAdd(v,right[i]));
  for(const name of['add-register','add-memory'])for(const sticky of[0,63]){
    init(0x3000,1,0x1f80|sticky);setXmm(0,left);setXmm(1,right);setMemory(0x3000,right);
    if(result.includes(null))miss(name);else run(name,{xmm:result});
  }
}
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
for(const change of[
  ()=>word(124,0),()=>word(124,0xfffffff0),()=>word(124,view+1),()=>put(view,2),()=>put(view+4,36),
  ()=>put(view+36,0),()=>put(view+36,simd+1),()=>put(view+36,0x3ff80),
])for(const quota of[0,1,2]){
  init(0x3000,quota);change();const want=expected(ram.slice(),{status:quota===2?7:2,next:quota?0x1005:0x1000,remaining:quota-(quota?1:0),retired:quota?1:0});
  if(quota)new DataView(want.buffer).setUint32(state+8,0x12345678,true);
  assert.equal(functions.get('integer-prefix')(state),quota===2?7:2);
  assert.deepEqual(ram,want,'first reached SIMD validates after the exact integer prefix');
  assert.deepEqual([reads,writes],[0,0]);
}
function firstOneBank(){
  const bank=ram.slice(x87,x87+100),d=new DataView(bank.buffer);
  for(const[at,value]of[[2,0x7d00],[4,0x3fff],[6,0x1e8],[16,0x1b]])d.setUint16(at,value,true);
  d.setUint32(8,0x1000,true);bank.set([0,0,0,0,0,0,0,0x80,0xff,0x3f],90);return bank;
}
// A valid x87 prefix proves only the 36-byte view, not the SIMD extension.
for(const change of[()=>put(view+4,36),()=>put(view+36,0)]){
  init(0x3000,2);data.setUint16(x87,0x037f,true);data.setUint16(x87+2,0x4700,true);data.setUint16(x87+4,0xffff,true);change();
  run('x87-then-simd',{status:7,next:0x1002,remaining:1,retired:1,x87Bank:firstOneBank()});
}
init(0x3000,2);data.setUint16(x87,0x037f,true);data.setUint16(x87+2,0x4700,true);data.setUint16(x87+4,0xffff,true);
run('x87-then-simd',{next:0x1005,retired:2,xmm:b,x87Bank:firstOneBank()});
// A successful SIMD proof does not validate the separate x87 bank pointer.
init(0x3000,2);put(view+8,0);run('simd-then-x87',{status:7,next:0x1003,remaining:1,retired:1,xmm:b});
// Reusing the exported function must validate a newly selected bank each time.
init();const originalSIMD=ram.slice(simd,simd+132);run('copy',{xmm:b});
const otherView=0x9100,otherSIMD=0xa400;
ram.set(ram.slice(view,view+40),otherView);put(otherView+36,otherSIMD);ram.set(originalSIMD,otherSIMD);payload.forEach((v,i)=>put(otherSIMD+20+i*4,v));
word(124,otherView);word(40,0x1000);word(48,1);word(52,0);word(56,0);run('copy',{xmm:payload,xmmAt:otherSIMD+4});
put(otherView+36,0);word(40,0x1000);word(48,1);word(52,0);word(56,0);miss('copy');
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
