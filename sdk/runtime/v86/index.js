import {V86} from './vendor/libv86.mjs';
import {KEY} from '../engine.js';
import {CPU_SHA256,PHYSICAL_BYTES,PARK,SHIPPING_JIT,CODEGEN_SWITCHES,BACKING_BASE,PAGE} from './layout.js';
import {PageTables} from './memory.js';
import {Traps} from './traps.js';
import {createPortBridge} from './ports.js';
import {cpuViews,importState,exportState,rememberStateBanks,forgetStateBanks,validateSegmentSelectors} from './state.js';

const INLINE_GATE_IDS=new Set([0x10001,0x10002,0xf000b,0x40225,0x4022a,0x4022e,0x40232,0x4023f]);
const DIRECT_BINDING_GATE_IDS=new Set([0x4023d,0x40253,0x40255]);
const DIRECT_STATE_INPUT_GATE_IDS=new Set([0x40228,0x4022c]);
const GUEST_RETURN_GATE_IDS=new Set([0x10001,0x40225,0x4022a,0x4022e,0x40232,0x4023f,0x4023d,0x40253,0x40255]);

export async function prepareV86(options={}, {signal}={}) {
  const memorySize=options.memorySize??PHYSICAL_BYTES;
  if(!Number.isInteger(memorySize) || memorySize<PHYSICAL_BYTES || memorySize>256*1024*1024)
    throw new Error('v86 physical memory must be between160 and256MiB');
  const response=await fetch(options.wasmUrl??new URL('./vendor/v86.wasm',import.meta.url),{signal});
  if(!response.ok)throw new Error(`v86 CPU HTTP${response.status}`);
  const bytes=await response.arrayBuffer();
  const hash=Array.from(new Uint8Array(await crypto.subtle.digest('SHA-256',bytes)),n=>n.toString(16).padStart(2,'0')).join('');
  const expected=options.sha256??options.cpuSha256??CPU_SHA256;
  if(hash!==expected || hash!==CPU_SHA256)throw new Error(`unsupported v86 CPU hash ${hash}`);
  const module=await WebAssembly.compile(bytes);
  signal?.throwIfAborted();
  let portBridge,portCache=null,portBridgeOriginalIO,portBridgeFallback;
  const emulator=new V86({autostart:false,memory_size:memorySize,log_level:0,
    wasm_fn:async imports=>{
      // ring3 supplies process time and devices; the cpu's machine timers stay inactive.
      imports.env.run_hardware_timers=()=>0;
      portBridgeOriginalIO=imports.env.io_port_write8;
      portBridgeFallback=(port,value)=>portCache?portCache.forwardPortIO(portBridgeOriginalIO,port,value):portBridgeOriginalIO(port,value);
      portBridge=createPortBridge(portBridgeFallback);imports.env.io_port_write8=portBridge.write8;
      return (await WebAssembly.instantiate(module,imports)).exports;
    }});
  await new Promise((resolve,reject)=>{
    const timeout=setTimeout(()=>reject(new Error('v86 initialization timed out')),15000);
    emulator.add_listener('emulator-loaded',()=>{clearTimeout(timeout);resolve();});
  });
  const cpu=emulator.v86.cpu,ex=cpu.wm.exports;
  cpu.reset_cpu();
  const jitConfig={};
  for(const [index,value] of SHIPPING_JIT) {
    ex.set_jit_config(index,value);
    if(ex.get_jit_config(index)!==value)throw new Error(`v86 JIT configuration mismatch ${index}`);
    jitConfig[index]=value;
  }
  for(const [name,value] of CODEGEN_SWITCHES) {
    ex[`set_${name}`](value);
    if(ex[`get_${name}`]()!==value)throw new Error(`v86 code generation mismatch ${name}`);
  }
  ex.set_relaxed_fpu(0);
  if(ex.get_relaxed_fpu()!==0)throw new Error('v86 strict FPU mode was not applied');
  const hp=ex.get_hypercall_page_ptr()>>>0,d=new DataView(cpu.wasm_memory.buffer);
  d.setUint32(hp+8,0,true);
  ex.set_park_eip(PARK);
  let generated=0,generatedBytes=0,finalized=0,attached=false,disposed=false;
  cpu.test_hook_did_generate_wasm=encoded=>{generated++;generatedBytes+=encoded.byteLength;};
  cpu.test_hook_did_finalize_wasm=()=>{finalized++;};
  const runtime={cpu,memory:cpu.wasm_memory,hash,portBridge,portBridgeOriginalIO,portBridgeFallback,
    async attach(engine,execution={}) {
      if(attached || disposed)throw new Error('v86 runtime cannot attach twice');
      signal?.throwIfAborted();
      if(engine.memory!==runtime.memory)throw new Error('v86 and Ring3 must share one Wasm memory');
      const cache=new V86CodeCache(engine,execution,cpu,runtime);
      cache.portBridge=portBridge;
      try {cache.portBridgeBound=portBridge.bind(cache)===true;}
      catch(error){try{cache.close();}catch(cleanup){error.cleanupFailure=cleanup.message;}throw error;}
      portCache=cache;
      attached=true;return cache;
    },
    counters:()=>({generatedModules:generated,finalizedModules:finalized,generatedModuleBytes:generatedBytes}),
    jitConfig,
    dispose() {if(disposed)return;disposed=true;portBridge.close();portCache=null;emulator.destroy();},
  };
  return runtime;
}

