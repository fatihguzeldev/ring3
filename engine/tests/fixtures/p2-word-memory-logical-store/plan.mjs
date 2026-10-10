import assert from 'node:assert/strict';
import {mkdirSync, writeFileSync} from 'node:fs';
import {resolve} from 'node:path';
import {fileURLToPath} from 'node:url';

export const SIZE = 4364, CODE = 0x1000, DATA = 0x4000, TOP = 0xfffff000;
const LOW = [[0, 1, 0x7fff, 0xffff, 0x8000, 0x1234, 0xaaaa, 0x5555],
  [0x7fff, 0xffff, 0, 0x8000, 1, 0xaaaa, 0x5555, 0x1234],
  [0x8000, 0x7fff, 0xffff, 0x7fff, 0x1234, 1, 0x5555, 0xaaaa]];
function record(magic, length, fields = [], version = 1) {
  const bytes = Buffer.alloc(length); bytes.write(magic); bytes.writeUInt32LE(0x10000 + version, 4); bytes.writeUInt32LE(length, 8);
  fields.forEach((n, i) => bytes.writeUInt32LE(n >>> 0, 16 + 4 * i)); return bytes;
}
function word(n) {const b = Buffer.alloc(4); b.writeUInt32LE(n >>> 0); return [...b];}
export function seedHex(pc, flags = 2, pattern = 0, overrides = {}) {
  const registers = LOW[pattern].map((low, i) => (((0xa101 + pattern * 0x111 + i * 0x101) << 16) | low) >>> 0);
  for (const [index, n] of Object.entries(overrides)) registers[Number(index)] = n >>> 0;
  const b = Buffer.alloc(140); record('R3ST', 56, [...registers, pc, flags]).copy(b);
  record('R3EX', 40, [3, 0, 0, 0, 0, 0], 3).copy(b, 56); record('R3MH', 40, [0, 0xdecafbad, 0, 0, 0, 0]).copy(b, 100);
  return b.toString('hex');
}
export function initialHex() {
  const b = Buffer.from(Array.from({length: SIZE}, (_, i) => (37 * i + 19) & 255)); Buffer.from(seedHex(CODE), 'hex').copy(b);
  const fp = record('R3FP', 128);
  for (const [at, n] of [[16, 0x27f], [18, 0x2840], [20, 0xaaaa], [22, 0x345], [32, 0x33], [34, 0x2b]]) fp.writeUInt16LE(n, at);
  fp.writeUInt32LE(0x12345678, 24); fp.writeUInt32LE(0x9abcdef0, 28);
  for (let i = 40; i < 120; i++) fp[i] = (29 * i + 7) & 255; fp.copy(b, 4236); return b.toString('hex');
}
const KINDS = ['and', 'or', 'xor'], OPCODES = [0x21, 0x09, 0x31];
const EDGES = [
  [[0,0xffff],[0xffff,0],[0x8000,0xffff],[0x7fff,0xffff],[0xff,0xff],[0xaaaa,0x5555],[0x1234,0x0f0f],[0xffff,0xffff]],
  [[0,0],[0,1],[0x7fff,0x8000],[0x8000,0],[0xfe,1],[0xaaaa,0x5555],[0x1234,0xf0],[0xffff,0]],
  [[0,0],[0xffff,0xffff],[0x8000,0],[0x7fff,0xffff],[0xff,1],[0xaaaa,0x5555],[0x1234,0xedcb],[0xffff,0]]
];
const ALIAS = DATA + 0x180, CONSUMER = DATA + 0x300;
export function dataHex() {
  const b = Buffer.from(Array.from({length:4096}, (_, i) => (73*i+11)&255));
  EDGES.flat().forEach((row,i)=>b.writeUInt16LE(row[0],2*i));
  for (const [at,n] of [[0x100,0x4100],[0x102,0x0f0f],[0x180,0x8001],[0x200,0x4200],[0x202,0x4202],[0x300,0xffff],[0x302,0x8000]]) b.writeUInt16LE(n,at);
  b.set([0x90,0xeb,0],0xe00); return b.toString('hex');
}
export function bankPlan() {
  const b=Buffer.alloc(4096,0xcc),forms=[],groups=[];let at=0;
  function form(axis,kind,source,tail,fields={}) {
    const raw=[0x66,OPCODES[KINDS.indexOf(kind)],tail[0]|source<<3,...tail.slice(1)];
    const row={id:forms.length,axis,kind,source,pc:CODE+at,length:raw.length,hex:Buffer.from(raw).toString('hex'),overrides:{},...fields};
    forms.push(row);b.set(raw,at);at+=raw.length;return row;
  }
  for(const[type,kind]of KINDS.entries())for(let sample=0;sample<8;sample++)form('edge',kind,sample,[5,...word(DATA+2*(type*8+sample))],{case:sample,memory:EDGES[type][sample][0],right:EDGES[type][sample][1]});
  const wraps=[[3,[0x83,...word(0x5180)],{3:0xfffff000}],[6,[4,0xf5,...word(ALIAS)],{6:0x20000000}],
    [1,[0x84,0x0b,...word(ALIAS)],{3:0x80000000,1:0x80000000}],[0,[0x84,0x30,...word(ALIAS)],{0:0xffffffff,6:1}],
    [5,[0x84,0x3d,...word(ALIAS)],{5:1,7:0xffffffff}],[2,[4,0x95,...word(ALIAS)],{2:0x40000000}],
    [3,[0x43,0xff],{3:ALIAS+1}],[4,[0x44,0x24,0xff],{4:ALIAS+1}]];
  for(const kind of KINDS)for(const[source,tail,overrides]of wraps)form('wrap',kind,source,tail,{overrides});
  b.set([0xeb,0xfe],at);at+=2;groups.push({id:'main',blocks:[[CODE,at]],instructions:49});
  at=0x200;
  for(const kind of KINDS) {
    for(let r=0;r<8;r++)form('alias',kind,r,r===4?[4,0x24]:r===5?[0x45,0]:[r],{alias:'base',overrides:{[r]:ALIAS}});
    for(let r=0;r<8;r++)if(r!==4)form('alias',kind,r,[4,r<<3|5,...word(ALIAS)],{alias:'index',overrides:{[r]:0}});
    form('alias',kind,3,[4,0x1b],{alias:'both',overrides:{3:ALIAS/2}});
  }
  b.set([0xeb,0xfe],at);at+=2;groups.push({id:'alias',blocks:[[CODE+0x200,at-0x200]],instructions:49});
  at=0x600;const faultForms=[];
  for(const kind of KINDS){const prefix=CODE+at;b[at++]=0x90;faultForms.push({...form('fault',kind,0,[3]),prefix});}
  b.set([0xeb,0xfe],at);at+=2;const faultBlock=[CODE+0x600,at-0x600];
  const paths=[],blocks=[faultBlock];
  for(let path=0;path<2;path++) {
    const offset=0x800+path*64,value=path?0xffff:0,opcode=path?0x31:0x21;
    const prefix=[0x66,0xb8,value&255,value>>8,0x66,0xbf,0xff,0xff,0x66,0x83,0xef,1];
    const piece=[0x66,opcode,3,0x66,0x8b,0x0b,0x0f,0x92,0xc3,0x0f,0x90,0xc2,0x0f,0x94,0xc4,path?0x75:0x74,2];
    const tail=[0x66,0x83,0xd7,0,0x0f,0x92,0xc7,0xeb,0];
    b.set(prefix,offset);b.set(piece,offset+12);b.set([0x0f,0x0b],offset+29);b.set(tail,offset+31);
    blocks.push([CODE+offset,29],[CODE+offset+31,9]);
    paths.push({id:path,pc:CODE+offset,arithmetic_pc:CODE+offset+12,end_pc:CODE+offset+40,instructions:12});
  }
  const nextPaths=[];
  for(let path=0;path<2;path++) {
    const offset=0xa00+path*64;
    const raw=[0x66,path?0x21:0x09,3,0x66,0x8b,0x1b,0x66,0x31,0x13,0x0f,0x94,0xc3,0x66,path?0x09:0x21,0x33,0xeb,0];
    b.set(raw,offset);blocks.push([CODE+offset,raw.length]);nextPaths.push({id:path,pc:CODE+offset,end_pc:CODE+offset+raw.length,instructions:6});
  }
  groups.push({id:'control',blocks,instructions:43},{id:'helper',blocks:[[faultForms[0].prefix,4]],instructions:2});
  const smc=[];
  for(const[id,offset,opcode]of[['changed',0xd00,0x31],['equal',0xd40,0x09]]) {
    const raw=[0x66,opcode,5,...word(CODE+offset),0x90,0xeb,0];b.set(raw,offset);
    groups.push({id,blocks:[[CODE+offset,10]],instructions:3},{id:id+'-successor',blocks:[[CODE+offset+7,3]],instructions:2});
    smc.push({id,pc:CODE+offset,next_pc:CODE+offset+7,end_pc:CODE+offset+10});
  }
  b.set([0x90,0xeb,0],0xe00);b.set([0x0f,0x0b],0xf00);
  groups.push({id:'other-code',blocks:[[CODE+0xe00,3]],instructions:2},{id:'unrelated',blocks:[[DATA+0xe00,3]],instructions:2});
  for(const g of groups)assert.ok(g.blocks.length<=8&&g.instructions<=64);
  assert.equal(forms.length,99);
  return {hex:b.toString('hex'),forms,fault_forms:faultForms,consumer:{paths,next_paths:nextPaths},smc,invalid_pc:CODE+0xf00,groups};
}
export function helperCases() {
  return [{id:'read-width',phase:'read',status:0,packet_hex:record('R3MH',40,[0,0x8001,0,0,0,1],2).toString('hex')},
    {id:'store-width',phase:'store',status:0,packet_hex:record('R3MH',40,[0,0,0,0,0,1],4).toString('hex')}];
}
export function makePlan() {
  const bank=bankPlan(),data=dataHex(),groups=new Map(bank.groups.map(g=>[g.id,g])),actions=[],contexts=[];let next=0;
  function context(owner,entries,role='normal') {
    const spec={id:contexts.length+1,owner,entries,role,key:[0x574d0000+contexts.length+1,0x4c53544f],pages:6};contexts.push(spec);let diagnostic=0;
    const add=(kind,fields={})=>actions.push({id:next++,context:spec.id,kind,...fields});
    const input=(offset,hex,label)=>add('input',{offset,hex,label});const host=(name,args,fields={})=>add('host',{name,args,...fields});
    const call=(group,budget,label,retired,fields={})=>add('call',{group,channel:owner==='resident'&&!entries?'dispatch':'direct',budget,label,planned_retired:retired,...fields});
    const seed=(pc,flags=0xcd7,overrides={},pattern=0,label='CPU seed')=>input(0,seedHex(pc,flags,pattern,overrides),label);
    const upload=(address,hex,label)=>{input(140,hex,label);host('upload',[address,hex.length/2]);};
    const diagnose=(address,count,phase)=>add('data',{address,count,file:`${spec.id}-${diagnostic++}-${phase}.bin`,label:count===1024?'complete readable RAM page':'bounded readable watched word'});
    const watch=address=>{diagnose(address-address%4,1,'watch');if(address%4===3)diagnose(address+1,1,'watch-second');};
    const resetWord=(address,n)=>{host('write8',[address,n&255]);host('write8',[address+1,n>>8]);};
    function compile(group,slot,file=group) {
      const g=groups.get(group),request=Buffer.alloc(g.blocks.length*(entries?4:8));
      g.blocks.forEach(([pc,n],i)=>{request.writeUInt32LE(pc,i*(entries?4:8));if(!entries)request.writeUInt32LE(n,i*8+4);});
      input(140,request.toString('hex'),'compiler descriptors');host(owner==='resident'?entries?'compile_resident_entries':'compile_resident':entries?'compile_entries':'compile',entries?[g.blocks.length,0]:[g.blocks.length],{group});
      add('module',{target:'child',group,slot,file:`${spec.id}-${file}.wasm`,helper_case:null});
      if(owner==='resident'){add('table',{group,slot});host('acknowledge_resident_installation',['key_low','key_high','unit_low','unit_high',slot],{group});if(slot===0){host('dispatcher_module',['key_low','key_high']);add('module',{target:'dispatcher',group:'dispatcher',slot:null,file:`${spec.id}-dispatcher.wasm`,helper_case:null});}}
    }
    function formSeed(f,flags,pattern=0) {const overrides={...f.overrides};if(f.axis==='edge')overrides[f.source]=(((0xb101+pattern*0x111+f.source*0x101)<<16)|f.right)>>>0;seed(f.pc,flags,overrides,pattern,'form and source seed');}
    function consumerSeed(path) {resetWord(CONSUMER+2*path.id,path.id?0x8000:0xffff);seed(path.pc,0xcd7,{3:CONSUMER+2*path.id,7:0xa808ffff});}
    function nextSeed(path) {upload(DATA,data,'independent current-address initial data');seed(path.pc,0xcd7,{0:path.id?0xa101fffd:0xa1010002,2:path.id?0xa3034200:0xa3030f0f,3:path.id?0x4202:0x4100,6:path.id?0xa7070001:0xa70700ff});}
    add('open');input(0,initialHex(),'full authored nondefault arena');host('map',[CODE,1,7]);upload(CODE,bank.hex,'complete literal bank');if(role!=='smc')host('protect',[CODE,1,5]);
    host('map',[DATA,1,role==='smc'?7:3]);upload(DATA,data,'literal data page');diagnose(DATA,1024,'initial-data');
    if(role==='normal'){host('map',[TOP,1,3]);upload(TOP,data,'literal top page');diagnose(TOP,1024,'initial-top');}
    if(role==='normal') {
      compile('main',0);
      for(const f of bank.forms.filter(f=>f.axis==='edge'&&(!entries||f.case<2)))for(const flags of[2,0xcd7]){const address=DATA+2*f.id;resetWord(address,f.memory);formSeed(f,flags,flags===2?0:2);call('main',1,entries?'entry form representative':'edge form and FLAGS',1,{form:f.id});watch(address);}
      if(!entries){for(const f of bank.forms.filter(f=>f.axis==='wrap')){formSeed(f,0xcd7);call('main',1,'wrapping effective address',1,{form:f.id});watch(ALIAS);}
        compile('alias',1);for(const f of bank.forms.filter(f=>f.axis==='alias')){formSeed(f,0xcd7);call('alias',1,'legal source address alias',1,{form:f.id});watch(ALIAS);}
        for(const f of bank.forms.filter(f=>f.axis==='alias'&&f.alias==='base'&&f.source===3)){
          seed(f.pc,0xcd7,{3:0xffffffff});call('alias',1,'overflow alias fault',0,{form:f.id});call('alias',1,'overflow alias unchanged retry',0,{form:f.id});
          input(28,Buffer.from(word(CONSUMER)).toString('hex'),'explicit overflow address-register different input');call('alias',1,'overflow alias repair completion',1,{form:f.id});watch(CONSUMER);
        }
      }
      compile('control',entries?1:2);
      if(!entries){
        const shapes=[{id:'unmapped',address:0x8000,map:0x8000},{id:'none',address:DATA,permission:DATA,bits:0},{id:'write-only',address:DATA,permission:DATA,bits:2},
          {id:'read-only',address:DATA,permission:DATA,bits:1},{id:'cross-unmapped',address:DATA+4095,map:DATA+4096},
          {id:'second-byte-read-only',address:DATA+4095,permission:DATA+4096,bits:1},{id:'overflow',address:0xffffffff}];
        for(const shape of shapes){if(shape.id==='second-byte-read-only'){host('map',[DATA+4096,1,3]);upload(DATA+4096,data,'cross-page data');}
          for(const f of bank.fault_forms.filter(f=>f.kind==='and'||['unmapped','read-only','second-byte-read-only','overflow'].includes(shape.id))){
            if(shape.permission!==undefined)host('protect',[shape.permission,1,shape.bits]);seed(f.prefix,0xcd7,{0:0xb1010000|(f.kind==='and'?0:f.kind==='or'?0xffff:0x8001),3:shape.address});
            call('control',20,`${shape.id} prefix fault`,1,{form:f.id});call('control',1,`${shape.id} unchanged retry`,0,{form:f.id});
            if(shape.id==='read-only'||shape.id==='second-byte-read-only')watch(shape.address);
            if(shape.map!==undefined){host('map',[shape.map,1,3]);upload(shape.map,data,'data-only fault repair');}
            else if(shape.permission!==undefined)host('protect',[shape.permission,1,3]);else input(28,Buffer.from(word(CONSUMER)).toString('hex'),'explicit overflow address-register different input');
            call('control',1,`${shape.id} repair completion`,1,{form:f.id});watch(shape.id==='overflow'?CONSUMER:shape.address);
            if(shape.map!==undefined)host('unmap',[shape.map,1]);
          }
          if(shape.id==='second-byte-read-only')host('unmap',[DATA+4096,1]);
        }
        for(const address of[DATA+17,DATA+4094,0xfffffffe])for(const f of bank.fault_forms){seed(f.pc,0xcd7,{3:address});call('control',1,'unaligned or last-two-byte store',1,{form:f.id});watch(address);}
        host('map',[DATA+4096,1,3]);upload(DATA+4096,data,'cross-page success data');for(const f of bank.fault_forms){seed(f.pc,0xcd7,{3:DATA+4095});call('control',1,'successful cross-page store',1,{form:f.id});watch(DATA+4095);}diagnose(DATA+4096,1024,'cross-final');host('unmap',[DATA+4096,1]);
      }
      for(const path of bank.consumer.paths){consumerSeed(path);call('control',13,'continuous consumer',12,{path:path.id});watch(CONSUMER+2*path.id);
        consumerSeed(path);call('control',3,'split consumer',3,{path:path.id});if(path.id===0){input(96,'01000000','set consumer cancel');call('control',5,'consumer cancel continuation',0,{path:path.id});input(96,'00000000','clear consumer cancel');}
        for(const n of[6,3])call('control',n,'split consumer',n,{path:path.id});call('control',1,'consumer no replay',0,{path:path.id});watch(CONSUMER+2*path.id);
      }
      if(!entries){for(const path of bank.consumer.next_paths){nextSeed(path);call('control',7,'current-address continuous',6,{path:path.id});watch(path.id?0x4200:0x4100);watch(path.id?0x4202:0x4102);
        nextSeed(path);for(const n of[1,2,3])call('control',n,'current-address split',n,{path:path.id});call('control',1,'current-address no replay',0,{path:path.id});watch(path.id?0x4200:0x4100);watch(path.id?0x4202:0x4102);}
        const path=bank.consumer.paths[0];seed(path.pc,0xcd7,{3:0x8000});call('control',20,'one-seed repair initial fault',3);call('control',1,'one-seed repair unchanged retry',0);
        host('map',[0x8000,1,3]);upload(0x8000,data,'declared one-seed repair upload');host('protect',[0x8000,1,1]);call('control',1,'one-seed repair read-success write fault',0);watch(0x8000);
        host('protect',[0x8000,1,3]);call('control',1,'one-seed repair store',1);call('control',9,'one-seed repair consumers',8);call('control',1,'one-seed repair no replay',0);diagnose(0x8000,1024,'repair-final');host('unmap',[0x8000,1]);
      }
      const path=bank.consumer.paths[0];seed(path.pc,0xcd7,{3:CONSUMER});call('control',0,'zero budget',0);input(96,'01000000','set cancel');call('control',20,'cancel positive budget',0);call('control',0,'cancel zero budget',0);input(96,'00000000','clear cancel');
      const request=Buffer.alloc(entries?8:16);request.writeUInt32LE(path.pc);if(!entries)request.writeUInt32LE(29,4);request.writeUInt32LE(bank.invalid_pc,entries?4:8);if(!entries)request.writeUInt32LE(2,12);
      input(140,request.toString('hex'),'late admitted store then UD2');host(owner==='resident'?entries?'compile_resident_entries':'compile_resident':entries?'compile_entries':'compile',entries?[2,0]:[2],{group:'late-failure',allowed_failure:true});
      seed(path.pc,0xcd7,{3:CONSUMER});call('control',1,'prior owner survival',1);add('guard',{group:'control',wrong_key:true});
    }else if(role==='helper'){
      const request=Buffer.alloc(8);request.writeUInt32LE(bank.fault_forms[0].prefix);request.writeUInt32LE(4,4);input(140,request.toString('hex'),'inert helper module descriptors');host('compile',[1],{group:'helper'});
      for(const h of helperCases()){add('module',{target:'child',group:'helper',slot:null,file:`${spec.id}-helper-${h.id}.wasm`,helper_case:h.id});seed(bank.fault_forms[0].prefix,0xcd7,{0:0xb1010001,3:DATA});call('helper',20,'malformed inert helper '+h.id,1,{channel:'direct',helper_case:h.id});}
    }else{
      if(owner==='resident'){compile('unrelated',0);compile('other-code',1);compile('changed',2);}else compile('changed',0);
      seed(bank.smc[0].pc,0xcd7,{0:0xb1010001});call('changed',10,'changed own-code store',1);
      call('changed',1,'stale changed direct',0,{channel:'direct'});
      compile('changed-successor',owner==='resident'?3:0);call('changed-successor',3,'fresh successor without replay',2);
      if(owner==='resident'){call('other-code',1,'other unit stale direct',0,{channel:'direct'});seed(DATA+0xe00);call('unrelated',2,'unrelated installed survivor',2);}
      compile('equal',owner==='resident'?4:0);seed(bank.smc[1].pc,0xcd7,{0:0xb1010000});call('equal',10,'same-value own-code store',1);call('equal',0,'same-value stale zero direct',0,{channel:'direct'});
      input(96,'01000000','set stale cancel');call('equal',0,'same-value stale cancel direct',0,{channel:'direct'});input(96,'00000000','clear stale cancel');
      if(owner==='resident'){call('equal',1,'same-value stale dispatcher',0,{channel:'dispatch'});call('equal',0,'stale dispatcher zero budget',0,{channel:'dispatch'});compile('equal-successor',5);call('equal-successor',3,'equal fresh successor without replay',2);}
    }
    diagnose(DATA,1024,'final-data');if(role==='normal')diagnose(TOP,1024,'final-top');add('close');add('closed_call',{group:role==='normal'?'control':role==='helper'?'helper':owner==='resident'?'equal-successor':'equal',channel:'direct',budget:1,label:'cached closed function',planned_retired:0});
  }
  for(const owner of['replacement','resident'])for(const entries of[false,true])context(owner,entries);
  context('replacement',false,'helper');context('replacement',false,'smc');context('resident',false,'smc');
  const count=kind=>actions.filter(a=>a.kind===kind).length,calls=actions.filter(a=>['call','closed_call'].includes(a.kind)),diagnostics=actions.filter(a=>a.kind==='data');
  const frames=actions.reduce((n,a)=>n+(['open','close'].includes(a.kind)?1:a.kind==='closed_call'?0:2),0);
  const counts={contexts:contexts.length,actions:actions.length,generated_calls:calls.length,planned_retired:calls.reduce((n,a)=>n+a.planned_retired,0),modules:count('module'),frames,raw_bytes:frames*SIZE,data_files:diagnostics.length,diagnostic_reads:diagnostics.reduce((n,a)=>n+a.count,0),inputs:count('input'),guards:count('guard'),tables:count('table'),files:10+count('module')+diagnostics.length,checkpoints:calls.length+count('guard')+count('close')+diagnostics.length+1};
  assert.ok(counts.generated_calls<=600&&counts.modules<=24&&counts.frames<=5000);
  return {schema_version:1,size:SIZE,code:CODE,data_address:DATA,top:TOP,data_hex:data,bank,helper_cases:helperCases(),contexts,actions,counts};
}
if(process.argv[1]&&resolve(process.argv[1])===fileURLToPath(import.meta.url)){const output=resolve(process.argv[2]);mkdirSync(output);const plan=makePlan();writeFileSync(resolve(output,'plan.json'),JSON.stringify(plan,null,2)+'\n',{flag:'wx'});console.log(JSON.stringify(plan.counts));}
