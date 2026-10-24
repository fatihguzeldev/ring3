import assert from 'node:assert/strict';
import {existsSync,readFileSync} from 'node:fs';
import {join} from 'node:path';

const legacyFields=[2,3,4,5,6,7,8,9,10,11,12,13,20,21,22,23,24,25,
  76,77,78,79,80,81,82,83,84,85,86,87,88,89,90,91,92,93,94,95,96,99,100,101,102,103];
const fields=[2,3,4,5,6,7,8,9,10,11,12,13,20,21,22,23,24,25,99];
const linkNames=fields=>['link.mode','link.valid','link.context',...fields.map(index=>`link.local${index}`)];
const previousSupportNames=['flags','materialize_numeric','resolve_status','translate_page','acquire_group',
  'store8','store16','store32','commit_pool_cold','commit_native','commit_raw'].map(name=>`support.${name}`);
export const supportNames=[...previousSupportNames,'support.record_trace'];
const poolSupportNames=[...supportNames.slice(0,8),'support.commit_pool'];
const legacySupportNames=[...supportNames.slice(0,8),'support.normalize_kernel'];
const supportForAbi=abi=>abi===1?legacySupportNames:abi===2?poolSupportNames:abi<5?previousSupportNames:supportNames;
const supportSources=new WeakMap(),supportInstances=new WeakMap();
const preciseCallbacks=new WeakMap();
let standaloneSupport;

export function coreImports(module) {
  const imports=WebAssembly.Module.imports(module),links=imports.filter(item=>item.name.startsWith('link.'));
  if(links.length)assert.deepEqual(links,linkNames(links.length===47?legacyFields:fields).map(name=>({module:'ring3',name,kind:'global'})),
    'the complete typed compiled-link state is the only added import surface');
  const support=imports.filter(item=>item.name.startsWith('support.'));
  if(support.length)assert.deepEqual(support,[...(support.some(item=>item.name==='support.normalize_kernel')?legacySupportNames:support.some(item=>item.name==='support.commit_pool')?poolSupportNames:support.some(item=>item.name==='support.record_trace')?supportNames:previousSupportNames).map(name=>({module:'ring3',name,kind:'function'})),
    {module:'ring3',name:'support.abi',kind:'global'}], 'the complete versioned per-CPU support ABI is imported directly');
  const recorder=imports.filter(item=>item.name==='trace.arena');
  if(recorder.length)assert.deepEqual(recorder,[{module:'ring3',name:'trace.arena',kind:'global'}]);
  const precise=imports.filter(item=>item.name==='precise');
  if(precise.length)assert.deepEqual(precise,[{module:'ring3',name:'precise',kind:'function'}],
    'exact-prefix ingress has one explicit CPU callback');
  return imports.filter(item=>!item.name.startsWith('link.')&&!item.name.startsWith('support.')&&item.name!=='trace.arena'&&item.name!=='precise');
}

export function coreSupportSource(memory) {
  let source=supportSources.get(memory);
  if(typeof source==='function') {
    source=source();assert(source.length>=8,'production support bytes are available outside a lease');
    supportSources.set(memory,source);
  }
  if(source)return source;
  if(!standaloneSupport) {
    const directory=process.argv[2];
    const path=['support.wasm','dispatch.wasm','driver.wasm'].map(name=>join(directory,name)).find(existsSync);
    assert(path,'standalone generated-Wasm fixture supplies actual production support code');
    standaloneSupport=readFileSync(path);
  }
  return standaloneSupport;
}

export function instantiateSupport(memory,source=coreSupportSource(memory)) {
  const module=new WebAssembly.Module(source);
  const table=()=>new WebAssembly.Table({element:'anyfunc',initial:4097,maximum:4097});
  const exports=new WebAssembly.Instance(module,{ring3:{memory,functions:table(),publishers:table(),
    probe:()=>assert.fail('standalone support never dispatches a guest edge'),
    cold:()=>assert.fail('standalone support never executes cold code')}}).exports;
  assert(exports.supportAbi instanceof WebAssembly.Global&&[1,2,3,4,5].includes(exports.supportAbi.value),'actual current or pinned-baseline support ABI');
  for(const name of supportForAbi(exports.supportAbi.value))assert.equal(typeof exports[name],'function',`actual Wasm ${name}`);
  return exports;
}

function bindings(memory) {
  let exports=supportInstances.get(memory);
  if(!exports){exports=instantiateSupport(memory);supportInstances.set(memory,exports);}
  return {...Object.fromEntries((supportForAbi(exports.supportAbi.value)).map(name=>[name,exports[name]])),'support.abi':exports.supportAbi};
}

export function linkGlobals(module,shared={}) {
  coreImports(module);
  const globals={};
  for(const item of WebAssembly.Module.imports(module).filter(item=>item.name.startsWith('link.')||item.name==='trace.arena')) {
    const local=Number(item.name.slice('link.local'.length));
    const type=item.name==='link.local99'?'i64':local>=76&&local<=83||local>=101&&local<=103?'f64':'i32';
    globals[item.name]=shared[item.name]??new WebAssembly.Global({value:type,mutable:true},type==='i64'?0n:0);
    assert(globals[item.name] instanceof WebAssembly.Global,'compiled-link imports are actual Wasm globals');
  }
  return globals;
}

export function instantiateCore(module,imports) {
  const supplied=imports?.ring3??{};
  const shared=WebAssembly.Module.imports(module).some(item=>item.name.startsWith('support.'))&&
    !WebAssembly.Module.imports(module).filter(item=>item.name.startsWith('support.')&&item.kind==='function').every(item=>typeof supplied[item.name]==='function')?bindings(supplied.memory):{};
  const instance=new WebAssembly.Instance(module,{...imports,
    ring3:{...shared,...linkGlobals(module,supplied),...supplied,
      precise:supplied.precise??preciseCallbacks.get(supplied.memory)??(()=>assert.fail('exact-prefix execution requires the actual CPU adapter')),
      'support.abi':supplied['support.abi']??supplied.supportAbi??shared['support.abi']}});
  const memory=instance.exports.memory,ptr=instance.exports.ring3_abi_v1_core_dispatch_code_ptr,
    len=instance.exports.ring3_abi_v1_core_dispatch_code_len;
  if(memory instanceof WebAssembly.Memory&&typeof ptr==='function'&&typeof len==='function')
    supportSources.set(memory,()=>{const at=ptr()>>>0,length=len()>>>0;
      assert(at>0&&length>=8&&at+length<=memory.buffer.byteLength,'valid production support descriptor');
      return new Uint8Array(memory.buffer).slice(at,at+length);});
  if(memory instanceof WebAssembly.Memory&&typeof instance.exports.ring3_abi_v1_core_precise==='function')
    preciseCallbacks.set(memory,instance.exports.ring3_abi_v1_core_precise);
  return instance;
}
