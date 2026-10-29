import { Asm,abs,EAX,ECX,EDX,EBX,ESP,EBP,ESI,EDI } from '../reference/bottleship/tools/guestbench/lib/asm.mjs';
import { CODE,DATA,STACK,TERMINAL,hex } from './kernels-v1.mjs';
const check=(ok,message)=>{if(!ok)throw new Error(message);};
const equal=(actual,expected,message)=>check(JSON.stringify(actual)===JSON.stringify(expected),message);
const BASE=0x100000,ENTRY=BASE+0x40,PASS=BASE+0x300,MARK_START=0xe0,MARK_END=0xe1,IMAGE_SIZE=0x1000;

export function buildImage(k,total){
  const bytes=new Uint8Array(IMAGE_SIZE),dv=new DataView(bytes.buffer),magic=0x1badb002,flags=0x10000;
  [magic,flags,(-(magic+flags))>>>0,BASE,BASE,BASE+IMAGE_SIZE,BASE+IMAGE_SIZE,ENTRY].forEach((v,i)=>dv.setUint32(i*4,v,true));dv.setUint32(PASS-BASE,total,true);
  const a=new Asm(ENTRY);a.label('repeat');a.db(0xfa);a.movImm(ESP,STACK+0x1000);a.enableFpuAndSse();a.db(0xd9).modrm(5,abs(DATA+0xf0));
  a.movImm(EAX,k.regs[EAX]).movImm(ECX,k.regs[ECX]).movImm(EDX,k.regs[EDX]).movImm(EBX,k.regs[EBX]).movImm(EBP,0).movImm(ESI,0).movImm(EDI,0).movImm(ESP,STACK+0xffc);
  a.db(0x6a,2,0x9d); // the wrapper establishes flags outside the shared user-code kernel
  a.db(0xe6,MARK_START);a.db(0xe9).dd(CODE-(a.addr+4));
  a.label('after');a.dec(abs(PASS)).jcc('nz','repeat').db(0xfa).hlt();a.jmpShort('after');
  const wrapper=a.link();check(0x40+wrapper.length<PASS-BASE,'wrapper overlaps its pass count');bytes.set(wrapper,0x40);
  const terminal=new Asm(TERMINAL);terminal.db(0xe6,MARK_END).db(0xe9).dd(a.labels.get('after')-(terminal.addr+4));
  return {bytes,terminal:terminal.link()};
}
export function configure(w,{precision='relaxed',profile='shipping'}={}){
  check(['strict','relaxed'].includes(precision)&&['core','shipping'].includes(profile),'unsupported profile');
  check(typeof w.set_relaxed_fpu==='function','missing relaxed-FPU API');w.set_relaxed_fpu(precision==='relaxed'?1:0);equal(Number(w.get_relaxed_fpu()),precision==='relaxed'?1:0,'precision readback');
  const configs=profile==='shipping'?[[4,0],[5,1],[10,0],[11,1],[12,1],[13,1],[15,0],[19,0],[21,0],[22,1]]:[[4,0],[5,0],[10,0],[11,0],[12,0],[13,0],[15,0],[19,0],[21,0],[22,0]];
  const actual=[];for(const [index,value]of configs){check(typeof w.set_jit_config==='function','missing JIT config API');w.set_jit_config(index,value);const got=Number(w.get_jit_config(index));equal(got,value,`jit config ${index}`);actual.push({index,value:got});}
  for(const [name,value]of [['jit_page_tails',profile==='shipping'?1:0],['jit_hot_edge_regions',0],['jit_guard_groups',profile==='shipping'?1:0]]){check(typeof w[`set_${name}`]==='function',`missing ${name}`);w[`set_${name}`](value);equal(Number(w[`get_${name}`]()),value,`${name} readback`);actual.push({name,value});}
  for(const name of ['set_opstats','set_dispatch_stats'])if(w[name])w[name](0);
  if(w.set_stack_raw_unsafe)w.set_stack_raw_unsafe(0);if(w.set_perm_map_reads)w.set_perm_map_reads(0);
  return actual;
}

