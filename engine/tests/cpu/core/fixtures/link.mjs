import assert from 'node:assert/strict';
import {existsSync,readFileSync} from 'node:fs';
import {join} from 'node:path';
import {bindNativeRecovery,moduleRecovery,retainedRecoveryBase} from '../../../../../sdk/runtime/recovery.js';

const legacyFields=[2,3,4,5,6,7,8,9,10,11,12,13,20,21,22,23,24,25,
  76,77,78,79,80,81,82,83,84,85,86,87,88,89,90,91,92,93,94,95,96,99,100,101,102,103];
const fields=[2,3,4,5,6,7,8,9,10,11,12,13,20,21,22,23,24,25,99];
const linkNames=fields=>['link.mode','link.valid','link.context',...fields.map(index=>`link.local${index}`)];
const previousSupportNames=['flags','materialize_numeric','resolve_status','translate_page','acquire_group',
  'store8','store16','store32','commit_pool_cold','commit_native','commit_raw'].map(name=>`support.${name}`);
const recorderSupportNames=[...previousSupportNames,'support.record_trace'];
export const supportNames=[...recorderSupportNames,'support.recover'];
const poolSupportNames=[...supportNames.slice(0,8),'support.commit_pool'];
const legacySupportNames=[...supportNames.slice(0,8),'support.normalize_kernel'];
const supportForAbi=abi=>abi===1?legacySupportNames:abi===2?poolSupportNames:abi<5?previousSupportNames:abi===5?recorderSupportNames:supportNames;
const supportSources=new WeakMap(),supportInstances=new WeakMap();
const preciseCallbacks=new WeakMap();
const recoveryProviders=new WeakMap(),standaloneRecovery=new WeakMap();
let standaloneSupport,standaloneMemoryIdentity=0n;

export function coreImports(module) {
  const imports=WebAssembly.Module.imports(module),links=imports.filter(item=>item.name.startsWith('link.'));
  if(links.length)assert.deepEqual(links,[...linkNames(links.length===47?legacyFields:fields),...(links.length===24?['link.continuation','link.port']:[])].map(name=>({module:'ring3',name,kind:'global'})),
    'the complete typed compiled-link state is the only added import surface');
  const support=imports.filter(item=>item.name.startsWith('support.'));
  if(support.length)assert.deepEqual(support,[...(support.some(item=>item.name==='support.normalize_kernel')?legacySupportNames:support.some(item=>item.name==='support.commit_pool')?poolSupportNames:support.some(item=>item.name==='support.recover')?supportNames:support.some(item=>item.name==='support.record_trace')?recorderSupportNames:previousSupportNames).map(name=>({module:'ring3',name,kind:'function'})),
    {module:'ring3',name:'support.abi',kind:'global'}], 'the complete versioned per-CPU support ABI is imported directly');
  const recorder=imports.filter(item=>item.name==='trace.arena');
  if(recorder.length)assert.deepEqual(recorder,[{module:'ring3',name:'trace.arena',kind:'global'}]);
  const precise=imports.filter(item=>item.name==='precise');
  if(precise.length)assert.deepEqual(precise,[{module:'ring3',name:'precise',kind:'function'}],
    'exact-prefix ingress has one explicit CPU callback');
  const scratch=imports.filter(item=>item.name==='recovery.scratch');
  if(scratch.length)assert.deepEqual(scratch,[{module:'ring3',name:'recovery.scratch',kind:'global'}]);
  const recovery=imports.filter(item=>item.name==='recovery.base');
  if(recovery.length) {assert.deepEqual(recovery,[{module:'ring3',name:'recovery.base',kind:'global'}]);moduleRecovery(module);}
  return imports.filter(item=>!item.name.startsWith('link.')&&!item.name.startsWith('support.')&&item.name!=='trace.arena'&&item.name!=='precise'&&item.name!=='recovery.base'&&item.name!=='recovery.scratch');
}