export const createV86Runtime=prepareV86;

export class V86CodeCache {
  constructor(engine,execution,cpu,runtime) {
    this.engine=engine;this.memory=engine.memory;this.cpu=cpu;this.runtime=runtime;
    const budget=execution.budget??50000;
    if(!Number.isInteger(budget) || budget<1 || budget>0xffffffff)throw new Error('invalid v86 execution budget');
    this.budget=Math.min(budget,100002);
    this.tables=new PageTables(engine,cpu);
    try {this.traps=new Traps(cpu,address=>this.readCodeByte(address));}catch(error){this.tables.rollback(error);throw error;}
    this.imported=false;this.closed=false;this.open=false;
    this.runs=0;this.regions=0;this.retired=0;this.work=0;this.gates=0;this.faults=0;
    this.dirtyPages=0;this.cowFaults=0;
    this.deferDirty=true;this.dirtyPending=false;this.deferredDirtyPublications=0;
    this.reuseNumericState=true;
    this.portGates=false;this.portGateModeTouched=false;this.portGateCalls=0;this.deniedPortCalls=0;
    this.portGateMode=0;this.portGatePatchedCount=0;
    this.inlineGates=false;this.inlineCompletions=0;this.inlineYields=0;this.inlineFailure=null;
    this.directGates=false;this.directCompletions=0;this.directDeclines=0;
    this.directBindings=false;this.directBindingCompletions=0;
    this.directStateInputs=false;this.directStateInputCompletions=0;
    this.guestReturns=false;this.guestReturnCompletions=0;
    this.graphicsOverlap=false;
    this.portBridge=null;this.portBridgeBound=false;this.wasmPorts=false;this.wasmPortsActive=false;
    this.wasmPortCompletions=0;this.wasmPortDeclines=0;this.wasmPortCheckpoints=0;this.portBridgeDeclinedPc=null;
    this.executionPolicyInitialized=false;
  }
  initializeExecutionPolicy() {
    if(this.inlineFailure)throw this.inlineFailure;
    if(this.executionPolicyInitialized || this.runs!==0 || this.imported || this.open || this.closed ||
        this.tables.ownsPin!==true || typeof this.inlineOwner?.canContinue!=='function' ||
        typeof this.inlineOwner?.observeClock!=='function')
      throw new Error('v86 startup policy requires its fresh closed owner');
    const api=this.engine.api,owner={paused:true,open:false};
    try {
      this.configureDirtyDeferral(true,owner);
      const inlineSupported=this.tables.dirtyGateFlagsSupported===true &&
        typeof api.component_cpu_set_gate_mode==='function' &&
        (typeof api.component_cpu_try_inline_gate==='function' || typeof api.component_cpu_try_complete_gate==='function');
      if(inlineSupported) {
        this.configurePortGates(true,owner);this.configureInlineGates(true,owner);
        if(typeof api.component_cpu_try_direct_gate==='function') {
          this.configureDirectGates(true,owner);
          if(typeof api.component_cpu_try_direct_gate_mask==='function')this.configureDirectBindings(true,owner);
        }
      }
      if(this.tables.globalCodeSupported===true)this.tables.configureGlobalCode(true,owner);
      if(this.tables.targetedDirtySupported===true && this.tables.nativeDirty && this.tables.policy.incrementalSync)
        this.tables.configureTargetedDirty(true,owner);
      this.executionPolicyInitialized=true;
    } catch(error) {this.inlineFailure=error;throw error;}
  }
  get pc() {return this.engine.get(48);}
  get guestReturnsSupported() {return this.tables.guestReturnsSupported===true;}
  get directStateInputsSupported() {return this.tables.directStateInputsSupported===true && typeof this.engine.api.component_cpu_try_direct_gate_mask==='function';}
  get wasmPortsSupported() {return this.portBridgeBound===true && this.tables.dirtyGateFlagsSupported===true && typeof this.engine.api.component_cpu_try_direct_gate_mask==='function';}
  configureWasmPorts(enabled,owner) {
    if(owner?.paused!==true || owner?.open!==false || this.open || this.closed || typeof enabled!=='boolean')
      throw new Error('v86 Wasm port changes require a closed paused owner');
    if(this.inlineFailure)throw this.inlineFailure;
    if(enabled && !this.wasmPortsSupported)throw new Error('native v86 Wasm ports are unsupported');
    this.publish();this.harvestPortBridge();this.portBridge?.disarm();this.wasmPortsActive=false;
    this.wasmPorts=enabled;return enabled;
  }
  canUseWasmPorts() {
    const owner=this.inlineOwner;
    return this.wasmPorts && this.wasmPortsSupported && !this.closed && !this.inlineFailure &&
      this.portGates && this.portGateMode===1 && !this.guestReturns && this.inlineGates && this.directGates && this.deferDirty &&
      !!owner?.canContinue() && (typeof owner.canBridge!=='function' || owner.canBridge());
  }
  harvestPortBridge() {
    if(!this.portBridgeBound)return null;
    const result=this.portBridge.harvest(),count=result.completed;
    this.wasmPortCompletions+=count;this.wasmPortDeclines+=result.declined;this.wasmPortCheckpoints+=result.checkpoints;
    this.directCompletions+=count;this.inlineCompletions+=count;this.gates+=count;this.portGateCalls+=count;
    this.deferredDirtyPublications+=count;this.directDeclines+=result.declined;
    this.directBindingCompletions+=result.bindings;this.directStateInputCompletions+=result.stateInputs;
    return result;
  }
  forwardPortIO(original,port,value) {
    if(!this.wasmPortsActive)return original(port,value);
    const result=this.harvestPortBridge(),previous=this.portBridgeDeclinedPc;
    this.portBridgeDeclinedPc=result.declinedPc;
    if(!this.canUseWasmPorts()){this.portBridge.disarm();this.wasmPortsActive=false;}
    try {return original(port,value);}finally{this.portBridgeDeclinedPc=previous;}
  }
  readCodeByte(address) {
    const {memory,tables}=this,page=address>>>12,buffer=memory.buffer;
    let views=this.codeByteViews;
    if(!views || views.buffer!==buffer || views.mapsPointer!==tables.maps || views.rightsPointer!==tables.rights)
      this.codeByteViews=views={buffer,mapsPointer:tables.maps,rightsPointer:tables.rights,
        maps:new Uint32Array(buffer,tables.maps,1<<20),rights:new Uint8Array(buffer,tables.rights,1<<20),
        bytes:new Uint8Array(buffer)};
    const mapped=views.maps[page];
    if(!mapped || mapped>tables.capacity || !(views.rights[page]&7))return null;
    return views.bytes[tables.ram+BACKING_BASE+(mapped-1)*PAGE+(address&4095)];
  }
  import() {
    if(this.inlineFailure)throw this.inlineFailure;
    if(this.closed)throw new Error('v86 adapter is closed');
    this.tables.refresh();
    importState(this.engine,this.cpu,this.traps);
    this.traps.pending=null;this.imported=true;
  }
  select() {
    if(this.inlineFailure)throw this.inlineFailure;
    if(!this.imported)this.import();
    return {id:'v86',pc:this.pc};
  }
  execute() {
    if(this.inlineFailure)throw this.inlineFailure;
    if(!this.imported)this.import();
    const cpu=this.cpu,h=this.engine,hp=cpu.wm.exports.get_hypercall_page_ptr()>>>0;
    let work=0,retired=0,stop=2,detail=0,auxiliary=0,vector=0,trap=null,deferAllowed=false;
    for(let attempt=0;attempt<8;attempt++) {
      const before=cpuViews(this.memory).data.getUint32(664,true);
      new DataView(this.memory.buffer).setUint32(hp,this.budget,true);
      let bridgeRun=false;
      this.open=true;
      try {
        if(this.canUseWasmPorts()) {
          this.dirtyPending=true;this.wasmPortsActive=true;bridgeRun=true;
          if(this.portBridge.arm((this.directBindings?0x7ff:0xff)|(this.directStateInputs?0x1800:0))!==true)
            throw new Error('v86 Wasm port bridge failed to arm');
        }
        cpu.main_loop();
      } catch(error) {
        if(!this.traps.isPortFault?.(error)){if(bridgeRun)this.inlineFailure??=error;throw error;}
      } finally {
        try {if(bridgeRun)this.harvestPortBridge();}
        finally {if(bridgeRun){this.portBridge.disarm();this.wasmPortsActive=false;}this.open=false;}
      }
      const delta=(cpuViews(this.memory).data.getUint32(664,true)-before)>>>0;
      trap=this.traps.pending;
      if(!delta && !trap)throw new Error(`v86 made no progress at0x${cpuViews(this.memory).data.getUint32(556,true).toString(16)}`);
      work+=delta;retired+=Math.max(0,delta-(trap?.privateInstructions??(trap?2:0)));
      if(trap?.registers)cpuViews(this.memory).regs.set(trap.registers);
      if(!(trap?.portGate && trap.privateInstructions===0 && trap.canonicalExported===true)) {
        exportState(h,cpu,trap);
        if(this.reuseNumericState!==false)rememberStateBanks(h,cpu);
      }
      this.dirtyPending=true;
      if(trap?.vector===14 && (trap.error&2) && this.tables.isCopyOnWrite(trap.address)) {
        this.publish();
        h.ok('component_cpu_write_fault',...KEY,trap.address);
        this.cowFaults++;this.import();
        if(attempt===7)throw new Error('v86 repeated copy-on-write fault');
        continue;
      }
      break;
    }
    if(trap && !trap.budgetYield) {
      vector=trap.vector;
      if(vector===6) {
        h.ok('component_cpu_gate',...KEY,trap.pc);
        const result=h.resultView(8),gate=result.getUint32(0,true);
        deferAllowed=this.tables.dirtyGateFlagsSupported===true && (result.getUint32(4,true)&1)!==0;
        if(gate && (!trap.portGate || (result.getUint32(4,true)&2)!==0)) {
          stop=7;vector=gate;this.gates++;if(trap.portGate)this.portGateCalls++;
        } else {
          stop=9;this.faults++;
          if(trap.portGate){vector=13;detail=0;this.deniedPortCalls++;}
        }
      } else if(vector===14) {
        stop=3;detail=trap.address;
        const access=trap.error&2?2:trap.address===trap.pc?4:1;
        auxiliary=1|(access<<8)|(this.tables.faultReason(trap.address)<<16);vector=(1<<8)|14;this.faults++;
      } else if(vector===0x29 || vector===0x2e) {
        throw new Error(`unsupported v86 software system entry INT${vector.toString(16)} at0x${trap.pc.toString(16)}`);
      } else if([0,1,3,4,5,7,10,11,12,13,16,19].includes(vector)) {
        stop=9;detail=vector===16?h.get(4236+18)&0xffff:vector===19?h.get(4364+16):trap.error;
        this.faults++;
        if(trap.portDenied)this.deniedPortCalls++;
      } else throw new Error(`unsupported v86 CPU exception ${vector} at0x${trap.pc.toString(16)}`);
    }
    if(work>0xffffffff || retired>work)throw new Error('invalid v86 execution accounting');
    if(this.deferDirty && stop===7 && deferAllowed)this.deferredDirtyPublications++;
    else this.publish();
    h.ok('component_cpu_publish',...KEY,stop,detail,auxiliary,vector,retired,work);
    this.imported=false;this.traps.pending=null;this.runs++;this.regions++;
    this.retired+=retired;this.work+=work;
    return {pc:this.pc,coreReason:stop,reason:h.get(72),detail:h.get(80),retired,work,
      inlineYield:trap?.budgetYield===true,
      observedClock:trap?.observedClock,
      kind:'v86',id:'v86',regions:1,chainedColdRuns:0,chainedColdWork:0,chainedColdRetired:0};
  }
  inlineNativePending() {
    const h=this.engine;
    if(h.api.pending_windows_thread_schedule())return true;
    h.ok('pending_windows_callback');
    return h.resultView(96).getUint32(12,true)!==0;
  }
  tryInlineGate(trap) {
    if(this.inlineFailure)throw this.inlineFailure;
    if(this.portBridgeDeclinedPc===trap.pc)return false;
    const owner=this.inlineOwner,h=this.engine;
    if(!this.inlineGates || !this.portGates || !this.deferDirty ||
        this.tables.dirtyGateFlagsSupported!==true || !owner?.canContinue())return false;
    if(this.directGates)return this.tryDirectGate(trap);
    exportState(h,this.cpu,trap);
    if(this.reuseNumericState!==false)rememberStateBanks(h,this.cpu);
    if(trap.portGate && trap.privateInstructions===0)trap.canonicalExported=true;
    this.dirtyPending=true;
    const atomic=typeof h.api.component_cpu_try_inline_gate==='function';
    let id;
    if(atomic) {
      trap.observedClock=owner.observeClock();
      if(!owner.canContinue())return false;
    } else {
      const lookup=()=>{
        h.ok('component_cpu_gate',...KEY,trap.pc);
        const result=h.resultView(8),gate=result.getUint32(0,true);
        return (result.getUint32(4,true)&3)===3 && INLINE_GATE_IDS.has(gate)?gate:0;
      };
      id=lookup();
      if(!id || this.inlineNativePending())return false;
      trap.observedClock=owner.observeClock();
      if(!owner.canContinue() || this.inlineNativePending() || lookup()!==id)return false;
    }
    try {
      // the enclosing main_loop owns all instruction credits, including these calls.
      let status;
      if(atomic) {
        status=h.status('component_cpu_try_inline_gate',...KEY,trap.pc);
        if(status===256)return false;
        if(status===0) {
          const record=h.resultView(112);id=record.getUint32(20,true);
          if(record.getUint32(0,true)!==0x46433352 || record.getUint32(4,true)!==0x10001 ||
              record.getUint32(8,true)!==112 || record.getUint32(32,true)!==trap.pc || !INLINE_GATE_IDS.has(id))
            throw new Error('invalid v86 atomic inline completion record');
        }
      } else {
        h.ok('component_cpu_publish',...KEY,7,0,0,id,0,0);
        status=h.status('component_cpu_try_complete_gate',...KEY);
      }
      if(status!==0 || h.get(72)!==3 || (!atomic && this.inlineNativePending()))
        throw Object.assign(new Error(`v86 inline gate completion failed ${status}`),{status,id,pc:trap.pc});
      importState(h,this.cpu,this.traps);
      this.inlineCompletions++;this.gates++;this.portGateCalls++;this.deferredDirtyPublications++;
      if(!owner.canContinue()) {this.inlineYields++;return 'yield';}
      return true;
    } catch(error) {this.inlineFailure=error;throw error;}
  }
  tryDirectGate(trap) {
    if(!trap.portGate || trap.privateInstructions!==0)return false;
    const h=this.engine;this.dirtyPending=true;
    try {
      validateSegmentSelectors(h,this.cpu,trap);
      const before=cpuViews(this.memory),bindings=this.directBindings===true,stateInputs=this.directStateInputs===true,
        masked=typeof h.api.component_cpu_try_direct_gate_mask==='function';
      if(bindings && !masked)throw new Error('native v86 direct bindings are unsupported');
      if(stateInputs && !this.directStateInputsSupported)throw new Error('native v86 direct state inputs are unsupported');
      const mask=(bindings?0x7ff:0xff)|(stateInputs?0x1800:0);
      const status=masked?h.status('component_cpu_try_direct_gate_mask',...KEY,trap.pc,before.regs[4],before.regs[0],mask)
        :h.status('component_cpu_try_direct_gate',...KEY,trap.pc,before.regs[4],before.regs[0]);
      if(status===256){this.directDeclines++;return false;}
      if(status!==0)throw Object.assign(new Error(`v86 direct gate completion failed ${status}`),{status,pc:trap.pc});
      const record=h.resultView(32),id=record.getUint32(16,true);
      if(record.getUint32(0,true)!==0x46493352 || record.getUint32(4,true)!==0x10001 ||
          record.getUint32(8,true)!==32 || record.getUint32(12,true)!==0 ||
          !(INLINE_GATE_IDS.has(id) || bindings && DIRECT_BINDING_GATE_IDS.has(id) || stateInputs && DIRECT_STATE_INPUT_GATE_IDS.has(id)))
        throw new Error('invalid v86 direct completion record');
      const returned=cpuViews(this.memory),continued=this.inlineOwner.canContinue(),
        guestReturn=this.guestReturns && this.portGateMode===2 && GUEST_RETURN_GATE_IDS.has(id) && continued;
      returned.regs[0]=record.getUint32(20,true);
      if(guestReturn)this.guestReturnCompletions++;
      else {
        returned.regs[4]=record.getUint32(28,true);const pc=record.getUint32(24,true);
        returned.data.setUint32(556,pc,true);returned.data.setUint32(560,pc,true);returned.data.setUint32(620,0xffffffff,true);
      }
      this.directCompletions++;this.inlineCompletions++;this.gates++;this.portGateCalls++;this.deferredDirtyPublications++;
      if(DIRECT_BINDING_GATE_IDS.has(id))this.directBindingCompletions++;
      if(DIRECT_STATE_INPUT_GATE_IDS.has(id))this.directStateInputCompletions++;
      if(!continued){this.inlineYields++;return 'yield';}
      return true;
    } catch(error){this.inlineFailure=error;throw error;}
  }
  configureDirectGates(enabled,owner) {
    if(owner?.paused!==true || owner?.open!==false || this.open || this.closed || typeof enabled!=='boolean')
      throw new Error('v86 direct gate changes require a closed paused owner');
    if(this.inlineFailure)throw this.inlineFailure;
    if(enabled && typeof this.engine.api.component_cpu_try_direct_gate!=='function')
      throw new Error('native v86 direct gates are unsupported');
    this.publish();this.directGates=enabled;return enabled;
  }
  configureDirectBindings(enabled,owner) {
    if(owner?.paused!==true || owner?.open!==false || this.open || this.closed || typeof enabled!=='boolean')
      throw new Error('v86 direct binding changes require a closed paused owner');
    if(this.inlineFailure)throw this.inlineFailure;
    if(enabled && typeof this.engine.api.component_cpu_try_direct_gate_mask!=='function')
      throw new Error('native v86 direct bindings are unsupported');
    this.publish();this.directBindings=enabled;return enabled;
  }
  configureDirectStateInputs(enabled,owner) {
    if(owner?.paused!==true || owner?.open!==false || this.open || this.closed || typeof enabled!=='boolean')
      throw new Error('v86 direct state input changes require a closed paused owner');
    if(this.inlineFailure)throw this.inlineFailure;
    if(enabled && !this.directStateInputsSupported)throw new Error('native v86 direct state inputs are unsupported');
    this.publish();this.directStateInputs=enabled;return enabled;
  }
  gateMode() {
    this.engine.ok('component_cpu_gate_mode',...KEY);const d=this.engine.resultView(16),
      mode=d.getUint32(0,true),total=d.getUint32(4,true),guestRetSites=d.getUint32(8,true),pendingRet=d.getUint32(12,true);
    if(mode>2 || guestRetSites>total || pendingRet>1)throw new Error('invalid native v86 gate mode');
    return {mode,total,guestRetSites,pendingRet};
  }
  configureGuestReturns(enabled,owner) {
    if(owner?.paused!==true || owner?.open!==false || this.open || this.closed || typeof enabled!=='boolean')
      throw new Error('v86 guest return changes require a closed paused owner');
    if(this.inlineFailure)throw this.inlineFailure;
    if(enabled===this.guestReturns)return enabled;
    if(!this.guestReturnsSupported || typeof this.engine.api.component_cpu_gate_mode!=='function')
      throw new Error('native v86 guest returns are unsupported');
    const current=this.gateMode();
    if(current.mode!==this.portGateMode)throw new Error('native v86 gate mode owner changed');
    this.applyPortGates(true,enabled?2:1);return enabled;
  }
  readGraphicsOverlap() {
    const d=this.engine.resultView(16),enabled=d.getUint32(0,true);
    if(enabled>1 || d.getUint32(4,true)!==0)throw new Error('invalid native v86 graphics overlap result');
    return {enabled:enabled===1,completed:d.getBigUint64(8,true)};
  }
  getGraphicsOverlap() {
    if(typeof this.engine.api.component_cpu_graphics_overlap!=='function')
      throw new Error('native v86 graphics overlap is unsupported');
    this.engine.ok('component_cpu_graphics_overlap',...KEY);return this.readGraphicsOverlap();
  }
  configureGraphicsOverlap(enabled,owner) {
    if(owner?.paused!==true || owner?.open!==false || this.open || this.closed || typeof enabled!=='boolean')
      throw new Error('v86 graphics overlap changes require a closed paused owner');
    if(this.inlineFailure)throw this.inlineFailure;
    if(enabled===this.graphicsOverlap)return enabled;
    if(typeof this.engine.api.component_cpu_set_graphics_overlap!=='function' ||
        typeof this.engine.api.component_cpu_graphics_overlap!=='function')
      throw new Error('native v86 graphics overlap is unsupported');
    this.publish();this.engine.ok('component_cpu_set_graphics_overlap',...KEY,enabled?1:0);
    if(this.readGraphicsOverlap().enabled!==enabled)throw new Error('invalid native v86 graphics overlap mode');
    this.graphicsOverlap=enabled;return enabled;
  }
  configureInlineGates(enabled,owner) {
    if(owner?.paused!==true || owner?.open!==false || this.open || this.closed || typeof enabled!=='boolean')
      throw new Error('v86 inline gate changes require a closed paused owner');
    if(this.inlineFailure)throw this.inlineFailure;
    if(enabled && (typeof this.inlineOwner?.canContinue!=='function' ||
        typeof this.inlineOwner?.observeClock!=='function' ||
        (typeof this.engine.api.component_cpu_try_complete_gate!=='function' &&
         typeof this.engine.api.component_cpu_try_inline_gate!=='function')))
      throw new Error('v86 inline gates require a synchronous owner and native completion');
    this.publish();this.inlineGates=enabled;
    this.traps.continueGate=enabled?trap=>this.tryInlineGate(trap):null;
    return enabled;
  }
  publish() {
    if(!this.dirtyPending)return;
    this.dirtyPages+=this.tables.publishDirty();this.dirtyPending=false;
  }
  configureDirtyDeferral(enabled,owner) {
    if(owner?.paused!==true || owner?.open!==false || typeof enabled!=='boolean')
      throw new Error('v86 dirty deferral changes require a closed paused owner');
    this.publish();this.deferDirty=enabled;return enabled;
  }
  applyPortGates(enabled,mode=enabled?(this.guestReturns?2:1):0) {
    if(typeof this.engine.api.component_cpu_set_gate_mode!=='function') {
      if(!enabled && !this.portGates)return false;
      throw new Error('native v86 port gates are unsupported');
    }
    this.publish();this.tables.refresh();
    this.engine.ok('component_cpu_set_gate_mode',...KEY,mode);
    this.portGateModeTouched=true;
    const result=this.engine.resultView(8),actual=result.getUint32(0,true),count=result.getUint32(4,true);
    if(actual!==mode || (!enabled && count))throw new Error('invalid native v86 gate mode result');
    this.portGates=enabled;this.guestReturns=mode===2;this.portGateMode=mode;this.portGatePatchedCount=count;
    this.traps.configurePortGates(enabled);
    // the native version change conservatively invalidates affected executable content.
    this.tables.refresh();this.cpu.full_clear_tlb();
    return enabled;
  }
  configurePortGates(enabled,owner) {
    if(owner?.paused!==true || owner?.open!==false || this.open || this.closed || typeof enabled!=='boolean')
      throw new Error('v86 port gate changes require a closed paused owner');
    if(enabled===this.portGates)return enabled;
    return this.applyPortGates(enabled);
  }
  sampleUsage() {return this.stats();}
  coldDiagnostics() {return null;}
  stats() {
    const counters=this.runtime.counters();
    return {backend:'v86',cpuHash:this.runtime.hash,fpuMode:'strict',hle:false,
      runs:this.runs,regions:this.regions,retired:this.retired,work:this.work,
      retirementSemantics:'v86 instruction counter; private handler instructions removed, guest retirement is approximate',
      admissions:counters.generatedModules,generatedModuleBytes:counters.generatedModuleBytes,
      installedUnits:counters.finalizedModules,v86:counters,jitConfig:this.runtime.jitConfig,
      workBudget:this.budget,gates:this.gates,faults:this.faults,cowFaults:this.cowFaults,
      dirtyPages:this.dirtyPages,deferDirty:this.deferDirty,
      portGates:this.portGates,portGateCalls:this.portGateCalls,deniedPortCalls:this.deniedPortCalls,
      portGateMode:this.portGateMode,portGatePatchedCount:this.portGatePatchedCount,
      guestReturns:this.guestReturns,guestReturnsSupported:this.guestReturnsSupported,guestReturnCompletions:this.guestReturnCompletions,
      graphicsOverlap:this.graphicsOverlap,
      wasmPorts:this.wasmPorts,wasmPortsSupported:this.wasmPortsSupported,
      wasmPortCompletions:this.wasmPortCompletions,wasmPortDeclines:this.wasmPortDeclines,wasmPortCheckpoints:this.wasmPortCheckpoints,
      inlineGates:this.inlineGates,inlineCompletions:this.inlineCompletions,inlineYields:this.inlineYields,
      directGates:this.directGates,directCompletions:this.directCompletions,directDeclines:this.directDeclines,
      directBindings:this.directBindings,directBindingCompletions:this.directBindingCompletions,
      directStateInputs:this.directStateInputs,directStateInputsSupported:this.directStateInputsSupported,directStateInputCompletions:this.directStateInputCompletions,
      inlineFailed:!!this.inlineFailure,
      deferredDirtyPublications:this.deferredDirtyPublications,wasmMemoryBytes:this.memory.buffer.byteLength,
      executePermissionEnforcement:false,faultAccessLength:'one-byte conservative fault span',
      x87TagSemantics:'v86 represents occupancy and derives nonempty tags from payload; incompatible imports fail',
      coldRuns:0,coldRetired:0,coldWork:0,chainedColdRuns:0,chainedColdRetired:0,chainedColdWork:0,
      preciseRuns:0,preciseRetired:0,preciseWork:0,chainEnabled:true};
  }
  close() {
    if(this.closed)return;
    if(this.open)throw new Error('cannot close an active v86 adapter');
    this.harvestPortBridge();this.portBridge?.disarm();this.wasmPortsActive=false;
    this.publish();
    if(this.portGateModeTouched || this.portGates)this.applyPortGates(false);
    this.portBridge?.close();this.portBridgeBound=false;this.wasmPorts=false;
    forgetStateBanks(this.cpu);
    this.tables.release();this.closed=true;this.imported=false;
  }
}
