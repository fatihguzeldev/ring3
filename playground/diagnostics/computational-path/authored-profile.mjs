import {Engine,ownedExecutionBindings} from '/runtime/engine.js';
import {CodeCache} from '/runtime/cache.js';
import {program,ENTRY,DATA,STACK} from './authored/compare-programs.mjs';
import {policy,reset,execute,validate,modules,hash} from './authored/compare-core.mjs';

export async function profileAuthored() {
  const bytes=new Uint8Array(await (await fetch('/engine.wasm')).arrayBuffer());
  const sha256=await hash(bytes);
  if(sha256!=='871eb8b438c138ecc247c853f44f643e6eee8989e2e9afb564a0b6ab988f8337') throw new Error('unexpected baseline artifact');
  const artifact={label:'baseline-authored',sha256,module:await WebAssembly.compile(bytes)};
  const engine=await Engine.create('/engine.wasm',32,undefined,{module:artifact.module,hash:sha256});
  ownedExecutionBindings(engine,true);
  const p=program('mixed-call-store');
  for(const[address,permission]of[[ENTRY,7],[DATA,3],[STACK,3]])engine.ok('map',address,1,permission);
  engine.ok('configure_execution_with_limits',policy.units,policy.wasmBytes,policy.promotionHits,policy.unitInstructions,policy.unitWasmBytes);
  const initial=new Uint8Array(engine.memory.buffer,engine.base,engine.api.arena_len()>>>0).slice();
  const codePage=new Uint8Array(4096);codePage.set(p.code);
  engine.request(codePage);engine.ok('upload',ENTRY,codePage.length);
  engine.request(p.data);engine.ok('upload',DATA,p.data.length);
  const cache=new CodeCache(engine,{slots:policy.units,budget:50000,receiptMode:'compiled'});
  const owners=new Map();for(const op of p.operations)owners.set(op.pc,cache.entry(op.pc));
  const c={artifact,p,engine,cache,initial,owners,codePage,moduleRecords:null};
  try {
    reset(c,32768);validate(c,execute(c,{collect:false}),32768);
    const topology=await modules(c);
    reset(c,262144);
    const profiler=new Profiler({sampleInterval:1,maxBufferSize:2000});
    let run,trace;
    try{run=execute(c,{collect:false});}finally{trace=await profiler.stop();}
    const checked=validate(c,run,262144);
    return {kind:'authored-native-generated',status:'PASS',engineSha256:sha256,
      sampleInterval:profiler.sampleInterval,retired:run.retired,oracle:checked.oracle,
      topology,trace,limits:['authored capability check; not game CPU attribution or production Worker timing']};
  } finally {c.engine.ok('close');}
}
