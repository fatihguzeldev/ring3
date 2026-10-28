import {X87, SIMD, SEGMENTS, YMM, SEGMENT_BASES} from './layout.js';

const CACHE_OFFSETS = [28,32,36,40,44,56];
const USER_FLAGS = 0x254fd7;

export function cpuViews(memory) {
  const buffer = memory.buffer;
  return {bytes:new Uint8Array(buffer), data:new DataView(buffer),
    regs:new Uint32Array(buffer,64,8), selectors:new Uint16Array(buffer,668,8),
    bases:new Uint32Array(buffer,736,8), limits:new Uint32Array(buffer,768,8)};
}

export function importState(engine, cpu, traps) {
  const v = cpuViews(engine.memory), d = v.data, b = engine.base;
  if (d.getUint32(b+12,true)) throw new Error('v86 does not support a pending SS debug shadow');
  for (let i=16;i<144;i++) if(v.bytes[b+YMM+i]) throw new Error('v86 does not support nonzero YMM upper lanes');
  for (let i=0;i<8;i++) v.regs[i]=d.getUint32(b+16+i*4,true);
  d.setUint32(556,d.getUint32(b+48,true),true);
  d.setUint32(560,d.getUint32(b+48,true),true);
  d.setUint32(120,d.getUint32(b+52,true),true);
  d.setUint32(100,0,true);
  v.bytes[612]=3;v.bytes[616]=0;v.bytes[648]=0;
  const segmentVersion=d.getUint16(b+SEGMENTS+4,true);
  for(let i=0;i<6;i++) {
    const selector=d.getUint16(b+SEGMENTS+16+i*2,true);
    const base=segmentVersion===3 ? d.getUint32(b+SEGMENT_BASES+16+i*4,true)
      : segmentVersion===1 || i>=4 ? d.getUint32(b+SEGMENTS+32+i*4,true) : 0;
    const cache=segmentVersion===1 ? (selector & ~3 ? (i===1?0xf3bfffff:0xf33fffff) : 0)
      : d.getUint32(b+SEGMENTS+CACHE_OFFSETS[i],true);
    if(i<4 && base) throw new Error('v86 experiment requires flat ES/CS/SS/DS');
    if((i===1 || i===2) && !(cache & 0x02000000)) throw new Error('v86 experiment requires 32-bit code and stack');
    const raw=cache & 0xfffff, limit=cache & 0x1000000 ? ((raw<<12)|0xfff)>>>0 : raw;
    v.selectors[i]=selector;v.bases[i]=base;v.limits[i]=limit;
    v.bytes[724+i]=cache===0?1:0;
    v.bytes[512+i]=cache===0?0:((cache>>>20)&15)|0x10|((cache>>>29)&3)<<5|0x80;
    traps.setDescriptor(selector,base,limit,v.bytes[512+i],!!(cache & 0x02000000));
  }
  v.bytes[800]=1;v.bytes[804]=1;v.bytes[808]=1;
  d.setUint32(620,0xffffffff,true);
  const fp=b+X87, sw=d.getUint16(fp+18,true), tags=d.getUint16(fp+20,true);
  v.bytes[1032]=(sw>>>11)&7;
  d.setUint16(1036,d.getUint16(fp+16,true),true);
  d.setUint16(1040,sw & ~0x3800,true);
  let empty=0;
  for(let i=0;i<8;i++) {
    if(((tags>>>(i*2))&3)===3)empty|=1<<i;
    v.bytes.fill(0,1152+i*16,1152+(i+1)*16);
    v.bytes.set(v.bytes.subarray(fp+40+i*10,fp+50+i*10),1152+i*16);
  }
  v.bytes[816]=empty;
  if((cpu.wm.exports.fpu_load_tag_word()&0xffff)!==tags)
    throw new Error('v86 cannot represent the supplied independent x87 tag word');
  for(const [to,from,size] of [[1044,22,2],[1048,24,4],[1052,32,2],[1056,28,4],[1060,34,2]])
    d.setUint32(to,size===2?d.getUint16(fp+from,true):d.getUint32(fp+from,true),true);
  d.setUint32(824,d.getUint32(b+SIMD+16,true),true);
  v.bytes.set(v.bytes.subarray(b+SIMD+32,b+SIMD+160),832);
  v.bytes[632]=0;
  cpu.update_state_flags();
}

export function exportState(engine,cpu,trap=null) {
  const v=cpuViews(engine.memory),d=v.data,b=engine.base;
  for(let i=0;i<8;i++)d.setUint32(b+16+i*4,i===4&&trap?trap.esp:v.regs[i],true);
  d.setUint32(b+48,trap?trap.pc:d.getUint32(556,true),true);
  const flags=(trap?trap.flags:cpu.wm.exports.get_eflags())>>>0;
  if(flags & ~USER_FLAGS)throw new Error(`v86 produced unsupported EFLAGS 0x${flags.toString(16)}`);
  d.setUint16(b+4,3,true);d.setUint32(b+12,0,true);d.setUint32(b+52,flags|2,true);
  const fp=b+X87;
  d.setUint16(fp+16,d.getUint16(1036,true),true);
  d.setUint16(fp+18,cpu.wm.exports.fpu_load_status_word(),true);
  d.setUint16(fp+20,cpu.wm.exports.fpu_load_tag_word(),true);
  for(const [from,to,size] of [[1044,22,2],[1048,24,4],[1052,32,2],[1056,28,4],[1060,34,2]])
    if(size===2)d.setUint16(fp+to,d.getUint32(from,true)&0xffff,true);else d.setUint32(fp+to,d.getUint32(from,true),true);
  for(let i=0;i<8;i++)v.bytes.set(v.bytes.subarray(1152+i*16,1162+i*16),fp+40+i*10);
  d.setUint32(b+SIMD+16,d.getUint32(824,true),true);
  v.bytes.set(v.bytes.subarray(832,960),b+SIMD+32);
  // only the flat windows segment profile is admitted; preserve its encoded caches.
  for(let i=0;i<6;i++) {
    const selector=trap&&i===1?trap.cs:trap&&i===2?trap.ss:v.selectors[i];
    const old=d.getUint16(b+SEGMENTS+16+i*2,true);
    if(selector!==old)throw new Error(`v86 changed unsupported segment selector ${i}: ${old}->${selector}`);
  }
}
