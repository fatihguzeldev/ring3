import assert from 'node:assert/strict';
import {instrumentWitnessModule,readWitness,setWitnessBank,setWitnessData,setWitnessAuthority,witnessCounters} from './witness.mjs';

class Reader {
  constructor(bytes){this.bytes=bytes;this.at=0;}
  byte(){assert(this.at<this.bytes.length,'complete authored Wasm input');return this.bytes[this.at++];}
  unsigned(){let result=0;for(let shift=0;shift<35;shift+=7){const b=this.byte();result+=(b&127)*2**shift;if(b<128)return result;}assert.fail('bounded Wasm u32');}
  signed(){let result=0n;for(let index=0;index<10;index++){const b=this.byte(),shift=BigInt(index*7);
    result|=BigInt(b&127)<<shift;if(b<128)return b&64?result-(1n<<(shift+7n)):result;}
    assert.fail('bounded signed Wasm integer');}
  take(length){assert(this.at+length<=this.bytes.length);const result=this.bytes.subarray(this.at,this.at+length);this.at+=length;return result;}
  text(){return new TextDecoder('utf-8',{fatal:true}).decode(this.take(this.unsigned()));}
  vector(read){return Array.from({length:this.unsigned()},()=>read());}
}

function operators(bytes) {
  const r=new Reader(bytes),locals=r.vector(()=>({count:r.unsigned(),type:r.byte()}));
  const localsEnd=r.at;
  const root={kind:0xff,open:-1},frames=[root],ops=[];
  while(r.at<bytes.length) {
    const node={op:r.byte(),offset:r.at-1,frames:frames.slice(),targets:[]},index=ops.length;
    const op=node.op;
    if(op===2||op===3||op===4){node.frame={kind:op,open:index,type:Number(r.signed())};frames.push(node.frame);}
    else if(op===5){node.frame=frames.at(-1);assert.equal(node.frame.kind,4);node.frame.otherwise=index;}
    else if(op===11){node.frame=frames.pop();assert(node.frame);node.frame.end=index;}
    else if(op===12||op===13||op===0xd5||op===0xd6)node.targets=[r.unsigned()];
    else if(op===14){node.targets=r.vector(()=>r.unsigned());node.targets.push(r.unsigned());}
    else if(op===16||op===18)node.call=r.unsigned();
    else if(op===17||op===19){r.unsigned();r.unsigned();}
    else if(op>=0x20&&op<=0x22)node.local=r.unsigned();
    else if(op===20||op===21||op>=0x23&&op<=0x26||op===0xd2)r.unsigned();
    else if(op===0x1c)r.vector(()=>r.byte());
    else if(op>=0x28&&op<=0x3e){node.align=r.unsigned();node.memoryOffset=r.unsigned();}
    else if(op===0x3f||op===0x40)r.unsigned();
    else if(op===0x41)node.value=Number(r.signed());
    else if(op===0x42)node.value=r.signed();
    else if(op===0xd0)r.signed();
    else if(op===0x43)r.take(4);
    else if(op===0x44)r.take(8);
    else if(op===0xfc){
      node.ext=r.unsigned();
      if([8,10,12,14].includes(node.ext)){r.unsigned();r.unsigned();}
      else if([9,11,13,15,16,17].includes(node.ext))r.unsigned();
      else assert(node.ext<=7,'supported authored bulk operation');
    }else if(op===0xfd){
      node.ext=r.unsigned();
      if(node.ext<=11||node.ext===92||node.ext===93){node.align=r.unsigned();node.memoryOffset=r.unsigned();}
      else if(node.ext===12||node.ext===13)r.take(16);
      else if(node.ext>=21&&node.ext<=34)r.byte();
      else if(node.ext>=84&&node.ext<=91){r.unsigned();r.unsigned();r.byte();}
      else assert(node.ext<=255,'supported authored SIMD operation');
    }else assert([0,1,15,0x1a,0x1b,0xd1,0xd3,0xd4].includes(op)||op>=0x45&&op<=0xc4,
      `supported authored opcode ${op.toString(16)}`);
    ops.push(node);
  }
  assert.equal(frames.length,0,'complete structured function');assert.equal(root.end,ops.length-1);
  const next=index=>index+1<ops.length?[index+1]:[];
  const target=(node,depth)=>{
    const frame=node.frames.at(-1-depth);assert(frame,'valid branch depth');
    return frame.kind===3?frame.open+1:frame.end+1;
  };
  const edges=ops.map((node,index)=>{
    if([0,15,18,19,21].includes(node.op)||node.op===11&&node.frame===root)return [];
    if(node.op===12||node.op===14)return node.targets.map(depth=>target(node,depth)).filter(index=>index<ops.length);
    if([13,0xd5,0xd6].includes(node.op))return [...next(index),target(node,node.targets[0])].filter(index=>index<ops.length);
    if(node.op===4)return [index+1,(node.frame.otherwise??node.frame.end)+1].filter(index=>index<ops.length);
    if(node.op===5)return node.frame.end+1<ops.length?[node.frame.end+1]:[];
    return next(index);
  });
  return {raw:bytes,localsEnd,locals,ops,edges};
}

function readModule(bytes) {
  const r=new Reader(bytes);assert.deepEqual([...r.take(8)],[0,97,115,109,1,0,0,0]);
  const names=new Map(),imports=[],globalImports=[],functions=[],types=[],functionTypes=[],exports=new Map();
  while(r.at<bytes.length) {
    const kind=r.byte(),section=new Reader(r.take(r.unsigned()));
    if(kind===0&&section.text()==='name') {
      while(section.at<section.bytes.length){const subsection=section.byte(),body=new Reader(section.take(section.unsigned()));
        if(subsection===1)body.vector(()=>{names.set(body.unsigned(),body.text());});}
    }else if(kind===1)types.push(...section.vector(()=>{
      assert.equal(section.byte(),0x60,'ordinary authored function type');
      return {parameters:section.vector(()=>section.byte()),results:section.vector(()=>section.byte())};
    }));
    else if(kind===2)section.vector(()=>{
      const module=section.text(),name=section.text(),type=section.byte();
      if(type===0)imports.push({module,name,type:section.unsigned()});
      else if(type===1){section.byte();const flags=section.unsigned();section.unsigned();if(flags&1)section.unsigned();assert(flags<4);}
      else if(type===2){const flags=section.unsigned();section.unsigned();if(flags&1)section.unsigned();assert(flags<4);}
      else if(type===3){assert(module==='ring3'&&(name.startsWith('link.')||name==='trace.arena'||name==='support.abi'));
        const valueType=section.byte(),mutable=section.byte();assert.equal(mutable,name==='support.abi'?0:1);
        if(name==='trace.arena')assert.equal(valueType,0x7f,'the private recorder capability is an i32 pointer');
        globalImports.push({module,name,valueType,mutable});}
      else assert.fail('only memory, function and typed-link imports in authored module');
    });
    else if(kind===3)functionTypes.push(...section.vector(()=>section.unsigned()));
    else if(kind===7)section.vector(()=>{const name=section.text(),type=section.byte(),index=section.unsigned();if(type===0)exports.set(name,index);});
    else if(kind===10)functions.push(...section.vector(()=>operators(section.take(section.unsigned()))));
  }
  assert.equal(functionTypes.length,functions.length);
  return {names,imports,globalImports,functions,types,functionTypes,exports};
}

export {readModule as readGeneratedModule};

const named=(module,name)=>[...module.names].find(([,value])=>value===name)?.[0];
const signature=(module,index)=>module.types[index<module.imports.length?module.imports[index].type:module.functionTypes[index-module.imports.length]];
const defined=(module,index)=>module.functions[index-module.imports.length];
function supportBinding(owner,support,name) {
  const imported=owner.imports.findIndex(item=>item.module==='ring3'&&item.name===name);
  const exported=support.exports.get(name);
  assert(imported>=0,`owner imports actual ${name}`);
  assert(Number.isInteger(exported)&&exported>=support.imports.length,`support defines actual ${name}`);
  assert.equal(support.names.get(exported),name,'support export identifies the actual named function');
  assert.deepEqual(signature(owner,imported),signature(support,exported),`${name}: actual import/export types match`);
  return{imported,exported};
}

