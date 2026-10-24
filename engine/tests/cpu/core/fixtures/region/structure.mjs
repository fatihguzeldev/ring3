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
    else if(op===0x23||op===0x24)node.global=r.unsigned();
    else if(op===20||op===21||op===0x25||op===0x26||op===0xd2)r.unsigned();
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
  const owned=module.globalImports.some(item=>item.module==='ring3'&&item.name==='link.continuation');
  assert.deepEqual(signature(module,imported),{parameters:Array(owned?3:2).fill(0x7f),results:[0x7f]},
    owned?'the owned canonical boundary consumes context, actual PC and ordinal'
      :'the pinned legacy canonical boundary consumes context and ordinal');
  const sites=module.functions.flatMap((fn,offset)=>fn.ops.flatMap(node=>node.op===16&&node.call===imported?
    [{function:offset+module.imports.length,offset:node.offset}]:[]));
  return{body:bodies[0].index,imported,owned,sites,ownerBytes:bytes.length,
    bodyOperators:defined(module,bodies[0].index).ops.length};
}

function inspectPortRouting(fn,module) {
  const precise=module.imports.findIndex(item=>item.module==='ring3'&&item.name==='precise');
  return precise<0?inspectLegacyPortRouting(fn):inspectHotSpanRouting(fn,module,precise);
}

