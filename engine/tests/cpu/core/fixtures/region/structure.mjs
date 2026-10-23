import assert from 'node:assert/strict';

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
    else if(op===0x42||op===0xd0)r.signed();
    else if(op===0x43)r.take(4);
    else if(op===0x44)r.take(8);
    else if(op===0xfc){
      node.ext=r.unsigned();
      if([8,10,12,14].includes(node.ext)){r.unsigned();r.unsigned();}
      else if([9,11,13,15,16,17].includes(node.ext))r.unsigned();
      else assert(node.ext<=7,'supported authored bulk operation');
    }else if(op===0xfd){
      node.ext=r.unsigned();
      if(node.ext<=11||node.ext===92||node.ext===93){r.unsigned();r.unsigned();}
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
  const names=new Map(),imports=[],functions=[],types=[],functionTypes=[],exports=new Map();
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
      else if(type===2){const flags=section.unsigned();section.unsigned();if(flags&1)section.unsigned();assert(flags<4);}
      else if(type===3){assert(module==='ring3'&&name.startsWith('link.'));section.byte();assert.equal(section.byte(),1);}
      else assert.fail('only memory, function and typed-link imports in authored module');
    });
    else if(kind===3)functionTypes.push(...section.vector(()=>section.unsigned()));
    else if(kind===7)section.vector(()=>{const name=section.text(),type=section.byte(),index=section.unsigned();if(type===0)exports.set(name,index);});
    else if(kind===10)functions.push(...section.vector(()=>operators(section.take(section.unsigned()))));
  }
  assert.equal(functionTypes.length,functions.length);
  return {names,imports,functions,types,functionTypes,exports};
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
  const wantedParameters=[...Array(8).fill(0x7c),...Array(8).fill(0x7e),...Array(15).fill(0x7f)];
  const wantedResults=[...Array(8).fill(0x7c),...Array(8).fill(0x7e),...Array(14).fill(0x7f)];
  const matches=(left,right)=>left?.length===right.length&&left.every((value,i)=>value===right[i]);
  if(!fn||!matches(type?.parameters,wantedParameters)||!matches(type?.results,wantedResults))return undefined;
  const stores=fn.ops.filter(node=>node.op>=0x36&&node.op<=0x3e),guards=[];
  if(!stores.length||fn.ops.some(node=>[16,17,18,19,20,21].includes(node.op)))return undefined;
  for(const store of stores){
    if(!(store.op===0x37&&store.memoryOffset===0||store.op===0x3b&&store.memoryOffset===8))return undefined;
    const guard=store.frames.findLast(frame=>{
      if(frame.kind!==4||frame.otherwise!==undefined)return false;
      const pattern=fn.ops.slice(frame.open-4,frame.open),mask=pattern[1]?.value;
      return pattern.length===4&&pattern[0].op===0x20&&pattern[0].local===24&&
        pattern[1].op===0x41&&mask>=1&&mask<=128&&(mask&(mask-1))===0&&pattern[2].op===0x71&&pattern[3].op===0x45;
    });
    if(!guard)return undefined;
    guards.push({storeOffset:store.offset,guardOffset:fn.ops[guard.open].offset,nativeBit:fn.ops[guard.open-3].value});
  }
  return {index,name:module.names.get(index),parameters:type.parameters,results:type.results,rawStoreGuards:guards,
    condition:'each raw80 store requires the corresponding incoming native-valid bit to be zero'};
}