function components(edges,allowed) {
  const seen=new Set(),order=[],reverse=edges.map(()=>[]);
  edges.forEach((targets,from)=>{if(allowed.has(from))for(const to of targets)if(allowed.has(to))reverse[to].push(from);});
  for(const start of allowed) {
    if(seen.has(start))continue;
    const pending=[[start,false]];
    while(pending.length){const[node,after]=pending.pop();if(after){order.push(node);continue;}if(seen.has(node))continue;
      seen.add(node);pending.push([node,true]);for(const next of edges[node])if(allowed.has(next)&&!seen.has(next))pending.push([next,false]);}
  }
  seen.clear();const result=[];
  for(const start of order.reverse()) {
    if(seen.has(start))continue;
    const pending=[start],group=[];seen.add(start);
    while(pending.length){const node=pending.pop();group.push(node);for(const next of reverse[node])if(!seen.has(next)){seen.add(next);pending.push(next);}}
    if(group.length>1||edges[start].includes(start))result.push(group);
  }
  return result;
}

function guardedNativeNormalizer(module,index) {
  const fn=module.functions[index-module.imports.length],type=module.types[module.functionTypes[index-module.imports.length]];
  const wantedParameters=[...Array(8).fill(0x7c),...Array(8).fill(0x7e),...Array(14).fill(0x7f),0x7e,0x7e];
  const wantedResults=[...Array(8).fill(0x7c),...Array(8).fill(0x7e),...Array(14).fill(0x7f)];
  const matches=(left,right)=>left?.length===right.length&&left.every((value,i)=>value===right[i]);
  if(!fn||!matches(type?.parameters,wantedParameters)||!matches(type?.results,wantedResults))return undefined;
  const stores=fn.ops.filter(node=>node.op>=0x36&&node.op<=0x3e),guards=[];
  if(!stores.length||fn.ops.some(node=>[16,17,18,19,20,21].includes(node.op)))return undefined;
  for(const store of stores){
    if(!(store.op===0x37&&store.memoryOffset===0||store.op===0x3b&&store.memoryOffset===8))return undefined;
    const guardProof=frame=>{
      if(frame.kind!==4||frame.otherwise!==undefined)return false;
      const pattern=fn.ops.slice(frame.open-4,frame.open),mask=pattern[1]?.value;
      if(pattern.length===4&&pattern[0].op===0x20&&pattern[0].local===24&&
        pattern[1].op===0x41&&mask>=1&&mask<=128&&(mask&(mask-1))===0&&pattern[2].op===0x71&&pattern[3].op===0x45)
        return {guardOffset:fn.ops[frame.open].offset,nativeBit:mask};
      const dynamic=fn.ops.slice(frame.open-6,frame.open);
      const written=store.frames.find(outer=>{
        const p=fn.ops.slice(outer.open-3,outer.open);
        return outer.kind===4&&outer.otherwise===undefined&&p.length===3&&p[0].op===0x20&&p[0].local===41&&
          p[1].op===0x41&&p[1].value===16&&p[2].op===0x71;
      });
      if(dynamic.length===6&&dynamic[0].op===0x20&&dynamic[0].local===24&&
        dynamic[1].op===0x41&&dynamic[1].value===1&&dynamic[2].op===0x20&&dynamic[2].local===38&&
        dynamic[3].op===0x74&&dynamic[4].op===0x71&&dynamic[5].op===0x45&&written)
        return {guardOffset:fn.ops[frame.open].offset,nativeCellLocal:38,writtenGuardOffset:fn.ops[written.open].offset};
      return undefined;
    };
    const guard=store.frames.map(guardProof).filter(Boolean).at(-1);
    if(!guard)return undefined;
    guards.push({storeOffset:store.offset,...guard});
  }
  return {index,name:module.names.get(index),parameters:type.parameters,results:type.results,rawStoreGuards:guards,
    condition:'each raw80 store requires the corresponding incoming native-valid bit to be zero'};
}

export function instrumentStateWitness(source,supportBytes) {
  assert(supportBytes,'the witness receives actual per-CPU production support bytes');
  const owner=readModule(source),support=readModule(supportBytes);
  for(const name of ['support.materialize_numeric','support.acquire_group','support.translate_page'])
    supportBinding(owner,support,name);
  const instrumented=instrumentWitnessModule(source,owner),shared=instrumentWitnessModule(supportBytes,support,true);
  const acquire=support.exports.get('support.acquire_group'),fn=defined(support,acquire);
  const groupProof=fn?{index:acquire,...signature(support,acquire),
    contextLoads:fn.ops.filter((node,index)=>node.op>=0x28&&node.op<=0x35&&fn.ops[index-1]?.op===0x20&&fn.ops[index-1]?.local===0)
      .map(node=>node.memoryOffset).sort((a,b)=>a-b),
    stores:fn.ops.filter(node=>node.op>=0x36&&node.op<=0x3e).length,
    calls:fn.ops.filter(node=>[16,17,18,19,20,21].includes(node.op)).length}:undefined;
  return {bytes:instrumented.bytes,supportBytes:shared.bytes,groupProof,
    poolProof:{owner:instrumented.proof,support:shared.proof},
    read:(instance,supportExports,bankPointer,physicalRange,authority)=>{
      assert(supportExports,'the witness observes the actual instrumented support instance');
      setWitnessBank(instance,bankPointer);setWitnessBank(supportExports,bankPointer);
      setWitnessData(instance,physicalRange);setWitnessData(supportExports,physicalRange);
      setWitnessAuthority(instance,authority);setWitnessAuthority(supportExports,authority);
      const own=readWitness(instance),other=readWitness(supportExports);
      return Object.fromEntries(witnessCounters.map(name=>[name,name.endsWith('_mask')?own[name]|other[name]:own[name]+other[name]]));
    }};
}

function controlAdmission(ops,index) {
  return ops[index]?.op===0x20&&ops[index].local===87&&
    ops[index+1]?.op===0x41&&ops[index+1].value===0xf3f&&ops[index+2]?.op===0x71;
}

// Identify actual executable instruction bodies independently of profiler names.
// Both former fast/precise copies imported the whole architectural state and
// guarded the active lease and owner identity; wrappers and support kernels do not.
function instructionBodies(module) {
  return module.functions.flatMap((fn,offset)=>{
    const index=offset+module.imports.length,type=signature(module,index);
    const loads=fn.ops.flatMap((node,i)=>node.op===0x28&&fn.ops[i-1]?.op===0x20&&
      fn.ops[i-1].local===0?[node.memoryOffset]:[]);
    const identity=fn.ops.some((node,i)=>node.op===0x29&&node.memoryOffset===120&&
      fn.ops[i-1]?.op===0x20&&fn.ops[i-1].local===0);
    if(type?.parameters.join(',')!=='127,127'||type.results.join(',')!=='127'||!identity||
      ![0,4,8,12,16,20,24,28,32,36,40,112].every(value=>loads.includes(value)))return[];
    return[{index,name:module.names.get(index),contextLoads:loads}];
  });
}

export function inspectCanonicalSpine(bytes) {
  const module=readModule(bytes),bodies=instructionBodies(module);
  assert.equal(bodies.length,1,'one actual guarded architectural body owns the canonical continuation');
  const imported=module.imports.findIndex(item=>item.module==='ring3'&&item.name==='numeric');
  assert(imported>=0,'this numerical owner imports the real canonical numeric boundary');
  assert.deepEqual(signature(module,imported),{parameters:[0x7f,0x7f],results:[0x7f]},
    'the inspected boundary is the actual context/ordinal canonical function');
  const sites=module.functions.flatMap((fn,offset)=>fn.ops.flatMap(node=>node.op===16&&node.call===imported?
    [{function:offset+module.imports.length,offset:node.offset}]:[]));
  return{body:bodies[0].index,imported,sites,ownerBytes:bytes.length,
    bodyOperators:defined(module,bodies[0].index).ops.length};
}

