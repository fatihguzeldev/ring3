import {GDT,IDT,TSS,STACK,HANDLERS,PARK,TRAP_PORT,physicalPrivate} from './layout.js';
import {cpuViews} from './state.js';

const ERROR_VECTORS = new Set([8,10,11,12,13,14,17]);

export class Traps {
  constructor(cpu) {
    this.cpu=cpu;this.memory=cpu.wasm_memory;this.ram=cpu.mem8.byteOffset;
    this.pending=null;
    this.initialize();
    cpu.io.register_write(TRAP_PORT,this,()=>this.capture());
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
      address:d.getUint32(588,true)};
    d.setUint32(556,PARK,true);
    d.setUint32(620,0xffffffff,true);
  }
}
