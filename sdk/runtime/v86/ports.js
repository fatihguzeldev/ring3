import {KEY} from '../engine.js';
import {SEGMENTS} from './layout.js';

const ids=[0x10001,0x10002,0xf000b,0x40225,0x4022a,0x4022e,0x40232,0x4023f,
  0x4023d,0x40253,0x40255,0x40228,0x4022c];
let dispatchModule;
const unsigned=value=>{
  const out=[];do{const byte=value&127;value>>>=7;out.push(byte|(value?128:0));}while(value);return out;
};
const signed=value=>{
  const out=[];value|=0;
  for(;;){const byte=value&127;value>>=7;const done=(value===0&&!(byte&64))||(value===-1&&(byte&64));out.push(byte|(done?0:128));if(done)return out;}
};
const name=value=>[...unsigned(value.length),...Array.from(value,char=>char.charCodeAt(0))];
const section=(id,bytes)=>[id,...unsigned(bytes.length),...bytes];
const vector=rows=>[...unsigned(rows.length),...rows.flat()];
const type=(parameters,result=false)=>[0x60,...unsigned(parameters),...Array(parameters).fill(0x7f),result?1:0,...(result?[0x7f]:[])];
const exported=(label,kind,index)=>[...name(label),kind,...unsigned(index)];
const body=(locals,code)=>{const bytes=[...vector(locals?[[...unsigned(locals),0x7f]]:[]),...code,0x0b];return [...unsigned(bytes.length),...bytes];};
const constant=value=>[0x41,...signed(value)];
const local=index=>[0x20,...unsigned(index)];
const get=index=>[0x23,...unsigned(index)];
const set=index=>[0x24,...unsigned(index)];
const call=index=>[0x10,...unsigned(index)];
const load=(offset,byte=false)=>[...constant(0),byte?0x2d:0x28,byte?0:2,...unsigned(offset)];
const store=(offset,value)=>[...constant(0),...value,0x36,2,...unsigned(offset)];
const increment=index=>[...get(index),...constant(1),0x6a,...set(index)];
const any=checks=>checks.flatMap((check,index)=>index?[...check,0x72]:check);

function shim() {
  dispatchModule??=new WebAssembly.Module(new Uint8Array([
    0,97,115,109,1,0,0,0,
    ...section(1,vector([type(2)])),
    ...section(2,vector([
      [...name('env'),...name('fallback'),0,0],
      [...name('env'),...name('table'),1,0x70,0,1],
    ])),
    ...section(3,vector([[0],[0]])),
    ...section(7,vector([exported('fallback',0,1),exported('write8',0,2)])),
    ...section(10,vector([
      body(0,[...local(0),...local(1),...call(0)]),
      body(0,[...local(0),...local(1),...constant(0),0x11,0,0]),
    ])),
  ]));
  return dispatchModule;
}

function kernel(resultPtr,selectorsPtr) {
  // globals hold only routing policy and per-batch counters, never guest payloads.
  const fallback=[...local(0),...local(1),...call(1),0x0f];
  const fallbackIf=condition=>[...condition,0x04,0x40,...fallback,0x0b];
  const fatal=(status,kind)=>[
    ...status,...set(10),...local(2),...set(11),...constant(kind),...set(12),
    ...get(10),...get(11),...get(12),...call(2),0x00,
  ];
  const code=[
    ...get(10),0x04,0x40,...get(10),...get(11),...get(12),...call(2),0x00,0x0b,
    ...fallbackIf([...get(0),0x45]),
    ...fallbackIf([...local(0),...constant(0xe1),0x47]),
    ...load(560),0x21,2,
    ...fallbackIf(any([
      [...load(612,true),...constant(3),0x47],
      [...load(556),...local(2),...constant(2),0x6a,0x47],
      ...[0,4,8].map(at=>[...load(668+at),...load(selectorsPtr+at),0x47]),
    ])),
    ...get(2),0x45,0x04,0x40,
      ...increment(7),...constant(7),...set(2),...fallback,
    0x0b,
    ...get(2),...constant(1),0x6b,...set(2),
    ...constant(KEY[0]),...constant(KEY[1]),...local(2),...load(80),...load(64),...get(1),...call(0),0x21,3,
    ...local(3),...constant(256),0x46,0x04,0x40,
      ...increment(4),...local(2),...set(8),...constant(1),...set(9),...fallback,
    0x0b,
    ...local(3),0x04,0x40,...fatal(local(3),1),0x0b,
    ...load(resultPtr+16),0x21,4,...constant(0),0x21,5,
    ...ids.flatMap((id,index)=>[
      ...local(4),...constant(id),0x46,0x04,0x40,...constant(1<<index),0x21,5,0x0b,
    ]),
    ...any([
      [...load(resultPtr),...constant(0x46493352),0x47],
      [...load(resultPtr+4),...constant(0x10001),0x47],
      [...load(resultPtr+8),...constant(32),0x47],
      [...load(resultPtr+12)],
      [...local(5),...get(1),0x71,0x45],
    ]),0x04,0x40,...fatal(constant(-1),2),0x0b,
    ...store(64,load(resultPtr+20)),...store(80,load(resultPtr+28)),
    ...store(556,load(resultPtr+24)),...store(560,load(resultPtr+24)),...store(620,constant(-1)),
    ...increment(3),
    ...local(5),...constant(0x700),0x71,0x04,0x40,...increment(5),0x0b,
    ...local(5),...constant(0x1800),0x71,0x04,0x40,...increment(6),0x0b,
  ];
  return new WebAssembly.Module(new Uint8Array([
    0,97,115,109,1,0,0,0,
    ...section(1,vector([type(2),type(6,true),type(3),type(1),type(0)])),
    ...section(2,vector([
      [...name('env'),...name('native'),0,1],
      [...name('env'),...name('fallback'),0,0],
      [...name('env'),...name('fatal'),0,2],
      [...name('env'),...name('memory'),2,0,1],
    ])),
    ...section(3,vector([[0],[3],[4],[4]])),
    ...section(6,vector(Array.from({length:13},()=>[0x7f,1,...constant(0),0x0b]))),
    ...section(7,vector([
      exported('write8',0,3),exported('arm',0,4),exported('disarm',0,5),exported('reset',0,6),
      ...['completed','declined','bindings','stateInputs','checkpoints','declinedPc','hasDeclined']
        .map((label,index)=>exported(label,3,index+3)),
    ])),
    ...section(10,vector([
      body(4,code),
      body(0,[...local(0),...set(1),...constant(1),...set(0),...constant(0),...set(2)]),
      body(0,[...constant(0),...set(0)]),
      body(0,[3,4,5,6,7,8,9].flatMap(index=>[...constant(0),...set(index)])),
    ])),
  ]));
}