function inspectPortRouting(fn) {
  const {ops}=fn,summary={tables:[],fullBlockAssignments:[],prefixQuotaGuards:[],errors:[]};
  const check=(okay,message)=>{if(!okay)summary.errors.push(message);};
  for(const[index,node]of ops.entries()){
    if(node.op===0x21&&node.local===151){
      const p=ops.slice(index-6,index);
      check(p.length===6&&p[0].op===0x20&&p[0].local===130&&p[1].op===0x45&&
        p[2].op===0x20&&p[2].local===12&&p[3].op===0x41&&p[3].value>0&&
        p[4].op===0x4f&&p[5].op===0x71,
        'FULL_BLOCK is established only from ordinary offset zero and sufficient whole-block quota');
      summary.fullBlockAssignments.push(node.offset);
    }
    // The two chunk selector forms are decoded rather than inferred from names.
    if(node.op===14){
      const outer=ops.slice(index-3,index),inner=ops.slice(index-10,index);
      const isOuter=outer.length===3&&outer[0].op===0x20&&outer[0].local===131&&
        outer[1].op===0x41&&outer[1].value===6&&outer[2].op===0x76;
      const isInner=inner.length===10&&inner[0].op===0x20&&inner[0].local===131&&
        inner[1].op===0x41&&inner[2].op===0x4b&&inner[3].op===4&&inner[3].frame.type===-1&&
        inner[4].op===0x20&&inner[4].local===131&&inner[5].op===0x41&&inner[5].value===inner[1].value&&
        inner[6].op===0x6b&&inner[7].op===5&&inner[8].op===0x41&&inner[8].value===0&&inner[9].op===11;
      if(isOuter||isInner){
        const start=index-(isOuter?3:10),bypass=ops.slice(start-3,start),branch=bypass[2];
        const target=branch?.frames.at(-1-branch.targets[0]);
        check(bypass.length===3&&bypass[0].op===0x20&&bypass[0].local===131&&
          bypass[1].op===0x45&&branch.op===13&&branch.targets[0]===0&&
          target===node.frames.at(-1)&&target.kind===2,
          'ordinary prefix zero branches past each real port selector into the shared first instruction');
        summary.tables.push({offset:node.offset,kind:isOuter?'chunk':'instruction',bypassOffset:branch?.offset});
      }
    }
    // Every exact-prefix quota test remains in the !FULL_BLOCK arm. Its
    // comparison has four preceding operands, not a second instruction body.
    if(node.op===0x4d&&ops[index-4]?.op===0x20&&ops[index-4].local===12&&
      ops[index-3]?.op===0x41&&ops[index-2]?.op===0x20&&ops[index-2].local===131&&ops[index-1]?.op===0x6b){
      const guard=node.frames.at(-1),p=guard?ops.slice(guard.open-2,guard.open):[];
      check(guard?.kind===4&&guard.otherwise===undefined&&p.length===2&&
        p[0].op===0x20&&p[0].local===151&&p[1].op===0x45,
        'exact-prefix quota checks are bypassed by the proven full-block mode');
      summary.prefixQuotaGuards.push(node.offset);
    }
  }
  check(summary.fullBlockAssignments.length>0,'the actual body establishes full-block mode');
  check(summary.tables.some(table=>table.kind==='chunk')&&summary.tables.some(table=>table.kind==='instruction'),
    'both bounded selector levels belong to the sole instruction body');
  check(summary.prefixQuotaGuards.length>0,'the sole body retains exact per-prefix quota guards');
  return summary;
}

function inspectNormalizerKernel(module,support) {
  const index=named(module,'guest.normalize_frame'),fn=defined(module,index);
  const binding=supportBinding(module,support,'support.normalize_kernel'),kernel=defined(support,binding.exported);
  if(!fn||!kernel)return {index,errors:['missing actual owner adapter or shared normalizer body']};
  const tables=fn.ops.flatMap((node,i)=>node.op===14?[{operator:i,offset:node.offset,targets:node.targets}]:[]);
  const perShapeStateArms=fn.ops.flatMap((node,i)=>node.op===4&&
    fn.ops[i-3]?.op===0x20&&fn.ops[i-3].local===30&&
    fn.ops[i-2]?.op===0x41&&fn.ops[i-1]?.op===0x46?[node.offset]:[]);
  const summary={index,bytes:fn.raw.length,operations:fn.ops.length,tables,perShapeStateArms,
    sharedIndex:binding.exported,sharedBytes:kernel.raw.length,
    ownerLoops:fn.ops.filter(node=>node.op===3).map(node=>node.offset),
    ownerStores:fn.ops.filter(node=>node.op>=0x36&&node.op<=0x3e).map(node=>node.offset),
    kernelLoops:kernel.ops.filter(node=>node.op===3).map(node=>node.offset),
    opaqueLowStores:kernel.ops.filter(node=>node.op===0x37&&node.memoryOffset===0).map(node=>node.offset),
    opaqueHighStores:kernel.ops.filter(node=>node.op===0x3b&&node.memoryOffset===8).map(node=>node.offset),
    descriptors:[],errors:[]};
  if(tables.length!==1)return summary;
  const{ops,edges}=fn,table=tables[0],check=(condition,message)=>{if(!condition)summary.errors.push(message);};
  const prefix=ops.slice(table.operator-3,table.operator);
  check(prefix.length===3&&prefix[0].op===0x20&&prefix[0].local===30&&
    prefix[1].op===0x41&&prefix[1].value===1&&prefix[2].op===0x6b,
    'the actual selector indexes shape IDs from the retained selected parameter');
  check(table.targets.length>2,'the authored unit has multiple real shape descriptor arms');
  const resumes=[];
  for(const target of edges[table.operator].slice(0,-1)){
    const arm=ops.slice(target,target+5),low=arm[0]?.value,high=arm[2]?.value;
    const valid=arm.length===5&&arm[0].op===0x42&&typeof low==='bigint'&&
      arm[1].op===0x21&&arm[1].local===31&&arm[2].op===0x42&&typeof high==='bigint'&&
      arm[3].op===0x21&&arm[3].local===32&&arm[4].op===12;
    check(valid,'each shape arm only assigns two I64 constants and branches to the shared kernel');
    if(!valid)continue;
    const lo=BigInt.asUintN(64,low),hi=BigInt.asUintN(64,high),cells=[],tags=[],effects=[];
    for(let logical=0;logical<8;logical++){
      const shift=BigInt(logical*8),payload=Number(lo>>shift&255n),effect=Number(hi>>shift&255n);
      const tag=payload>>>3;cells.push(payload&7);tags.push(tag);effects.push(effect);
      check(tag<20,'packed tag recipes only reference entry/class/value codes0..19');
      check((effect&15)<=8,'packed inherited dirty source is0..7 or exact None sentinel8');
      check(logical===0||(effect&224)===0,'only descriptor byte0 carries TOP delta bits');
    }
    resumes.push(edges[target+4][0]);
    summary.descriptors.push({lowHex:lo.toString(16).padStart(16,'0'),highHex:hi.toString(16).padStart(16,'0'),
      cells,tags,effects,topDeltaModulo8:Number(hi>>5n&7n)});
  }
  check(summary.descriptors.length===table.targets.length-1,'every explicit branch-table shape target has one constant descriptor');
  check(resumes.length>0&&new Set(resumes).size===1,'all constant descriptor arms converge before the same kernel body');
  const fallback=edges[table.operator].at(-1);
  check(ops[fallback]?.op===0,'the selector default rejects an impossible private shape ID before normalization');
  check(summary.ownerLoops.length===0&&summary.ownerStores.length===0,
    'the owner adapter contains no generic state loop or raw-bank write');
  check(ops.every(node=>[0,2,11,12,14,16,0x20,0x21,0x41,0x42,0x6b].includes(node.op))&&
    ops.filter(node=>node.op===0x21).every(node=>[31,32].includes(node.local)),
    'the owner contains only descriptor selection, unchanged argument forwarding and its shared call');
  const calls=ops.flatMap((node,i)=>[16,17,18,19,20,21].includes(node.op)?[i]:[]),call=calls[0];
  check(calls.length===1&&ops[call]?.op===16&&ops[call]?.call===binding.imported,
    'the owner adapter calls only the actual imported shared normalizer');
  const inputs=[...Array.from({length:30},(_,i)=>i),31,32],arguments_=ops.slice(call-inputs.length,call);
  check(arguments_.length===inputs.length&&arguments_.every((node,i)=>node.op===0x20&&node.local===inputs[i]),
    'the adapter forwards every immutable snapshot field followed by both selected descriptor constants');
  check(resumes.every(resume=>resume===call-inputs.length),'every descriptor arm converges at the same shared call argument list');
  check(ops[call+1]?.op===11&&ops[call+1].frame.kind===0xff,
    'the owner returns the shared result tuple without substituting or rewriting any field');
  const snapshot=[...Array(8).fill(0x7c),...Array(8).fill(0x7e),...Array(14).fill(0x7f)];
  check(JSON.stringify(signature(module,index))===JSON.stringify({parameters:[...snapshot,0x7f],results:snapshot}),
    'owner adapter retains the complete selected-shape31-input/30-output ABI');
  check(JSON.stringify(signature(support,binding.exported))===JSON.stringify({parameters:[...snapshot,0x7e,0x7e],results:snapshot}),
    'shared kernel takes the exact30-field snapshot and two explicit I64 descriptors');
  const kernelOps=kernel.ops;
  check(!kernelOps.some(node=>node.op===14),'the shared kernel contains no owner shape dispatch');
  check(!kernelOps.some(node=>[0x21,0x22].includes(node.op)&&node.local<24),
    'all eight native/low/high input cells stay immutable while masks, tags and aliases are reconstructed');
  check(!kernelOps.some(node=>[0x21,0x22].includes(node.op)&&[30,31].includes(node.local)),
    'both explicit descriptor parameters stay immutable throughout shared normalization');
  check(!kernelOps.some(node=>[16,17,18,19,20,21].includes(node.op)),'the shared kernel has no delegated or indirect per-shape body');
  const loop=kernelOps.find(node=>node.op===3)?.frame;
  if(loop){
    const before=kernelOps.slice(loop.open-2,loop.open),tail=kernelOps.slice(loop.end-7,loop.end);
    check(before.length===2&&before[0].op===0x41&&before[0].value===0&&before[1].op===0x21&&before[1].local===39,
      'the shared slot loop starts at logical slot0');
    check(tail.length===7&&tail[0].op===0x20&&tail[0].local===39&&tail[1].op===0x41&&tail[1].value===1&&
      tail[2].op===0x6a&&tail[3].op===0x22&&tail[3].local===39&&tail[4].op===0x41&&tail[4].value===8&&
      tail[5].op===0x47&&tail[6].op===13&&tail[6].frames.at(-1-tail[6].targets[0])===loop,
      'the single loop advances once through exactly eight logical slots');
    check(kernelOps.filter(node=>node.op>=0x36&&node.op<=0x3e).every(node=>node.frames.includes(loop)),
      'all raw-bank writes belong to the shared eight-slot loop');
  }else check(false,'the constant arms feed an actual per-CPU shared slot loop');
  return summary;
}

