import assert from 'node:assert/strict';
import {coldFixture,coldLiteral} from '../../../../support/core_wasm/cold_observed.mjs';
import {cacheCallbacks} from '../link.mjs';
import {readGeneratedModule} from './structure.mjs';
import {instrumentWitnessModule,readWitness,setWitnessBank} from './witness.mjs';

const ROOT=0x1000,PRIVATE=0x1020,FLAGS=0x403,FP=4236;
const FULL80='01000000000000c00040',THREE='00000000000000c00040';
const REGISTERS=[0x13579bdf,2,0x2468ace0,0x3000,0x4800,0x89abcdef,ROOT,0x76543210];
const unhex=text=>Uint8Array.from(Buffer.from(text.replaceAll(' ',''),'hex'));
const wordHex=value=>{const b=Buffer.alloc(4);b.writeUInt32LE(value>>>0);return b.toString('hex');};
const add=n=>(REGISTERS[0]+n)>>>0;

function image(initial,{full=false,completed=false}={}) {
  const fp=initial.slice(),d=new DataView(fp.buffer);
  for(const[offset,value]of[[16,0x27f],[18,completed?0x20:0x200],[20,0xfff4],
    [22,completed?0xc1:0x5a5],[32,completed?0x1b:0x33],[34,0x5b]])d.setUint16(offset,value,true);
  d.setUint32(24,completed?0x1003:0x11223344,true);d.setUint32(28,0x55667788,true);
  [full&&!completed?FULL80:THREE,'00000000000000000080','0000000000000040ff3f',
    '00000000000000c0ff7f','7856341200000080ff3f','ffffffffffffffffffff',
    'efcdab8967452301807f','ffffffffffffffffffff']
    .forEach((value,index)=>fp.set(unhex(value),40+10*index));
  return fp;
}

function instrument(world) {
  const instances=[];
  for(const unit of world.cache.units.values()) {
    const source=world.moduleRecords.get(unit.id).source;
    const diagnostic=instrumentWitnessModule(source,readGeneratedModule(source));
    const generated=new WebAssembly.Instance(new WebAssembly.Module(diagnostic.bytes),{ring3:{
      memory:world.memory,read:world.api.core_read,write:world.api.core_write,
      ...cacheCallbacks(world.cache),...world.cache.dispatch,'support.abi':world.cache.dispatch.supportAbi,
      'recovery.base':unit.recoveryBase,
    }}).exports;
    Object.assign(unit,{run:generated.run,link:generated.link,publish:generated.publish});
    world.cache.table.set(unit.slot,unit.link);world.cache.publishers.set(unit.slot,unit.publish);
    setWitnessBank(generated,world.word(world.context,92));instances.push(generated);
  }
  return()=>instances.reduce((sum,instance)=>sum+readWitness(instance).body_entries,0);
}

