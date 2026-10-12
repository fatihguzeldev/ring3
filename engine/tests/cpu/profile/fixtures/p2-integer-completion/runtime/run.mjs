import assert from 'node:assert/strict';
import {readFileSync,writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {createHash} from 'node:crypto';
import {readEngine} from '../../../../../support/wasm/engine.mjs';

const [enginePath,output,root,mode='all']=process.argv.slice(2),engine=readEngine(enginePath);
assert.ok(['all','prefix-boundaries'].includes(mode),'known integer fixture mode');
const initial=readFileSync(join(output,'initial-arena.bin')),SIZE=initial.length,T=140,DATA=0x5010;
const hash=b=>createHash('sha256').update(b).digest('hex'),hex=s=>Buffer.from(s,'hex');
const contexts=[],modules=[],observations=[],helpers=[],diagnostics=[];
const sourcePaths=['engine/src/cpu/x86/atomic.rs','engine/src/cpu/x86/decode/atomic.rs',
 'engine/src/cpu/x86/decode/flow.rs','engine/src/cpu/x86/decode/lower.rs','engine/src/cpu/x86/decode/profile.rs',
 'engine/src/cpu/x86/ir.rs','engine/src/cpu/dbt/region.rs','engine/src/cpu/dbt/wasm/atomic.rs','engine/src/cpu/dbt/wasm/integer_system.rs',
 'engine/src/cpu/dbt/wasm/integer/arithmetic.rs', 'engine/src/cpu/dbt/wasm/integer/bits.rs', 'engine/src/cpu/dbt/wasm/integer/carry_rotates.rs', 'engine/src/cpu/dbt/wasm/integer/decimal.rs', 'engine/src/cpu/dbt/wasm/integer/dispatch.rs', 'engine/src/cpu/dbt/wasm/integer/flags.rs', 'engine/src/cpu/dbt/wasm/integer/mod.rs', 'engine/src/cpu/dbt/wasm/integer/multiply_divide.rs', 'engine/src/cpu/dbt/wasm/integer/operands.rs', 'engine/src/cpu/dbt/wasm/integer/rotates.rs', 'engine/src/cpu/dbt/wasm/integer/shifts.rs','engine/src/process/cpu/integer.rs', 'engine/src/process/cpu/mod.rs', 'engine/src/process/mod.rs','engine/src/abi/cpu_helper.rs',
 'engine/tests/cpu/profile/cases/native/cpu_integer_completion.rs', 'engine/tests/cpu/profile/native.rs', 'engine/tests/cpu/profile/native.rs','engine/tests/cpu/control/cases/native/cpu_prefixed_moffs_near_control.rs', 'engine/tests/cpu/control/native.rs', 'engine/tests/cpu/control/native.rs','engine/tests/cpu/atomic/cases/native/cpu_lock_exchange_prefixes.rs', 'engine/tests/cpu/atomic/native.rs', 'engine/tests/cpu/atomic/native.rs','engine/tests/cpu/profile/cases/wasm/cpu_integer_completion_wasm.rs', 'engine/tests/cpu/profile/wasm.rs', 'engine/tests/support/wasm.rs', 'engine/tests/cpu/profile/wasm.rs', 'engine/tests/support/wasm.rs',
 'engine/tests/cpu/profile/fixtures/p2-integer-completion/runtime/run.mjs'];
const pins=()=>Object.fromEntries(sourcePaths.map(p=>[p,hash(readFileSync(join(root,p)))])),beforePins=pins();
function record(magic,size,fields,version=1){const b=Buffer.alloc(size);b.write(magic);b.writeUInt16LE(version,4);b.writeUInt16LE(1,6);b.writeUInt32LE(size,8);fields.forEach((v,i)=>b.writeUInt32LE(v>>>0,16+i*4));return b;}
const state=s=>record('R3ST',56,[...s.r,s.pc,s.flags]);
const exit=(reason,retired,detail=0,address=0,access=0,length=0)=>record('R3EX',40,[reason,retired,detail,address,access,length],6);
const packet=(width,status=0,detail=0,address=0,access=0)=>record('R3MH',40,[status,0,detail,address,access,width],7);
const dwords=values=>{const b=Buffer.alloc(values.length*4);values.forEach((v,i)=>b.writeUInt32LE(v>>>0,4*i));return b;};
const bytes=(value,width)=>{const b=Buffer.alloc(width);let n=BigInt(value);for(let i=0;i<width;i++){b[i]=Number(n&255n);n>>=8n;}return b;};
const arena=c=>Buffer.from(new Uint8Array(c.memory.buffer,c.base,SIZE));
function input(c,offset,b){new Uint8Array(c.memory.buffer).set(b,c.base+offset);}
function host(c,name,args=[]){assert.equal(c.api[name](...args),0,name);}
function upload(c,address,b){input(c,T,b);host(c,'upload',[address,b.length]);}
function cpu(pc,flags=0xcd7){return{r:[0xa1b2c3d4,DATA,0x3456789a,0x456789ab,0x7080,0x56789abc,0x6789abcd,0x789abcde],pc,flags};}
const parity=n=>{let count=0;for(let b=n&255;b;b>>>=1)count+=b&1;return count%2===0;};
function arithmetic(kind,width,old,source,flags){
 const bits=BigInt(width*8),modulus=1n<<bits,mask=modulus-1n,sign=modulus>>1n;
 const a=BigInt(old)&mask,b=BigInt(source)&mask,c=BigInt(flags&1);
 if(kind==='not')return{value:a^mask,flags};
 if(['bts','btr','btc'].includes(kind)){const bit=1n<<(b&(bits-1n));return{value:kind==='bts'?a|bit:kind==='btr'?a&~bit:a^bit,flags:(flags&~1)|Number((a&bit)!==0n)};}
 let result,cf=0,of=false,af=false;
 if(['and','or','xor'].includes(kind))result=kind==='and'?a&b:kind==='or'?a|b:a^b;
 else{
  const subtract=['sub','sbb','dec','neg'].includes(kind),left=kind==='neg'?0n:a;
  const right=kind==='neg'?a:['inc','dec'].includes(kind)?1n:b;
  const carry=['adc','sbb'].includes(kind)?c:0n;
  const full=subtract?left-right-carry:left+right+carry;result=full&mask;
  cf=Number(subtract?full<0n:full>=modulus);
  if(['inc','dec'].includes(kind))cf=Number(c);
  // signed arithmetic is evaluated without a host-width overflow shortcut.
  const signed=x=>(x&sign)!==0n?x-modulus:x;
  const signedFull=subtract?signed(left)-signed(right)-carry:signed(left)+signed(right)+carry;
  of=signedFull < -sign || signedFull >= sign;
  af=((left^right^result)&16n)!==0n;
 }
 return{value:result,flags:((flags&~0x8d5)|2|cf|(parity(Number(result&255n))?4:0)|(result===0n?64:0)|((result&sign)!==0n?128:0)|(af?16:0)|(of?2048:0))>>>0};
}
const rows=[];
const kinds=[['add',0,0x00],['adc',1,0x10],['sub',2,0x28],['sbb',3,0x18]];
for(const width of[1,2,4]){
 const prefix=width===2?'66':'',mask=(1n<<BigInt(width*8))-1n,sign=1n<<BigInt(width*8-1);
 for(const[kind,id,opcode]of kinds)rows.push({name:`${kind}${width*8}-carry-boundary`,code:prefix+'f0'+(opcode+(width===1?0:1)).toString(16).padStart(2,'0')+'01',kind,id,width,
  old:['add','adc'].includes(kind)?mask:0n,source:['adc'].includes(kind)?0:['sbb'].includes(kind)?Number(mask):1});
 for(const[kind,id,opcode,group]of[['inc',7,0xfe,0],['dec',8,0xfe,1],['neg',9,0xf6,3],['not',10,0xf6,2]])rows.push({name:`${kind}${width*8}-signed-or-identity`,code:prefix+'f0'+(opcode+(width===1?0:1)).toString(16)+(1+(group<<3)).toString(16).padStart(2,'0'),kind,id,width,
  old:kind==='inc'?sign-1n:kind==='dec'?sign:kind==='neg'?sign:0x55n,source:0});
}
for(const width of[1,2])for(const[kind,id,opcode]of[['and',4,0x20],['or',5,0x08],['xor',6,0x30]])rows.push({name:`${kind}${width*8}-full-width`,code:(width===2?'66':'')+'f0'+(opcode+(width===1?0:1)).toString(16).padStart(2,'0')+'01',kind,id,width,old:width===1?0xa5n:0xa55an,source:width===1?0xf0:0xff00});
for(const[kind,id,opcode]of[['bts',11,'ab'],['btr',12,'b3'],['btc',13,'bb']])rows.push({name:`${kind}16-negative-register-index`,code:'66f00f'+opcode+'01',kind,id,width:2,old:0x8001n,source:0xffff,selected:DATA-2});
rows.push({name:'btc32-negative-register-index',code:'f00fbb01',kind:'btc',id:13,width:4,old:0x80000001n,source:0xffffffff,selected:DATA-4});
for(const width of[2,4])for(const[kind,id,group]of[['bts',11,5],['btr',12,6],['btc',13,7]])rows.push({name:`${kind}${width*8}-immediate-index-masked`,code:(width===2?'66':'')+'f00fba'+(1+(group<<3)).toString(16)+'ff',kind,id,width,old:0x8001n,source:255,immediate:true});
rows.push({name:'add8-original-AH-source',code:'f00021',kind:'add',id:0,width:1,old:0x7fn,source:0x81,highByte:true});
rows.push({name:'add16-CX-is-EA-parent',code:'66f00109',kind:'add',id:0,width:2,old:0xfff0n,source:DATA,aliasSource:true});
// These legal prefix forms reuse the existing atomic old-value helpers, not cpu_integer.
rows.push(
 {name:'explicit-LOCK-XCHG8-original-AH',code:'f08621',exchange:'xchg',width:1,old:0x7fn,highByte:true},
 {name:'explicit-LOCK-XCHG16-F0-before-66',code:'f0668701',exchange:'xchg',width:2,old:0xbeefn},
 {name:'explicit-LOCK-XCHG32',code:'f08701',exchange:'xchg',width:4,old:0x12345678n},
 {name:'LOCK-CMPXCHG16-F0-before-66-match',code:'f0660fb111',exchange:'cas',width:2,old:0xc3d4n},
 {name:'LOCK-XADD16-F0-before-66-wrap',code:'f0660fc101',exchange:'xadd',width:2,old:0xbeefn},
);
for(const locked of[false,true])for(const matches of[false,true])rows.push({name:`cmpxchg8b-${locked?'locked':'ordinary'}-${matches?'match':'mismatch'}`,code:(locked?'f0':'')+'0fc709',cas:true,width:8,old:0x1122334455667788n,matches});
for(const leaf of[0,1,0x80000001])rows.push({name:`cpuid-leaf-${leaf.toString(16)}`,code:'0fa2',cpuid:leaf});
rows.push({name:'rdtsc-two-live-executions',code:'0f310f31',timestamp:true});
for(const[code,base,offset,oldEbx,oldAl]of[['6467d7',0x5000,0x10,0xfffffff0,0x20],['6567d7',0x7000,0x10,0xfffffff0,0x20],['64d7',0x5000,0x30,0x10,0x20],['67d7',0,0x5010,0x15000,0x10]])rows.push({name:`xlat-${code}-wrap-before-segment`,code,xlat:true,address:base+offset,oldEbx,oldAl});
function open(owner,entries,blocks){
 assert.ok(blocks.length<=8,'capture remains within the existing eight-block limit');
 const instance=new WebAssembly.Instance(engine.module,{}),c={id:contexts.length+1,owner,entries,memory:instance.exports.memory,api:{},tsc:0};
 for(const[name,fn]of Object.entries(instance.exports))if(name.startsWith('ring3_abi_v1_')&&typeof fn==='function')c.api[name.slice(13)]=fn;
 host(c,'open',[6,c.id,0x494e5447]);c.base=c.api.arena_ptr()>>>0;input(c,0,initial);
 for(const page of[0x1000,0x5000,0x7000])host(c,'map',[page,1,7]);
 host(c,'set_segment_base',[4,0x5000]);host(c,'set_segment_base',[5,0x7000]);
 host(c,'configure_segment_descriptor',[0x43,0x7000,3,1,1]);
 for(const b of blocks)upload(c,b.pc,b.bytes);
 const descriptors=Buffer.alloc(blocks.length*(entries?4:8));blocks.forEach((b,i)=>{const at=i*(entries?4:8);descriptors.writeUInt32LE(b.pc,at);if(!entries)descriptors.writeUInt32LE(b.bytes.length,at+4);});input(c,T,descriptors);
 host(c,owner==='resident'?(entries?'compile_resident_entries':'compile_resident'):(entries?'compile_entries':'compile'),entries?[blocks.length,0]:[blocks.length]);
 const a=arena(c),pointer=owner==='resident'?a.readUInt32LE(156):c.api.module_ptr()>>>0,length=owner==='resident'?a.readUInt32LE(160):c.api.module_len()>>>0;
 c.binding=owner==='resident'?[c.id,0x494e5447,a.readUInt32LE(148),a.readUInt32LE(152)]:[];
 host(c,owner==='resident'?'segment_load_resident':'segment_load',[...c.binding,5,0x43]);
 const bytes=Buffer.from(new Uint8Array(c.memory.buffer,pointer,length)),module=new WebAssembly.Module(bytes),ring3={...c.api};
 for(const im of WebAssembly.Module.imports(module).filter(x=>x.module==='ring3'&&!x.name.startsWith('guard'))){ring3[im.name]=(...args)=>{
  const before=arena(c),status=c.api[im.name](...args),after=arena(c);
  if(im.name.startsWith('cpu_integer')){const allowed=Buffer.from(before);allowed.set(after.subarray(100,140),100);allowed.set(after.subarray(T,T+24),T);assert.deepEqual(after,allowed,'integer helper writes only packet and 24-byte payload');}
  helpers.push({context:c.id,name:im.name,args:args.map(v=>v>>>0),status,input:before.subarray(T,T+24).toString('hex'),packet:after.subarray(100,140).toString('hex')});return status;
 };}
 c.run=new WebAssembly.Instance(module,{env:{memory:c.memory},ring3}).exports.run;
 const file=`module-${c.id}.wasm`;writeFileSync(join(output,file),bytes);modules.push({context:c.id,file,sha256:hash(bytes),bytes:length,imports:WebAssembly.Module.imports(module)});contexts.push({context:c.id,owner,entries});return c;
}
function execute(c,label,s,{next=s,reason=1,retired=1,width,source,descriptor,cas=false,timestamp=false,fault,helperReturn=0,helperCount=width===undefined?0:1,budget=1,cancel=0,readByte,legacyPacket,seed=true}={}){
 if(seed)input(c,0,state(s));else assert.deepEqual(arena(c).subarray(0,56),state(s),label+' resumes the observed CPU without upload');input(c,96,dwords([cancel]));const before=arena(c),start=helpers.length;
 const expected=Buffer.from(before);expected.set(state(next));expected.set(exit(reason,retired,...(fault??[0,0,0,0])),56);
 if(width!==undefined&&helperCount!==0){
  const payload=[source??before.readUInt32LE(T),s.flags,s.r[0],s.r[2],s.r[3],s.r[1]];
  if(!fault){payload[1]=next.flags;if(cas||timestamp){payload[2]=next.r[0];payload[3]=next.r[2];}}
  expected.set(dwords(payload),T);expected.set(packet(width,fault?1:0,...(fault?[fault[0],fault[1],fault[2]]:[])),100);
 }
 if(readByte!==undefined)expected.set(record('R3MH',40,[0,readByte,0,0,0,1],2),100);
 if(legacyPacket)expected.set(legacyPacket,100);
 const status=c.run(c.base,c.base+56,budget,c.base+96),after=arena(c);assert.equal(status,0,label+' status');assert.deepEqual(after,expected,label+' entire arena');
 const calls=helpers.slice(start);assert.equal(calls.length,helperCount,label+' helper count');
 for(const call of calls){assert.equal(call.status,helperReturn,label+' helper return');if(descriptor!==undefined)assert.equal(call.args.at(-2),descriptor,label+' descriptor');}
 const file=`arena-${observations.length+1}.bin`;writeFileSync(join(output,file),after);observations.push({context:c.id,label,status,reason,retired,cpu:next,helpers:calls.length,arena:{file,sha256:hash(after),bytes:after.length}});
}
function read(c,address,width){const actual=Buffer.alloc(width);for(let i=0;i<width;i++){host(c,'read8',[address+i]);const p=arena(c);actual[i]=p.readUInt32LE(120);diagnostics.push({context:c.id,address:address+i,value:actual[i]});}return actual;}
function caseRun(c,block,row){
 const s=cpu(block.pc),next={...s,r:[...s.r],pc:block.pc+hex(row.code).length};
 if(row.kind){
  if(row.aliasSource)s.r[1]=DATA;else if(row.highByte)s.r[0]=0xa1b281d4;else if(!row.immediate)s.r[0]=row.source>>>0;
  next.r=[...s.r];const result=arithmetic(row.kind,row.width,row.old,row.source,s.flags),selected=row.selected??DATA;
  upload(c,selected,bytes(row.old,row.width));next.flags=result.flags;
  execute(c,row.name,s,{next,width:row.width,source:row.source,descriptor:row.id|(row.width<<8)});
  assert.deepEqual(read(c,selected,row.width),bytes(result.value,row.width),row.name+' memory');
 }else if(row.exchange){
  const width=row.width,mask=(1n<<BigInt(width*8))-1n;
  const source=row.highByte?((s.r[0]>>>8)&255):(s.r[0]>>>0);
  let stored=BigInt(source)&mask;
  if(row.exchange==='cas'){
   stored=BigInt(s.r[2])&mask;next.flags=0x446; // expected AX equals the declared old word.
  }else{
   const low=Number(row.old);
   next.r[0]=row.highByte?((s.r[0]&0xffff00ff)|(low<<8))>>>0:
    width===2?((s.r[0]&0xffff0000)|low)>>>0:low;
   if(row.exchange==='xadd'){
    const result=arithmetic('add',width,row.old,source,s.flags);stored=result.value;next.flags=result.flags;
   }
  }
  upload(c,DATA,bytes(row.old,width));
  const version=width===1?5:width===2?6:1,length=width===4?0:width;
  execute(c,row.name,s,{next,helperCount:1,legacyPacket:record('R3MH',40,[0,Number(row.old),0,0,0,length],version)});
  assert.deepEqual(read(c,DATA,width),bytes(stored,width),row.name+' memory');
 }else if(row.cas){
  s.r[0]=row.matches?0x55667788:0;s.r[2]=row.matches?0x11223344:0;s.r[3]=0xdeadbeef;s.r[1]=DATA;
  next.r=[...s.r];next.flags=(s.flags&~64)|(row.matches?64:0);if(!row.matches){next.r[0]=0x55667788;next.r[2]=0x11223344;}
  upload(c,DATA,bytes(row.old,8));execute(c,row.name,s,{next,width:8,descriptor:0x10000,cas:true});
  assert.deepEqual(read(c,DATA,8),bytes(row.matches?0x00005010deadbeefn:row.old,8));
 }else if(row.cpuid!==undefined){
  s.r[0]=row.cpuid;next.r=[...s.r];for(const i of[0,1,2,3])next.r[i]=0;
  if(row.cpuid===0){next.r[0]=7;next.r[3]=0x676e6952;next.r[2]=0x676e4533;next.r[1]=0x20656e69;}
  if(row.cpuid===1){next.r[0]=0x600;next.r[1]=0x00d80201;next.r[2]=0x07888111;next.r[3]=0x800;}
  if(row.cpuid===0x80000001){next.r[1]=0x20;next.r[2]=0x08000000;}
  execute(c,row.name,s,{next});
 }else if(row.timestamp){
  execute(c,'RDTSC budget-zero has no event',s,{next:s,width:0,helperCount:0,retired:0,budget:0});
  execute(c,'RDTSC cancelled has no event',s,{next:s,width:0,helperCount:0,retired:0,reason:2,cancel:1});
  next.r=[...s.r];next.r[0]=++c.tsc;next.r[2]=0;next.pc=block.pc+2;
  execute(c,'RDTSC first live event',s,{next,width:0,descriptor:0x20000,timestamp:true});
  const second={...next,r:[...next.r],pc:block.pc+4};second.r[0]=++c.tsc;
  execute(c,'RDTSC second live event',next,{next:second,width:0,descriptor:0x20000,timestamp:true});
 }else if(row.xlat){
  s.r[0]=(s.r[0]&0xffffff00)|row.oldAl;s.r[3]=row.oldEbx;next.r=[...s.r];next.r[0]=((s.r[0]&0xffffff00)|0xa7)>>>0;
  upload(c,row.address,Buffer.from([0xa7]));execute(c,row.name,s,{next,helperCount:1,readByte:0xa7});
  if(row.code==='6467d7'||row.code==='6567d7'){
   const segment=row.code.startsWith('64')?4:5;
   host(c,c.owner==='resident'?'segment_load_resident':'segment_load',[...c.binding,segment,0]);
   const denied={...s,r:[...s.r]};denied.r[3]=0x3000;denied.r[0]&=0xffffff00;
   execute(c,`null ${segment===4?'FS':'GS'} XLAT raises GP before unmapped memory`,denied,{reason:11,retired:0});
   host(c,c.owner==='resident'?'segment_load_resident':'segment_load',[...c.binding,segment,segment===4?0x3b:0x43]);
  }
 }
}
function prefixBoundaryCases(owner,entries){
 // address16 selects a two-byte absolute offset; FS/GS bases apply after that offset.
 const forms=[
  {name:'WORD address16 moffs load consumes disp16',pc:0x1000,code:'6667a11050',address:0x5010,load:true},
  {name:'WORD address16 moffs store consumes disp16',pc:0x1020,code:'6667a31050',address:0x5010,load:false},
  {name:'FS WORD address16 moffs load adds cached base',pc:0x1040,code:'646667a11000',address:0x5010,load:true},
  {name:'GS WORD address16 moffs store adds cached base',pc:0x1060,code:'656667a31200',address:0x7012,load:false},
 ];
 const m=open(owner,entries,[...forms.map(row=>({pc:row.pc,bytes:Buffer.concat([hex(row.code),hex('ebfe')])})),{pc:0x1f00,bytes:hex('0f31ebfe')}]);
 for(const row of forms){
  const s=cpu(row.pc),next={...s,r:[...s.r],pc:row.pc+hex(row.code).length};
  const original=Buffer.from(row.load?[0x11,0xef,0xbe,0x22]:[0x55,0x66,0x77,0x88]);upload(m,row.address-1,original);
  if(row.load)next.r[0]=((s.r[0]&0xffff0000)|0xbeef)>>>0;
  execute(m,row.name,s,{next,helperCount:1,legacyPacket:record('R3MH',40,[0,row.load?0xbeef:0,0,0,0,2],row.load?2:4)});
  const call=helpers.at(-1);assert.equal(call.name,row.load?'read16':owner==='resident'?'store_resident16':'store16');
  assert.deepEqual(call.args.slice(row.load?-1:-2),row.load?[row.address]:[row.address,0xc3d4],row.name+' exact selected address/source');
  const expected=Buffer.from(original);if(!row.load)expected.set([0xd4,0xc3],1);
  assert.deepEqual(read(m,row.address-1,4),expected,row.name+' word plus adjacent canaries');
 }host(m,'close');
 // segment and address prefixes are ignored for direct near control; SS.B32 still selects ESP.
 const pairs=[
  {name:'DWORD',pc:0x1000,target:0x1100,call:'6467e8f9000000',ret:'6567c20400',width:4,adjust:4},
  {name:'WORD',pc:0x1200,target:0x1300,call:'646766e8fa00',ret:'656766c20600',width:2,adjust:6},
 ];
 const blocks=pairs.flatMap(row=>[{pc:row.pc,bytes:hex(row.call)},{pc:row.target,bytes:hex(row.ret)},
  {pc:row.pc+hex(row.call).length,bytes:hex('ebfe')}]);blocks.push({pc:0x1f00,bytes:hex('0f31ebfe')});
 const c=open(owner,entries,blocks);
 for(const segment of[4,5])host(c,owner==='resident'?'segment_load_resident':'segment_load',[...c.binding,segment,0]);
 assert.equal(arena(c).readUInt16LE(4524+24),0);assert.equal(arena(c).readUInt16LE(4524+26),0);
 assert.equal(arena(c).readUInt32LE(4524+48),0x5000);assert.equal(arena(c).readUInt32LE(4524+52),0x7000);
 for(const row of pairs){
  const s=cpu(row.pc),returnPc=row.pc+hex(row.call).length,stack=0x7078,original=Buffer.from(Array.from({length:16},(_,i)=>0x90+i));
  upload(c,stack,original);const afterCall={...s,r:[...s.r],pc:row.target};afterCall.r[4]-=row.width;
  execute(c,`${row.name} prefixed direct CALL ignores null FS/base and address16`,s,{next:afterCall,helperCount:1,
   legacyPacket:record('R3MH',40,[0,0,0,0,0,row.width===2?2:0],row.width===2?4:1)});
  let call=helpers.at(-1);assert.equal(call.name,row.width===2?(owner==='resident'?'store_resident16':'store16'):(owner==='resident'?'store_resident32':'store32'));
  assert.deepEqual(call.args.slice(-2),[afterCall.r[4],returnPc],'CALL writes the full consumed return PC at the SS stack address');
  const pushed=Buffer.from(original);pushed.set(bytes(returnPc,row.width),afterCall.r[4]-stack);
  assert.deepEqual(read(c,stack,16),pushed,'CALL changes exactly the operand-width return slot');
  const afterReturn={...afterCall,r:[...afterCall.r],pc:returnPc};afterReturn.r[4]=s.r[4]+row.adjust;
  execute(c,`${row.name} prefixed near RET resumes live stack and ignores null GS/base`,afterCall,{next:afterReturn,seed:false,helperCount:1,
   legacyPacket:record('R3MH',40,[0,returnPc,0,0,0,row.width===2?2:0],row.width===2?2:1)});
  call=helpers.at(-1);assert.equal(call.name,row.width===2?'read16':'read32');assert.deepEqual(call.args,[afterCall.r[4]],'RET reads the exact live SS stack slot');
  assert.deepEqual(read(c,stack,16),pushed,'RET preserves the return slot and adjacent stack bytes');
 }host(c,'close');
}
for(const owner of['replacement','resident'])for(const entries of[false,true]){
 prefixBoundaryCases(owner,entries);
 if(mode==='prefix-boundaries')continue;
 for(let start=0;start<rows.length;start+=7){
  const batch=rows.slice(start,start+7),blocks=batch.map((row,i)=>({pc:0x1000+i*32,bytes:Buffer.concat([hex(row.code),hex('ebfe')])}));
  // a dormant timestamp keeps the extended exit ABI uniform in pure system/XLAT batches.
  blocks.push({pc:0x1f00,bytes:hex('0f31ebfe')});const c=open(owner,entries,blocks);
  for(let i=0;i<batch.length;i++)caseRun(c,blocks[i],batch[i]);host(c,'close');
 }
 // split read precedes any write check; protect/map-only repairs reuse live source and CPU.
 const c=open(owner,entries,[{pc:0x1000,bytes:hex('f00101ebfe')}]);let s=cpu(0x1000);s.r[0]=1;s.r[1]=0x5ffe;
 upload(c,0x5ffe,Buffer.from([0xff,0xff]));host(c,'protect',[0x5000,1,1]);
 execute(c,'split ADD read fault precedes earlier read-only byte',s,{width:4,source:1,descriptor:0x400,reason:5,retired:0,fault:[1,0x6000,1,4]});
 host(c,'map',[0x6000,1,3]);execute(c,'split ADD mapping repair exposes write permission',s,{width:4,source:1,descriptor:0x400,reason:5,retired:0,fault:[2,0x5ffe,2,4]});
 host(c,'protect',[0x5000,1,3]);let result=arithmetic('add',4,0xffffn,1,s.flags),next={...s,r:[...s.r],pc:0x1003,flags:result.flags};
 execute(c,'split ADD permission-only retry commits once',s,{next,width:4,source:1,descriptor:0x400});assert.deepEqual(read(c,0x5ffe,4),bytes(0x10000n,4));host(c,'close');
 // all control targets are captured, so a one-instruction budget observes the selected PC.
 const branches=[['loop',0xe2,0xabcd0002,0xcd7,true],['loope',0xe1,0xabcd0002,0xcd7,true],['loopne',0xe0,0xabcd0002,0xc97,true],['jcxz',0xe3,0xabcd0000,0xcd7,true]];
 for(let start=0;start<branches.length;start+=2){
  const group=branches.slice(start,start+2),bs=group.flatMap(([,op],i)=>{const pc=0x1000+i*32;return[{pc,bytes:Buffer.from([0x67,op,2])},{pc:pc+3,bytes:hex('ebfe')},{pc:pc+5,bytes:hex('ebfe')}];});bs.push({pc:0x1f00,bytes:hex('0f31ebfe')});const b=open(owner,entries,bs);
  for(let i=0;i<group.length;i++)for(const taken of[true,false]){
   const[name,,count,flags]=group[i],s=cpu(0x1000+i*32,flags);s.r[1]=taken?count:0xabcd0001;
   const next={...s,r:[...s.r],pc:s.pc+(taken?5:3)};if(name!=='jcxz')next.r[1]=(s.r[1]&0xffff0000)|((s.r[1]-1)&65535);
   execute(b,`${name} CX ${taken?'taken':'fallthrough'} preserves ECX high16 and FLAGS`,s,{next});
  }host(b,'close');
 }
 // even identity transactions invalidate a consumed executable owner after retirement.
 const m=open(owner,entries,[{pc:0x1000,bytes:hex('f0830100ebfe')}]);s=cpu(0x1000);s.r[1]=0x1000;const old=0x000183f0n;
 result=arithmetic('add',4,old,0,s.flags);next={...s,r:[...s.r],pc:0x1004,flags:result.flags};
 execute(m,'identity LOCK ADD executable commit retires before stale owner',s,{next,width:4,source:0,descriptor:0x400,reason:6,helperReturn:11});
 const before=arena(m),at=helpers.length;assert.notEqual(m.run(m.base,m.base+56,1,m.base+96),0);assert.deepEqual(arena(m),before,'stale owner is entirely neutral');assert.equal(helpers.length,at);host(m,'close');
}
assert.deepEqual(pins(),beforePins);
const result={status:'PASS',mode,engine:{path:enginePath,sha256:engine.sha256,bytes:engine.bytes.length},arena_bytes:SIZE,source_pins:beforePins,
 counts:{contexts:contexts.length,modules:modules.length,runs:observations.length,retired:observations.reduce((n,r)=>n+r.retired,0),helpers:helpers.length,diagnostics:diagnostics.length},contexts,modules,observations,helpers,diagnostics,
 claim:'Actual generated Wasm under four bound profiles: independently calculated width arithmetic/FLAGS, bit-string selected EA, aliases, explicit LOCK XCHG8/16/32 and F0-before-66 WORD CAS/XADD, CMPXCHG8B match/mismatch, software CPUID, virtual RDTSC, CX control flow, segmented/address16 XLAT and WORD moffs, prefixed near CALL/RET with live SS stack continuation, live split-fault repair and mandatory identity executable-write currentness. Entire dynamic arena compared on every recorded run; selected RAM only. No hardware concurrency, physical timestamp, or complete ISA claim.'};
writeFileSync(join(output,'result.json'),JSON.stringify(result,null,2)+'\n');console.log(JSON.stringify({status:'PASS',...result.counts,output}));
