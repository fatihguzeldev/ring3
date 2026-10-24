import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {Engine,KEY,record,view} from '../../../../../../sdk/runtime/engine.js';
import {CodeCache as CurrentCodeCache} from '../../../../../../sdk/runtime/cache.js';
import {readGeneratedModule} from './structure.mjs';
import {instrumentWitnessModule,readWitness,setWitnessBank,witnessCounters} from './witness.mjs';

const ONE='0000000000000080ff3f',HALF='0000000080000080ff3f';
const THREE_HALVES='0000000080010080ff3f',TWO_ULPS='0000000000020080ff3f',WIDE='00000000000000807f40';
const regs=[0,2,0,0x3000,0x4800,0,0x1800,0];
const data=page=>page.set(Buffer.from('0000803f00008033','hex'));
const local='d903 d84304 d84304 d84304 d95b08 ffe6';
const cfg=[[0x1000,'d903 d84304 eb19'],[0x1020,'d84304 d84304 d95b08 ffe6']];

function packingProof(source) {
  const module=readGeneratedModule(source),index=[...module.names].find(([,name])=>name==='guest.run')?.[0];
  assert(Number.isInteger(index));const {ops}=module.functions[index-module.imports.length];
  const generic=(nodes,i)=>nodes[i]?.op===0x20&&nodes[i].local===46&&nodes[i+1]?.op===0x42&&
    nodes[i+1].value===40n&&nodes[i+2]?.op===0x88&&nodes[i+3]?.op===0x21&&nodes[i+3].local===47;
  const guards=ops.flatMap((node,i)=>{
    const p=ops.slice(i-4,i);
    if(node.op!==4||node.frame.otherwise===undefined||p.length!==4||p[0].op!==0x20||p[0].local!==87||
      p[1].op!==0x41||p[1].value!==0x300||p[2].op!==0x71||p[3].op!==0x45)return[];
    const native=ops.slice(i+1,node.frame.otherwise),packed=ops.slice(node.frame.otherwise+1,node.frame.end);
    return[{offset:node.offset,end:node.frame.end,
      native:native.some((op,n)=>op.op===0xb6&&native[n+1]?.op===0xbc&&native[n+2]?.op===0xad&&
        native[n+3]?.op===0x21&&native[n+3].local===47),
      generic:packed.some((_op,n)=>generic(packed,n))}];
  });
  const stores=ops.flatMap((node,i)=>node.op===0x36&&node.memoryOffset===0&&
    ops[i-3]?.op===0x20&&ops[i-3].local===52&&ops[i-2]?.op===0x20&&ops[i-2].local===47&&ops[i-1]?.op===0xa7?
    [{offset:node.offset,operator:i}]:[]);
  return{guards,stores};
}

function observe(t,wanted,{helpers=0,precise=0,writes=1,packs,conditional=true,queries=0},checks) {
  const source=t.bytes(t.api.core_code_ptr()>>>0,t.api.core_code_len()>>>0),before=t.version();
  const id=new DataView(t.engine.memory.buffer).getBigUint64(t.context()+120,true),run=t.check(wanted);
  assert.equal(run.activations.length,1);assert.equal(t.result.modules.length,1);
  assert.equal(run.helperCalls,helpers,'only planned canonical effects use the retained owner ordinal');
  assert.equal(run.preciseCalls,precise,'rejected or partial arithmetic uses the exact adapter once per required suffix');
  assert.equal(t.version(),before+BigInt(writes));
  assert.equal(new DataView(t.engine.memory.buffer).getBigUint64(t.context()+120,true),id);
  assert.deepEqual(t.bytes(t.api.core_code_ptr()>>>0,t.api.core_code_len()>>>0),source,'the actual owner bytes remain immutable');
  const witness=t.stateWitness(),structure=packingProof(source);
  t.result.arithmeticWitness=witness;t.result.arithmeticStructure=structure;t.result.semanticComplete=true;
  checks.push(()=>{
    assert.equal(witness.fp_generic_f32_packs,packs,'actual raw80-to-f32 packing follows the admitted current precision');
    assert.equal(witness.fp_single_precision_queries,queries,'a dominating mode-correlated value fact removes repeated operand precision queries');
    if(conditional){
      assert.equal(structure.guards.length,1,'one actual imported-CW precision guard selects this arithmetic-produced store');
      assert(structure.guards[0].native&&structure.guards[0].generic,
        'the real PC24 arm uses exact native bits and the real PC53 arm retains generic rounding');
      assert.equal(structure.stores.length,1,'both precision arms converge on one actual memory payload store');
      assert(structure.stores[0].operator>structure.guards[0].end,'memory preparation and commit follow precision selection');
    }
  });
}