export function registerRecoveryProvider(memory,api) {
  assert(memory instanceof WebAssembly.Memory);
  const names=['core_context_ptr','core_code_ptr','core_code_len'];
  const provider=Object.fromEntries(names.map(name=>{assert.equal(typeof api[name],'function');return[name,api[name]];}));
  for(const name of['core_continuation_ptr','core_recovery_scratch_ptr'])if(typeof api[name]==='function')provider[name]=api[name];
  assert(!standaloneRecovery.has(memory),'real engine memory cannot use a standalone metadata arena');
  const previous=recoveryProviders.get(memory);
  if(previous)assert(names.every(name=>previous[name]===provider[name]),'one actual engine provider per memory');
  if(previous)for(const name of['core_continuation_ptr','core_recovery_scratch_ptr'])
    if(previous[name]&&provider[name])assert.equal(previous[name],provider[name],'stable native private recovery provider');
  recoveryProviders.set(memory,Object.freeze({...previous,...provider}));
}

export function registerStandaloneRecovery(memory,bytes,{transientCpu=false}={}) {
  assert(memory instanceof WebAssembly.Memory);
  assert.equal(typeof transientCpu,'boolean');
  assert(!recoveryProviders.has(memory)&&!standaloneRecovery.has(memory),'one explicit standalone arena per memory');
  assert(Number.isInteger(bytes)&&bytes>0&&bytes<=16*1024*1024,'bounded standalone metadata reservation');
  const start=memory.buffer.byteLength,pages=Math.ceil(bytes/65536),transport=132096+136;
  assert(!transientCpu||bytes>=transport,'standalone CPU transport fits the existing reservation');
  assert(start+pages*65536<=0x100000000,'standalone metadata fits Wasm32');
  memory.grow(pages);
  const arena={next:start,end:start+bytes,modules:new WeakMap()};
  if(transientCpu) {
    assert(standaloneMemoryIdentity<0xffffffffffffffffn,'distinct standalone memory identity');
    const scratch=start,header=start+132096,d=new DataView(memory.buffer);
    const global=value=>new WebAssembly.Global({value:'i32',mutable:false},value|0);
    arena.cpu={scratch,header,memoryIdentity:++standaloneMemoryIdentity,nonce:0n,active:false,
      scratchBase:global(scratch)};
    for(const[offset,value]of[[0,0x5233504b],[4,1],[8,132096],[12,8192],[20,header],[40,1024]])
      d.setUint32(scratch+offset,value,true);
    d.setBigUint64(scratch+24,arena.cpu.memoryIdentity,true);
    d.setUint32(header,2,true);d.setBigUint64(header+24,arena.cpu.memoryIdentity,true);
    arena.next+=transport;
  }
  standaloneRecovery.set(memory,arena);
}

export function beginStandaloneRecovery(memory,context,identity) {
  const arena=standaloneRecovery.get(memory),cpu=arena?.cpu;
  assert(cpu&&!recoveryProviders.has(memory),'only the explicit standalone CPU host can arm this transport');
  assert(Number.isInteger(context)&&context>0&&context+128<=cpu.scratch,'bounded authored CPU context');
  assert(typeof identity==='bigint'&&identity>0n&&identity<=0xffffffffffffffffn,'immutable selected module identity');
  const d=new DataView(memory.buffer),{scratch,header}=cpu;
  assert(!cpu.active&&d.getUint32(scratch+48,true)===0&&d.getBigUint64(scratch+32,true)===0n&&
    d.getBigUint64(scratch+152,true)===0n,'standalone leases never overlap');
  assert(cpu.nonce<0xffffffffffffffffn,'standalone lease nonce never aliases an earlier lease');
  cpu.nonce++;
  new Uint8Array(memory.buffer,header,136).fill(0);
  for(const[offset,value]of[[0,2],[4,1],[8,context],[96,d.getUint32(context+32,true)],[100,0xffffffff],[112,1]])
    d.setUint32(header+offset,value,true);
  for(const[offset,value]of[[16,cpu.nonce],[24,cpu.memoryIdentity],[80,identity],[104,cpu.nonce]])
    d.setBigUint64(header+offset,value,true);
  d.setUint32(scratch+16,context,true);d.setBigUint64(scratch+32,cpu.nonce,true);
  cpu.active=true;
}

