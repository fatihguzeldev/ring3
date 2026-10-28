import {V86} from './vendor/libv86.mjs';
import {KEY} from '../engine.js';
import {CPU_SHA256,PHYSICAL_BYTES,PARK,SHIPPING_JIT,CODEGEN_SWITCHES} from './layout.js';
import {PageTables} from './memory.js';
import {Traps} from './traps.js';
import {cpuViews,importState,exportState} from './state.js';

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
  const emulator=new V86({autostart:false,memory_size:memorySize,log_level:0,
    wasm_fn:async imports=>{
      // ring3 supplies process time and devices; the cpu's machine timers stay inactive.
      imports.env.run_hardware_timers=()=>0;
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
  const runtime={cpu,memory:cpu.wasm_memory,hash,
    async attach(engine,execution={}) {
      if(attached || disposed)throw new Error('v86 runtime cannot attach twice');
      signal?.throwIfAborted();
      if(engine.memory!==runtime.memory)throw new Error('v86 and Ring3 must share one Wasm memory');
      const cache=new V86CodeCache(engine,execution,cpu,runtime);
      attached=true;return cache;
    },
    counters:()=>({generatedModules:generated,finalizedModules:finalized,generatedModuleBytes:generatedBytes}),
    jitConfig,
    dispose() {if(disposed)return;disposed=true;emulator.destroy();},
  };
  return runtime;
}

export const createV86Runtime=prepareV86;

class V86CodeCache {
  constructor(engine,execution,cpu,runtime) {
    this.engine=engine;this.memory=engine.memory;this.cpu=cpu;this.runtime=runtime;
    const budget=execution.budget??50000;
    if(!Number.isInteger(budget) || budget<1 || budget>0xffffffff)throw new Error('invalid v86 execution budget');
    this.budget=Math.min(budget,100002);
    this.tables=new PageTables(engine,cpu);
    try {this.traps=new Traps(cpu);}catch(error){this.tables.rollback(error);throw error;}
    this.imported=false;this.closed=false;this.open=false;
    this.runs=0;this.regions=0;this.retired=0;this.work=0;this.gates=0;this.faults=0;
    this.dirtyPages=0;this.cowFaults=0;
  }
  get pc() {return this.engine.get(48);}
  import() {
    if(this.closed)throw new Error('v86 adapter is closed');
    this.tables.refresh();
    importState(this.engine,this.cpu,this.traps);
    this.traps.pending=null;this.imported=true;
  }
  select() {
    if(!this.imported)this.import();
    return {id:'v86',pc:this.pc};
  }
  execute() {
    if(!this.imported)this.import();
    const cpu=this.cpu,h=this.engine,hp=cpu.wm.exports.get_hypercall_page_ptr()>>>0;
    let work=0,retired=0,stop=2,detail=0,auxiliary=0,vector=0,trap=null;
    for(let attempt=0;attempt<8;attempt++) {
      const before=cpuViews(this.memory).data.getUint32(664,true);
      new DataView(this.memory.buffer).setUint32(hp,this.budget,true);
      this.open=true;
      try {cpu.main_loop();} finally {this.open=false;}
      const delta=(cpuViews(this.memory).data.getUint32(664,true)-before)>>>0;
      trap=this.traps.pending;
      if(!delta && !trap)throw new Error(`v86 made no progress at0x${cpuViews(this.memory).data.getUint32(556,true).toString(16)}`);
      work+=delta;retired+=Math.max(0,delta-(trap?2:0));
      exportState(h,cpu,trap);
      this.dirtyPages+=this.tables.publishDirty();
      if(trap?.vector===14 && (trap.error&2) && this.tables.isCopyOnWrite(trap.address)) {
        h.ok('component_cpu_write_fault',...KEY,trap.address);
        this.cowFaults++;this.import();
        if(attempt===7)throw new Error('v86 repeated copy-on-write fault');
        continue;
      }
      break;
    }
    if(trap) {
      vector=trap.vector;
      if(vector===6) {
        h.ok('component_cpu_gate',...KEY,trap.pc);
        const gate=h.resultView(4).getUint32(0,true);
        if(gate){stop=7;vector=gate;this.gates++;}else {stop=9;this.faults++;}
      } else if(vector===14) {
        stop=3;detail=trap.address;
        const access=trap.error&2?2:trap.address===trap.pc?4:1;
        auxiliary=1|(access<<8)|(this.tables.faultReason(trap.address)<<16);vector=(1<<8)|14;this.faults++;
      } else if(vector===0x29 || vector===0x2e) {
        throw new Error(`unsupported v86 software system entry INT${vector.toString(16)} at0x${trap.pc.toString(16)}`);
      } else if([0,1,3,4,5,7,10,11,12,13,16,19].includes(vector)) {
        stop=9;detail=vector===16?h.get(4236+18)&0xffff:vector===19?h.get(4364+16):trap.error;
        this.faults++;
      } else throw new Error(`unsupported v86 CPU exception ${vector} at0x${trap.pc.toString(16)}`);
    }
    if(work>0xffffffff || retired>work)throw new Error('invalid v86 execution accounting');
    h.ok('component_cpu_publish',...KEY,stop,detail,auxiliary,vector,retired,work);
    this.imported=false;this.traps.pending=null;this.runs++;this.regions++;
    this.retired+=retired;this.work+=work;
    return {pc:this.pc,coreReason:stop,reason:h.get(72),detail:h.get(80),retired,work,
      kind:'v86',id:'v86',regions:1,chainedColdRuns:0,chainedColdWork:0,chainedColdRetired:0};
  }
  publish() {}
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
      dirtyPages:this.dirtyPages,wasmMemoryBytes:this.memory.buffer.byteLength,
      executePermissionEnforcement:false,faultAccessLength:'one-byte conservative fault span',
      x87TagSemantics:'v86 represents occupancy and derives nonempty tags from payload; incompatible imports fail',
      coldRuns:0,coldRetired:0,coldWork:0,chainedColdRuns:0,chainedColdRetired:0,chainedColdWork:0,
      preciseRuns:0,preciseRetired:0,preciseWork:0,chainEnabled:true};
  }
  close() {
    if(this.closed)return;
    if(this.open)throw new Error('cannot close an active v86 adapter');
    this.tables.release();this.closed=true;this.imported=false;
  }
}