function inspectExitEpilogue(module,name) {
  const named=wanted=>[...module.names].find(([,name])=>name===wanted)?.[0];
  const index=named(name),fn=module.functions[index-module.imports.length];
  const publication=named('guest.publish'),bodies=new Set(instructionBodies(module).map(body=>body.index));
  const summary={name,index,publicationCalls:[],explicitReturns:[],sharedExitBranches:0,instructionBodyCalls:[],errors:[]};
  if(!fn){summary.errors.push('missing defined function');return summary;}
  const{ops,edges}=fn,check=(okay,message)=>{if(!okay)summary.errors.push(message);};
  ops.forEach((node,index)=>{
    if(Number.isInteger(publication)&&node.call===publication)summary.publicationCalls.push(index);
    if(bodies.has(node.call))summary.instructionBodyCalls.push({offset:node.offset,target:node.call});
    if(node.op===15)summary.explicitReturns.push(index);
  });
  if(summary.publicationCalls.length!==1){summary.errors.push('requires one actual publication call');return summary;}
  const publish=summary.publicationCalls[0],numeric=module.imports.some(item=>item.name==='numeric')&&
    module.globalImports.some(item=>item.name==='link.local76');
  const fields=[0,...Array.from({length:12},(_,i)=>i+2),...Array.from({length:6},(_,i)=>i+20),
    ...(numeric?Array.from({length:21},(_,i)=>i+76):[]),99,...(numeric?[100,101,102,103]:[])];
  const parameters=module.types[module.functionTypes[publication-module.imports.length]]?.parameters;
  check(parameters?.length===fields.length,'publisher type carries the complete retained state');
  const firstArgument=publish-fields.length,args=ops.slice(firstArgument,publish);
  check(args.length===fields.length&&args.every((node,i)=>node.op===0x20&&node.local===fields[i]),
    'the sole publication receives context and every live boundary local in ABI order');
  check(ops[publish].frames.length===1,'publication is outside every guest control block');
  const body=ops[0]?.op===2?ops[0].frame:undefined;
  if(!body||body.end>=firstArgument){summary.errors.push('publication must follow the closed outer exit block');return summary;}
  summary.outerBlock={open:body.open,end:body.end};
  summary.normalizationOperations=firstArgument-body.end-1;
  check(body.type===-64&&body.open===0,'one empty outer block wraps the original guest body');
  const serviceTests=ops.flatMap((node,i)=>node.op===4&&ops[i-1]?.op===0x20&&ops[i-1].local===157?[i]:[]);
  let serviceLoop,innerExit,serviceTest;
  if(Number.isInteger(named('guest.select_frame'))){
    check(serviceTests.length===1,'one actual service-resume test separates computed restart from terminal publication');
    if(serviceTests.length===1){
      serviceTest=ops[serviceTests[0]].frame;serviceLoop=ops[serviceTests[0]].frames.at(-1);
      innerExit=ops[serviceLoop?.open+1]?.frame;
      const resume=ops.slice(serviceTest.open+1,serviceTest.end);
      const resumeTail=resume.slice(-3),canonicalPrefix=resume.slice(0,-3);
      check(serviceLoop?.kind===3&&serviceLoop.type===-64&&
        ops[serviceLoop.open].frames.at(-1)===body&&innerExit?.kind===2&&innerExit.type===-64&&
        innerExit.open===serviceLoop.open+1&&innerExit.end<serviceTest.open,
        'the retained body exits one inner block to an in-body commit inside its bootstrap-once service loop');
      check(serviceTest.otherwise===undefined&&resumeTail.length===3&&resumeTail[0].op===0x41&&resumeTail[0].value===0&&
        resumeTail[1].op===0x21&&resumeTail[1].local===157&&resumeTail[2].op===12&&
        resumeTail[2].frames.at(-1-resumeTail[2].targets[0])===serviceLoop,
        'a requested restart clears its request and branches to the same service loop without re-running bootstrap');
      if(canonicalPrefix.length){
        const guard=canonicalPrefix[3]?.frame,imported=module.imports.findIndex(item=>item.module==='ring3'&&item.name==='numeric');
        const calls=ops.flatMap((node,i)=>node.op===16&&node.call===imported?[i]:[]),call=calls[0];
        check(canonicalPrefix[0]?.op===0x20&&canonicalPrefix[0].local===157&&
          canonicalPrefix[1]?.op===0x41&&canonicalPrefix[1].value===2&&canonicalPrefix[2]?.op===0x46&&
          canonicalPrefix[3]?.op===4&&guard?.type===-64&&guard.otherwise===undefined&&
          canonicalPrefix.at(-1)?.op===11&&canonicalPrefix.at(-1).frame===guard,
          'the added service is wholly inside the exact canonical-kind2 arm before the unchanged restart tail');
        check(imported>=0&&calls.length===1&&ops[call]?.frames.includes(guard)&&
          ops[call-2]?.op===0x20&&ops[call-2].local===0&&ops[call-1]?.op===0x20&&ops[call-1].local===172,
          'the sole canonical callback consumes the actual context and runtime request ordinal inside its service guard');
        summary.canonicalService={guardOpen:guard?.open,guardEnd:guard?.end,call,imported};
      }
      const terminal=ops.slice(serviceTest.end+1,serviceLoop.end);
      check(terminal.length===3&&terminal[0].op===0x20&&terminal[0].local===108&&
        terminal[1].op===0x21&&terminal[1].local===108&&terminal[2].op===12&&
        terminal[2].frames.at(-1-terminal[2].targets[0])===body,
        'terminal service completion forwards its saved reason to the sole outer publication');
      const commitCalls=ops.flatMap((node,i)=>node.call===named('guest.select_frame')?[i]:[]);
      check(commitCalls.length===1&&commitCalls[0]>innerExit.end&&commitCalls[0]<serviceTest.open,
        'the one in-body commit serves both terminal prefixes and computed restarts');
      summary.service={loopOpen:serviceLoop.open,innerOpen:innerExit.open,innerEnd:innerExit.end,resumeTest:serviceTest.open};
    }
  }
  const exits=[];
  for(let i=body.open+1;i<body.end;i++){
    const node=ops[i];
    if(node.local===108&&[0x20,0x21,0x22].includes(node.op)){
      if(node.op===0x20)check(serviceTest&&i===serviceTest.end+1,
        'only terminal service completion reads the reason inside the outer body');
      else{
        check(node.op===0x21,'the guest body only assigns the reason local');
        const next=ops[i+1],target=next?.frames.at(-1-next.targets[0]);
        check(next?.op===12&&(target===body||target===innerExit),
          'every assigned reason immediately branches to the exact shared exit or commit service');
      }
    }
    if([12,13,14].includes(node.op))for(const depth of node.targets){
      const target=node.frames.at(-1-depth);
      check(target?.kind!==0xff,'guest branches cannot bypass publication through the function label');
      if(target===body||target===innerExit){
        check(node.op===12&&ops[i-1]?.op===0x21&&ops[i-1]?.local===108,
          'every shared exit stores its reason before the unconditional outer branch');
        exits.push(i);
      }
    }
  }
  summary.sharedExitBranches=exits.length;
  check(exits.length>1,'multiple authored guest exits share the one publication site');
  const bare=summary.explicitReturns.filter(index=>index<body.end),tailReturn=ops.length-2;
  check(bare.length===1&&summary.explicitReturns.length===2&&summary.explicitReturns.at(-1)===tailReturn&&
    ops[tailReturn].frames.length===1,'only the invalid-context guard bypasses the single shared epilogue return');
  const rejected=bare[0],guard=ops[rejected]?.frames.at(-1);
  const prefix=guard?ops.slice(body.open+1,guard.open):[];
  const guardPattern=[0x20,0x28,0x41,0x47,0x20,0x29,0x42,0x52,0x72];
  check(guard?.kind===4&&guard.otherwise===undefined&&guard.end===rejected+1&&
    ops[rejected-1]?.op===0x41&&ops[rejected-1]?.value===8&&guard.open===rejected-2&&
    prefix.length===guardPattern.length&&prefix.every((node,i)=>node.op===guardPattern[i])&&
    prefix[0]?.local===0&&prefix[4]?.local===0&&prefix[1]?.memoryOffset===112&&
    prefix[2]?.value===1&&prefix[5]?.memoryOffset===120,
    'the sole bare return is the untouched active-context/identity guard returning Infrastructure');
  // Cut the real publisher node from the decoded CFG. Every valid-context path to a normal
  // return or implicit function end must disappear; otherwise publication can be bypassed.
  const reached=new Set(),pending=guard?[guard.end+1]:[];
  while(pending.length){const node=pending.pop();if(node===publish||reached.has(node))continue;
    reached.add(node);pending.push(...edges[node]);}
  check(![...reached].some(index=>ops[index].op===15||ops[index].op===11&&ops[index].frame.kind===0xff),
    'the sole publisher dominates all valid-context normal returns');
  const sentinel=exits.filter(index=>ops[index-2]?.op===0x41&&ops[index-2]?.value===-1);
  summary.precisionSentinels=sentinel.length;
  check(sentinel.length===0,'the sole body has no precision-tail sentinel');
  check(summary.instructionBodyCalls.length===0,'instruction bodies never call themselves or another precision copy');
  check(ops[publish+1]?.op===0x20&&ops[publish+1]?.local===108&&publish+2===tailReturn&&
    ops[tailReturn]?.op===15&&ops[tailReturn+1]?.frame?.kind===0xff,
    'the sole body returns its saved reason immediately after its one publication');
  return summary;
}

