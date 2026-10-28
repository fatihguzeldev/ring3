import {GDT,IDT,TSS,STACK,HANDLERS,PARK,TRAP_PORT,physicalPrivate} from './layout.js';
import {cpuViews} from './state.js';

const ERROR_VECTORS = new Set([8,10,11,12,13,14,17]);
const PORT_IO_FAULT=Object.freeze({portIOFault:true});
const IO_PREFIXES=new Set([0x26,0x2e,0x36,0x3e,0x64,0x65,0x66,0x67,0xf2,0xf3]);
export const GATE_PORT=0xe1;

export class Traps {
  constructor(cpu,readCode=null) {
    this.cpu=cpu;this.memory=cpu.wasm_memory;this.ram=cpu.mem8.byteOffset;
    this.pending=null;this.portGates=false;this.readCode=readCode;
    this.initialize();
    cpu.io.register_write(TRAP_PORT,this,()=>this.capture());
    cpu.io.register_write(GATE_PORT,this,()=>this.capturePort(true),()=>this.capturePort(false),()=>this.capturePort(false));
    cpu.io.register_read(GATE_PORT,this,()=>this.capturePort(false),()=>this.capturePort(false),()=>this.capturePort(false));
  }
  configurePortGates(enabled) {
    const v=cpuViews(this.memory),bitmap=this.ram+TSS+0x68,limit=enabled?0x88:0x67;
    v.bytes.fill(0xff,bitmap,this.ram+TSS+0x89);
    if(enabled)v.bytes[bitmap+(GATE_PORT>>>3)]&=~(1<<(GATE_PORT&7));
    this.setDescriptor(0x28,physicalPrivate(TSS),limit,0x89,false);v.limits[6]=limit;
    this.portGates=enabled;
  }
  isPortFault(error) {return error===PORT_IO_FAULT && this.pending?.portDenied===true;}
  isReservedStringIO(pc,dx) {
    if(!this.portGates || (dx&0xffff)!==GATE_PORT || !this.readCode)return false;
    for(let offset=0;offset<15;offset++) {
      const opcode=this.readCode((pc+offset)>>>0);
      if(opcode===0x6c || opcode===0x6e)return true;
      if(!IO_PREFIXES.has(opcode))return false;
    }
    return false;
  }
  capturePort(write8) {
    if(this.pending)throw new Error('nested v86 adapter trap');
    const v=cpuViews(this.memory),d=v.data;
    if(v.bytes[612]!==3)throw new Error('v86 gate port accessed outside guest ring3');
    const pc=d.getUint32(560,true),candidate=this.portGates && write8 && d.getUint32(556,true)===((pc+2)>>>0)
      && this.readCode?.(pc)===0xe6 && this.readCode((pc+1)>>>0)===GATE_PORT;
    this.pending={vector:candidate?6:13,error:0,pc,cs:v.selectors[1],ss:v.selectors[2],
      flags:this.cpu.wm.exports.get_eflags()>>>0,esp:v.regs[4],address:0,
      portGate:candidate,portDenied:!candidate,privateInstructions:0,
      registers:candidate?null:Array.from(v.regs)};
    d.setUint32(556,PARK,true);d.setUint32(620,0xffffffff,true);
    if(!candidate)throw PORT_IO_FAULT;
  }
  setDescriptor(selector,base,limit,access,default32=true) {
    if(!(selector & ~3))return;
    if(selector & 4)throw new Error('v86 experiment does not support LDT selectors');
    const index=selector>>>3;
    if(index>=512 || (access & 0x60) && [1,2,5].includes(index))
      throw new Error(`v86 private GDT collides with selector 0x${selector.toString(16)}`);
    const d=new DataView(this.memory.buffer),at=this.ram+GDT+index*8;
    const pages=limit>0xfffff;
    if(pages && (limit & 4095)!==4095)throw new Error('v86 cannot encode segment limit');
    const raw=pages?limit>>>12:limit;
    d.setUint16(at,raw&0xffff,true);d.setUint16(at+2,base&0xffff,true);
    d.setUint8(at+4,(base>>>16)&255);d.setUint8(at+5,access);
    d.setUint8(at+6,((raw>>>16)&15)|(default32?0x40:0)|(pages?0x80:0));
    d.setUint8(at+7,base>>>24);
  }
  initialize() {
    const v=cpuViews(this.memory),d=v.data;
    v.bytes.fill(0,this.ram+GDT,this.ram+GDT+4096*8);
    this.setDescriptor(0x08,0,0xffffffff,0x9b);
    this.setDescriptor(0x10,0,0xffffffff,0x93);
    this.setDescriptor(0x28,physicalPrivate(TSS),0x67,0x89,false);
    d.setUint32(this.ram+TSS+4,physicalPrivate(STACK+8192),true);
    d.setUint16(this.ram+TSS+8,0x10,true);
    d.setUint16(this.ram+TSS+0x66,0x68,true);
    for(let vector=0;vector<256;vector++) {
      const handler=physicalPrivate(HANDLERS)+vector*16,at=this.ram+IDT+vector*8;
      d.setUint16(at,handler&0xffff,true);d.setUint16(at+2,0x08,true);
      d.setUint8(at+4,0);d.setUint8(at+5,[3,4,0x29,0x2e].includes(vector)?0xee:0x8e);d.setUint16(at+6,handler>>>16,true);
      v.bytes.set([0x68,vector,0,0,0,0xe6,TRAP_PORT,0xeb,0xfe],this.ram+HANDLERS+vector*16);
    }
    d.setUint32(564,256*8-1,true);d.setUint32(568,physicalPrivate(IDT),true);
    d.setUint32(572,4095,true);d.setUint32(576,physicalPrivate(GDT),true);
    v.selectors[6]=0x28;v.bases[6]=physicalPrivate(TSS);v.limits[6]=0x67;
    v.bytes[1128]=1;
  }
  capture() {
    if(this.pending)throw new Error('nested v86 adapter trap');
    const v=cpuViews(this.memory),d=v.data,esp=v.regs[4];
    if(v.bytes[612]!==0 || esp<physicalPrivate(STACK) || esp>=physicalPrivate(STACK+8192))
      throw new Error(`v86 trap outside private stack at 0x${esp.toString(16)}`);
    const at=this.ram+GDT+(esp-physicalPrivate(GDT));
    const vector=d.getUint32(at,true),hasError=ERROR_VECTORS.has(vector),frame=at+4+(hasError?4:0);
    const cs=d.getUint32(frame+4,true)&0xffff;
    if((cs&3)!==3)throw new Error(`v86 fault ${vector} occurred in adapter ring0 code`);
    this.pending={vector,error:hasError?d.getUint32(at+4,true):0,
      pc:d.getUint32(frame,true),cs,flags:d.getUint32(frame+8,true),
      esp:d.getUint32(frame+12,true),ss:d.getUint32(frame+16,true),
      address:d.getUint32(588,true),privateInstructions:2};
    if(vector===14 && this.isReservedStringIO(this.pending.pc,v.regs[2])) {
      this.pending.vector=13;this.pending.error=0;this.pending.address=0;this.pending.portDenied=true;
    }
    d.setUint32(556,PARK,true);
    d.setUint32(620,0xffffffff,true);
  }
}