function inspectExactSuffixService(fn,module,precise) {
  const {ops}=fn,errors=[],check=(okay,message)=>{if(!okay)errors.push(message);};
  const local=(node,op,index)=>node?.op===op&&node.local===index;
  const constant=(node,value)=>node?.op===0x41&&node.value===value;
  const immediate=node=>node?.local??node?.global??node?.call??node?.value??node?.memoryOffset;
  const sequence=(nodes,wanted)=>nodes.length===wanted.length&&wanted.every(([op,value],i)=>
    nodes[i]?.op===op&&(value===undefined||immediate(nodes[i])===value));
  const exact=frame=>frame?.kind===4&&local(ops[frame.open-3],0x20,157)&&
    constant(ops[frame.open-2],3)&&ops[frame.open-1]?.op===0x46;
  const mask=(frame,bit)=>frame?.kind===4&&local(ops[frame.open-3],0x20,175)&&
    constant(ops[frame.open-2],bit)&&ops[frame.open-1]?.op===0x71;
  const imported=name=>module.imports.findIndex(item=>item.module==='ring3'&&item.name===name);
  const children=(parent,from=parent.open,to=parent.end)=>ops.flatMap(node=>node.op===4&&
    node.frame.open>from&&node.frame.open<to&&node.frames.at(-1)===parent?[node.frame]:[]);
  const sites=ops.flatMap((node,i)=>node.op===16&&node.call===precise?[i]:[]);
  const expected=module.imports.some(item=>item.module==='ring3'&&item.name==='numeric')?1:0;
  check(sites.length===expected,'the numerical body has exactly one shared exact-suffix callsite; integer bodies have none');
  if(sites.length!==1)return{errors,sites,publications:[],entries:[]};
  const numeric=imported('numeric'),numericSites=ops.flatMap((node,i)=>node.op===16&&node.call===numeric?[i]:[]);
  const combined=numericSites.length===1,call=sites[0],choice=combined?ops[call].frames.findLast(exact):undefined;
  const shared=combined?choice&&ops[choice.open].frames.at(-1):ops[call].frames.findLast(frame=>
    frame.kind===4&&local(ops[frame.open-3],0x20,157)&&constant(ops[frame.open-2],2)&&ops[frame.open-1]?.op===0x4f);
  const service=shared&&ops[shared.open].frames.at(-1);
  check((!combined||choice?.type===-1)&&shared?.kind===4&&shared.type===-64&&shared.otherwise===undefined&&
    local(ops[shared.open-3],0x20,157)&&constant(ops[shared.open-2],2)&&ops[shared.open-1]?.op===0x4f&&
    service?.kind===4&&local(ops[service.open-1],0x20,157),
    'only kind3 selects precise inside the shared SERVICE>=2 callback arm');
  if(!shared||(combined&&!choice))return{errors,sites,publications:[],entries:[]};
  const numericCall=numericSites[0];
  check(numericSites.length===(combined?1:0)&&(!combined||choice.otherwise===call+1&&call===choice.open+2&&
    local(ops[call-1],0x20,0)&&numericCall===choice.otherwise+4&&choice.end===numericCall+1&&
    sequence(ops.slice(choice.otherwise+1,numericCall),[[0x20,0],[0x20,173],[0x20,172]])&&
    local(ops[choice.end+1],0x21,19)&&JSON.stringify(signature(module,numeric))===
      JSON.stringify({parameters:[0x7f,0x7f,0x7f],results:[0x7f]})),
    'one result choice calls precise(Context) or the unchanged owned numeric(Context,actualPC,ordinal)');
  const resultEnd=combined?choice.end:call;
  check(local(ops[call-1],0x20,0)&&local(ops[resultEnd+1],0x21,19),
    'the actual precise callback consumes Context and saves its one real stop reason');
  const setup=combined?ops[shared.open+4]?.frame:undefined,valid=module.globalImports.findIndex(item=>item.name==='link.valid');
  const setupStart=combined?setup?.open+1:shared.open+1,setupEnd=combined?setup?.end:setupStart+12;
  check((!combined||exact(setup)&&setup.otherwise===undefined)&&sequence(ops.slice(setupStart,setupEnd),[
    [0x41,0x38d5ffff],[0x21,175],[0x20,10],[0x21,173],[0x41,0],[0x24,valid],
    [0x20,0],[0x20,13],[0x36,40],[0x20,0],[0x20,12],[0x36,44],
  ]),'kind3 alone selects the complete exact transfer mask, actual PC/counts and canonical link authority');
  const materialize=imported('support.materialize_numeric'),materializeSites=ops.flatMap((node,i)=>
    node.op===16&&node.call===materialize&&i>shared.open&&i<shared.end?[i]:[]);
  const materializeCall=materializeSites[0],materializeGuard=combined?ops[materializeCall]?.frames.at(-1):undefined;
  check(materializeSites.length===1&&(combined?materializeGuard?.kind===4&&materializeGuard.otherwise===undefined&&
    sequence(ops.slice(materializeGuard.open-4,materializeGuard.open),
      [[0x20,175],[0x41,1<<30],[0x71],[0x45]])&&
    sequence(ops.slice(materializeGuard.open+1,materializeGuard.end),[[0x20,0],[0x28,92],[16,materialize]])&&
    ops[materializeGuard.open].frames.at(-1)===shared:
    materializeCall===setupEnd+2&&sequence(ops.slice(setupEnd,materializeCall+1),[[0x20,0],[0x28,92],[16,materialize]])&&
    ops[materializeCall].frames.at(-1)===shared),
    'the actual pool materializer is conditional on PRESERVE for shared callbacks, or unconditional for exact-only service');
  const resetStart=combined?materializeGuard?.end+1:materializeCall+1;
  const reset=ops.slice(resetStart,resetStart+20);
  check(sequence(reset,[...[84,85,86,153,154,178,100].flatMap(index=>[[0x41,0],[0x21,index]]),
    [0x42,0n],[0x21,177],[0x41,1],[0x21,126],[0x41,0],[0x21,109]]),
    'the shared callback drops only its local FP view and selects an exact fresh continuation');
  const inputs=children(shared,resetStart+19,combined?choice.open:call);
  const flagsInput=inputs.find(frame=>local(ops[frame.open-1],0x22,14));
  const flags=imported('support.flags');
  check(Number.isInteger(flagsInput?.open)&&flagsInput.otherwise===undefined&&sequence(ops.slice(flagsInput.open-6,flagsInput.open),
    [[0x20,175],[0x41,16],[0x76],[0x41,0xfff],[0x71],[0x22,14]])&&
    sequence(ops.slice(flagsInput.open+1,flagsInput.end),[
      ...[11,20,25,21,22,23,24,14].map(index=>[0x20,index]),[16,flags],[0x21,11],
      [0x20,0],[0x20,11],[0x36,36],
    ]),'the shared flags publication materializes exactly the runtime transfer mask and unchanged lazy tuple');
  const maskFrame=(parent,frames,bit,wanted,label)=>{
    const found=frames.filter(frame=>mask(frame,bit));
    check(found.length===1&&found[0].otherwise===undefined&&ops[found[0].open].frames.at(-1)===parent&&
      sequence(ops.slice(found[0].open+1,found[0].end),wanted),label);return found[0];
  };
  maskFrame(shared,inputs,1<<28,[[0x41,0],[0x21,20]],'flags-output callbacks clear only the old lazy kind');
  for(let register=0;register<8;register++)maskFrame(shared,inputs,1<<register,
    [[0x20,0],[0x20,register+2],[0x36,register*4]],'each GPR input is published exactly under its own transfer bit');
  maskFrame(shared,inputs,1<<29,[[0x20,0],[0x20,173],[0x36,32]],
    'generic callbacks receive the actual service PC while specialized callbacks retain selective Context semantics');
  const callbackStart=combined?choice.open-3:call-1;
  check(sequence(ops.slice(callbackStart-4,callbackStart),[[0x20,0],[0x28,80],[0x20,99],[0x37,0]])&&
    sequence(ops.slice(resultEnd+2,resultEnd+14),[[0x20,0],[0x28,80],[0x29,0],[0x21,99],
      ...[104,132,133,150].flatMap(index=>[[0x41,0],[0x21,index]])]),
    'the actual callback is immediately bracketed by transaction-authority flush/reload and complete translation-cache invalidation');
  const normalized=ops[resultEnd+17]?.frame;
  check(sequence(ops.slice(resultEnd+14,resultEnd+17),[[0x20,19],[0x41,10],[0x4b]])&&
    normalized?.kind===4&&normalized.otherwise===undefined&&
    sequence(ops.slice(normalized.open+1,normalized.end),[[0x41,8],[0x21,19]]),
    'invalid native stop reasons normalize to Infrastructure before outputs or reentry');
  const completed=combined?ops.findIndex((node,i)=>i>resultEnd&&i<shared.end&&local(node,0x21,14)):-1;
  check(!combined||completed>=0&&sequence(ops.slice(completed-10,completed+1),[
    [0x20,19],[0x45],[0x20,19],[0x41,6],[0x46],[0x72],[0x20,19],[0x41,10],[0x46],[0x72],[0x21,14],
  ]),'canonical completion remains exactly success0, committed SMC6 or system entry10');
  const reload=combined?ops[completed+6]?.frame:undefined;
  check(!combined||sequence(ops.slice(completed+1,completed+6),[[0x20,157],[0x41,3],[0x46],[0x20,14],[0x72]])&&
    reload?.kind===4&&reload.otherwise===undefined,'all exact outcomes, or completed canonical outcomes only, reload declared outputs');
  const outputParent=combined?reload:shared,outputStart=combined?reload?.open:normalized?.end;
  const outputFrames=outputParent?children(outputParent,outputStart??outputParent.open):[];
  if(outputParent){
    for(let register=0;register<8;register++)maskFrame(outputParent,outputFrames,1<<(8+register),
      [[0x20,0],[0x28,register*4],[0x21,register+2]],'each declared GPR output reloads the actual callback value');
    maskFrame(outputParent,outputFrames,1<<28,[[0x20,0],[0x28,36],[0x21,11],[0x41,0],[0x21,20]],
      'declared flags outputs reload the actual canonical FLAGS and clear its lazy kind');
    check(!combined||!ops.slice(reload.open,reload.end).some(node=>[0x21,0x22].includes(node.op)&&node.local===14),
      'output reload cannot overwrite the saved canonical-completion predicate');
  }
  const lastOutput=outputFrames.filter(frame=>mask(frame,1<<28)).at(-1);
  const post=combined&&reload?ops[reload.end+4]?.frame:undefined;
  check(!combined||exact(post)&&Number.isInteger(post.otherwise),'kind3 and canonical completion retain distinct post-callback accounting');
  const postStart=combined?post?.open+1:lastOutput?.end+1,postEnd=combined?post?.otherwise:shared.end;
  if(!Number.isInteger(postStart))return{errors,sites,publications:[],entries:[],guardOpen:shared.open,guardEnd:shared.end};
  check(sequence(ops.slice(postStart,postStart+9),[[0x20,0],[0x28,32],[0x21,10],
    [0x20,0],[0x28,40],[0x21,13],[0x20,0],[0x28,44],[0x21,12]]),
    'exact completion reloads actual PC, retired count and live remaining quota on every outcome');
  const marker=ops[postStart+10]?.local;
  check(Number.isInteger(marker)&&marker>=179&&sequence(ops.slice(postStart+9,postStart+19),
    [[0x41,0],[0x21,marker],[0x41,0],[0x21,129],[0x41,0],[0x21,109],[0x41,1],[0x21,126],[0x41,0],[0x21,157]]),
    'exact completion clears the typed ingress marker, GP proofs and selected frame before any reentry');
  const retry=ops.findIndex((node,i)=>i>=postStart&&i<postEnd&&local(node,0x20,19)&&
    constant(ops[i+1],1)&&ops[i+2]?.op===0x46);
  check(retry>=0,'only a real NeedCode result can request private hot reentry');
  const entries=[];
  function tree(at) {
    if(constant(ops[at],0))return at+1;
    const equal=ops[at+3]?.frame,less=equal&&ops[equal.otherwise+4]?.frame;
    const pc=ops[at+1]?.value,work=ops[at+5]?.value;
    if(!(local(ops[at],0x20,10)&&Number.isInteger(pc)&&ops[at+1]?.op===0x41&&ops[at+2]?.op===0x46&&
      ops[at+3]?.op===4&&equal?.type===-1&&Number.isInteger(equal.otherwise)&&
      local(ops[at+4],0x20,12)&&ops[at+5]?.op===0x41&&work>0&&ops[at+6]?.op===0x4f&&
      equal.otherwise===at+7&&local(ops[equal.otherwise+1],0x20,10)&&
      constant(ops[equal.otherwise+2],pc)&&ops[equal.otherwise+3]?.op===0x49&&
      ops[equal.otherwise+4]?.op===4&&less?.type===-1&&Number.isInteger(less.otherwise)))return null;
    const left=tree(less.open+1);if(left!==less.otherwise)return null;
    entries.push({pc:pc>>>0,instructions:work});
    const right=tree(less.otherwise+1);if(right!==less.end||equal.end!==less.end+1)return null;
    return equal.end+1;
  }
  let afterTree=retry>=0?tree(retry+3):null,reusedHotEntry=false;
  const hotEntry=named(module,'guest.hot_entry');
  if(retry>=0&&sequence(ops.slice(retry+3,retry+6),[[0x20,10],[0x20,12],[16,hotEntry]])) {
    const hot=defined(module,hotEntry),h=hot?.ops;let at=0;
    const count=()=>{
      if(constant(h?.[at],0)){at++;return true;}
      const start=at,equal=h?.[at+3]?.frame,less=equal&&h[equal.otherwise+4]?.frame;
      const pc=h?.[at+1]?.value,work=h?.[at+4]?.value;
      if(!(local(h?.[at],0x20,0)&&h?.[at+1]?.op===0x41&&h?.[at+2]?.op===0x46&&
        equal?.type===-1&&equal.otherwise===start+5&&h?.[start+4]?.op===0x41&&work>0&&
        local(h?.[equal.otherwise+1],0x20,0)&&constant(h?.[equal.otherwise+2],pc)&&
        h?.[equal.otherwise+3]?.op===0x49&&less?.type===-1))return false;
      at=less.open+1;if(!count()||at!==less.otherwise)return false;
      entries.push({pc:pc>>>0,instructions:work});at=less.otherwise+1;
      if(!count()||at!==less.end||equal.end!==less.end+1)return false;
      at=equal.end+1;return true;
    };
    const counted=count();
    check(counted&&sequence(h?.slice(at)??[],[[0x22,2],[0x41,0],[0x47],[0x20,1],[0x20,2],[0x4f],[0x71],[11]])&&
      JSON.stringify(signature(module,hotEntry))===JSON.stringify({parameters:[0x7f,0x7f],results:[0x7f]})&&
      hot?.locals.length===1&&hot.locals[0].count===1&&hot.locals[0].type===0x7f,
      'shared public/private eligibility reuse calls only the exact PC/minimum-fuel pure decision tree');
    afterTree=retry+6;
    reusedHotEntry=true;
  }
  const reentry=afterTree===null?undefined:ops[afterTree+1]?.frame;
  check(afterTree!==null&&ops[afterTree]?.op===0x71&&ops[afterTree+1]?.op===4&&reentry?.type===-64&&
    Number.isInteger(reentry.otherwise)&&entries.every((entry,i)=>i===0||entries[i-1].pc<entry.pc),
    'private reentry requires an exact certified PC and its live minimum fuel through one ordered decision tree');
  const lookup=named(module,'guest.lookup');let lookupCalls=0;
  const dynamic=(from,to,input)=>{
    const prefix=[[0x20,input],[0x21,10]],port=[
      [0x20,10],[16,lookup],[0x22,26],[0x41,14],[0x76],[0x21,130],
      [0x20,26],[0x41,16383],[0x71],[0x41,-1],[0x20,26],[0x41,-1],[0x47],[0x1b],[0x21,26],
    ],nodes=ops.slice(from,to);
    if(sequence(nodes,prefix))return true;
    if(sequence(nodes,[...prefix,...port])){lookupCalls++;return true;}
    return false;
  };
  if(reentry){
    check(dynamic(reentry.open+1,reentry.otherwise-2,10)&&
      constant(ops[reentry.otherwise-2],1)&&local(ops[reentry.otherwise-1],0x21,157)&&
      local(ops[reentry.otherwise+1],0x20,19)&&local(ops[reentry.otherwise+2],0x21,108)&&
      ops[reentry.otherwise+3]?.op===12&&reentry.end===reentry.otherwise+4&&postEnd===reentry.end+1,
      'eligible private reentry requests the existing service loop; every other precise result terminates with its actual reason');
  }
  if(combined&&post){
    const completedArm=ops[post.otherwise+2]?.frame,stopped=completedArm&&ops[completedArm.open+12]?.frame;
    check(local(ops[post.otherwise+1],0x20,14)&&completedArm?.kind===4&&Number.isInteger(completedArm.otherwise)&&
      sequence(ops.slice(completedArm.open+1,completedArm.open+11),[
        [0x20,12],[0x41,1],[0x6b],[0x21,12],[0x20,13],[0x41,1],[0x6a],[0x21,13],[0x20,174],[0x21,10],
      ])&&local(ops[completedArm.open+11],0x20,19)&&stopped?.kind===4&&stopped.otherwise===undefined&&
      sequence(ops.slice(stopped.open+1,stopped.end),[[0x41,0],[0x21,157],[0x20,19],[0x21,108],[12]])&&
      dynamic(stopped.end+1,completedArm.otherwise-4,174)&&
      sequence(ops.slice(completedArm.otherwise-4,completedArm.otherwise),[[0x41,0],[0x21,129],[0x41,1],[0x21,157]])&&
      sequence(ops.slice(completedArm.otherwise+1,completedArm.end),[
        [0x20,173],[0x21,10],[0x41,0],[0x21,157],[0x20,19],[0x21,108],[12],
      ])&&post.end===completedArm.end+1&&shared.end===post.end+1,
      'canonical0/6/10 alone retire one instruction; success resumes at NEXT, committed stops terminate, and failures retain the request PC');
  }
  const calls=ops.flatMap((node,i)=>node.op===16&&i>shared.open&&i<shared.end?[node.call]:[]);
  const allowed=[materialize,flags,precise,...(combined?[numeric]:[]),...Array(lookupCalls).fill(lookup),
    ...(reusedHotEntry?[hotEntry]:[])].sort((a,b)=>a-b);
  check(JSON.stringify(calls.sort((a,b)=>a-b))===JSON.stringify(allowed),
    'the shared service contains only the verified materializer, flags resolver, exclusive callbacks and existing eligibility/lookup calls');
  check(!ops.slice(shared.open,shared.end).some(node=>node.op===0x24&&node.global!==valid),
    'the shared service cannot change continuation pointers, owner IDs, mode or nonce authority');
  check(ops.slice(shared.open,shared.end).every((node,index)=>{
    if(node.op===0x37)return node.memoryOffset===0&&sequence(
      ops.slice(shared.open+index-3,shared.open+index),[[0x20,0],[0x28,80],[0x20,99]]);
    if(node.op===0x36)return [0,4,8,12,16,20,24,28,32,36,40,44].includes(node.memoryOffset);
    return !(node.op>=0x38&&node.op<=0x3e);
  }),'service publication cannot write owner identity, source certificates, nonce or unrelated memory');
  return{errors,sites,guardOpen:shared.open,guardEnd:shared.end,publications:[],entries,shared:true,
    canonicalCall:numericCall,callbackChoice:choice?.open,mode:combined?'canonical-and-exact':'exact-only',
    viewResetOffset:ops[resetStart+3]?.offset};
}