function inspectWidePoolCommit(owner,support) {
  const summary={errors:[],descriptors:[],loops:[]},check=(okay,message)=>{if(!okay)summary.errors.push(message);};
  if(!owner.imports.some(item=>item.name==='support.commit_pool'))return{...summary,errors:['missing actual selective committer']};
  const binding=supportBinding(owner,support,'support.commit_pool'),adapterIndex=named(owner,'guest.commit_frame');
  const adapter=defined(owner,adapterIndex),kernel=defined(support,binding.exported);
  const snapshot=[...Array(8).fill(0x7c),...Array(8).fill(0x7e),...Array(21).fill(0x7f),...Array(3).fill(0x7c)];
  check(JSON.stringify(signature(owner,adapterIndex))===JSON.stringify({parameters:[...snapshot,0x7f],results:[]}),
    'owner adapter takes the complete exact snapshot plus frame ID');
  check(JSON.stringify(signature(support,binding.exported))===JSON.stringify({parameters:[...snapshot,0x7e,0x7e],results:[]}),
    'shared committer takes the unchanged snapshot plus two descriptors and returns no shuffled tuple');
  if(!adapter||!kernel)return{...summary,errors:[...summary.errors,'missing actual defined commit functions']};
  const{ops,edges}=adapter,tables=ops.flatMap((node,i)=>node.op===14?[i]:[]);
  check(tables.length===1,'the owner selects descriptor constants through one real table');
  check(!ops.some(node=>node.op===3||node.op>=0x28&&node.op<=0x3e),'the owner adapter has no payload loop or memory access');
  const calls=ops.flatMap((node,i)=>node.op===16?[i]:[]);
  check(calls.length===1&&ops[calls[0]].call===binding.imported,'the owner calls only the actual shared committer');
  if(calls.length===1){const p=ops.slice(calls[0]-42,calls[0]);
    check(p.length===42&&p.every((node,i)=>node.op===0x20&&node.local===(i<40?i:i===40?41:42)),
      'the exact snapshot is forwarded unchanged with the selected two constants');}
  if(tables.length===1){const table=tables[0],p=ops.slice(table-3,table);
    check(p.length===3&&p[0].op===0x20&&p[0].local===40&&p[1].op===0x41&&p[1].value===1&&p[2].op===0x6b,
      'the descriptor table consumes the exact selected FrameState ID');
    for(const start of new Set(edges[table].slice(0,-1))){const arm=ops.slice(start,start+5);
      check(arm.length===5&&arm[0].op===0x42&&arm[1].op===0x21&&arm[1].local===41&&
        arm[2].op===0x42&&arm[3].op===0x21&&arm[3].local===42&&arm[4].op===12,
        'each actual frame arm selects constants without repeating payload work');
      if(arm[0]?.op===0x42&&arm[2]?.op===0x42)summary.descriptors.push({low:String(arm[0].value),high:String(arm[2].value)});
    }
  }
  const loops=kernel.ops.filter(node=>node.op===3);
  check(loops.length===2,'independent physical tag and payload masks each have one selective loop');
  for(const node of loops){
    const loop=node.frame,guard=node.frames.at(-1),body=kernel.ops.slice(loop.open+1,loop.end),tail=kernel.ops.slice(loop.end-7,loop.end);
    const scans=body.filter(op=>op.op===0x68);
    check(guard.kind===4&&guard.otherwise===undefined&&guard.open===loop.open-1&&
      kernel.ops[guard.open-1]?.op===0x20&&kernel.ops[guard.open-1].local===43,'zero work masks bypass their loop');
    check(scans.length===1,'one ctz selects the next changed physical bit');
    check(tail.length===7&&tail[0].op===0x20&&tail[0].local===43&&tail[1].op===0x20&&tail[1].local===43&&
      tail[2].op===0x41&&tail[2].value===1&&tail[3].op===0x6b&&tail[4].op===0x71&&tail[5].op===0x22&&
      tail[5].local===43&&tail[6].op===13&&tail[6].frames.at(-1-tail[6].targets[0])===loop,
      'the actual loop removes one set bit with mask&(mask-1), rather than scanning all eight slots');
    summary.loops.push({offset:node.offset,bitScans:scans.length});
  }
  check(!kernel.ops.some(node=>[16,17,18,19,20,21].includes(node.op)),'selective commit invokes no canonical or host helper');
  summary.adapterIndex=adapterIndex;summary.sharedIndex=binding.exported;summary.parameters=signature(support,binding.exported).parameters;
  return summary;
}

