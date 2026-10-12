import assert from 'node:assert/strict';
import {readFileSync,writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {createHash} from 'node:crypto';
import {readEngine} from '../support/engine.mjs';
const [enginePath,output,root]=process.argv.slice(2),engine=readEngine(enginePath),initial=readFileSync(join(output,'initial-arena.bin'));
const SIZE=initial.length,SV=4364,FP=4236,SG=4524,T=140,DATA=0x5010;
const hash=b=>createHash('sha256').update(b).digest('hex'),hex=s=>Buffer.from(s,'hex');
const dwords=v=>{const b=Buffer.alloc(v.length*4);v.forEach((n,i)=>b.writeUInt32LE(n>>>0,i*4));return b;};
const qwords=v=>{const b=Buffer.alloc(v.length*8);v.forEach((n,i)=>b.writeBigUInt64LE(n,i*8));return b;};
const fill=n=>Buffer.alloc(16,n),zero=fill(0),parents=()=>[0x10203040,0x21314151,0x32425262,0x43536373,0x7080,0x65758595,0x768696a6,0x8797a7b7];
const record=(magic,version,size,values)=>{const b=Buffer.alloc(size);b.write(magic);b.writeUInt16LE(version,4);b.writeUInt16LE(1,6);b.writeUInt32LE(size,8);values.forEach((n,i)=>b.writeUInt32LE(n>>>0,16+i*4));return b;};
const state=(r,pc,flags)=>record('R3ST',1,56,[...r,pc,flags]);
const exit=(reason,retired,detail=0,address=0,access=0,length=0)=>record('R3EX',6,40,[reason,retired,detail,address,access,length]);
const packet=(length,status=0,detail=0,address=0,access=0)=>record('R3MH',7,40,[status,0,detail,address,access,length]);
const savedImage=Buffer.alloc(512);savedImage.writeUInt16LE(0x37f);savedImage.writeUInt32LE(0x1f80,24);savedImage.writeUInt32LE(0xffff,28);savedImage.fill(0x31,160,176);savedImage.fill(0x42,176,192);savedImage.fill(0x66,464);
const restoredImage=Buffer.alloc(512);restoredImage.writeUInt16LE(0x37f);restoredImage.writeUInt16LE(0x1800,2);restoredImage[4]=0x18;restoredImage.writeUInt32LE(0x5f80,24);restoredImage.writeUInt32LE(0xffff,28);
const restoredX87=record('R3FP',1,128,[]);restoredX87.writeUInt16LE(0x37f,16);restoredX87.writeUInt16LE(0x1800,18);restoredX87.writeUInt16LE(0xfd3f,20);
for(let physical=0;physical<8;physical++){const raw=Buffer.alloc(10);if(physical!==4){raw.writeBigUInt64LE(0x8000000000000000n+BigInt(physical));raw.writeUInt16LE(0x3fff,8);}raw.copy(restoredX87,40+physical*10);raw.copy(restoredImage,32+((physical+5)&7)*16);restoredImage.fill(0x30+physical,160+physical*16,176+physical*16);}
const cases=[
 {name:'movlps-store-raw-bits-and-canaries',code:'0f1300',a:hex('341200000000f07f1122334455667788'),write:hex('341200000000f07f6666666666666666'),writeLength:8},
 {name:'paddb-wrap',code:'660ffcc1',direct:true,a:fill(255),b:fill(1),out:zero},
 {name:'paddsb-saturate',code:'660fecc1',direct:true,a:fill(127),b:fill(1),out:fill(127)},
 {name:'pmaddwd-overflow',code:'660ff5c1',direct:true,a:hex('00800080008000800080008000800080'),b:hex('00800080008000800080008000800080'),out:dwords([0x80000000,0x80000000,0x80000000,0x80000000])},
 {name:'pshufd-self-alias',code:'660f70c01b',direct:true,a:dwords([1,2,3,4]),out:dwords([4,3,2,1])},
 {name:'pandn-operand-order',code:'660fdfc1',direct:true,a:fill(0xf0),b:fill(0x33),out:fill(3)},
 {name:'punpckhbw-source-alias',code:'660f68c0',direct:true,a:Buffer.from(Array.from({length:16},(_,i)=>i)),out:hex('080809090a0a0b0b0c0c0d0d0e0e0f0f')},
 {name:'pmovmskb-high-bit-order',code:'660fd7c0',direct:true,a:hex('80008000800080008000800080008000'),gpr:0x5555},
 {name:'pextrw-zero-extends',code:'660fc5c003',direct:true,a:hex('010203040506ffff090a0b0c0d0e0f10'),gpr:0xffff},
 {name:'psrlq-full64-count',code:'660fd3c1',a:fill(255),b:qwords([0x100000000n,0n]),out:zero},
 {name:'addss-nearest-even',code:'f30f58c1',a:dwords([0x3f800000,1,2,3]),b:dwords([0x33800000,4,5,6]),out:dwords([0x3f800000,1,2,3]),mxcsr:0x1f80,afterMxcsr:0x1fa0},
 {name:'addss-round-up',code:'f30f58c1',a:dwords([0x3f800000,1,2,3]),b:dwords([0x33800000,4,5,6]),out:dwords([0x3f800001,1,2,3]),mxcsr:0x5f80,afterMxcsr:0x5fa0},
 {name:'addpd-two-exact-lanes',code:'660f58c1',a:qwords([0x3ff0000000000000n,0x4000000000000000n]),b:qwords([0x4000000000000000n,0x4008000000000000n]),out:qwords([0x4008000000000000n,0x4014000000000000n])},
 {name:'sqrtss-integer-radical',code:'f30f51c1',a:dwords([0,1,2,3]),b:dwords([0x40000000,0,0,0]),out:dwords([0x3fb504f3,1,2,3]),afterMxcsr:0x1fa0},
 {name:'snan-source1-priority',code:'f30f58c1',a:dwords([0x7f801234,1,2,3]),b:dwords([0x7fc05678,4,5,6]),out:dwords([0x7fc01234,1,2,3]),afterMxcsr:0x1f81},
 {name:'divide-unmasked-stops',code:'f30f5ec1',a:dwords([0x3f800000,1,2,3]),b:zero,mxcsr:0x1d80,afterMxcsr:0x1d84,reason:13,retired:0,detail:0x1d84},
 {name:'overflow-unmasked-suppresses-own-precision',code:'f30f59c1',a:dwords([0x7f7fffff,1,2,3]),b:dwords([0x40000000,0,0,0]),mxcsr:0x1b80,afterMxcsr:0x1b88,reason:13,retired:0,detail:0x1b88},
 {name:'exact-tiny-unmasked-underflow',code:'f30f59c1',a:dwords([0x00800000,1,2,3]),b:dwords([0x3f000000,0,0,0]),mxcsr:0x1780,afterMxcsr:0x1790,reason:13,retired:0,detail:0x1790},
 {name:'ftz-ignored-when-underflow-unmasked',code:'f30f59c1',a:dwords([0x00800000,1,2,3]),b:dwords([0x3f000000,0,0,0]),mxcsr:0x9780,afterMxcsr:0x9790,reason:13,retired:0,detail:0x9790},
 {name:'tiny-before-rounding-to-normal-unmasked',code:'f30f59c1',a:dwords([0x00800000,1,2,3]),b:dwords([0x3f7fffff,0,0,0]),mxcsr:0x1780,afterMxcsr:0x1790,reason:13,retired:0,detail:0x1790},
 {name:'tiny-before-rounding-to-normal-masked',code:'f30f59c1',a:dwords([0x00800000,1,2,3]),b:dwords([0x3f7fffff,0,0,0]),out:dwords([0x00800000,1,2,3]),afterMxcsr:0x1fb0},
 {name:'tiny-before-rounding-to-normal-ftz',code:'f30f59c1',a:dwords([0x00800000,1,2,3]),b:dwords([0x3f7fffff,0,0,0]),out:dwords([0,1,2,3]),mxcsr:0x9f80,afterMxcsr:0x9fb0},
 {name:'double-tiny-before-rounding-to-normal',code:'f20f59c1',a:qwords([0x0010000000000000n,0x123456789abcdef0n]),b:qwords([0x3fefffffffffffffn,0n]),mxcsr:0x1780,afterMxcsr:0x1790,reason:13,retired:0,detail:0x1790},
 {name:'cvtsd2ss-tiny-before-rounding-to-normal',code:'f20f5ac1',a:dwords([1,2,3,4]),b:qwords([0x380fffffe0000000n,0n]),mxcsr:0x1780,afterMxcsr:0x1790,reason:13,retired:0,detail:0x1790},
 {name:'packed-overflow-retains-other-lane-precision',code:'0f59c1',a:dwords([0x7f7fffff,0x3f800001,0,0]),b:dwords([0x40000000,0x3f800001,0,0]),mxcsr:0x1b80,afterMxcsr:0x1ba8,reason:13,retired:0,detail:0x1ba8},
 {name:'packed-invalid-retains-other-lane-denormal',code:'0f59c1',a:dwords([0x7f800000,1,0,0]),b:dwords([0,0x3f800000,0,0]),mxcsr:0x1f00,afterMxcsr:0x1f03,reason:13,retired:0,detail:0x1f03},
 {name:'minss-qnan-masked-invalid-source2',code:'f30f5dc1',a:dwords([0x7fc01234,1,2,3]),b:zero,out:dwords([0,1,2,3]),afterMxcsr:0x1f81},
 {name:'minss-qnan-unmasked-invalid',code:'f30f5dc1',a:dwords([0x7fc01234,1,2,3]),b:zero,mxcsr:0x1f00,afterMxcsr:0x1f01,reason:13,retired:0,detail:0x1f01},
 {name:'ucomiss-unordered-flags',code:'0f2ec1',a:dwords([0x7fc01234,1,2,3]),b:zero,flags:0x447},
 {name:'cvttps2dq-indefinite',code:'f30f5bc1',a:zero,b:dwords([0x7fc01234,0x3fc00000,0xbfc00000,0x4f000000]),out:dwords([0x80000000,1,0xffffffff,0x80000000]),afterMxcsr:0x1fa1},
 {name:'movdqu-unaligned-read',code:'f30f6f00',direct:true,a:zero,memory:fill(0x35),address:DATA+1,out:fill(0x35)},
 {name:'movaps-alignment-before-unmapped',code:'0f2800',direct:true,a:fill(0x19),address:0x6001,reason:11,retired:0,detail:0},
 {name:'movdqu-read-unmapped',code:'f30f6f00',direct:true,a:fill(0x19),address:0x6000,reason:5,retired:0,detail:1,fault:0x6000,access:1,length:16},
 {name:'movdqu-unaligned-write',code:'f30f7f00',direct:true,a:fill(0x5a),address:DATA+1,write:fill(0x5a)},
 {name:'movntq-unaligned-writable-store',code:'0fe700',direct:true,mmx:true,a:hex('0123456789abcdef'),address:DATA+1,write:hex('0123456789abcdef'),writeLength:8},
 {name:'movntps-still-requires-alignment',code:'0f2b00',direct:true,a:fill(0x5a),address:DATA+1,reason:11,retired:0,unchangedMemory:fill(0x66)},
 {name:'movntdq-still-requires-alignment',code:'660fe700',direct:true,a:fill(0x5a),address:DATA+1,reason:11,retired:0,unchangedMemory:fill(0x66)},
 {name:'maskmovdqu-sparse',code:'660ff7c1',a:Buffer.from(Array.from({length:16},(_,i)=>i+1)),b:hex('80008000000000800000000000000080'),masked:0x8085,address:DATA},
 {name:'maskmovdqu-zero-null-segment-no-access',code:'660ff7c1',a:fill(0x51),b:zero,masked:0,address:0xffffffff,nullDs:true},
 {name:'maskmovdqu-selected-null-segment-gp',code:'660ff7c1',a:fill(0x51),b:fill(0x80),address:DATA,nullDs:true,reason:11,retired:0,compiledGp:true},
 {name:'mmx-paddb-wrap',code:'0ffcc1',direct:true,mmx:true,a:fill(255),b:fill(1),out:zero},
 {name:'pending-x87-stops-mmx-before-helper',code:'0ffcc1',direct:true,mmx:true,pendingX87:true,a:fill(255),b:fill(1),reason:12,retired:0,detail:0x8081},
 {name:'sse-independent-of-pending-x87',code:'660ffcc1',direct:true,pendingX87:true,a:fill(255),b:fill(1),out:zero},
 {name:'emms-preserves-raw80',code:'0f77',empty:true},
 {name:'ldmxcsr-reserved-gp',code:'0fae10',address:DATA,memory:dwords([0x10000,0,0,0]),reason:11,retired:0},
 {name:'stmxcsr-exact-dword',code:'0fae18',address:DATA,mxcsr:0x5f80,write:dwords([0x5f80]),writeLength:4},
 {name:'fxsave-write-only-preserves-software-tail',code:'0fae00',address:DATA,a:fill(0x31),b:fill(0x42),seedMemory:Buffer.alloc(512,0x66),write:savedImage,writeLength:512,writeOnly:true},
 {name:'fxrstor-physical-tags-and-xmm',code:'0fae08',address:DATA,memory:restoredImage,afterX87:restoredX87,afterXmm:restoredImage.subarray(160,288),afterMxcsr:0x5f80},
];
const sourcePaths=['engine/src/cpu/x86/simd.rs','engine/src/cpu/x86/simd/packed.rs','engine/src/cpu/x86/simd/float.rs','engine/src/cpu/x86/decode/simd.rs','engine/src/cpu/dbt/wasm/simd.rs','engine/src/cpu/dbt/wasm/simd/direct.rs','engine/src/process/simd.rs','engine/src/abi/x86/simd.rs','engine/tests/simd_semantics.rs','engine/tests/cpu_simd_wasm.rs','engine/tests/fixtures/p2-simd/run.mjs'];
const sourceHashes=()=>Object.fromEntries(sourcePaths.map(p=>[p,hash(readFileSync(join(root,p)))])),beforePins=sourceHashes();
const observations=[],helpers=[],modules=[],contexts=[],raw_protocol=[];let count=0;
const arena=c=>Buffer.from(new Uint8Array(c.memory.buffer,c.base,SIZE));
const input=(c,at,b)=>new Uint8Array(c.memory.buffer,c.base+at,b.length).set(b);
const host=(c,name,args)=>assert.equal(c.api[name](...args),0,name);
const upload=(c,address,b)=>{input(c,T,b);host(c,'upload',[address,b.length]);};
function open(owner,entries,batch){
 const instance=new WebAssembly.Instance(engine.module,{}),c={owner,entries,memory:instance.exports.memory,api:{},id:++count};
 for(const[name,fn]of Object.entries(instance.exports))if(name.startsWith('ring3_abi_v1_')&&typeof fn==='function')c.api[name.slice(13)]=fn;
 host(c,'open',[8,c.id,0x53494d44]);c.base=c.api.arena_ptr()>>>0;input(c,0,initial);
 for(const[page,bits]of[[0x1000,7],[0x5000,3],[0x7000,3]])host(c,'map',[page,1,bits]);
 const blocks=batch.map((row,i)=>({pc:0x1000+i*32,bytes:Buffer.concat([hex(row.code),hex('ebfe')])}));for(const b of blocks)upload(c,b.pc,b.bytes);
 host(c,'protect',[0x1000,1,5]);input(c,T,dwords(blocks.flatMap(b=>entries?[b.pc]:[b.pc,b.bytes.length])));
 host(c,owner==='resident'?(entries?'compile_resident_entries':'compile_resident'):(entries?'compile_entries':'compile'),entries?[blocks.length,0]:[blocks.length]);
 const a=arena(c),binding=owner==='resident'?{lo:a.readUInt32LE(T+8),hi:a.readUInt32LE(T+12),pointer:a.readUInt32LE(T+16),length:a.readUInt32LE(T+20)}:{pointer:c.api.module_ptr()>>>0,length:c.api.module_len()>>>0};
 const bytes=Buffer.from(new Uint8Array(c.memory.buffer,binding.pointer,binding.length)),module=new WebAssembly.Module(bytes),ring3={};
 for(const im of WebAssembly.Module.imports(module)){if(im.kind!=='function')continue;ring3[im.name]=(...args)=>{
  if(c.raw&&im.name.startsWith('guard'))return 0;
  let result;
  if(c.raw&&(im.name==='simd_execute'||im.name==='simd_execute_resident')){
   const p=packet(0);p.writeUInt32LE(c.raw.value??0,20);input(c,100,p);input(c,T+512,dwords([c.raw.gpr??0xdeadbeef,c.raw.flags]));result=0;
  }else result=c.api[im.name](...args);
  if(!im.name.startsWith('guard'))helpers.push({context:c.id,name:im.name,args:args.map(v=>v>>>0),status:result,packet:arena(c).subarray(100,140).toString('hex'),...(c.raw?{nonforwarded:im.name==='simd_execute'||im.name==='simd_execute_resident'}:{})});return result;
 };}
 c.run=new WebAssembly.Instance(module,{env:{memory:c.memory},ring3}).exports.run;
 writeFileSync(join(output,`module-${c.id}.wasm`),bytes);modules.push({context:c.id,owner,entries,sha256:hash(bytes),bytes:bytes.length,imports:WebAssembly.Module.imports(module)});contexts.push({context:c.id,owner,entries});return[c,blocks];
}
function test(c,block,row){
 input(c,0,initial);const r=parents();r[0]=row.address??DATA;r[7]=row.address??DATA;input(c,0,state(r,block.pc,0xcd7));input(c,56,exit(1,0));
 input(c,SV+16,dwords([row.mxcsr??0x1f80]));
 if(row.pendingX87){input(c,FP+16,hex('7e030100'));}
 if(row.nullDs){input(c,SG+22,hex('0000'));}
 if(row.mmx){input(c,FP+40,(row.a??zero).subarray(0,8));input(c,FP+50,(row.b??zero).subarray(0,8));}else{input(c,SV+32,row.a??zero);input(c,SV+48,row.b??zero);}
 upload(c,0x5000,fill(0x66));upload(c,DATA,fill(0x66));upload(c,DATA+16,fill(0x66));if(row.memory)upload(c,row.address??DATA,row.memory);
 if(row.seedMemory)upload(c,row.address??DATA,row.seedMemory);
 if(row.writeOnly)host(c,'protect',[0x5000,1,2]);
 const expected=arena(c),reason=row.reason??1,retired=row.retired??1;
 if(row.gpr!==undefined)r[0]=row.gpr;
 expected.set(state(r,retired?block.pc+hex(row.code).length:block.pc,row.flags??0xcd7));expected.set(exit(reason,retired,row.detail??0,row.fault??0,row.access??0,row.length??0),56);
 if(row.out){if(row.mmx){expected.set(row.out.subarray(0,8),FP+40);expected.fill(255,FP+48,FP+50);}else expected.set(row.out,SV+32);}
 if(row.mmx&&retired){expected.writeUInt16LE(0,FP+20);expected.writeUInt16LE(expected.readUInt16LE(FP+18)&~0x3800,FP+18);}
 if(row.empty)expected.writeUInt16LE(0xffff,FP+20);
 if(row.afterX87)expected.set(row.afterX87,FP);
 if(row.afterXmm)expected.set(row.afterXmm,SV+32);
 if(row.pendingX87&&reason===12)expected.writeUInt16LE(0x8081,FP+18);
 expected.writeUInt32LE(row.afterMxcsr??row.mxcsr??0x1f80,SV+16);
 const h=helpers.length,status=c.run(c.base,c.base+56,1,c.base+96);assert.equal(status,0,row.name);
 const events=helpers.slice(h),last=events.at(-1),directRegister=row.direct&&!row.memory&&!row.write&&reason===1;
 if(directRegister||reason===12)assert.equal(events.length,0,row.name+' has no host execution helper');else assert.ok(last,row.name+' helper');
 // Scratch bytes are validated from the ordered canonical calls below; only those spans may differ.
 const actual=arena(c);expected.set(actual.subarray(100,140),100);expected.set(actual.subarray(T,T+512),T);expected.set(actual.subarray(T+512,T+520),T+512);
 assert.deepEqual(actual,expected,row.name+' complete arena');
 if(reason===1&&last){assert.equal(last.packet,packet(row.write?row.writeLength??16:row.masked!==undefined?16:row.direct&&row.memory?row.memory.length:0).toString('hex'));if(!row.direct)assert.equal(actual.readUInt32LE(T+516),row.flags??0xcd7);}
 else if(last){const p=hex(last.packet);assert.equal(p.readUInt32LE(16),row.compiledGp?0:reason===13?5:reason===11?3:1);assert.equal(p.readUInt32LE(24),row.detail??0);}
 if(row.masked!==undefined){assert.equal(events.at(-1).args.at(-1),row.masked);assert.equal(actual.readUInt32LE(T+512),row.masked);}
 if(row.memory&&reason!==11){assert.equal(events[0].name,'cpu_read');assert.deepEqual(actual.subarray(T,T+row.memory.length),row.memory);}
 if(row.writeOnly)host(c,'protect',[0x5000,1,3]);
 if(row.write||row.masked){const wanted=row.write??Buffer.from(fill(0x66).map((v,i)=>row.masked&(1<<i)?row.a[i]:v)),address=row.address??DATA;for(let i=0;i<wanted.length;i++){host(c,'read8',[address+i]);const p=arena(c);assert.equal(p.readUInt32LE(120),wanted[i],row.name+' memory byte'+i);}}
 if(row.unchangedMemory){for(let i=0;i<row.unchangedMemory.length;i++){host(c,'read8',[(row.address??DATA)+i]);assert.equal(arena(c).readUInt32LE(120),row.unchangedMemory[i],row.name+' preserved memory byte'+i);}}
 observations.push({context:c.id,name:row.name,status,retired,reason,cpu:actual.subarray(0,56).toString('hex'),exit:actual.subarray(56,96).toString('hex'),simd:actual.subarray(SV,SV+160).toString('hex'),x87:actual.subarray(FP,FP+128).toString('hex'),helper_calls:events.length});
}
for(const owner of['replacement','resident'])for(const entries of[false,true])for(let start=0;start<cases.length;start+=8){const batch=cases.slice(start,start+8),[c,blocks]=open(owner,entries,batch);for(let i=0;i<batch.length;i++)test(c,blocks[i],batch[i]);writeFileSync(join(output,`arena-${c.id}.bin`),arena(c));host(c,'close',[]);}
// Raw execution payloads are nonforwarded, with a permissive owner guard. They
// independently challenge publication after a well-formed V7 success packet.
const rawCases=[
 {name:'raw-reserved-bit-before-gpr',code:'f30f2cc1',flags:0xcd5},
 {name:'raw-preserved-df-before-memory-write',code:'0f1300',flags:0x8d7},
 {name:'raw-high-reserved-before-gpr',code:'f30f2cc1',flags:0x1cd7},
 {name:'raw-nonzero-helper-value-before-gpr',code:'f30f2cc1',flags:0xcd7,value:1},
 {name:'raw-invalid-compare-relation',code:'0f2ec1',flags:0x443},
 {name:'raw-valid-compare-flags',code:'0f2ec1',flags:0x403,valid:true},
];
for(const owner of['replacement','resident'])for(const entries of[false,true]){
 const[c,blocks]=open(owner,entries,rawCases);
 for(let i=0;i<rawCases.length;i++){
  const row=rawCases[i],block=blocks[i],r=parents();r[0]=DATA;
  input(c,0,initial);input(c,0,state(r,block.pc,0xcd7));input(c,56,exit(1,0));
  input(c,SV+32,dwords([0x3f800000,1,2,3]));input(c,SV+48,dwords([0x40000000,4,5,6]));
  upload(c,DATA,fill(0x66));const expected=arena(c),retired=row.valid?1:0,reason=row.valid?1:7;
  expected.set(state(r,block.pc+(row.valid?hex(row.code).length:0),row.valid?row.flags:0xcd7));expected.set(exit(reason,retired,row.valid?0:2),56);
  const raw=packet(0);raw.writeUInt32LE(row.value??0,20);expected.set(raw,100);expected.set(dwords([0xdeadbeef,row.flags]),T+512);
  const h=helpers.length;c.raw=row;const status=c.run(c.base,c.base+56,1,c.base+96);delete c.raw;
  assert.equal(status,0,row.name);const events=helpers.slice(h);assert.equal(events.length,1,row.name+' no read/write or extra helper');assert.equal(events[0].nonforwarded,true);
  assert.deepEqual(arena(c),expected,row.name+' complete neutral publication');
  const actual=arena(c);for(let offset=0;offset<16;offset++){host(c,'read8',[DATA+offset]);assert.equal(arena(c).readUInt32LE(120),0x66,row.name+' preserved RAM');}
  raw_protocol.push({context:c.id,owner,entries,name:row.name,status,reason,retired,cpu:actual.subarray(0,56).toString('hex'),exit:actual.subarray(56,96).toString('hex'),arena_hex:actual.toString('hex'),helper_calls:events.length});
 }
 writeFileSync(join(output,`arena-${c.id}.bin`),arena(c));host(c,'close',[]);
}
assert.deepEqual(sourceHashes(),beforePins);const result={status:'PASS',engine:{path:enginePath,sha256:engine.sha256,bytes:engine.bytes.length},arena_bytes:SIZE,counts:{contexts:contexts.length,modules:modules.length,runs:observations.length,raw_protocol_runs:raw_protocol.length,helpers:helpers.length},contexts,modules,observations,raw_protocol,helpers,source_sha256:beforePins,scope:'Authored instruction probes under four bound profiles; exact CPU/SIMD/x87 arena except explicitly observed scratch transfer overlay, literal semantic outputs and canonical packet checks. No original compiler/performance claim.'};
writeFileSync(join(output,'result.json'),JSON.stringify(result,null,2));console.log(JSON.stringify({status:'PASS',...result.counts,output}));