export function runArithmeticCases(fixture) {
  const checks=[];
  for(const single of[true,false])for(const blocks of[false,true])fixture(
    `arithmetic-mode-correlation-${single?'pc24':'pc53'}-${blocks?'cfg-edge':'local-chain'}`,{
      segments:blocks?cfg:[[0x1000,local]],registers:regs,control:single?0x7f:0x27f,data,stateWitness:true,
    },t=>{
      t.writeExpected(0x3008,single?'0000803f':'0200803f');
      observe(t,{pc:0x1800,retired:blocks?7:6,budget:blocks?7:6,
        fp:t.fp({status:single?0x20:0x220,opcode:0x15b,fip:blocks?0x1026:0x100b,fdp:0x3008,
          registers:{7:single?ONE:THREE_HALVES}})}, {packs:single?0:1},checks);
    });

  for(const single of[true,false])fixture(`arithmetic-mode-correlation-${single?'pc24-rejects':'pc53-retains'}-contradictory-secondary-input`,{
    segments:cfg,registers:regs,control:single?0x7f:0x27f,data,stateWitness:true,
    initialFp:{status:0x200,tag:0xfffc,registers:{0:HALF}},
  },t=>{
    new DataView(t.engine.memory.buffer).setUint32(t.context()+32,0x1020,true);
    t.writeExpected(0x3008,'0200803f');
    observe(t,{pc:0x1800,retired:4,budget:4,
      fp:t.fp({status:single?0x820:0xa20,opcode:0x15b,fip:0x1026,fdp:0x3008,
        registers:{0:single?TWO_ULPS:THREE_HALVES}})}, {precise:single?1:0,packs:single?0:1},checks);
  });

  for(const single of[true,false])for(const budget of[1,2])fixture(
    `arithmetic-mode-correlation-${single?'pc24':'pc53'}-quota-prefix-${budget}`,{
      segments:[[0x1000,local]],registers:regs,control:single?0x7f:0x27f,data,stateWitness:true,
    },t=>observe(t,{pc:budget===1?0x1002:0x1005,retired:budget,budget,
      fp:t.fp({status:0x3800+(single&&budget===2?0x20:0),tag:0x3fff,
        opcode:budget===1?0x103:0x43,fip:budget===1?0x1000:0x1002,fdp:budget===1?0x3000:0x3004,
        registers:{7:budget===1||single?ONE:HALF}})}, {precise:1,writes:0,packs:0},checks));

  fixture('arithmetic-mode-correlation-fldcw-kills-prior-single-result-fact',{
    segments:[[0x1000,'d903 d84304 d96b0c d84304 d95b08 ffe6']],registers:regs,
    control:0x7f,stateWitness:true,data:page=>{data(page);new DataView(page.buffer).setUint16(12,0x27f,true);},
  },t=>{
    t.writeExpected(0x3008,'0000803f');
    const expected=t.fp({status:0x20,opcode:0x15b,fip:0x100b,fdp:0x3008,registers:{7:HALF}});
    new DataView(expected.buffer,expected.byteOffset,expected.length).setUint16(16,0x27f,true);
    observe(t,{pc:0x1800,retired:6,budget:6,fp:expected}, {helpers:1,packs:1},checks);
  });

  fixture('arithmetic-mode-correlation-directed-rounding-retains-canonical-suffix',{
    segments:[[0x1000,local]],registers:regs,control:0xa7f,data,stateWitness:true,
  },t=>{
    t.writeExpected(0x3008,'0200803f');
    observe(t,{pc:0x1800,retired:6,budget:6,fp:t.fp({status:0x220,opcode:0x15b,fip:0x100b,fdp:0x3008,
      registers:{7:THREE_HALVES}})}, {precise:1,packs:0},checks);
  });

  fixture('arithmetic-mode-correlation-canonical-wide-single-result-is-not-native24',{
    segments:[[0x1000,'d903 d8c0 eb1a'],[0x1020,'d95b08 ffe6']],registers:regs,
    control:0x7f,stateWitness:true,data:page=>new DataView(page.buffer).setUint32(0,0x7f000000,true),
  },t=>{
    // 2^127+2^127 is exactly2^128 at x87 precision24. It exceeds binary32
    // range; the later float32 store must raise masked OE+PE and set C1.
    t.writeExpected(0x3008,'0000807f');
    observe(t,{pc:0x1800,retired:5,budget:5,
      fp:t.fp({status:0x228,opcode:0x15b,fip:0x1020,fdp:0x3008,registers:{7:WIDE}})},
      {precise:2,packs:0},checks);
  });

  fixture('arithmetic-mode-correlation-raw-sign-producer-does-not-grant-native-admission',{
    segments:[[0x1000,'d9e0 d803 d95b08 ffe6']],registers:regs,
    control:0x27f,stateWitness:true,data:()=>{},
    initialFp:{tag:0xfffc,registers:{0:'01000000000000c00040'}},
  },t=>{
    // FCHS preserves the exact -(3+2^-62) payload. A loaded raw cell does not
    // establish the native admission needed by FADD; PC53 rounds it to -3.
    t.writeExpected(0x3008,'000040c0');
    observe(t,{pc:0x1800,retired:4,budget:4,
      fp:t.fp({status:0x820,opcode:0x15b,fip:0x1004,fdp:0x3008,
        registers:{0:'00000000000000c000c0'}})},
      {precise:1,packs:0,conditional:false},checks);
  });
  for(const check of checks)check();
}