function inspectPoolCommit(owner,support) {
  const selectorIndex=named(owner,'guest.select_frame');
  if(!Number.isInteger(selectorIndex))return{...inspectWidePoolCommit(owner,support),transport:'wide-snapshot'};
  const summary={transport:'in-body',errors:[],descriptors:[],loops:[],writers:[]};
  const check=(okay,message)=>{if(!okay)summary.errors.push(message);};
  const selector=defined(owner,selectorIndex),run=defined(owner,named(owner,'guest.run'));
  check(JSON.stringify(signature(owner,selectorIndex))===JSON.stringify({parameters:[0x7f],results:[0x7e,0x7e]}),
    'the frame selector receives only FrameID and returns two descriptor constants');
  if(!selector||!run)return{...summary,errors:[...summary.errors,'missing actual selector or instruction body']};
  const {ops,edges}=selector,tables=ops.flatMap((node,i)=>node.op===14?[i]:[]);
  check(tables.length===1,'one real table selects exact frame descriptors');
  check(ops.every(node=>[0,2,11,12,14,0x20,0x21,0x41,0x42,0x6b].includes(node.op))&&
    ops.filter(node=>node.op===0x21).every(node=>[1,2].includes(node.local)),
    'the selector has only constant descriptor dispatch, without payload work, loops or delegated calls');
  if(tables.length===1){
    const table=tables[0],p=ops.slice(table-3,table);
    check(p.length===3&&p[0].op===0x20&&p[0].local===0&&p[1].op===0x41&&p[1].value===1&&p[2].op===0x6b,
      'descriptor dispatch consumes FrameID minus one');
    for(const start of new Set(edges[table].slice(0,-1))){const arm=ops.slice(start,start+5);
      check(arm.length===5&&arm[0].op===0x42&&arm[1].op===0x21&&arm[1].local===1&&
        arm[2].op===0x42&&arm[3].op===0x21&&arm[3].local===2&&arm[4].op===12,
        'each frame arm selects two constants without snapshot transport');
      if(arm[0]?.op===0x42&&arm[2]?.op===0x42)summary.descriptors.push({low:String(arm[0].value),high:String(arm[2].value)});
    }
  }
  check(ops.at(-3)?.op===0x20&&ops.at(-3).local===1&&ops.at(-2)?.op===0x20&&ops.at(-2).local===2,
    'the selected descriptors are returned unchanged');
  const selected=run.ops.flatMap((node,i)=>node.call===selectorIndex?[i]:[]);
  check(selected.length===1,'the actual instruction body has one shared in-body descriptor call');
  if(selected.length===1){const i=selected[0];
    check(run.ops[i-1]?.op===0x20&&run.ops[i-1].local===109&&
      run.ops[i+1]?.op===0x21&&run.ops[i+1].local===156&&run.ops[i+2]?.op===0x21&&run.ops[i+2].local===155,
      'the body passes only selected FrameID and retains both returned descriptors in its own locals');
  }
  for(const [name,parameters,wantedOffsets]of[
    ['support.commit_native',[0x7f,0x7f,0x7c,0x7f,0x7f],[0,20,24,28]],
    ['support.commit_raw',[0x7f,0x7f,0x7e,0x7f],[8,16,20,24,28]],
  ]){
    if(!owner.imports.some(item=>item.name===name)){check(false,`missing actual ${name}`);continue;}
    const binding=supportBinding(owner,support,name),fn=defined(support,binding.exported);
    check(JSON.stringify(signature(support,binding.exported))===JSON.stringify({parameters,results:[0x7f]}),
      `${name} receives one physical cell and returns its exact payload classification`);
    const stores=fn.ops.filter(node=>node.op>=0x36&&node.op<=0x3e);
    const pointer=fn.ops.slice(0,8),pointerLocal=parameters.length;
    check(pointer.length===8&&pointer[0].op===0x20&&pointer[0].local===0&&pointer[1].op===0x20&&
      pointer[1].local===1&&pointer[2].op===0x41&&pointer[2].value===5&&pointer[3].op===0x74&&
      pointer[4].op===0x6a&&pointer[5].op===0x41&&pointer[5].value===128&&pointer[6].op===0x6a&&
      pointer[7].op===0x21&&pointer[7].local===pointerLocal,
      `${name} addresses exactly pool +128 +physicalSlot*32`);
    check(JSON.stringify(stores.map(node=>node.memoryOffset).sort((a,b)=>a-b))===JSON.stringify(wantedOffsets),
      `${name} writes only one private physical payload record and its representation certificate`);
    check(!fn.ops.some(node=>[3,14,16,17,18,19,20,21].includes(node.op)),
      `${name} has no slot scan, source-cell dispatch or delegated call`);
    const calls=run.ops.flatMap((node,i)=>node.call===binding.imported?[i]:[]),cells=[];
    for(const i of calls){const p=run.ops.slice(i-parameters.length,i);
      const native=name==='support.commit_native',cell=p[2]?.local-(native?76:110);
      check(p.length===parameters.length&&p.every(node=>node.op===0x20)&&p[0].local===152&&p[1].local===167&&
        cell>=0&&cell<8&&(native?p[3].local===170&&p[4].local===87:p[3].local===118+cell),
        `${name} receives only the selected static source cell, physical destination and certificate`);
      const guard=run.ops[i].frames.at(-1),admission=guard?run.ops.slice(guard.open-3,guard.open):[];
      check(guard?.kind===4&&guard.type===-1&&admission.length===3&&admission[0].op===0x20&&
        admission[0].local===85&&admission[1].op===0x41&&admission[1].value===1<<cell&&admission[2].op===0x71,
        'the actual representation-valid bit chooses native versus exact opaque writes');
      cells.push(cell);
    }
    check(JSON.stringify(cells.slice().sort((a,b)=>a-b))===JSON.stringify([0,1,2,3,4,5,6,7]),
      `one shared router contains exactly one ${name} call per possible source cell`);
    summary.writers.push({name,...binding,...signature(support,binding.exported),calls: calls.map(i=>run.ops[i].offset),cells});
  }
  const routers=run.ops.flatMap((node,i)=>node.op===14&&run.ops[i-1]?.op===0x20&&
    run.ops[i-1].local===161?[i]:[]);
  check(routers.length===1&&JSON.stringify(run.ops[routers[0]]?.targets)===JSON.stringify([0,1,2,3,4,5,6,7,8]),
    'one source-cell router selects the eight static cases and an explicit invalid default');
  const loops=run.ops.filter(node=>node.op===3&&run.ops[node.frame.open+1]?.op===0x20&&
    run.ops[node.frame.open+1].local===158&&run.ops[node.frame.open+2]?.op===0x68);
  check(loops.length===2,'one in-body selective loop owns tags and one owns changed payloads');
  for(const node of loops){const loop=node.frame,guard=node.frames.at(-1),tail=run.ops.slice(loop.end-7,loop.end);
    check(guard?.kind===4&&guard.otherwise===undefined&&guard.open===loop.open-1&&
      run.ops[guard.open-1]?.op===0x20&&run.ops[guard.open-1].local===158,
      'an empty changed mask bypasses its actual commit loop');
    check(tail.length===7&&tail[0].op===0x20&&tail[0].local===158&&tail[1].op===0x20&&tail[1].local===158&&
      tail[2].op===0x41&&tail[2].value===1&&tail[3].op===0x6b&&tail[4].op===0x71&&tail[5].op===0x22&&
      tail[5].local===158&&tail[6].op===13&&tail[6].frames.at(-1-tail[6].targets[0])===loop,
      'the actual loop removes one changed bit rather than walking eight payloads');
    summary.loops.push({offset:node.offset,bitScans:1});
  }
  const snapshot=[...Array(8).fill(0x7c),...Array(8).fill(0x7e),...Array(21).fill(0x7f),...Array(3).fill(0x7c)];
  const coldIndex=named(owner,'guest.commit_frame_cold');
  if(!Number.isInteger(coldIndex)||!owner.imports.some(item=>item.name==='support.commit_pool_cold'))
    check(false,'the explicit cold canonical-observation adapter remains separate');
  else{
    const cold=supportBinding(owner,support,'support.commit_pool_cold');
    check(JSON.stringify(signature(owner,coldIndex))===JSON.stringify({parameters:[...snapshot,0x7f],results:[]})&&
      JSON.stringify(signature(support,cold.exported))===JSON.stringify({parameters:[...snapshot,0x7e,0x7e],results:[]}),
      'only the explicit cold helper path retains the exact41/42-field snapshot ABI');
    summary.cold={adapterIndex:coldIndex,...cold};
  }
  summary.selectorIndex=selectorIndex;
  return summary;
}

