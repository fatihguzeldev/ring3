import assert from 'node:assert/strict';
import {readFileSync,writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {createHash} from 'node:crypto';
import {readEngine} from '../../../../../../support/wasm/engine.mjs';

const [enginePath,output,root]=process.argv.slice(2),engine=readEngine(enginePath);
const initial=readFileSync(join(output,'initial-arena.bin')),SIZE=initial.length,FP=4236;
const hash=b=>createHash('sha256').update(b).digest('hex'),contexts=[],modules=[],observations=[],helpers=[];
const sourcePaths=['engine/src/cpu/x86/x87.rs','engine/src/cpu/x86/x87/value.rs', 'engine/src/math/mod.rs', 'engine/src/math/extended/mod.rs', 'engine/src/math/extended/value.rs', 'engine/src/math/integer.rs', 'engine/src/lib.rs','engine/src/cpu/x86/x87/execute.rs','engine/src/cpu/x86/x87/transcendental.rs', 'engine/src/math/extended/transcendental.rs', 'engine/src/cpu/x86/decode/x87.rs','engine/src/process/cpu/x87.rs', 'engine/src/process/cpu/mod.rs', 'engine/src/process/mod.rs','engine/src/cpu/dbt/wasm/x87.rs','engine/src/cpu/dbt/wasm/cpu_result.rs','engine/src/cpu/x86/ir.rs','engine/src/cpu/x86/decode/profile.rs','engine/src/cpu/x86/decode/lower.rs','engine/src/cpu/dbt/wasm/emitter.rs','engine/src/cpu/dbt/wasm/memory.rs','engine/src/cpu/dbt/wasm/integer/arithmetic.rs', 'engine/src/cpu/dbt/wasm/integer/bits.rs', 'engine/src/cpu/dbt/wasm/integer/carry_rotates.rs', 'engine/src/cpu/dbt/wasm/integer/decimal.rs', 'engine/src/cpu/dbt/wasm/integer/dispatch.rs', 'engine/src/cpu/dbt/wasm/integer/flags.rs', 'engine/src/cpu/dbt/wasm/integer/mod.rs', 'engine/src/cpu/dbt/wasm/integer/multiply_divide.rs', 'engine/src/cpu/dbt/wasm/integer/operands.rs', 'engine/src/cpu/dbt/wasm/integer/rotates.rs', 'engine/src/cpu/dbt/wasm/integer/shifts.rs','engine/src/abi/cpu_helper.rs','engine/src/abi/arena.rs','engine/tests/cpu/x87/numeric/wasm/extended.rs', 'engine/tests/cpu/x87/wasm.rs', 'engine/tests/support/wasm.rs', 'engine/tests/cpu/x87/numeric/fixtures/p2-x87-extended/runtime/run.mjs'];
sourcePaths.push('engine/tests/cpu/x87/stack/compatibility/native.rs');
assert.equal(new Set(sourcePaths).size,sourcePaths.length);
const pins=()=>Object.fromEntries(sourcePaths.map(p=>[p,hash(readFileSync(join(root,p)))])),beforePins=pins();
function record(magic,size,fields,version=1){const b=Buffer.alloc(size);b.write(magic);b.writeUInt16LE(version,4);b.writeUInt16LE(1,6);b.writeUInt32LE(size,8);fields.forEach((v,i)=>b.writeUInt32LE(v>>>0,16+i*4));return b;}
function ext(exponent,significand,negative=false){const b=Buffer.alloc(10);b.writeBigUInt64LE(BigInt(significand));b.writeUInt16LE(exponent|(negative?0x8000:0),8);return b;}
const ZERO=ext(0,0),ONE=ext(0x3fff,0x8000000000000000n),ONE_PLUS=ext(0x3fff,0x8000000000000001n),TWO_PLUS=ext(0x4000,0x8000000000000001n),PI_QUARTER=ext(0x3ffe,0xc90fdaa22168c235n);
function empty(){return{cw:0x37f,sw:0,tag:0xffff,opcode:0,ip:0,dp:0,cs:0,ds:0,regs:Array.from({length:8},(_,i)=>ext(0x3fff,0x8000000000000100n+BigInt(i)))};}
function clone(f){return{...f,regs:f.regs.map(b=>Buffer.from(b))};}
function fp(f){const b=record('R3FP',128,[]);for(const[o,v]of[[16,f.cw],[18,f.sw],[20,f.tag],[22,f.opcode]])b.writeUInt16LE(v,o);b.writeUInt32LE(f.ip,24);b.writeUInt32LE(f.dp,28);b.writeUInt16LE(f.cs,32);b.writeUInt16LE(f.ds,34);f.regs.forEach((v,i)=>b.set(v,40+10*i));return b;}
function stack(values){const f=empty();f.sw=0;for(let i=0;i<values.length;i++){f.regs[i]=values[i];f.tag=(f.tag&~(3<<(i*2)))|((values[i].equals(ZERO)?1:0)<<(i*2));}return f;}
const state=s=>record('R3ST',56,[...s.r,s.pc,s.flags]);
const arena=c=>Buffer.from(new Uint8Array(c.memory.buffer,c.base,SIZE));
function input(c,offset,b){new Uint8Array(c.memory.buffer).set(b,c.base+offset);}
function host(c,name,args=[]){assert.equal(c.api[name](...args),0,name);}
function upload(c,address,b){input(c,140,b);host(c,'upload',[address,b.length]);}
function cpu(pc,flags=0xcd7){return{r:[0x12345678,0x23456789,0x3456789a,0x456789ab,0x7040,0x56789abc,0x6789abcd,0x789abcde],pc,flags};}
function seed(c,s,f){input(c,0,state(s));input(c,FP,fp(f));input(c,96,Buffer.alloc(4));}
const absolute=(opcode,address)=>{const bytes=Buffer.alloc(4);bytes.writeUInt32LE(address);return[...opcode,...bytes];};
const blocks=[
 [0x1000,[...absolute([0xdb,0x2d],0x5010),0xd8,0xc0,...absolute([0xdb,0x3d],0x5020)]],
 ...[[0xf0,'F2XM1'],[0xf1,'FYL2X'],[0xf2,'FPTAN'],[0xf3,'FPATAN'],[0xf9,'FYL2XP1'],[0xfb,'FSINCOS'],[0xfe,'FSIN'],[0xff,'FCOS']].map(([op,label],i)=>[0x1200+i*0x20,[0xd9,op],label]),
 [0x1400,absolute([0xdb,0x3d],0x5ffc)],
 [0x1500,absolute([0xdb,0x2d],0x6000)],
 [0x1600,absolute([0xdb,0x3d],0x1600)],
 [0x1700,[...absolute([0xdb,0x2d],0x5030),...absolute([0xdb,0x0d],0x5040)]],
 [0x1800,[...absolute([0xd9,0x2d],0x5060),0xd9,0xfa,0xbb,0x55,0,0,0,0xdf,0xe0,0x9b]],
 [0x1900,[0xdf,0xe9]],
 [0x1a00,[...absolute([0xdd,0x35],0x5070),...absolute([0xdd,0x25],0x5070)]],
].map(([pc,body,label])=>({pc,bytes:Buffer.from([...body,0xeb,0xfe]),label}));
assert.equal(blocks.length,16);
function open(owner,entries,captured,segmented=false){
 const instance=new WebAssembly.Instance(engine.module,{}),c={id:contexts.length+1,owner,entries,memory:instance.exports.memory,api:{}};
 for(const name of ['open','close','arena_ptr','map','protect','upload','read8','compile','compile_entries','compile_resident','compile_resident_entries','module_ptr','module_len','guard','guard_resident','x87_op','x87_resident_op','set_segment_base','configure_segment_descriptor','segment_load','segment_load_resident','simd_execute','simd_execute_resident','cpu_fxsave','cpu_fxsave_resident'])c.api[name]=instance.exports['ring3_abi_v1_'+name];
 host(c,'open',[3,c.id,0x583837]);c.base=c.api.arena_ptr()>>>0;input(c,0,initial);
 assert.ok(captured.length<=8,'capture remains within the existing eight-block limit');
 host(c,'map',[0x1000,1,7]);host(c,'map',[0x5000,1,3]);if(segmented){host(c,'map',[0x7000,1,3]);host(c,'set_segment_base',[4,0x5000]);host(c,'configure_segment_descriptor',[0x43,0x7000,3,1,1]);}for(const b of captured)upload(c,b.pc,b.bytes);
 const descriptors=Buffer.alloc(captured.length*(entries?4:8));captured.forEach((b,i)=>{const offset=i*(entries?4:8);descriptors.writeUInt32LE(b.pc,offset);if(!entries)descriptors.writeUInt32LE(b.bytes.length,offset+4);});input(c,140,descriptors);
 host(c,owner==='resident'?(entries?'compile_resident_entries':'compile_resident'):(entries?'compile_entries':'compile'),entries?[captured.length,0]:[captured.length]);
 const a=arena(c),pointer=owner==='resident'?a.readUInt32LE(156):c.api.module_ptr()>>>0,length=owner==='resident'?a.readUInt32LE(160):c.api.module_len()>>>0;
 c.binding=owner==='resident'?[c.id,0x583837,a.readUInt32LE(148),a.readUInt32LE(152)]:[];if(segmented)host(c,owner==='resident'?'segment_load_resident':'segment_load',[...c.binding,5,0x43]);
 const bytes=Buffer.from(new Uint8Array(c.memory.buffer,pointer,length)),module=new WebAssembly.Module(bytes),ring3={...c.api};
 for(const imp of WebAssembly.Module.imports(module).filter(x=>x.module==='ring3'&&(x.name.includes('x87')||x.name.startsWith('simd_execute')||x.name.startsWith('cpu_fxsave')))){ring3[imp.name]=(...args)=>{
  const before=arena(c),status=c.api[imp.name](...args),after=arena(c),allowed=Buffer.from(before);allowed.set(after.subarray(100,140),100);allowed.set(after.subarray(FP,FP+128),FP);if(imp.name.startsWith('simd_execute'))allowed.set(after.subarray(140,660),140);assert.deepEqual(after,allowed,'FP helper changes only declared FP/packet/transfer regions');
  helpers.push({context:c.id,name:imp.name,args:args.map(x=>x>>>0),status,packet:after.subarray(100,140).toString('hex')});return status;
 };}
 c.run=new WebAssembly.Instance(module,{env:{memory:c.memory},ring3}).exports.run;
 const file=`module-${c.id}.wasm`;writeFileSync(join(output,file),bytes);modules.push({context:c.id,file,bytes:bytes.length,sha256:hash(bytes),imports:WebAssembly.Module.imports(module)});contexts.push({id:c.id,owner,entries});return c;
}
function run(c,label,s,f,{reason=1,retired=1,fault=[0,0,0,0],packet=[0,s.flags,0,0,0,0],helperReturn=0,helperCount=1,transfer,emptyStore=false}={}){
 const before=arena(c),at=helpers.length,status=c.run(c.base,c.base+56,1,c.base+96),after=arena(c);
 assert.equal(status,0,label+' run status');
 const expected=Buffer.from(before);expected.set(state(s));expected.set(record('R3EX',40,[reason,retired,...fault],6),56);expected.set(fp(f),FP);expected.set(record('R3MH',40,packet,7),100);if(transfer)expected.set(transfer,140);
 if(emptyStore){
  // only the empty source raw value and destination raw value/tag lack a literal oracle.
  // compare every other defined field directly; never copy observed bytes into expected.
  const {source,destination}=emptyStore;
  assert.ok(Number.isInteger(source)&&source>=0&&source<8,label+' source physical index');
  assert.ok(Number.isInteger(destination)&&destination>=0&&destination<8,label+' destination physical index');
  assert.deepEqual(after.subarray(0,FP),expected.subarray(0,FP),label+' non-FP prefix');
  assert.deepEqual(after.subarray(FP+128),expected.subarray(FP+128),label+' non-FP tail');
  for(const [start,end]of [[0,18],[22,40],[120,128]]){
   assert.deepEqual(after.subarray(FP+start,FP+end),expected.subarray(FP+start,FP+end),label+' defined FP fields');
  }
  assert.equal(after.readUInt16LE(FP+18)&0xb8ff,f.sw&0xb8ff,label+' TOP and defined exception status');
  const tagMask=0xffff^(3<<(destination*2));
  assert.equal(after.readUInt16LE(FP+20)&tagMask,f.tag&tagMask,label+' unaffected physical tags');
  for(let physical=0;physical<8;physical++)if(physical!==source&&physical!==destination){
   const start=FP+40+physical*10;
   assert.deepEqual(after.subarray(start,start+10),expected.subarray(start,start+10),label+' unaffected physical raw80 '+physical);
  }
 }else assert.deepEqual(after,expected,label+' full arena');
 assert.equal(helpers.length-at,helperCount,label+' helper count');if(helperCount!==0)assert.equal(helpers.at(-1).status,helperReturn);
 const file=`arena-${observations.length+1}.bin`;writeFileSync(join(output,file),after);observations.push({context:c.id,label,status,reason,retired,cpu:s,fp:after.subarray(FP,FP+128).toString('hex'),arena:{file,bytes:after.length,sha256:hash(after)}});
 if(emptyStore)observations.at(-1).fp_oracle={defined_status_mask:0xb8ff,source:emptyStore.source,destination:emptyStore.destination,defined_tag_mask:0xffff^(3<<(emptyStore.destination*2)),unchecked:['source raw80','destination raw80/tag','condition bits C0/C1/C2/C3']};
}
function pointer(f,pc,opcode,address,selector=0x23,base=0){const n=clone(f);n.ip=pc;n.opcode=opcode;n.cs=0x1b;if(address!==undefined){n.dp=(address-base)>>>0;n.ds=selector;}return n;}
function read(c,address,length){const b=Buffer.alloc(length);for(let i=0;i<length;i++){host(c,'read8',[address+i]);b[i]=arena(c).readUInt32LE(120);}return b;}
for(const owner of ['replacement','resident'])for(const entries of [false,true]){
 let c=open(owner,entries,blocks.slice(0,8));upload(c,0x5010,ONE_PLUS);let s=cpu(0x1000),f=empty();seed(c,s,f);
 host(c,'protect',[0x5000,1,0]);run(c,'FLD80 read fault is FP neutral',s,f,{reason:5,retired:0,fault:[2,0x5010,1,10],packet:[1,0,2,0x5010,1,10]});host(c,'protect',[0x5000,1,3]);
 f=pointer(f,0x1000,0x32d,0x5010);f.sw=0x3800;f.tag=0x3fff;f.regs[7]=ONE_PLUS;s={...s,pc:0x1006};run(c,'FLD80 retains binary64-invisible bit',s,f,{packet:[0,s.flags,0,0,0,10]});
 f=pointer(f,0x1006,0x0c0);f.regs[7]=TWO_PLUS;s={...s,pc:0x1008};run(c,'FADD80 retains full significand',s,f);
 f=pointer(f,0x1008,0x33d,0x5020);f.sw=0;f.tag=0xffff;s={...s,pc:0x100e};run(c,'FSTP80 exact store and pop',s,f,{packet:[0,s.flags,0,0,0,10]});assert.deepEqual(read(c,0x5020,10),TWO_PLUS);
 // each numerical input is declared; the load/add/store sequence above is live and unreseeded.
 for(const [i,values,result,push,exceptions]of [
  [0,[ONE],ONE,false,0],[1,[ext(0x4002,0x8000000000000000n),ext(0x4000,0xc000000000000000n)],ext(0x4002,0x9000000000000000n),false,0],
  [2,[ZERO],ZERO,true,0],[3,[ONE,ONE],PI_QUARTER,false,0x220],[4,[ZERO,ext(0x4000,0xc000000000000000n)],ZERO,false,0],
  [5,[ZERO],ZERO,true,0],[6,[ZERO],ZERO,false,0],[7,[ZERO],ONE,false,0],
 ]){
  if(i===7){host(c,'close');c=open(owner,entries,[blocks[8],blocks[7],...blocks.slice(9).filter(b=>b.pc!==0x1600)]);}
  const pc=0x1200+i*0x20,op=blocks[1+i].bytes[1];s=cpu(pc);f=stack(values);seed(c,s,f);let expected=pointer(f,pc,0x100|op);
  if([1,3,4].includes(i)){expected.regs[1]=result;expected.tag|=3;expected.tag=(expected.tag&~(3<<2))|((result.equals(ZERO)?1:0)<<2);expected.sw=0x800|exceptions;}
  else{expected.regs[0]=result;expected.tag=(expected.tag&~3)|(result.equals(ZERO)?1:0);expected.sw=exceptions;}
  if(push){expected.regs[7]=ONE;expected.tag=(expected.tag&~(3<<14));expected.sw=0x3800|exceptions;}
  s={...s,pc:pc+2};run(c,blocks[1+i].label,s,expected);
 }
 // out-of-range trigonometric values keep the original80 operand and set C2.
 s=cpu(0x12c0);f=stack([ext(0x403e,0x8000000000000000n)]);seed(c,s,f);let expected=pointer(f,s.pc,0x1fe);expected.sw|=0x400;s={...s,pc:s.pc+2};run(c,'FSIN architectural C2 range',s,expected);
 // full ten-byte write preflight must precede pop and preserve the current live input on failure.
 s=cpu(0x1400);f=stack([ONE_PLUS]);seed(c,s,f);run(c,'FSTP80 split-page fault retains stack',s,f,{reason:5,retired:0,fault:[1,0x6000,2,10],packet:[1,0,1,0x6000,2,10]});host(c,'map',[0x6000,1,3]);
 expected=pointer(f,s.pc,0x33d,0x5ffc);expected.sw=0x800;expected.tag=0xffff;s={...s,pc:s.pc+6};run(c,'FSTP80 mapping-only retry pops once',s,expected,{packet:[0,s.flags,0,0,0,10]});assert.deepEqual(read(c,0x5ffc,10),ONE_PLUS);
 // pending unmasked exception precedes even a denied memory operand.
 host(c,'protect',[0x6000,1,0]);s=cpu(0x1500);f=stack([ONE]);f.cw=0x37e;f.sw=1;seed(c,s,f);expected=clone(f);expected.sw=0x8081;
 run(c,'waiting x87 exception before memory',s,expected,{reason:12,retired:0,fault:[0x8081,0,0,0],packet:[4,0,0x8081,0,0,10]});
 // FISTTP ignores RC and converts the live value only after the complete store is writable.
 upload(c,0x5030,ext(0x3fff,0xc000000000000000n));s=cpu(0x1700);f=empty();seed(c,s,f);expected=pointer(f,s.pc,0x32d,0x5030);expected.sw=0x3800;expected.tag=0x3fff;expected.regs[7]=ext(0x3fff,0xc000000000000000n);s={...s,pc:0x1706};run(c,'FISTTP input FLD',s,expected,{packet:[0,s.flags,0,0,0,10]});
 f=pointer(expected,s.pc,0x30d,0x5040);f.sw=32;f.tag=0xffff;s={...s,pc:0x170c};run(c,'FISTTP32 truncation',s,f,{packet:[0,s.flags,0,0,0,4]});assert.deepEqual(read(c,0x5040,4),Buffer.from([1,0,0,0]));
 // FLDCW can unmask sticky exceptions without trapping until the following waiting operation.
 upload(c,0x5060,Buffer.from([0x7e,0x03]));s=cpu(0x1800);f=stack([ONE]);f.sw=1;seed(c,s,f);expected=clone(f);expected.cw=0x37e;expected.sw=0x8081;s={...s,pc:0x1806};run(c,'FLDCW deferred pending exception',s,expected,{packet:[0,s.flags,0,0,0,2]});
 // a new unmasked invalid result is latched; integer and FN instructions retire before FWAIT delivers #MF.
 s=cpu(0x1806);f=stack([ext(0x3fff,0x8000000000000000n,true)]);f.cw=0x37e;seed(c,s,f);expected=pointer(f,s.pc,0x1fa);expected.sw=0x8081;s={...s,pc:0x1808};run(c,'FSQRT unmasked invalid defers delivery and suppresses destination',s,expected);
 s={...s,r:s.r.map((v,i)=>i===3?0x55:v),pc:0x180d};run(c,'integer MOV retires while x87 exception is pending',s,expected,{helperCount:0});
 s={...s,r:s.r.map((v,i)=>i===0?((v&0xffff0000)|0x8081)>>>0:v),pc:0x180f};run(c,'FNSTSW AX observes pending exception without delivering it',s,expected,{helperCount:0});
 run(c,'FWAIT delivers deferred exception before retiring',s,expected,{reason:12,retired:0,fault:[0x8081,0,0,0],packet:[4,0,0x8081,0,0,0]});
 s=cpu(0x1900);f=stack([ONE,ext(0x4000,0x8000000000000000n)]);seed(c,s,f);expected=pointer(f,s.pc,0x7e9);expected.sw=0x800;expected.tag|=3;s={...s,pc:0x1902,flags:0x403};run(c,'FUCOMIP publishes integer flags and pops once',s,expected,{packet:[0,s.flags,0,0,0,0]});
 // FSAVE/FRSTOR preserve the physical80 values across the legacy108-byte layout.
 s=cpu(0x1a00);f=stack([ONE_PLUS,ONE]);f.sw|=0x400;seed(c,s,f);expected=empty();expected.regs=f.regs.map(b=>Buffer.from(b));s={...s,pc:0x1a06};run(c,'FNSAVE108 resets control while preserving raw registers',s,expected,{packet:[0,s.flags,0,0,0,108]});
 const environment=Buffer.alloc(108);environment.fill(0xff,0,28);environment.writeUInt16LE(f.cw,0);environment.writeUInt16LE(f.sw,4);environment.writeUInt16LE(f.tag,8);environment.writeUInt32LE(f.ip,12);environment.writeUInt16LE(f.cs,16);environment.writeUInt16LE(f.opcode,18);environment.writeUInt32LE(f.dp,20);environment.writeUInt16LE(f.ds,24);environment.writeUInt16LE(0,26);for(let i=0;i<8;i++)environment.set(f.regs[i],28+i*10);assert.deepEqual(read(c,0x5070,108),environment);
 s={...s,pc:0x1a0c};run(c,'FRSTOR108 restores stack and control',s,f,{packet:[0,s.flags,0,0,0,108]});
 // the executable write commits and retires before the consumed module becomes stale.
 host(c,'close');c=open(owner,entries,[blocks[11]]);
 s=cpu(0x1600);f=stack([ONE_PLUS]);seed(c,s,f);expected=pointer(f,s.pc,0x33d,0x1600);expected.sw=0x800;expected.tag=0xffff;s={...s,pc:0x1606};run(c,'FSTP80 consumed-code commit',s,expected,{reason:6,packet:[0,s.flags,0,0,0,10],helperReturn:11});
 const before=arena(c);assert.notEqual(c.run(c.base,c.base+56,1,c.base+96),0);assert.deepEqual(arena(c),before,'stale x87 refusal is arena neutral');host(c,'close');
}
// separate captures exercise selected FS/GS offsets without increasing any finite compile limit.
const segmentBlocks=[
 {pc:0x1b00,bytes:Buffer.from([...absolute([0x64,0xdb,0x2d],0x14),...absolute([0xd9,0x35],0x5200),...absolute([0x0f,0xae,0x05],0x5500),0xeb,0xfe])},
 {pc:0x1b20,bytes:Buffer.from([...absolute([0x65,0xdb,0x2d],0x28),...absolute([0xd9,0x35],0x5200),...absolute([0x0f,0xae,0x05],0x5500),0xeb,0xfe])},
];
function savedEnvironment(f){const b=Buffer.alloc(28,0xff);for(const[o,v]of[[0,f.cw],[4,f.sw],[8,f.tag],[16,f.cs],[18,f.opcode],[24,f.ds],[26,0]])b.writeUInt16LE(v,o);b.writeUInt32LE(f.ip,12);b.writeUInt32LE(f.dp,20);return b;}
function savedExtended(c,f){const a=arena(c),b=Buffer.from(a.subarray(140,652));b.fill(0,0,464);b.writeUInt16LE(f.cw,0);b.writeUInt16LE(f.sw,2);b[4]=Array.from({length:8},(_,i)=>((f.tag>>>(2*i))&3)!==3?1<<i:0).reduce((a,b)=>a|b,0);b.writeUInt16LE(f.opcode,6);b.writeUInt32LE(f.ip,8);b.writeUInt16LE(f.cs,12);b.writeUInt32LE(f.dp,16);b.writeUInt16LE(f.ds,20);b.writeUInt32LE(a.readUInt32LE(4380),24);b.writeUInt32LE(0xffff,28);const top=(f.sw>>>11)&7;for(let i=0;i<8;i++)b.set(f.regs[(top+i)&7],32+16*i);b.set(a.subarray(4396,4524),160);return b;}
for(const owner of ['replacement','resident'])for(const entries of [false,true]){
 const c=open(owner,entries,segmentBlocks,true);upload(c,0x5014,ONE_PLUS);upload(c,0x7028,ONE);
 for(const [i,address,selector,base,value]of[[0,0x5014,0x3b,0x5000,ONE_PLUS],[1,0x7028,0x43,0x7000,ONE]]){
  let s=cpu(segmentBlocks[i].pc),f=empty();seed(c,s,f);f=pointer(f,s.pc,0x32d,address,selector,base);f.sw=0x3800;f.tag=0x3fff;f.regs[7]=value;s={...s,pc:s.pc+7};run(c,`${i===0?'FS':'GS'} FLD80 captures selected offset/selector`,s,f,{packet:[0,s.flags,0,0,0,10]});
  s={...s,pc:s.pc+6};run(c,'FSTENV retains prior selected segment pointer',s,f,{packet:[0,s.flags,0,0,0,28]});assert.deepEqual(read(c,0x5200,28),savedEnvironment(f));
  const image=savedExtended(c,f),transfer=Buffer.concat([image,Buffer.from([0,0,0,0]),Buffer.alloc(4)]);transfer.writeUInt32LE(s.flags,516);s={...s,pc:s.pc+7};run(c,'FXSAVE stores current CS and selected data offset/selector',s,f,{packet:[0,0,0,0,0,512],helperCount:2,transfer});assert.deepEqual(read(c,0x5500,464),image.subarray(0,464));assert.deepEqual(read(c,0x56d0,48),Buffer.alloc(48));
 }
 host(c,'close');
}

// register-only compatibility cases reuse this runner's existing transport and finite captures.
const registerStoreBlocks = [
  ...Array.from({ length: 8 }, (_, index) => ({
    pc: 0x1c00 + index * 8,
    index,
    ordinary: false,
    bytes: Buffer.from([0xd9, 0xd8 + index, 0xeb, 0xfe]),
  })),
  { pc: 0x1c40, index: 1, ordinary: true, bytes: Buffer.from([0xdd, 0xd9, 0xeb, 0xfe]) },
];
function registerStoreInput() {
  const f = stack(
    Array.from({ length: 8 }, (_, index) => ext(0x3fff, 0x8000000000000100n + BigInt(index))),
  );
  f.sw = 0x3800;
  f.ip = 0xf1234567;
  f.opcode = 0x7ff;
  f.dp = 0xfedcba98;
  f.cs = 0xf135;
  f.ds = 0xe246;
  return f;
}
for (const owner of ['replacement', 'resident'])
  for (const entries of [false, true]) {
    for (const captured of [registerStoreBlocks.slice(0, 8), registerStoreBlocks.slice(8)]) {
      const c = open(owner, entries, captured);
      assert.equal(SIZE, 4756, 'current complete arena includes YMM upper state');
      for (const offset of [4396, 4628])
        input(
          c,
          offset,
          Buffer.from(Array.from({ length: 128 }, (_, index) => (index * 29 + offset) & 255)),
        );
      for (const block of captured) {
        const name = block.ordinary ? 'FSTP' : 'FSTPNCE',
          opcode = (block.ordinary ? 0x500 : 0x100) | (0xd8 + block.index);
        let s = cpu(block.pc),
          f = registerStoreInput();
        seed(c, s, f);
        const expected = pointer(f, s.pc, opcode),
          destination = (7 + block.index) & 7;
        expected.regs[destination] = Buffer.from(f.regs[7]);
        expected.tag = 0xc000;
        expected.sw = 0;
        s = { ...s, pc: s.pc + 2 };
        run(c, `${name} ST(${block.index}) physical rawcopy and TOP7 wrap`, s, expected);
        if (!block.ordinary && block.index === 3) {
          for (const cw of [0x037f, 0x037e]) {
            s = cpu(block.pc);
            f = registerStoreInput();
            f.tag = 0xc000;
            f.cw = cw;
            seed(c, s, f);
            const next = pointer(f, s.pc, opcode);
            next.sw = 0;
            s = { ...s, pc: s.pc + 2 };
            run(c, `FSTPNCE empty CW${cw.toString(16)} no new IE/SF/MF and one pop`, s, next, {
              emptyStore: { destination, source: 7 },
            });
          }
        }
        if (!block.ordinary && block.index === 7) {
          s = cpu(block.pc);
          f = registerStoreInput();
          f.cw = 0x037e;
          f.sw |= 1;
          seed(c, s, f);
          const next = clone(f);
          next.sw |= 0x8080;
          run(c, 'FSTPNCE pending MF precedes register copy and pop', s, next, {
            reason: 12,
            retired: 0,
            fault: [0xb881, 0, 0, 0],
            packet: [4, 0, 0xb881, 0, 0, 0],
          });
        }
        if (block.ordinary) {
          s = cpu(block.pc);
          f = registerStoreInput();
          f.tag = 0xc000;
          f.cw = 0x037f;
          seed(c, s, f);
          const next = pointer(f, s.pc, opcode);
          next.sw = 0x41;
          s = { ...s, pc: s.pc + 2 };
          run(c, 'ordinary FSTP empty masked source still raises IE/SF and pops', s, next, {
            emptyStore: { destination, source: 7 },
          });
        }
      }
      host(c, 'close');
    }
  }
assert.deepEqual(pins(),beforePins);
writeFileSync(join(output,'result.json'),JSON.stringify({engine:{path:enginePath,sha256:engine.sha256,bytes:engine.bytes.length},source_pins:beforePins,arena_bytes:SIZE,contexts,modules,observations,helper_trace:helpers,claim:'Actual generated Wasm x87 genuine extended80 live arithmetic/load/store, all8 transcendental families under declared mathematical80 policy, selected conversions/control/comparison/environment with current CS and FS/GS offset/selector FSTENV+FXSAVE, transactional fullspan fault repair, deferred x87 exception delivery/pending priority and consumed-code writes. No Intel-specific transcendental approximation bitidentity claim; selected RAM only.'},null,2)+'\n');
console.log(JSON.stringify({output,contexts:contexts.length,modules:modules.length,runs:observations.length,helpers:helpers.length}));