export function instrumentStateWitness(source) {
  const module=readModule(source),original=new WebAssembly.Module(source);
  const globalBase=WebAssembly.Module.imports(original).filter(item=>item.kind==='global').length;
  const normalizer=[...module.names].find(([,name])=>name==='guest.normalize_frame')?.[0];
  const materializer=[...module.names].find(([,name])=>name==='guest.materialize_numeric')?.[0];
  assert(Number.isInteger(materializer),'the raw80 materializer is a real defined function');
  const u=value=>{const result=[];do{const byte=value&127;value>>>=7;result.push(byte|(value?128:0));}while(value);return Uint8Array.from(result);};
  const concat=chunks=>Uint8Array.from(Buffer.concat(chunks.map(value=>Buffer.from(value))));
  const section=(id,bytes)=>concat([Uint8Array.of(id),u(bytes.length),bytes]);
  const increment=index=>concat([Uint8Array.of(0x23),u(globalBase+index),Uint8Array.of(0x41,1,0x6a,0x24),u(globalBase+index)]);
  const exports=['test.normalizer_calls','test.materializer_calls','test.raw80_stores','test.fp_metadata_writes',
    'test.body_entries','test.control_admissions'];
  const input=new Reader(source);input.take(8);const output=[source.subarray(0,8)];let added=false;
  while(input.at<source.length){
    const id=input.byte(),bytes=input.take(input.unsigned());let rewritten=bytes;
    assert.notEqual(id,6,'generated region has no preexisting defined globals');
    if(id===7){
      output.push(section(6,Uint8Array.of(exports.length,...Array(exports.length).fill([0x7f,1,0x41,0,0x0b]).flat())));
      const r=new Reader(bytes),count=r.unsigned(),entries=exports.map((name,index)=>{
        const encoded=new TextEncoder().encode(name);return concat([u(encoded.length),encoded,Uint8Array.of(3),u(globalBase+index)]);
      });
      rewritten=concat([u(count+entries.length),r.take(bytes.length-r.at),...entries]);added=true;
    }else if(id===10){
      const r=new Reader(bytes),count=r.unsigned(),bodies=[];
      for(let defined=0;defined<count;defined++){
        const raw=r.take(r.unsigned()),index=defined+module.imports.length,fn=module.functions[defined];let body=raw;
        if(index===normalizer||index===materializer||['guest.run','guest.precise'].includes(module.names.get(index))){
          const insertions=new Map(),insert=(at,bytes)=>insertions.set(at,[...(insertions.get(at)??[]),bytes]);
          if(index===normalizer||index===materializer){
            insert(fn.localsEnd,increment(index===normalizer?0:1));
            for(const node of fn.ops)if(node.op===0x37&&node.memoryOffset===0||node.op===0x3b&&node.memoryOffset===8)
              insert(node.offset,increment(2));
          }else{
            insert(fn.localsEnd,increment(4));
            for(const[index,node]of fn.ops.entries()){
              if([0x21,0x22].includes(node.op)&&node.local===91)insert(node.offset,increment(3));
              if(controlAdmission(fn.ops,index))insert(node.offset,increment(5));
            }
          }
          const chunks=[];let start=0;
          for(const[at,values]of[...insertions].sort(([a],[b])=>a-b)){chunks.push(raw.subarray(start,at),...values);start=at;}
          chunks.push(raw.subarray(start));body=concat(chunks);
        }
        bodies.push(u(body.length),body);
      }
      assert.equal(r.at,bytes.length);rewritten=concat([u(count),...bodies]);
    }
    output.push(section(id,rewritten));
  }
  assert(added,'diagnostic globals are exported alongside actual generated entry points');
  return {bytes:concat(output),read:instance=>Object.fromEntries(exports.map(name=>[name.slice(5),instance.exports[name].value]))};
}

function controlAdmission(ops,index) {
  return ops[index]?.op===0x20&&ops[index].local===87&&
    ops[index+1]?.op===0x41&&ops[index+1].value===0xf3f&&ops[index+2]?.op===0x71;
}