// the caller supplies an already-loaded, stopped V86 instance; no helper imports are wrapped
export async function runKernel(emulator,k,{warmups=1,rounds=1,precision='relaxed',profile='shipping',onRecord=()=>{}}={}){
  check(Number.isInteger(rounds)&&rounds>=1&&rounds<=8&&Number.isInteger(warmups)&&warmups>=1&&warmups<=4,'unsupported run bounds');
  const built=buildImage(k,rounds+warmups),cpu=emulator.v86.cpu,w=cpu.wm.exports,record={kind:k.kind,cw:k.cw,N:k.N,retired:k.retired,runs:[]};
  cpu.reboot_internal();cpu.reset_memory();cpu.load_multiboot(built.bytes.buffer);record.config=configure(w,{precision,profile});
  let timer,ended=false,active=null,jitFinalized=0,pass=0;const previousFinalize=cpu.test_hook_did_finalize_wasm;
  cpu.test_hook_did_finalize_wasm=(...args)=>{jitFinalized++;if(previousFinalize)previousFinalize(...args);};
  cpu.write_blob(k.code,CODE);cpu.write_blob(built.terminal,TERMINAL);cpu.write_blob(k.data,DATA);cpu.write_blob(k.stack,STACK);
  try{
    await new Promise((resolveRun,rejectRun)=>{
      const fail=error=>{if(ended)return;ended=true;clearTimeout(timer);record.error=String(error?.stack??error);onRecord(record);rejectRun(error);};
      const halt=()=>{if(ended)return;try{check(active===null,'halt during shared kernel');equal(pass,rounds+warmups,'pass count');ended=true;clearTimeout(timer);resolveRun();}catch(e){fail(e);}};
      emulator.bus.register('cpu-event-halt',halt);
      cpu.io.register_write(MARK_START,cpu,()=>{check(active===null,'overlapping markers');active={ms:performance.now(),counter:cpu.instruction_counter[0]>>>0,jitFinalized};});
      cpu.io.register_write(MARK_END,cpu,()=>{
        const endMs=performance.now();try{
          check(active!==null,'end without start');const row={pass,warmup:pass<warmups,elapsedMs:endMs-active.ms,knownGuestRetired:k.retired,instructionCounterStart:active.counter,instructionCounterEnd:cpu.instruction_counter[0]>>>0,jitFinalizedStart:active.jitFinalized,jitFinalizedEnd:jitFinalized,regs:Array.from(cpu.reg32,x=>x>>>0),data:hex(emulator.read_memory(DATA,4096)),stack:hex(emulator.read_memory(STACK,4096))};record.runs.push(row);active=null;pass++;onRecord(record);
          equal(row.regs,k.expectedRegs,'final registers');equal(row.data,hex(k.expectedData),'whole data page');equal(row.stack,hex(k.expectedStack),'whole stack page');row.validated=true;onRecord(record);
          cpu.write_blob(k.data,DATA);cpu.write_blob(k.stack,STACK);
        }catch(e){fail(e);}
      });
      timer=setTimeout(()=>fail(new Error('own-code v86 timeout')),60000);emulator.run().catch(fail);
    });
  }finally{if(timer)clearTimeout(timer);await emulator.stop();cpu.test_hook_did_finalize_wasm=previousFinalize;}
  const measured=record.runs.filter(r=>!r.warmup);equal(measured.length,rounds,'measured pass count');record.summary={elapsedMs:measured.map(x=>x.elapsedMs),nsPerKnownGuestInstruction:measured.map(x=>x.elapsedMs*1e6/k.retired),jitFinalizationsDuringMeasured:measured.reduce((sum,x)=>sum+x.jitFinalizedEnd-x.jitFinalizedStart,0),jitFinalizedAtMeasuredStart:measured.map(x=>x.jitFinalizedStart)};onRecord(record);return record;
}