export function inspectRegion(bytes,supportBytes) {
  assert(supportBytes,'the structural proof receives actual per-CPU production support bytes');
  const module=readModule(bytes),support=readModule(supportBytes),runIndex=[...module.names].find(([,name])=>name==='guest.run')?.[0]??module.exports.get('run');
  assert(Number.isInteger(runIndex)&&runIndex>=module.imports.length,'defined guest run');
  const run=module.functions[runIndex-module.imports.length];
  const legacy=module.imports.some(item=>item.name==='support.normalize_kernel');
  const binding=legacy?supportBinding(module,support,'support.normalize_kernel'):undefined;
  const kernelProof=binding?guardedNativeNormalizer(support,binding.exported):undefined;
  const adapter=named(module,'guest.normalize_frame');
  const conditionalNative=new Map(kernelProof?[[adapter,{...kernelProof,adapterIndex:adapter,scope:'per-CPU support'}]]:[]);
  const materializers=new Set(module.imports.flatMap((item,index)=>
    ['numeric','support.materialize_numeric'].includes(item.name)?[index]:[]));
  const rawStore=node=>node.op===0x37&&node.memoryOffset===0||node.op===0x3b&&node.memoryOffset===8;
  module.functions.forEach((fn,index)=>{
    const id=index+module.imports.length;
    if(!conditionalNative.has(id)&&fn.ops.some(node=>node.op===0x37&&node.memoryOffset===0)&&
      fn.ops.some(node=>node.op===0x3b&&node.memoryOffset===8))materializers.add(id);
  });
  let changed;
  do{changed=false;module.functions.forEach((fn,index)=>{
    const id=index+module.imports.length;
    if(id!==runIndex&&!materializers.has(id)&&fn.ops.some(node=>materializers.has(node.call))){materializers.add(id);changed=true;}
  });}while(changed);
  const reachable=new Set(),pending=[0];
  while(pending.length){const node=pending.pop();if(!reachable.has(node)){reachable.add(node);pending.push(...run.edges[node]);}}
  const arithmetic=node=>node.op>=0xa0&&node.op<=0xa3;
  const cycles=exclude=>components(run.edges,new Set([...reachable].filter(index=>!exclude(run.ops[index]))))
    .filter(group=>group.some(index=>arithmetic(run.ops[index])))
    .map(group=>({operations:group.length,arithmetic:group.filter(index=>arithmetic(run.ops[index])).length}));
  const canonical=node=>rawStore(node)||materializers.has(node.call);
  const dispatch=node=>node.op===14||module.names.get(node.call)==='guest.lookup';
  const rotations=new Set();
  for(let start=0;start+17<run.ops.length;start++) {
    const group=run.ops.slice(start,start+18),first=group[0].local;
    const direction=first===76?1:first===83?-1:0;
    if(!direction||group[0].op!==0x20||group[1].op!==0x21||group[1].local!==97)continue;
    if(!Array.from({length:7},(_,i)=>i).every(i=>group[2+i*2].op===0x20&&
      group[2+i*2].local===first+direction*(i+1)&&group[3+i*2].op===0x21&&
      group[3+i*2].local===first+direction*i))continue;
    if(group[16].op===0x20&&group[16].local===97&&group[17].op===0x21&&
      group[17].local===first+direction*7)rotations.add(run.ops[start]);
  }
  const eagerLocals=[10,26,38,42,89,90,91,93,94];
  const eagerFrame=node=>[0x21,0x22].includes(node.op)&&eagerLocals.includes(node.local);
  const admissions=new Set(run.ops.filter((_,index)=>controlAdmission(run.ops,index)));
  const bodies=instructionBodies(module);
  return {runIndex,runName:module.names.get(runIndex),locals:run.locals,instructionBodies:bodies,
    fpLinkGlobals:module.globalImports.filter(item=>/^link\.local(?:7[6-9]|8[0-9]|9[0-6]|10[0-3])$/.test(item.name)),
    poolCommit:inspectPoolCommit(module,support),
    legacyNormalizers:[...module.names.values(),...support.names.values()].filter(name=>
      name==='guest.normalize_frame'||name==='support.normalize_kernel'),
    commitPoolSignature:module.imports.some(item=>['support.commit_pool','support.commit_pool_cold'].includes(item.name))?(()=>{
      const name=module.imports.some(item=>item.name==='support.commit_pool_cold')?'support.commit_pool_cold':'support.commit_pool';
      const{imported,exported}=supportBinding(module,support,name);
      const fn=defined(support,exported);
      return{index:exported,ownerImport:imported,...signature(support,exported),
        loops:fn.ops.filter(node=>node.op===3).map(node=>({offset:node.offset,
          bitScans:fn.ops.slice(node.frame.open,node.frame.end).filter(op=>op.op===0x68).length}))};
    })():undefined,
    portRouting:inspectPortRouting(run),
    nativeFloatStores:run.ops.flatMap((node,index)=>{
      const single=node.op===0x38,p=run.ops.slice(index-(single?3:2),index);
      return(single||node.op===0x39)&&node.memoryOffset===0&&p[0]?.op===0x20&&p[0].local===52&&
        p[1]?.op===0x20&&p[1].local===48&&(!single||p[2]?.op===0xb6)?
        [{offset:node.offset,format:single?'f32':'f64',pointerLocal:52,valueLocal:48}]:[];
    }),
    arithmeticCycles:cycles(()=>false),withoutCanonicalization:cycles(canonical),
    withoutCanonicalizationOrDispatch:cycles(node=>canonical(node)||dispatch(node)),
    runtimeStackRotations:[...rotations].map(node=>node.offset),
    eagerFrameWrites:Object.fromEntries(eagerLocals.map(local=>[local,
      run.ops.filter(node=>[0x21,0x22].includes(node.op)&&node.local===local).length])),
    withoutCanonicalizationDispatchOrStackRotation:cycles(node=>canonical(node)||dispatch(node)||rotations.has(node)),
    withoutCanonicalizationDispatchOrDynamicFrame:cycles(node=>canonical(node)||dispatch(node)||rotations.has(node)||eagerFrame(node)),
    withoutCanonicalizationDispatchOrEagerControl:cycles(node=>canonical(node)||dispatch(node)||rotations.has(node)||
      [0x21,0x22].includes(node.op)&&[10,26].includes(node.local)),
    controlAdmissionSites:[...admissions].map(node=>node.offset),
    withoutCanonicalizationDispatchOrControlAdmission:cycles(node=>canonical(node)||dispatch(node)||rotations.has(node)||admissions.has(node)),
    conditionalNativeCanonicalizers:[...conditionalNative.values()],
    normalizerKernel:legacy?inspectNormalizerKernel(module,support):undefined,
    materializers:[...materializers].map(index=>({index,name:module.names.get(index)??module.imports[index]?.name})),
    materializerSignature:['support.materialize_numeric'].map(name=>{
      const{imported,exported}=supportBinding(module,support,name);return{index:exported,ownerImport:imported,name,...signature(support,exported)};
    }),
    statusResolverSignature:['support.resolve_status'].map(name=>{
      const{imported,exported}=supportBinding(module,support,name);return{index:exported,ownerImport:imported,name,...signature(support,exported)};
    }),
    pageTranslatorSignature:['support.translate_page'].map(name=>{
      const{imported,exported}=supportBinding(module,support,name);return{index:exported,ownerImport:imported,name,...signature(support,exported)};
    }),
    exitEpilogues:['guest.run'].map(name=>inspectExitEpilogue(module,name)),
    limits:['narrow decoder for generated authored modules; scalar f64 arithmetic cycles only',
      'a retained cycle proves a canonicalization-free emitted path, not its runtime frequency or speed']};
}