function inspectExitEpilogue(module,name) {
  const named=wanted=>[...module.names].find(([,name])=>name===wanted)?.[0];
  const index=named(name),fn=module.functions[index-module.imports.length];
  const publication=named('guest.publish'),precise=named('guest.precise');
  const summary={name,index,publicationCalls:[],explicitReturns:[],sharedExitBranches:0,preciseCalls:[],errors:[]};
  if(!fn){summary.errors.push('missing defined function');return summary;}
  const{ops,edges}=fn,check=(okay,message)=>{if(!okay)summary.errors.push(message);};
  ops.forEach((node,index)=>{
    if(Number.isInteger(publication)&&node.call===publication)summary.publicationCalls.push(index);
    if(Number.isInteger(precise)&&node.call===precise)summary.preciseCalls.push(index);
    if(node.op===15)summary.explicitReturns.push(index);
  });
  if(summary.publicationCalls.length!==1){summary.errors.push('requires one actual publication call');return summary;}
  const publish=summary.publicationCalls[0],numeric=module.imports.some(item=>item.name==='numeric');
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
  const exits=[];
  for(let i=body.open+1;i<body.end;i++){
    const node=ops[i];
    if(node.local===108&&[0x20,0x21,0x22].includes(node.op)){
      check(node.op===0x21,'the guest body only assigns the reason local');
      const next=ops[i+1];
      check(next?.op===12&&next.frames.at(-1-next.targets[0])===body,
        'every assigned reason immediately branches to the shared outer exit');
    }
    if([12,13,14].includes(node.op))for(const depth of node.targets){
      const target=node.frames.at(-1-depth);
      check(target?.kind!==0xff,'guest branches cannot bypass publication through the function label');
      if(target===body){
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
  summary.preciseTailSentinels=sentinel.length;
  if(name==='guest.run'){
    check(sentinel.length>0,'fast guest quota guards have a precise-tail sentinel');
    check(summary.preciseCalls.length===1,'fast guest has one shared precise-tail call');
    const call=summary.preciseCalls[0],conditional=ops[call]?.frames.at(-1);
    const comparison=conditional?ops.slice(conditional.open-3,conditional.open):[];
    check(call>publish&&conditional?.kind===4&&conditional.type===-1&&
      comparison.length===3&&comparison[0].op===0x20&&comparison[0].local===108&&
      comparison[1].op===0x41&&comparison[1].value===-1&&comparison[2].op===0x46&&
      ops[call-2]?.op===0x20&&ops[call-2]?.local===0&&ops[call-1]?.op===0x20&&ops[call-1]?.local===12&&
      call===conditional.open+3&&conditional.otherwise===call+1&&conditional.end===call+3&&
      ops[call+2]?.op===0x20&&ops[call+2]?.local===108&&conditional.end+1===tailReturn&&
      ops[tailReturn]?.op===15&&ops[tailReturn+1]?.frame?.kind===0xff,
      'only sentinel -1 selects precise(CTX,REMAIN), after publication; ordinary exits return their saved reason');
  }else{
    check(sentinel.length===0&&summary.preciseCalls.length===0,'precise guest has no sentinel or recursive precise call');
    check(ops[publish+1]?.op===0x20&&ops[publish+1]?.local===108&&publish+2===tailReturn&&
      ops[tailReturn]?.op===15&&ops[tailReturn+1]?.frame?.kind===0xff,
      'precise guest returns its saved reason immediately after publication');
  }
  return summary;
}

export function inspectRegion(bytes) {
  const module=readModule(bytes),runIndex=[...module.names].find(([,name])=>name==='guest.run')?.[0]??module.exports.get('run');
  assert(Number.isInteger(runIndex)&&runIndex>=module.imports.length,'defined guest run');
  const run=module.functions[runIndex-module.imports.length];
  const conditionalNative=new Map(module.functions.flatMap((_,defined)=>{
    const index=defined+module.imports.length,proof=guardedNativeNormalizer(module,index);return proof?[[index,proof]]:[];
  }));
  const materializers=new Set(module.imports.flatMap((item,index)=>item.name==='numeric'?[index]:[]));
  const rawStore=node=>node.op===0x37&&node.memoryOffset===0||node.op===0x3b&&node.memoryOffset===8;
  module.functions.forEach((fn,index)=>{
    const id=index+module.imports.length;
    if(module.names.get(id)==='guest.materialize_numeric'||!conditionalNative.has(id)&&
      fn.ops.some(node=>node.op===0x37&&node.memoryOffset===0)&&fn.ops.some(node=>node.op===0x3b&&node.memoryOffset===8))materializers.add(id);
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
  return {runIndex,runName:module.names.get(runIndex),locals:run.locals,
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
    materializers:[...materializers].map(index=>({index,name:module.names.get(index)??module.imports[index]?.name})),
    materializerSignature:[...module.names].filter(([,name])=>name==='guest.materialize_numeric')
      .map(([index,name])=>({index,name,...module.types[module.functionTypes[index-module.imports.length]]})),
    statusResolverSignature:[...module.names].filter(([,name])=>name==='guest.resolve_status')
      .map(([index,name])=>({index,name,...module.types[module.functionTypes[index-module.imports.length]]})),
    pageTranslatorSignature:[...module.names].filter(([,name])=>name==='guest.translate_page')
      .map(([index,name])=>({index,name,...module.types[module.functionTypes[index-module.imports.length]]})),
    exitEpilogues:['guest.run','guest.precise'].map(name=>inspectExitEpilogue(module,name)),
    limits:['narrow decoder for generated authored modules; scalar f64 arithmetic cycles only',
      'a retained cycle proves a canonicalization-free emitted path, not its runtime frequency or speed']};
}

export function assertAnalyzedStateStructure(proof) {
  assert(proof.withoutCanonicalizationDispatchOrStackRotation.length>0,
    'the balanced arithmetic loop repeats without per-push/pop eight-cell runtime rotations');
  assertRegionStructure(proof);
  assert(proof.withoutCanonicalizationDispatchOrEagerControl.length>0,
    'the balanced arithmetic loop also bypasses per-instruction PC/index writes');
  assert(proof.withoutCanonicalizationDispatchOrControlAdmission.length>0,
    'the balanced arithmetic loop reuses admitted control facts across its backedge');
  assert.equal(proof.conditionalNativeCanonicalizers.length,1,'one actual guarded native/opaque frame normalizer');
  const witness=proof.nativeStateWitness;
  assert(witness,'the actual native loop supplies a linked pre-publication state witness');
  assert(witness.normalizer_calls>0,'the exercised loop actually invokes the encoded conditional normalizer');
  assert.equal(witness.raw80_stores,0,'native internal normalization does not execute raw80 stores');
  assert.equal(witness.materializer_calls,0,'native internal edges do not invoke the canonical materializer');
  assert(witness.control_admissions>0&&witness.control_admissions<=witness.body_entries,
    'the exercised control admission runs at most once per fresh body import, including a possible precise tail');
  assert.equal(witness.afterBankHex,witness.beforeBankHex,'the full private raw80 bank stays unchanged before explicit publication');
  assert(witness.fp_metadata_writes<12,
    'metadata is reconstructed at analyzed frame boundaries instead of all twelve executed x87 operations');
}

export function assertRegionStructure(proof) {
  assert(proof.arithmeticCycles.length>0,'the actual run contains a cyclic f64 arithmetic path');
  assert(proof.withoutCanonicalization.length>0,'an arithmetic loop can repeat without raw80 stores or materializing helpers');
  assert(proof.withoutCanonicalizationOrDispatch.length>0,'the reducible arithmetic loop also bypasses the general block dispatcher');
  assert.equal(proof.materializerSignature.length,1,'one private numerical materializer');
  assert.deepEqual(proof.materializerSignature[0].parameters,[...Array(8).fill(0x7c),...Array(10).fill(0x7f)],
    'materializer receives eight f64 values and complete integer metadata');
  assert.deepEqual(proof.materializerSignature[0].results,[],'materialization has no result');
  assert.equal(proof.statusResolverSignature.length,1,'one private exact status resolver');
  assert.deepEqual(proof.statusResolverSignature[0].parameters,[0x7f,0x7f,0x7c,0x7c,0x7c],
    'status resolution receives the recipe, prior status, both exact operands and rounded result');
  assert.deepEqual(proof.statusResolverSignature[0].results,[0x7f],'status resolution returns the exact architectural status');
  assert.equal(proof.pageTranslatorSignature.length,1,'one private page translation helper');
  assert.deepEqual(proof.pageTranslatorSignature[0].parameters,[0x7f,0x7f,0x7f],
    'translation receives context, guest linear address and requested permissions');
  assert.deepEqual(proof.pageTranslatorSignature[0].results,[0x7f,0x7f,0x7f],
    'translation returns the physical slot, page base and complete permission mask');
  for(const exit of proof.exitEpilogues)assert.deepEqual(exit.errors,[],`${exit.name}: complete common-exit control-flow proof`);
}