export function inspectSharedCallbackSpine(bytes) {
  const module=readModule(bytes),index=named(module,'guest.run');
  const precise=module.imports.findIndex(item=>item.module==='ring3'&&item.name==='precise');
  const fn=defined(module,index);
  if(!fn||precise<0)return{errors:['missing actual guest body or precise import'],sites:[],publications:[],entries:[]};
  return inspectExactSuffixService(fn,module,precise);
}

function inspectHotSpanRouting(fn,module,precise) {
  const {ops,edges}=fn,summary={kind:'exact-adapter',tables:[],fullBlockAssignments:[],prefixQuotaGuards:[],
    spanGuards:[],wrappers:[],errors:[]};
  const check=(okay,message)=>{if(!okay)summary.errors.push(message);};
  const local=(node,op,index)=>node?.op===op&&node.local===index;
  const constant=(node,value)=>node?.op===0x41&&node.value===value;
  check(JSON.stringify(signature(module,precise))===JSON.stringify({parameters:[0x7f],results:[0x7f]}),
    'the real precise import takes the active Context pointer and returns one stop reason');
  const outer=ops[0]?.op===2?ops[0].frame:undefined;
  const service=ops.find(node=>node.op===3&&node.frames.at(-1)===outer)?.frame;
  const inner=service&&ops[service.open+1]?.op===2?ops[service.open+1].frame:undefined;
  check(outer?.kind===2&&service?.kind===3&&inner?.kind===2,
    'hot-span rejection shares the actual outer publication/in-body commit service');
  for(const[index,node]of ops.entries()){
    if(node.op===0x21&&node.local===151){
      if(constant(ops[index-1],0))continue;
      summary.fullBlockAssignments.push(node.offset);
      const end=ops[index-2],guard=end?.op===11?end.frame:undefined;
      const p=guard?ops.slice(guard.open-7,guard.open):[];
      const admitted=constant(ops[index-1],1)&&guard?.kind===4&&guard.type===-64&&guard.otherwise===undefined&&
        p.length===7&&local(p[0],0x20,130)&&constant(p[1],0)&&p[2].op===0x47&&
        local(p[3],0x20,12)&&p[4].op===0x41&&p[4].value>0&&p[5].op===0x49&&p[6].op===0x72;
      check(admitted,'every FULL_BLOCK=1 follows an exact offset and certified minimum-fuel rejection guard');
      if(!admitted)continue;
      const tail=ops.slice(guard.end-7,guard.end),branch=tail[6];
      const target=branch?.op===12?branch.frames.at(-1-branch.targets[0]):undefined;
      check(tail.length===7&&constant(tail[0],2)&&constant(tail[1],11)&&local(tail[2],0x20,12)&&
        tail[3].op===0x45&&tail[4].op===0x1b&&local(tail[5],0x21,108)&&branch?.op===12&&
        (target===inner||target===outer),
        'rejected ingress selects Budget only at zero quota, otherwise ExactPrefix11, then branches to the common exit');
      // The rejected arm must never fall through into FULL_BLOCK, including
      // through an unexpected nested branch. Stop at the shared exit itself.
      const reached=new Set(),pending=[guard.open+1],stop=target?.end+1;
      while(pending.length){const at=pending.pop();if(at===stop||reached.has(at))continue;
        reached.add(at);pending.push(...(edges[at]??[]));}
      check(!reached.has(guard.end+1)&&!reached.has(index),
        'every rejected-arm path exits before establishing hot-span permission');
      const entry=ops[guard.open+3]?.frame;
      check(local(ops[guard.open+1],0x20,130)&&ops[guard.open+2]?.op===0x45&&
        entry?.kind===4&&entry.otherwise===undefined&&ops[entry.end-2]?.op===0x41&&
        local(ops[entry.end-1],0x21,10),'rejected block-start ingress retains its exact PC before publication');
      summary.spanGuards.push({offset:ops[guard.open].offset,fullOffset:node.offset,pc:ops[entry?.end-2]?.value>>>0,
        instructions:p[4].value,exitOffset:branch?.offset});
    }
    if(node.op===0x4d&&local(ops[index-4],0x20,12)&&ops[index-3]?.op===0x41&&
      local(ops[index-2],0x20,131)&&ops[index-1]?.op===0x6b)summary.prefixQuotaGuards.push(node.offset);
    if(node.op===14){
      const outer=ops.slice(index-3,index),inner=ops.slice(index-10,index);
      if(outer.length===3&&local(outer[0],0x20,131)&&constant(outer[1],6)&&outer[2].op===0x76||
        inner.length===10&&local(inner[0],0x20,131)&&inner[1].op===0x41&&inner[2].op===0x4b&&inner[3].op===4&&
        local(inner[4],0x20,131)&&inner[5].op===0x41&&inner[6].op===0x6b&&inner[7].op===5&&
        constant(inner[8],0)&&inner[9].op===11)summary.tables.push({offset:node.offset});
    }
  }
  check(summary.fullBlockAssignments.length>0&&summary.spanGuards.length===summary.fullBlockAssignments.length,
    'each actual hot span establishes permission exactly after its validated rejection guard');
  check(summary.prefixQuotaGuards.length===0,'the hot body contains no old per-instruction exact-quota checks');
  check(summary.tables.length===0,'the hot body contains no dense arbitrary instruction-prefix selector tables');
  const body=named(module,'guest.run'),publisher=named(module,'guest.publish_link');
  for(const[exportName,name,linked]of[['run','guest.entry',false],['link','guest.link',true]]){
    const index=named(module,name),wrapper=defined(module,index),record={name,index,bodyCalls:[],preciseCalls:[]};
    summary.wrappers.push(record);
    check(Number.isInteger(index)&&module.exports.get(exportName)===index&&wrapper,
      `${exportName} exposes its actual named precise-aware entry wrapper`);
    if(!wrapper)continue;
    check(JSON.stringify(signature(module,index))===JSON.stringify({parameters:[0x7f,0x7f],results:[0x7f]}),
      `${name} retains the context/quota entry ABI`);
    wrapper.ops.forEach((node,i)=>{
      if(node.op===16&&node.call===body)record.bodyCalls.push(i);
      if(node.op===16&&node.call===precise)record.preciseCalls.push(i);
    });
    check(record.bodyCalls.length===1&&record.preciseCalls.length===1,
      `${name} has one hot-body call site and one exact CPU adapter call site`);
    const call=record.bodyCalls[0],exact=record.preciseCalls[0],w=wrapper.ops;
    const prefix=w.slice(call-2,call+5),guard=w[call+4]?.frame;
    check(prefix.length===7&&local(prefix[0],0x20,0)&&local(prefix[1],0x20,1)&&
      prefix[2].op===16&&prefix[2].call===body&&local(prefix[3],0x22,2)&&constant(prefix[4],11)&&
      prefix[5].op===0x46&&prefix[6].op===4&&guard?.kind===4&&guard.otherwise===undefined&&
      exact>guard.open&&exact<guard.end&&w[exact].frames.includes(guard)&&local(w[exact-1],0x20,0),
      `${name} calls the real precise adapter only after the published hot body returns ExactPrefix11`);
    if(linked){
      const before=w.slice(guard?.open+1,exact);
      check(before.length===3&&local(before[0],0x20,0)&&before[1].op===16&&before[1].call===publisher&&
        local(before[2],0x20,0),
        'the linked wrapper explicitly publishes actual canonical state immediately before exact execution');
    }else check(exact===guard?.open+2,
      'the standalone wrapper enters exact execution immediately after the canonical hot-body publication');
  }
  summary.exactSuffix=inspectExactSuffixService(fn,module,precise);
  summary.errors.push(...summary.exactSuffix.errors);
  check(module.functions.reduce((count,fn)=>count+fn.ops.filter(node=>node.op===16&&node.call===precise).length,0)===
    2+summary.exactSuffix.sites.length,'only the two verified public wrappers and verified shared service invoke precise');
  if(summary.exactSuffix.sites.length)check(JSON.stringify(summary.exactSuffix.entries)===JSON.stringify(
    summary.spanGuards.map(({pc,instructions})=>({pc,instructions})).sort((a,b)=>a.pc-b.pc)),
    'the private service exposes exactly the same certified minimum-fuel spans without expanding public entry permission');
  return summary;
}