export function createPortBridge(originalIOImport) {
  if(typeof originalIOImport!=='function')throw new TypeError('invalid v86 I/O fallback');
  const table=new WebAssembly.Table({element:'anyfunc',initial:1,maximum:1}),module=shim();
  const instance=new WebAssembly.Instance(module,{env:{fallback:originalIOImport,table}});
  table.set(0,instance.exports.fallback);
  const dispatch=Object.freeze({module,instance,table,fallback:instance.exports.fallback,originalIO:originalIOImport});
  let binding=null,cache=null,closed=false;
  const failure=()=>{if(cache?.inlineFailure)throw cache.inlineFailure;};
  return {
    write8:instance.exports.write8,
    dispatch,
    get binding(){return binding;},
    bind(owner) {
      if(closed || binding)throw new Error('v86 port bridge already bound or closed');
      const native=owner?.engine?.api?.component_cpu_try_direct_gate_mask;
      if(typeof native!=='function')return false;
      const memory=owner.memory,resultPtr=owner.engine.base+140,selectorsPtr=owner.engine.base+SEGMENTS+16;
      if(!(memory instanceof WebAssembly.Memory) || owner.engine.memory!==memory || owner.cpu?.wasm_memory!==memory ||
          !Number.isSafeInteger(resultPtr) || resultPtr<140 || resultPtr%4 || selectorsPtr+12>memory.buffer.byteLength)
        throw new Error('invalid v86 port bridge memory owner');
      const module=kernel(resultPtr,selectorsPtr);
      const fatal=(status,pc,kind)=>{
        owner.inlineFailure??=Object.assign(new Error(kind===2?'invalid v86 Wasm port completion record':`v86 Wasm port completion failed ${status}`),
          {operation:'component_cpu_try_direct_gate_mask',status,pc:pc>>>0});
        throw owner.inlineFailure;
      };
      const instance=new WebAssembly.Instance(module,{env:{memory,native,fallback:originalIOImport,fatal}});
      binding=Object.freeze({module,instance,memory,native,write8:instance.exports.write8,resultPtr,selectorsPtr});
      cache=owner;table.set(0,binding.write8);return true;
    },
    arm(mask) {
      failure();
      if(closed || !binding)throw new Error('v86 port bridge is not bound');
      if(!Number.isInteger(mask) || mask<1 || mask>0x1fff)throw new Error('invalid v86 port bridge mask');
      binding.instance.exports.arm(mask);return true;
    },
    harvest() {
      if(!binding)return {completed:0,declined:0,bindings:0,stateInputs:0,checkpoints:0,declinedPc:null};
      const ex=binding.instance.exports,values={};
      for(const label of ['completed','declined','bindings','stateInputs','checkpoints'])values[label]=ex[label].value>>>0;
      values.declinedPc=ex.hasDeclined.value?ex.declinedPc.value>>>0:null;
      ex.reset();return values;
    },
    disarm() {binding?.instance.exports.disarm();return false;},
    close() {binding?.instance.exports.disarm();table.set(0,dispatch.fallback);closed=true;},
  };
}