export function runExactSuffixWindowCases(artifact,report) {
  const code=new Uint8Array(4096);
  code.set(unhex('8d4001 d8c1 8d4002 eb16'));
  code.set(unhex('8d4004 89430c ffe6'),PRIVATE-ROOT);
  coldFixture(artifact,report,'exact-suffix-resumes-private-window-span-without-public-ingress',{
    code,registers:REGISTERS,flags:FLAGS,referencePrepared:true,
  },world=>{
    const native=image(world.initialFp),opaque=image(world.initialFp,{full:true});
    const baseline=world.cache.entry(ROOT);
    world.reset({pc:ROOT,registers:REGISTERS,flags:FLAGS,fp:native});world.cache.select(ROOT);
    assert.equal(world.raw.core_trace_start(ROOT),0);
    const before=world.snapshot(),train=world.execute(84,ROOT);
    const trainFp=image(native,{completed:true});new DataView(trainFp.buffer).setUint16(18,0,true);
    coldLiteral(world,before,train,{pc:ROOT,retired:84,budget:84,registers:[add(84),...REGISTERS.slice(1)],
      flags:FLAGS,fp:trainFp,writes:[{address:0x300c,bytes:wordHex(add(84)),count:12}],versionWrites:12,
      identity:world.bigint(world.context,120)});
    assert.equal(world.raw.core_trace_finish(ROOT,64,16),0);
    const window=world.cache.select(ROOT);assert(window&&window.id!==baseline.id);
    const source=world.moduleRecords.get(window.id),retained={id:window.id,slot:window.slot,
      bytes:window.bytes,sha256:source.sha256};
    const countBodies=instrument(world),checks=[];
    const admissions=world.cache.admissions,generated=world.cache.generatedModuleBytes;
    const runLiteral=(budget,target)=>{
      const registers=[...REGISTERS];registers[6]=target;
      world.reset({pc:ROOT,registers,flags:FLAGS,fp:opaque});world.cache.select(ROOT);
      const start=world.snapshot(),bodies=countBodies(),precise=world.cache.preciseRuns;
      const run=world.execute(budget,ROOT);
      const completed=budget!==0,stored=budget>=6,increment=budget===0?0:budget===2?1:7;
      const pc=budget===0?ROOT:budget===2?0x1005:budget===6?0x1026:target;
      coldLiteral(world,start,run,{pc,retired:budget,budget,registers:[add(increment),...registers.slice(1)],
        flags:FLAGS,fp:completed?image(opaque,{completed:true}):opaque,
        writes:stored?[{address:0x300c,bytes:wordHex(add(7))}]:[],versionWrites:Number(stored),
        identity:world.bigint(world.context,120)});
      const observed={budget,target,id:String(run.id),bodies:countBodies()-bodies,
        precise:world.cache.preciseRuns-precise};
      (world.result.privateSuffixRuns??=[]).push(observed);
      checks.push(()=>{
        assert.equal(observed.bodies,1,'the private hot span resumes inside the current window body');
        assert.equal(observed.precise,Number(completed),'one actual precise suffix serves only a nonzero fallback');
        assert.equal(run.id,window.id,'the exact callback retains the same window owner');
      });
    };
    for(const[budget,target]of[[7,ROOT],[7,0x1800],[0,ROOT],[2,ROOT],[6,ROOT]])runLiteral(budget,target);

    // an internal restart capability never grants a public entry at the same pc.
    world.reset({pc:PRIVATE,registers:REGISTERS,flags:FLAGS,fp:opaque});world.cache.select(ROOT);
    const wrongBefore=world.snapshot(),bodies=countBodies(),retired=world.word(world.context,40);
    assert.equal(world.raw.core_begin(),0);
    let stop;
    try {stop=window.run(world.context,3)>>>0;}
    finally {assert.equal(world.raw.core_end(),0);}
    const wrong={coreReason:stop,pc:world.word(world.context,32),retired:world.word(world.context,40)-retired,
      work:3-world.word(world.context,44),id:window.id,regions:1};
    world.engine.ok('core_export',stop,wrong.work);world.result.runs.push({...wrong,id:String(wrong.id)});
    coldLiteral(world,wrongBefore,wrong,{pc:PRIVATE,retired:0,work:0,budget:3,stop:1,
      registers:REGISTERS,flags:FLAGS,fp:opaque,identity:window.id});
    assert.equal(countBodies()-bodies,1);
    world.result.semanticComplete=true;
    assert.equal(world.cache.admissions,admissions);assert.equal(world.cache.generatedModuleBytes,generated);
    assert.equal(world.cache.units.get(retained.id),window);assert.equal(window.slot,retained.slot);
    assert.equal(window.bytes,retained.bytes);assert.equal(world.moduleRecords.get(window.id).sha256,retained.sha256);
    assert.equal(world.cache.table.get(window.slot),window.link);
    assert.equal(world.cache.publishers.get(window.slot),window.publish);
    assert.equal(world.helperCalls.length,0,'the real exact suffix is not a numerical ordinal stand-in');
    for(const check of checks)check();
  });
}