export function runArithmeticOwnerCases(artifact,report) {
  const checks=[];
  for(const single of[true,false]){
    const result={name:`arithmetic-mode-correlation-${single?'pc24':'pc53'}-actual-two-owner-pool`,
      controls:[],modules:[],probes:[],complete:false};report.cases.push(result);
    const instance=new WebAssembly.Instance(artifact.module,{ring3_observability:{now_ns:()=>0n}});
    const raw=Object.fromEntries(Object.entries(instance.exports).filter(([key])=>key.startsWith('ring3_abi_v1_'))
      .map(([key,value])=>[key.slice(13),value]));
    const memory=instance.exports.memory,bytes=(pointer,length)=>new Uint8Array(memory.buffer).slice(pointer,pointer+length);
    const word=(pointer,offset=0)=>new DataView(memory.buffer).getUint32(pointer+offset,true);
    const hash=source=>createHash('sha256').update(source).digest('hex'),diagnostics=[],units=new Map();
    let cache,engine,opened=false,helpers=0,prepares=0,initialBank;
    const status=(name,wanted,...args)=>{const actual=raw[name](...args)>>>0;
      result.controls.push({name,wanted,actual});assert.equal(actual,wanted,`${result.name}: ${name}`);};
    const api={...raw,core_prepare:(...args)=>{prepares++;return raw.core_prepare(...args);},
      core_numeric:(...args)=>{helpers++;return raw.core_numeric(...args);},
      core_chain_cold:()=>assert.fail('both immutable compiled owners are already retained'),
      [typeof raw.core_chain_select==='function'?'core_chain_select':'core_chain_probe']:(context,pc,selectionRetired)=>{
        assert.equal(context,cache.context);assert.equal(pc,0x1040);assert.equal(result.probes.length,0);
        const bank=word(context,92),physical7=bank+640+7*32;
        result.probes.push({pc,bankHex:Buffer.from(bytes(bank,100)).toString('hex'),
          value:new DataView(memory.buffer).getFloat64(physical7,true),
          valid:word(physical7,20),proofs:word(physical7,24),cw:word(physical7,28)});
        const slot=(raw.core_chain_select??raw.core_chain_probe)(context,pc,selectionRetired)>>>0;assert.equal(slot,units.get(pc).slot);return slot;
      }};
    try{
      engine=new Engine(api,memory,artifact.sha256);status('open',0,12,...KEY);opened=true;engine.base=raw.arena_ptr()>>>0;
      const code=new Uint8Array(4096),page=new Uint8Array(4096);data(page);
      code.set(Buffer.from('d903d84304ffe6','hex'));
      code.set(Buffer.from('d84304d84304d95b08ffe7','hex'),0x40);
      for(const[address,image]of[[0x1000,code],[0x3000,page],[0x4000,new Uint8Array(4096)]]){
        status('map',0,address,1,address===0x1000?7:3);engine.request(image);status('upload',0,address,4096);
      }
      const state=record('R3ST',56),s=view(state),registers=[0,2,0,0x3000,0x4800,0,0x1040,0x1800];
      registers.forEach((value,i)=>s.setUint32(16+i*4,value,true));s.setUint32(48,0x1000,true);s.setUint32(52,2,true);
      new Uint8Array(memory.buffer,engine.base,56).set(state);
      const fpAddress=engine.base+4236,control=single?0x7f:0x27f;
      new DataView(memory.buffer).setUint16(fpAddress+16,control,true);
      new Uint8Array(memory.buffer,fpAddress+60,10).set(Buffer.from('01000000000000c00040','hex'));
      new Uint8Array(memory.buffer,fpAddress+90,10).set(Buffer.from('00000000000000000080','hex'));
      const initialFp=bytes(fpAddress,128),initialSimd=bytes(engine.base+4364,160),initialYmm=bytes(engine.base+4612,144);
      const Cache=artifact.CodeCache??CurrentCodeCache;cache=new Cache(engine,{budget:7,instructions:64,blocks:8,chain:true});
      for(const pc of[0x1000,0x1040]){
        const unit=cache.entry(pc),source=bytes(raw.core_code_ptr()>>>0,raw.core_code_len()>>>0);
        units.set(pc,unit);result.modules.push({pc,id:String(unit.id),slot:unit.slot,sha256:hash(source),source});
      }
      status('core_lookup',0,0x1000);status('core_begin',0);status('core_end',0);
      const context=cache.context,bank=word(context,92),initialContext=bytes(context,128);
      initialBank=bytes(bank,100);
      const backing=word(context,64),pages=word(context,68),initialRam=bytes(backing,pages*4096);
      const initialStamps=bytes(word(context,84),pages*16),dataSlot=word(word(context,56)+3*4)-1;
      const sequence=()=>new DataView(memory.buffer).getBigUint64(word(context,80),true),before=sequence();
      const dispatchSource=bytes(raw.core_dispatch_code_ptr()>>>0,raw.core_dispatch_code_len()>>>0);
      const shared=instrumentWitnessModule(dispatchSource,readGeneratedModule(dispatchSource),true);
      const support=new WebAssembly.Instance(new WebAssembly.Module(shared.bytes),{ring3:{memory,
        functions:cache.table,publishers:cache.publishers,probe:api.core_chain_select??api.core_chain_probe,cold:api.core_chain_cold}}).exports;
      cache.dispatch=support;diagnostics.push(support);
      for(const module of result.modules){
        const diagnostic=instrumentWitnessModule(module.source,readGeneratedModule(module.source));
        const generated=new WebAssembly.Instance(new WebAssembly.Module(diagnostic.bytes),{ring3:{memory,
          read:api.core_read,write:api.core_write,numeric:api.core_numeric,precise:api.core_precise,
          ...support,'support.abi':support.supportAbi}}).exports;
        const unit=units.get(module.pc);Object.assign(unit,{run:generated.run,link:generated.link,publish:generated.publish});
        cache.table.set(unit.slot,unit.link);cache.publishers.set(unit.slot,unit.publish);diagnostics.push(generated);
        module.packing=packingProof(module.source);module.instrumentedSha256=hash(diagnostic.bytes);delete module.source;
      }
      for(const item of diagnostics)setWitnessBank(item,bank);
      const prepared=prepares,run=cache.execute(units.get(0x1000)),last=units.get(0x1040);
      result.run={...run,id:String(run.id)};
      assert.deepEqual([run.coreReason,run.pc,run.retired,run.work,run.regions],[2,0x1800,7,7,2]);assert.equal(run.id,last.id);
      const expectedContext=initialContext.slice(),c=view(expectedContext);
      c.setUint32(32,0x1800,true);c.setUint32(40,7,true);c.setUint32(44,0,true);c.setBigUint64(120,last.id,true);
      assert.deepEqual(bytes(context,128),expectedContext,'all private Context128 fields and the selected final owner');
      const expectedState=state.slice();view(expectedState).setUint32(48,0x1800,true);
      assert.deepEqual(bytes(engine.base,56),expectedState,'all public scalar state');
      const expectedFp=initialFp.slice(),f=view(expectedFp);
      f.setUint16(18,single?0x20:0x220,true);f.setUint16(20,0xffff,true);f.setUint16(22,0x15b,true);
      f.setUint32(24,0x1046,true);f.setUint32(28,0x3008,true);
      f.setUint16(32,0x1b,true);f.setUint16(34,0x23,true);
      expectedFp.set(Buffer.from(single?ONE:THREE_HALVES,'hex'),110);
      assert.deepEqual(bytes(fpAddress,128),expectedFp,'complete FP state, PC53 precision and untouched empty ghosts');
      assert.deepEqual(bytes(engine.base+4364,160),initialSimd);assert.deepEqual(bytes(engine.base+4612,144),initialYmm);
      const expectedRam=initialRam.slice();expectedRam.set(Buffer.from(single?'0000803f':'0200803f','hex'),dataSlot*4096+8);
      assert.deepEqual(bytes(backing,expectedRam.length),expectedRam,'entire code/data/stack RAM');assert.equal(sequence(),before+1n);
      const expectedStamps=initialStamps.slice();view(expectedStamps).setBigUint64(dataSlot*16+8,before+1n,true);
      assert.deepEqual(bytes(word(context,84),expectedStamps.length),expectedStamps,'only one physical data-page content stamp changes');
      assert.equal(helpers,0);assert.equal(prepares,prepared);assert.equal(result.probes.length,1);
      assert.deepEqual([cache.admissions,cache.units.size,cache.transientAdmissions,cache.transientRuns],[2,2,0,0]);
      for(const module of result.modules){status('core_lookup',0,module.pc);assert.equal(hash(bytes(raw.core_code_ptr()>>>0,raw.core_code_len()>>>0)),module.sha256);}
      const counters=Object.fromEntries(witnessCounters.map(name=>[name,diagnostics.reduce((total,item)=>
        name.endsWith('_mask')?total|readWitness(item)[name]:total+readWitness(item)[name],0)]));
      result.arithmeticWitness=counters;result.semanticComplete=true;
      checks.push(()=>{
        assert.equal(counters.fp_generic_f32_packs,single?0:1,'actual ownerB packs only the PC53 value generically');
        assert.equal(counters.fp_single_precision_queries,0,'mode-correlated arithmetic survives the compiled owner boundary');
        assert.equal(counters.fp_value_precision_admissions,0,'the actual producer certificate satisfies ownerB first-use precision admission');
        assert.equal(counters.fp_operand_bounds,0,'the actual producer certificate satisfies ownerB first-use finite-range admission');
        const probe=result.probes[0];assert.equal(probe.value,single?1:1+2**-24);assert.equal(probe.valid,1);
        assert(probe.proofs&8,'the real producer writes the new conditional certificate into its physical payload record');
        if(!single)assert.equal(probe.proofs&4,0,'a PC53-produced value is never mislabeled unconditional Native24');
        assert.equal(probe.cw,control,'the conditional certificate belongs to the current actual CW');
        assert.equal(probe.bankHex,Buffer.from(initialBank).toString('hex'),'the compiled boundary does not publish canonical FP state');
        const packed=result.modules[1].packing;assert.equal(packed.guards.length,1);
        assert(packed.guards[0].native&&packed.guards[0].generic);assert.equal(packed.stores.length,1);
      });
      result.complete=true;
    }catch(error){result.failure={message:String(error),stack:error.stack};throw error;}
    finally{if(cache?.open)raw.core_end();if(opened)status('close',0);}
  }
  for(const check of checks)check();
}
