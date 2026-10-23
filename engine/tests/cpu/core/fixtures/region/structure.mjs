import assert from 'node:assert/strict';

class Reader {
  constructor(bytes){this.bytes=bytes;this.at=0;}
  byte(){assert(this.at<this.bytes.length,'complete authored Wasm input');return this.bytes[this.at++];}
  unsigned(){let result=0;for(let shift=0;shift<35;shift+=7){const b=this.byte();result+=(b&127)*2**shift;if(b<128)return result;}assert.fail('bounded Wasm u32');}
  signed(){for(let index=0;index<10;index++)if(this.byte()<128)return;assert.fail('bounded signed Wasm integer');}
  take(length){assert(this.at+length<=this.bytes.length);const result=this.bytes.subarray(this.at,this.at+length);this.at+=length;return result;}
  text(){return new TextDecoder('utf-8',{fatal:true}).decode(this.take(this.unsigned()));}
  vector(read){return Array.from({length:this.unsigned()},()=>read());}
}

function operators(bytes) {
  const r=new Reader(bytes),locals=r.vector(()=>({count:r.unsigned(),type:r.byte()}));
  const root={kind:0xff,open:-1},frames=[root],ops=[];
  while(r.at<bytes.length) {
    const node={op:r.byte(),offset:r.at-1,frames:frames.slice(),targets:[]},index=ops.length;
    const op=node.op;
    if(op===2||op===3||op===4){r.signed();node.frame={kind:op,open:index};frames.push(node.frame);}
    else if(op===5){node.frame=frames.at(-1);assert.equal(node.frame.kind,4);node.frame.otherwise=index;}
    else if(op===11){node.frame=frames.pop();assert(node.frame);node.frame.end=index;}
    else if(op===12||op===13||op===0xd5||op===0xd6)node.targets=[r.unsigned()];
    else if(op===14){node.targets=r.vector(()=>r.unsigned());node.targets.push(r.unsigned());}
    else if(op===16||op===18)node.call=r.unsigned();
    else if(op===17||op===19){r.unsigned();r.unsigned();}
    else if(op===20||op===21||op>=0x20&&op<=0x26||op===0xd2)r.unsigned();
    else if(op===0x1c)r.vector(()=>r.byte());
    else if(op>=0x28&&op<=0x3e){node.align=r.unsigned();node.memoryOffset=r.unsigned();}
    else if(op===0x3f||op===0x40)r.unsigned();
    else if(op===0x41||op===0x42||op===0xd0)r.signed();
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
  return {locals,ops,edges};
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

export function inspectRegion(bytes) {
  const module=readModule(bytes),runIndex=[...module.names].find(([,name])=>name==='guest.run')?.[0]??module.exports.get('run');
  assert(Number.isInteger(runIndex)&&runIndex>=module.imports.length,'defined guest run');
  const run=module.functions[runIndex-module.imports.length];
  const materializers=new Set(module.imports.flatMap((item,index)=>item.name==='numeric'?[index]:[]));
  const rawStore=node=>node.op===0x37&&node.memoryOffset===0||node.op===0x3b&&node.memoryOffset===8;
  module.functions.forEach((fn,index)=>{
    const id=index+module.imports.length;
    if(module.names.get(id)==='guest.materialize_numeric'||fn.ops.some(node=>node.op===0x37&&node.memoryOffset===0)
      &&fn.ops.some(node=>node.op===0x3b&&node.memoryOffset===8))materializers.add(id);
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
  return {runIndex,runName:module.names.get(runIndex),locals:run.locals,
    arithmeticCycles:cycles(()=>false),withoutCanonicalization:cycles(canonical),
    withoutCanonicalizationOrDispatch:cycles(node=>canonical(node)||dispatch(node)),
    materializers:[...materializers].map(index=>({index,name:module.names.get(index)??module.imports[index]?.name})),
    materializerSignature:[...module.names].filter(([,name])=>name==='guest.materialize_numeric')
      .map(([index,name])=>({index,name,...module.types[module.functionTypes[index-module.imports.length]]})),
    statusResolverSignature:[...module.names].filter(([,name])=>name==='guest.resolve_status')
      .map(([index,name])=>({index,name,...module.types[module.functionTypes[index-module.imports.length]]})),
    pageTranslatorSignature:[...module.names].filter(([,name])=>name==='guest.translate_page')
      .map(([index,name])=>({index,name,...module.types[module.functionTypes[index-module.imports.length]]})),
    limits:['narrow decoder for generated authored modules; scalar f64 arithmetic cycles only',
      'a retained cycle proves a canonicalization-free emitted path, not its runtime frequency or speed']};
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
}