export function inspectPortDepth(bytes) {
  const module=readModule(bytes);
  assert.equal(instructionBodies(module).length,1,'one actual architectural instruction body serves normal, quota and interior entries');
  return Object.fromEntries(['guest.run'].map(name=>{
    const index=[...module.names].find(([,value])=>value===name)?.[0];
    assert(Number.isInteger(index)&&index>=module.imports.length,`actual defined ${name}`);
    const fn=module.functions[index-module.imports.length];
    return[name,{maxDepth:fn.ops.reduce((maximum,node)=>Math.max(maximum,node.frames.length-1),0),
      branchTables:fn.ops.filter(node=>node.op===14).length,operators:fn.ops.length}];
  }));
}

export function assertAnalyzedStateStructure(proof) {
  assertSingleInstructionBody(proof);
  const witness=proof.nativeStateWitness;
  assert(witness,'the actual native loop supplies a linked pre-publication state witness');
  assert.equal(witness.normalizer_calls,0,'physical ownership retires the actual recurring eight-slot normalizer');
  assert.equal(witness.wide_snapshot_calls,0,
    'the actual ordinary loop exit never transports its unused FP cells through a wide snapshot call');
  assert(proof.withoutCanonicalizationDispatchOrStackRotation.length>0,
    'the balanced arithmetic loop repeats without per-push/pop eight-cell runtime rotations');
  assertRegionStructure(proof);
  assert(proof.withoutCanonicalizationDispatchOrEagerControl.length>0,
    'the balanced arithmetic loop also bypasses per-instruction PC/index writes');
  assert(proof.withoutCanonicalizationDispatchOrControlAdmission.length>0,
    'the balanced arithmetic loop reuses admitted control facts across its backedge');
  assert(witness.record_writes>0,'the executed body really commits its private physical pool');
  assert(witness.record_writes<=6*(8+2),'only the initial import and two changed ghost slots write private records');
  assert.equal(witness.raw80_stores,0,'native internal normalization does not execute raw80 stores');
  assert.equal(witness.materializer_calls,0,'native internal edges do not invoke the canonical materializer');
  assert(witness.control_admissions>0&&witness.control_admissions<=witness.body_entries,
    'the exercised control admission runs at most once per fresh import into the sole body');
  assert.equal(witness.afterBankHex,witness.beforeBankHex,'the full private raw80 bank stays unchanged before explicit publication');
  assert(witness.fp_metadata_writes<12,
    'metadata is reconstructed at analyzed frame boundaries instead of all twelve executed x87 operations');
}

export function assertRegionStructure(proof) {
  assertSingleInstructionBody(proof);
  assert.deepEqual(proof.legacyNormalizers,[],'no generic eight-slot normalizer remains encoded in owner or shared support');
  assert.deepEqual(proof.fpLinkGlobals,[],'physical FP state is not serialized through a cross-owner tuple');
  assert.equal(proof.poolCommit.transport,'in-body','ordinary commits select live cells within the actual instruction body');
  assert.deepEqual(proof.poolCommit.errors,[],'real descriptors feed changed-bit loops and small typed physical record writers');
  assert.equal(proof.poolCommit.writers.length,2,'the shared native and opaque writers commit only their selected cells');
  assert.equal(proof.poolCommit.loops.length,2,'independent tag and payload write masks are consumed in the actual body');
  assert(proof.commitPoolSignature,'the explicit cold canonical-observation committer remains bound to actual shared support');
  assert.equal(proof.commitPoolSignature.parameters.length,42,'only the cold committer retains the exact snapshot ABI');
  assert.deepEqual(proof.commitPoolSignature.results,[],'the cold committer publishes directly to the private pool');
  assert(proof.commitPoolSignature.loops.some(loop=>loop.bitScans>0),'cold commit still selects changed physical slots by set bits');
  assert(proof.arithmeticCycles.length>0,'the actual run contains a cyclic f64 arithmetic path');
  assert(proof.withoutCanonicalization.length>0,'an arithmetic loop can repeat without raw80 stores or materializing helpers');
  assert(proof.withoutCanonicalizationOrDispatch.length>0,'the reducible arithmetic loop also bypasses the general block dispatcher');
  assert.equal(proof.materializerSignature.length,1,'one actual per-CPU numerical materializer');
  assert.deepEqual(proof.materializerSignature[0].parameters,[0x7f],
    'the observation materializer receives the private bank/pool owner pointer');
  assert.deepEqual(proof.materializerSignature[0].results,[],'materialization has no result');
  assert.equal(proof.statusResolverSignature.length,1,'one actual per-CPU exact status resolver');
  assert.deepEqual(proof.statusResolverSignature[0].parameters,[0x7f,0x7f,0x7c,0x7c,0x7c],
    'status resolution receives the recipe, prior status, both exact operands and rounded result');
  assert.deepEqual(proof.statusResolverSignature[0].results,[0x7f],'status resolution returns the exact architectural status');
  assert.equal(proof.pageTranslatorSignature.length,1,'one actual per-CPU page translation helper');
  assert.deepEqual(proof.pageTranslatorSignature[0].parameters,[0x7f,0x7f,0x7f],
    'translation receives context, guest linear address and requested permissions');
  assert.deepEqual(proof.pageTranslatorSignature[0].results,[0x7f,0x7f,0x7f],
    'translation returns the physical slot, page base and complete permission mask');
  for(const exit of proof.exitEpilogues)assert.deepEqual(exit.errors,[],`${exit.name}: complete common-exit control-flow proof`);
}

function assertSingleInstructionBody(proof) {
  assert.equal(proof.instructionBodies.length,1,
    'one actual architectural instruction body serves normal, quota and interior entries without duplicated fast/precise lowering');
  assert.deepEqual(proof.instructionBodies.map(body=>[body.index,body.name]),[[proof.runIndex,'guest.run']],
    'the sole guarded architectural body is the actual named guest.run');
  assert.deepEqual(proof.portRouting.errors,[],
    'full ordinary entries bypass the actual port selectors and per-prefix quota guards in the same instruction body');
}