export function endStandaloneRecovery(memory) {
  const cpu=standaloneRecovery.get(memory)?.cpu;
  assert(cpu?.active&&!recoveryProviders.has(memory),'one explicit standalone CPU lease is active');
  const d=new DataView(memory.buffer),{scratch,header}=cpu;
  d.setUint32(scratch+48,0,true);d.setBigUint64(scratch+32,0n,true);d.setBigUint64(scratch+152,0n,true);
  d.setUint32(header+4,0,true);d.setBigUint64(header+16,0n,true);
  d.setBigUint64(header+80,0n,true);d.setBigUint64(header+104,0n,true);d.setUint32(header+112,0,true);
  cpu.active=false;
}

export function recoveryBindings(module,memory,retainedBase) {
  const metadata=moduleRecovery(module),scratch=scratchBindings(module,memory);
  if(!metadata)return scratch;
  assert(memory instanceof WebAssembly.Memory,'recovery metadata uses its actual Wasm memory');
  if(retainedBase!==undefined)return {...scratch,
    'recovery.base':retainedRecoveryBase(module,memory,retainedBase)};
  const provider=recoveryProviders.get(memory);
  if(provider) {
    const rawContext=provider.core_context_ptr();
    assert(Number.isInteger(rawContext)&&rawContext>=-0x80000000&&rawContext<=0xffffffff,'32-bit native context pointer');
    const context=rawContext<0?rawContext+0x100000000:rawContext;
    assert(context>0&&context%8===0&&context+128<=memory.buffer.byteLength,'valid selected native recovery owner');
    const identity=new DataView(memory.buffer).getBigUint64(context+120,true);
    const rawPointer=provider.core_code_ptr(),length=provider.core_code_len();
    assert(Number.isInteger(rawPointer)&&rawPointer>=-0x80000000&&rawPointer<=0xffffffff,'32-bit native module pointer');
    const pointer=rawPointer<0?rawPointer+0x100000000:rawPointer;
    assert(pointer>0&&Number.isInteger(length)&&length>=8&&length<=16*1024*1024&&
      pointer+length<=memory.buffer.byteLength,'valid selected native module span');
    const source=new Uint8Array(memory.buffer).slice(pointer,pointer+length);
    assert.equal(new DataView(memory.buffer).getBigUint64(context+120,true),identity,'selected owner remains stable');
    return {...scratch,'recovery.base':bindNativeRecovery(module,source,{memory,pointer,length,identity})};
  }
  // Standalone generated-Wasm controls have no Rust allocation or engine heap.
  // Their exact same portable blob gets a private tail outside guest mappings.
  const arena=standaloneRecovery.get(memory);
  assert(arena,'standalone metadata requires explicit unmapped-tail reservation before fixture views');
  let base=arena.modules.get(module);
  if(!base) {
    const pointer=arena.next;
    assert(pointer+metadata.bytes.length<=arena.end,'standalone metadata stays inside its reserved private tail');
    new Uint8Array(memory.buffer,pointer,metadata.bytes.length).set(metadata.bytes);
    base=new WebAssembly.Global({value:'i32',mutable:false},pointer>=0x80000000?pointer-0x100000000:pointer);
    arena.next+=metadata.bytes.length;arena.modules.set(module,base);
  }
  return {...scratch,'recovery.base':base};
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
  assert(exports.supportAbi instanceof WebAssembly.Global&&[1,2,3,4,5,6].includes(exports.supportAbi.value),'actual current or pinned-baseline support ABI');
  for(const name of supportForAbi(exports.supportAbi.value))assert.equal(typeof exports[name],'function',`actual Wasm ${name}`);
  const provider=recoveryProviders.get(memory);
  if(exports['link.continuation'] instanceof WebAssembly.Global) {
    if(typeof provider?.core_continuation_ptr==='function')
      exports['link.continuation'].value=provider.core_continuation_ptr()|0;
    else if(standaloneRecovery.get(memory)?.cpu)
      exports['link.continuation'].value=standaloneRecovery.get(memory).cpu.header|0;
  }
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
    globals[item.name]=shared[item.name]??(item.name==='link.continuation'?supportInstances.get(shared.memory)?.[item.name]:undefined)??new WebAssembly.Global({value:type,mutable:true},type==='i64'?0n:0);
    assert(globals[item.name] instanceof WebAssembly.Global,'compiled-link imports are actual Wasm globals');
  }
  return globals;
}

