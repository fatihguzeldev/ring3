const active = new WeakMap();
const nativePhases = new Map([
  ['component_cpu_view','nativeView'], ['component_cpu_dirty','nativeDirty'],
  ['component_cpu_ack_mappings','nativeMappingAck'],
  ['component_cpu_collect_dirty','nativeDirtyCollect'],
  ['component_cpu_gate','nativeGate'], ['component_cpu_publish','nativePublish'],
  ['component_cpu_write_fault','nativeCow'],
]);

export function instrumentV86(cache, measurements) {
  if(cache.open)throw new Error('cannot instrument an active v86 adapter');
  if(typeof measurements?.runExclusive!=='function' || !Array.isArray(measurements.exclusiveStack))
    throw new Error('v86 instrumentation requires synchronous exclusive phase measurements');
  if(measurements.exclusiveStack.length)throw new Error('cannot instrument inside an active measurement scope');
  if(active.has(cache))throw new Error('v86 adapter is already instrumented');
  const {cpu,tables,traps,engine}=cache, installed=[];
  const counters={imports:0,selections:0,executions:0,cpuRuns:0,traps:0,
    refreshCalls:0,mappingRebuilds:0,mappingUpdates:0,scannedMappingEntries:0,clearedPtes:0,
    dirtyPublications:0,scannedPtes:0,scannedDirectories:0,dirtySlots:0,tlbFlushes:0,jitFlushes:0,
    tlbFlushReasons:{mappingChange:0,dirtyReset:0,other:0},
    jitFlushReasons:{hostCodeVersion:0,mappingChange:0,other:0},
    nativeCalls:{nativeView:0,nativeDirty:0,nativeMappingAck:0,nativeDirtyCollect:0,nativeGate:0,nativePublish:0,nativeCow:0}};
  let enabled=true, refreshContext=null, dirtyDepth=0, guestPages=1<<20;
  const phase=(name,operation)=>measurements.runExclusive(name==='run'?'v86.run':`v86.bridge.${name}`,operation);
  const wrap=(owner,name,operation)=>{
    if(!owner || typeof owner[name]!=='function')throw new Error(`missing v86 measurement target ${name}`);
    const original=owner[name],descriptor=Object.getOwnPropertyDescriptor(owner,name);
    const replacement=function(...args){return operation.call(this,original,args);};
    Object.defineProperty(owner,name,{value:replacement,configurable:true,writable:true,
      enumerable:descriptor?.enumerable??false});
    installed.push({owner,name,descriptor,replacement});
  };
  const restoreProperties=()=>{
    for(const {owner,name,replacement} of installed)
      if(Object.getOwnPropertyDescriptor(owner,name)?.value!==replacement)
        throw new Error(`v86 measurement target changed before restore: ${name}`);
    for(let index=installed.length-1;index>=0;index--) {
      const {owner,name,descriptor}=installed[index];
      if(descriptor)Object.defineProperty(owner,name,descriptor);else delete owner[name];
    }
  };
  try {
    for(const [method,name,counter] of [['import','import','imports'],['select','select','selections'],
      ['execute','execute','executions']]) {
      wrap(cache,method,function(original,args){
        counters[counter]++;
        return phase(name,()=>original.apply(this,args));
      });
    }
    wrap(cpu,'main_loop',function(original,args){
      counters.cpuRuns++;
      return phase('run',()=>original.apply(this,args));
    });
    wrap(traps,'capture',function(original,args){
      counters.traps++;
      return phase('trap',()=>original.apply(this,args));
    });
    wrap(tables,'descriptor',function(original,args){
      return phase('descriptor',()=>{
        const result=original.apply(this,args);
        if(Number.isInteger(result?.guestPages))guestPages=result.guestPages;
        return result;
      });
    });
    wrap(tables,'refresh',function(original,args){
      counters.refreshCalls++;
      return phase('pageRefresh',()=>{
        const previous=refreshContext,context={epoch:this.mappingEpoch},oldRows=this.pages.length;
        refreshContext=context;
        try {
          const result=original.apply(this,args);
          if(this.mappingEpoch!==context.epoch) {
            counters.mappingUpdates++;
            if(this.lastMappingFullRebuild??true){counters.mappingRebuilds++;counters.clearedPtes+=oldRows;}
            counters.scannedMappingEntries+=this.lastMappingScanCount??guestPages;
          }
          return result;
        } finally {refreshContext=previous;}
      });
    });
    wrap(tables,'publishDirty',function(original,args){
      counters.dirtyPublications++;
      return phase('dirtyPublication',()=>{
        const count=this.pages.length;dirtyDepth++;
        try {
          const result=original.apply(this,args);
          counters.scannedPtes+=this.lastDirtyScanCount??count;
          counters.scannedDirectories+=this.lastDirtyDirectoryCount??0;
          if(Number.isInteger(result))counters.dirtySlots+=result;
          return result;
        } finally {dirtyDepth--;}
      });
    });
    wrap(cpu,'full_clear_tlb',function(original,args){
      counters.tlbFlushes++;
      counters.tlbFlushReasons[dirtyDepth?'dirtyReset':refreshContext?'mappingChange':'other']++;
      return phase('tlbFlush',()=>original.apply(this,args));
    });
    wrap(cpu,'jit_clear_cache',function(original,args){
      counters.jitFlushes++;
      const reason=!refreshContext?'other':tables.mappingEpoch!==refreshContext.epoch?'mappingChange':'hostCodeVersion';
      counters.jitFlushReasons[reason]++;
      return phase('jitFlush',()=>original.apply(this,args));
    });
    wrap(engine,'ok',function(original,args){
      const name=nativePhases.get(args[0]);
      if(!name)return original.apply(this,args);
      counters.nativeCalls[name]++;
      return phase(name,()=>original.apply(this,args));
    });
  } catch(error) {
    try {restoreProperties();}catch(cleanup){error.cleanupFailure=cleanup.message;}
    throw error;
  }
  const snapshot=()=>({enabled,counters:{...counters,
    tlbFlushReasons:{...counters.tlbFlushReasons},jitFlushReasons:{...counters.jitFlushReasons},
    nativeCalls:{...counters.nativeCalls}},
    semantics:{
      timing:'selfElapsedNs is synchronous exclusive time; elapsedNs includes child phases and must not be added to them',
      run:'synchronous v86 main_loop excluding measured bridge trap handling; includes CPU code generation, not later asynchronous Wasm compilation',
      import:'self time excludes page refresh; includes canonical state import and adapter bookkeeping',
      execute:'self time includes state export, accounting and fault classification; pure state export is not separately measured',
      scans:'completed harvest candidates use policy scan counts; native validation and clear passes are included in elapsed time but not counted again; mappingRebuilds counts only full rebuilds',
      flushes:'adapter calls through JS cpu properties only; internal v86 Wasm flushes are included in v86.run',
      off:'restore removes all wrappers; no stage clocks or instrumentation counter writes remain',
    }});
  const controller={snapshot,restore(){
    if(!enabled)return snapshot();
    if(cache.open || measurements.exclusiveStack.length)throw new Error('cannot restore active v86 measurements');
    restoreProperties();enabled=false;active.delete(cache);
    return snapshot();
  }};
  active.set(cache,controller);
  return controller;
}