function inspectLegacyPortRouting(fn) {
  const {ops}=fn,summary={tables:[],fullBlockAssignments:[],prefixQuotaGuards:[],errors:[]};
  summary.kind='in-body-prefix';
  const check=(okay,message)=>{if(!okay)summary.errors.push(message);};
  for(const[index,node]of ops.entries()){
    if(node.op===0x21&&node.local===151){
      // Exact entry invalidation may clear this permission. Only establishing
      // a nonzero permission needs the whole-block quota/offset proof below.
      if(ops[index-1]?.op===0x41&&ops[index-1].value===0)continue;
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
  const precise=module.imports.findIndex(item=>item.module==='ring3'&&item.name==='precise');
  const exactSuffix=precise<0?{errors:[],sites:[],publications:[]}:inspectExactSuffixService(fn,module,precise);
  summary.errors.push(...exactSuffix.errors);summary.exactSuffix=exactSuffix;
  const outerPublications=summary.publicationCalls.filter(index=>ops[index].frames.length===1);
  check(summary.publicationCalls.length===1+exactSuffix.publications.length&&outerPublications.length===1&&
    summary.publicationCalls.every(index=>outerPublications.includes(index)||exactSuffix.publications.includes(index)),
    'only the verified shared precise service adds publication before the sole terminal epilogue');
  summary.publicationCalls=outerPublications;
  if(summary.publicationCalls.length!==1){summary.errors.push('requires one terminal publication call');return summary;}
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
      const resumeTail=resume.slice(-3);let canonicalPrefix=resume.slice(0,-3);
      check(serviceLoop?.kind===3&&serviceLoop.type===-64&&
        ops[serviceLoop.open].frames.at(-1)===body&&innerExit?.kind===2&&innerExit.type===-64&&
        innerExit.open===serviceLoop.open+1&&innerExit.end<serviceTest.open,
        'the retained body exits one inner block to an in-body commit inside its bootstrap-once service loop');
      check(serviceTest.otherwise===undefined&&resumeTail.length===3&&resumeTail[0].op===0x41&&resumeTail[0].value===0&&
        resumeTail[1].op===0x21&&resumeTail[1].local===157&&resumeTail[2].op===12&&
        resumeTail[2].frames.at(-1-resumeTail[2].targets[0])===serviceLoop,
        'a requested restart clears its request and branches to the same service loop without re-running bootstrap');
      if(exactSuffix.shared){
        check(exactSuffix.guardOpen===serviceTest.open+4&&exactSuffix.guardEnd===serviceTest.end-4&&
          canonicalPrefix.length===exactSuffix.guardEnd-serviceTest.open,
          'the verified SERVICE>=2 callback spine is the complete service prefix before the unchanged restart tail');
        canonicalPrefix=[];
        summary.canonicalService={guardOpen:exactSuffix.guardOpen,guardEnd:exactSuffix.guardEnd,
          call:exactSuffix.canonicalCall,imported:module.imports.findIndex(item=>item.module==='ring3'&&item.name==='numeric'),
          mode:exactSuffix.mode};
      }
      if(canonicalPrefix.length){
        const guard=canonicalPrefix[3]?.frame,imported=module.imports.findIndex(item=>item.module==='ring3'&&item.name==='numeric');
        const calls=ops.flatMap((node,i)=>node.op===16&&node.call===imported?[i]:[]),call=calls[0];
        check(canonicalPrefix[0]?.op===0x20&&canonicalPrefix[0].local===157&&
          canonicalPrefix[1]?.op===0x41&&canonicalPrefix[1].value===2&&canonicalPrefix[2]?.op===0x46&&
          canonicalPrefix[3]?.op===4&&guard?.type===-64&&guard.otherwise===undefined&&
          canonicalPrefix.at(-1)?.op===11&&canonicalPrefix.at(-1).frame===guard,
          'the added service is wholly inside the exact canonical-kind2 arm before the unchanged restart tail');
        const owned=module.globalImports.some(item=>item.module==='ring3'&&item.name==='link.continuation');
        const inputs=owned?[0,173,172]:[0,172],args=ops.slice(call-inputs.length,call);
        check(imported>=0&&calls.length===1&&ops[call]?.frames.includes(guard)&&
          JSON.stringify(signature(module,imported))===JSON.stringify({parameters:Array(inputs.length).fill(0x7f),results:[0x7f]})&&
          args.length===inputs.length&&args.every((node,i)=>node.op===0x20&&node.local===inputs[i]),
          owned?'the sole owned callback consumes context, actual request PC and ordinal inside its service guard'
            :'the sole legacy callback consumes context and runtime request ordinal inside its service guard');
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
  const privateRequests=ops.flatMap((node,i)=>{
    if(node.op!==0x20||node.local!==108||ops[i+1]?.op!==0x41||ops[i+1].value!==-3||ops[i+2]?.op!==0x46)return[];
    const mapper=ops[i+3]?.frame;
    const valid=ops[i+3]?.op===4&&mapper?.kind===4&&mapper.otherwise===undefined&&mapper.end===i+6&&
      ops[i+4]?.op===0x41&&ops[i+4].value===3&&ops[i+5]?.op===0x21&&ops[i+5].local===157&&
      i>innerExit?.end&&mapper.end<serviceTest?.open;
    check(valid,'the private -3 request is mapped once to exact service3 after recovery and before callback dispatch');
    return valid?[i]:[];
  });
  check(privateRequests.length===(exactSuffix.sites.length?1:0),
    'only an emitted exact callback adds one private-sentinel service mapper');
  const exits=[];
  for(let i=body.open+1;i<body.end;i++){
    const node=ops[i];
    if(node.local===108&&[0x20,0x21,0x22].includes(node.op)){
      if(node.op===0x20)check(serviceTest&&(i===serviceTest.end+1||privateRequests.includes(i)),
        'only terminal completion and the verified private exact-request mapper read the saved reason');
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
  const scalarArithmetic=node=>node.op>=0xa0&&node.op<=0xa3;
  const arithmeticKernels=new Map();
  const kernelNames=new Set(['guest.fp.add','guest.fp.sub','guest.fp.rsub','guest.fp.mul',
    'guest.fp.mul_exact','guest.fp.div','guest.fp.rdiv']);
  const forbidden=node=>node.op>=0x23&&node.op<=0x40||[17,18,19,20,21,0xfc,0xfd].includes(node.op);
  for(const[index,name]of module.names)if(kernelNames.has(name)) {
    assert(index>=module.imports.length,'the arithmetic kernel is defined in this owner');
    const fn=defined(module,index),type=signature(module,index);
    assert.deepEqual(type,{parameters:[0x7c,0x7c,0x7f,0x7f],results:[0x7c,0x7f,0x7f,0x7f]});
    assert(!fn.ops.some(node=>forbidden(node)||node.op===16&&module.names.get(node.call)!=='guest.fp.status'),
      'a cyclic arithmetic call reaches only a memory-free private numerical kernel');
    for(const node of fn.ops.filter(node=>node.op===16)) {
      assert(node.call>=module.imports.length,'private status resolution never imports a callback');
      const status=defined(module,node.call);
      assert.deepEqual(signature(module,node.call),{parameters:[0x7f,0x7f,0x7c,0x7c,0x7c],results:[0x7f]});
      assert(!status.ops.some(node=>forbidden(node)||node.op===16),'private status resolution is a pure leaf');
    }
    const arithmetic=fn.ops.filter(scalarArithmetic).length;
    assert(arithmetic>0,'the validated kernel contains actual scalar floating arithmetic');
    arithmeticKernels.set(index,{index,name,arithmetic});
  }
  const arithmetic=node=>scalarArithmetic(node)||node.op===16&&arithmeticKernels.has(node.call);
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
    portRouting:inspectPortRouting(run,module),
    nativeFloatStores:run.ops.flatMap((node,index)=>{
      const single=node.op===0x38,p=run.ops.slice(index-(single?3:2),index);
      return(single||node.op===0x39)&&node.memoryOffset===0&&p[0]?.op===0x20&&p[0].local===52&&
        p[1]?.op===0x20&&p[1].local===48&&(!single||p[2]?.op===0xb6)?
        [{offset:node.offset,format:single?'f32':'f64',pointerLocal:52,valueLocal:48}]:[];
    }),
    arithmeticKernels:[...arithmeticKernels.values()],
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
    limits:['narrow decoder for authored modules; direct scalar f64 arithmetic or calls to verified pure numerical kernels',
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
      branchTables:fn.ops.filter(node=>node.op===14).length,operators:fn.ops.length,
      portRouting:inspectPortRouting(fn,module)}];
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