// Diagnostic owners keep the production SDK lease and native owner authority.
// Public legacy exports retain their selected-owner contract under ABI5.
export function cacheCallbacks(cache) {
  registerRecoveryProvider(cache.memory,cache.native);
  return {
    numeric:cache.continuationCore
      ? (context,pc,ordinal)=>cache.open?cache.invokeNumeric(context,pc,ordinal)
        :cache.native.core_numeric(context,ordinal)
      : cache.native.core_numeric,
    precise:typeof cache.native.core_precise==='function'
      ? context=>cache.open?cache.invokePrecise(context):cache.native.core_precise(context) : undefined,
  };
}

export function cacheResolver(cache) {
  const resolve=cache.continuationCore?cache.native.core_chain_resolve_owned
    : cache.native.core_chain_select??cache.native.core_chain_probe;
  assert.equal(typeof resolve,'function','the negotiated dispatcher uses its actual native resolver');
  return resolve;
}

export function resolverExport(api) {
  return typeof api.core_chain_resolve_owned==='function'?'core_chain_resolve_owned'
    :typeof api.core_chain_select==='function'?'core_chain_select':'core_chain_probe';
}

export function sourceLookup(cache,pc=cache.pc) {
  cache.pristine();
  assert.equal(cache.native.core_lookup(pc)>>>0,0,'explicit native source lookup retains its private port');
  const context=cache.native.core_context_ptr()>>>0,buffer=cache.memory.buffer;
  assert(context>0&&context%8===0&&context+128<=buffer.byteLength,'valid retained source context');
  assert.equal(context,cache.context,'source lookup retains the existing CPU context');
  const id=new DataView(buffer).getBigUint64(context+120,true),unit=cache.units.get(id);
  assert(id!==0n&&unit,'the exact source owner is already instantiated and retained');
  return unit;
}

export function sourceInspectionApi(api,result) {
  if(typeof api.core_chain_resolve_executable_owned!=='function')return api;
  const resolver=resolverExport(api);
  result.manualSourceInspection={automaticFormation:false,resolver,
    scope:'forced intermediate-state inspection through the retained legacy source-selection contract'};
  // Keep every negotiated capability and its stable binding. These authored
  // controls deliberately inspect short/private source ingress; ordinary
  // executable selection is exercised separately by the natural SDK cases.
  return Object.freeze({...api,core_chain_resolve_executable_owned:(context,pc,retired,remaining)=>{
    assert(Number.isInteger(remaining)&&remaining>=0&&remaining<=0xffffffff,'actual live executable quota');
    return api[resolver](context,pc,retired);
  }});
}

export function bindCacheContinuation(cache,exports) {
  if(!cache.continuationCore)return;
  const pointer=cache.native.core_continuation_ptr()>>>0;
  assert(pointer>0&&pointer%8===0&&pointer+120<=cache.memory.buffer.byteLength,
    'the measured dispatcher uses the real stable native continuation allocation');
  assert(exports['link.continuation'] instanceof WebAssembly.Global);
  assert(exports['link.port'] instanceof WebAssembly.Global);
  exports['link.continuation'].value=pointer;exports['link.port'].value=0;
}

export function forceNativeInspection(cache,result) {
  if(!cache.continuationCore)return;
  const dispatch=cache.dispatch,run=dispatch.run;
  bindCacheContinuation(cache,dispatch);
  result.forcedNativeInspection=true;
  cache.dispatch={...dispatch,run(...args){
    // keep the real ticket/header available to shared recovery; only the
    // optional generated directory route is disabled for this inspection.
    const pointer=dispatch['link.continuation'].value>>>0,buffer=cache.memory.buffer;
    assert(pointer>0&&pointer%8===0&&pointer+120<=buffer.byteLength,'the diagnostic retains its actual continuation header');
    const view=new DataView(buffer),format=view.getUint32(pointer,true);
    assert([1,2].includes(format),'the diagnostic uses a negotiated continuation format');
    const rows=view.getUint32(pointer+32,true);
    view.setUint32(pointer+32,0,true);
    try{return Reflect.apply(run,dispatch,args);}
    finally{new DataView(cache.memory.buffer).setUint32(pointer+32,rows,true);}
  }};
}

