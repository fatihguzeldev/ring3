import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {harness, state, exit, registers, setSegments} from './support.mjs';
const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root, 'explicit selected engine, fresh output and source root');
const literalHash = () => createHash('sha256').update(readFileSync(new URL(import.meta.url))).digest('hex');
const beforeLiteralHash = literalHash();
const h = await harness(enginePath, output, root);
const context = {bases:[0,0xffffe000,0,0,0,0]};
function success(input, pc, value) {const want=exit(Buffer.from(input),1,1);want.writeUInt32LE(pc,48);want.writeUInt32LE(2,52);if(value!==undefined)want.writeUInt32LE(value,16);return want;}
for (const resident of [false,true]) for (const entries of [false,true]) {
  for (const half of ['high','low']) {
    const c=h.open(resident,entries,`split MOV physical ${half} identity`,[[0],[0x1000],[0xfffff000]]);
    h.upload(c,0xfffffffd,Buffer.from('b87856','hex'));h.upload(c,0,Buffer.from('3412ebfe','hex'));
    const input=state(h.initial,{...context,pc:0x1ffd});h.put(c,0,input);
    const material=h.compile(c,'split instruction and terminal block snapshot',[[0x1ffd,7]]);
    if(material){
      const want=success(input,0x2002,0x12345678);
      h.observe(c,'wrapped MOV reads literal immediate and keeps logical EIP',{input,want});
      if(resident)h.lookup(c,'wrapped resident lookup finds matching physical material',input,0x1ffd,material);
      h.upload(c,0x1ffd,Buffer.from([0xb8]));h.observe(c,'logical decoy write keeps wrapped capture current',{input,want});
      h.upload(c,half==='high'?0xfffffffd:0,Buffer.from([half==='high'?0xb8:0x34]));
      h.observe(c,'equal write to consumed split half invalidates held source',{input,want:input,status:4,kind:'physical-currentness'});
      if(resident)h.lookup(c,'stale wrapped resident partition reports invalidated source',input,0x1ffd,null,4);
    }else h.skip(c,'wrapped MOV semantics and currentness','split instruction capture');
    h.host(c,'close');
  }
  for(const half of ['high','low']){
    const c=h.open(resident,entries,`split gate physical ${half} identity`,[[0],[0x1000],[0xfffff000]]);
    h.upload(c,0xffffffff,Buffer.from([0x0f]));h.upload(c,0,Buffer.from([0x0b]));
    const input=state(h.initial,{...context,pc:0x1fff});h.put(c,0,input);
    const material=h.compile(c,'split UD2 gate capture',[[0x1fff,2]],[],{gates:[[0x1fff,77]]});
    if(material){
      const want=exit(Buffer.from(input),8,0,3);want.writeUInt32LE(77,80);
      h.observe(c,'split gate is a zero-retirement RF-preserving boundary',{input,want});
      h.upload(c,half==='high'?0xffffffff:0,Buffer.from([half==='high'?0x0f:0x0b]));
      h.observe(c,'equal write to gate split half invalidates held gate',{input,want:input,status:4,kind:'physical-currentness'});
    }else h.skip(c,'split gate boundary semantics','split gate capture');
    h.host(c,'close');
  }
  for(const consumed of [0,2,3]){
    const c=h.open(resident,entries,`runtime CS fault consumed prefix ${consumed}`,[[0],[0x3000],[0xfffff000]]);
    h.upload(c,0xfffff000,Buffer.from('e9f90f0000','hex'));
    h.upload(c,0xfffffffe,Buffer.from([0x66,0x66]));h.upload(c,0,Buffer.from([0x66]));
    const csLimit=consumed===0?0x1ffd:consumed===2?0x1fff:0x2000;
    const starter=state(h.initial,{...context,pc:0x1000,csLimit:consumed===0?0x2000:csLimit});h.put(c,0,starter);
    const source=h.compile(c,'same-key starter enters uncaptured logical PC',[[0x1000,5]]);
    if(source){
      const miss=exit(Buffer.from(starter),3,1);miss.writeUInt32LE(0x1ffe,48);miss.writeUInt32LE(2,52);
      const observed=h.observe(c,'executed starter yields attested NeedCode',{input:starter,want:miss,budget:2});
      if(observed.passed){
        const stoppedInput=Buffer.from(miss);if(consumed===0)setSegments(stoppedInput,{...context,csLimit});
        h.put(c,0,stoppedInput);const repaired=h.compile(c,'stopped repair captures exact consumed physical prefix',[],[],{stopped:true});
        if(repaired){
          const input=state(h.initial,{...context,pc:0x1ffe,csLimit}),want=exit(Buffer.from(input),11,0);
          h.observe(c,'logical CS fault preserves RF before instruction begin',{input,want});
          h.upload(c,0,Buffer.from([0x66]));
          h.observe(c,consumed===3?'consumed low prefix byte invalidates repaired material':'unfetched low byte does not stale context fault',
            {input,want:consumed===3?input:want,status:consumed===3?4:0,kind:consumed===3?'physical-currentness':'semantic'});
          if(consumed<3){
            h.upload(c,0xfffffffe,Buffer.from([0x66]));
            h.observe(c,consumed===0?'source-free fault remains current after high byte write':'consumed high prefix byte invalidates repaired material',
              {input,want:consumed===0?want:input,status:consumed===0?0:4,kind:consumed===0?'semantic':'physical-currentness'});
          }
        }else h.skip(c,'repaired prefix identity semantics','stopped prefix capture');
      }else h.skip(c,'stopped prefix capture','attested NeedCode');
    }else h.skip(c,'runtime prefix repair','starter compilation');
    h.host(c,'close');
  }
  for(const low of [false,true]){
    const c=h.open(resident,entries,low?'wrapped low page lacks Execute then repairs':'wrapped low page is absent',low?[[0,3],[0xfffff000]]:[[0xfffff000]]);
    h.upload(c,0xffffffff,Buffer.from([0x66]));if(low)h.upload(c,0,Buffer.from('90ebfe','hex'));
    const input=state(h.initial,{...context,pc:0x1fff});h.put(c,0,input);
    h.compile(c,'wrapped tail Execute refusal is neutral admission',[[0x1fff,4]],[],{expectedStatus:10});
    if(low){h.host(c,'protect',[0,1,7]);h.put(c,0,input);const material=h.compile(c,'adding Execute repairs valid wrapped code',[[0x1fff,4]]);
      if(material)h.observe(c,'repaired wrapped prefixed NOP retires once',{input,want:success(input,0x2001)});
      else h.skip(c,'repaired wrapped NOP','Execute repair compilation');}
    h.host(c,'close');
  }
}
assert.equal(literalHash(),beforeLiteralHash,'executed literal probe stays fixed');
h.finish([
 'Finite CS translated linear wrapping only, valid logical offsets and loaded CS authority retained.',
 'Literal full arena/RAM and real helper trace checks across replacement/resident and extent/entry owners; exact split instruction/gate/prefix physical identity, permission repair and source-free controls.',
 'Existing logical4GiB CS limit policy, ordinary data/fetch nonwrapping APIs, decode keys and resource limits unchanged. No PAE/system-paging implementation, named-family equivalence, browser, throughput or game claim.'
],{rows:148,contexts:36,modules:44});