function scratchBindings(module,memory) {
  const imports=WebAssembly.Module.imports(module).filter(item=>item.name==='recovery.scratch');
  if(!imports.length)return {};
  assert.deepEqual(imports,[{module:'ring3',name:'recovery.scratch',kind:'global'}]);
  assert(memory instanceof WebAssembly.Memory,'shared recovery uses its actual Wasm memory');
  const provider=recoveryProviders.get(memory),standalone=standaloneRecovery.get(memory)?.cpu;
  if(!provider&&standalone)return {'recovery.scratch':standalone.scratchBase};
  assert.equal(typeof provider?.core_recovery_scratch_ptr,'function','shared recovery requires the actual CPU-private workspace');
  const pointer=provider.core_recovery_scratch_ptr()>>>0;
  assert(pointer>0&&pointer%8===0&&pointer+132096<=memory.buffer.byteLength,'bounded native shared recovery workspace');
  return {'recovery.scratch':new WebAssembly.Global({value:'i32',mutable:false},pointer|0)};
}

export function instantiateCore(module,imports) {
  const supplied=imports?.ring3??{};
  const shared=WebAssembly.Module.imports(module).some(item=>item.name.startsWith('support.'))&&
    !WebAssembly.Module.imports(module).filter(item=>item.name.startsWith('support.')&&item.kind==='function').every(item=>typeof supplied[item.name]==='function')?bindings(supplied.memory):{};
  const recovery=recoveryBindings(module,supplied.memory,supplied['recovery.base']);
  if(supplied['recovery.scratch']!==undefined) {
    assert(supplied['recovery.scratch'] instanceof WebAssembly.Global,'supplied workspace is a typed global');
    assert.equal(supplied['recovery.scratch'].value,recovery['recovery.scratch']?.value,'supplied workspace matches the actual CPU binding');
  }
  const instance=new WebAssembly.Instance(module,{...imports,
    ring3:{...shared,...linkGlobals(module,supplied),...supplied,...recovery,
      numeric:(...args)=>args.length===3?supplied.numeric(args[0],args[2]):supplied.numeric(...args),
      precise:supplied.precise??preciseCallbacks.get(supplied.memory)??(()=>assert.fail('exact-prefix execution requires the actual CPU adapter')),
      'support.abi':supplied['support.abi']??supplied.supportAbi??shared['support.abi']}});
  const memory=instance.exports.memory,ptr=instance.exports.ring3_abi_v1_core_dispatch_code_ptr,
    len=instance.exports.ring3_abi_v1_core_dispatch_code_len;
  if(memory instanceof WebAssembly.Memory&&typeof ptr==='function'&&typeof len==='function')
    supportSources.set(memory,()=>{const at=ptr()>>>0,length=len()>>>0;
      assert(at>0&&length>=8&&at+length<=memory.buffer.byteLength,'valid production support descriptor');
      return new Uint8Array(memory.buffer).slice(at,at+length);});
  if(memory instanceof WebAssembly.Memory&&typeof instance.exports.ring3_abi_v1_core_code_ptr==='function')
    registerRecoveryProvider(memory,Object.fromEntries(['core_context_ptr','core_code_ptr','core_code_len','core_continuation_ptr','core_recovery_scratch_ptr']
      .map(name=>[name,instance.exports['ring3_abi_v1_'+name]])));
  if(memory instanceof WebAssembly.Memory&&typeof instance.exports.ring3_abi_v1_core_precise==='function')
    preciseCallbacks.set(memory,instance.exports.ring3_abi_v1_core_precise);
  return instance;
}
