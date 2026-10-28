import {KEY} from '../engine.js';
import {PAGE,BACKING_BASE,DIRECTORY,TABLES,GDT,PRIVATE_VA} from './layout.js';

export class PageTables {
  constructor(engine,cpu) {
    this.engine=engine;this.cpu=cpu;this.ram=cpu.mem8.byteOffset;this.pages=[];
    this.mappingEpoch=null;this.codeVersion=null;this.identity=null;
    this.descriptor();
    const length=this.capacity*PAGE;
    if(BACKING_BASE+length>cpu.mem8.length)throw new Error('v86 physical RAM is too small for Ring3 backing');
    engine.ok('component_cpu_attach',...KEY,this.ram+BACKING_BASE,length);
    const d=new DataView(engine.memory.buffer);
    new Uint8Array(engine.memory.buffer,this.ram+DIRECTORY,PAGE*1025).fill(0);
    for(let index=0;index<1024;index++)d.setUint32(this.ram+DIRECTORY+index*4,(TABLES+index*PAGE)|7,true);
    for(let index=0;index<8;index++) {
      const page=(PRIVATE_VA>>>12)+index;
      d.setUint32(this.pte(page),(GDT+index*PAGE)|3,true);
    }
    d.setUint32(580,0x80010033,true);d.setUint32(592,DIRECTORY,true);d.setUint32(596,0x600,true);
    this.refresh();
  }
  pte(page) {return this.ram+TABLES+page*4;}
  descriptor() {
    const h=this.engine;h.ok('component_cpu_view',...KEY);
    const d=h.resultView(128);
    if(d.getUint32(0,true)!==0x56433352 || d.getUint32(8,true)!==128)
      throw new Error('invalid external CPU memory descriptor');
    const values={backing:d.getUint32(16,true),length:d.getUint32(20,true),maps:d.getUint32(24,true),
      permissions:d.getUint32(28,true),guestPages:d.getUint32(32,true),capacity:d.getUint32(36,true),
      mapped:d.getUint32(40,true),privateSlots:d.getUint32(52,true),identity:d.getBigUint64(64,true),
      transaction:d.getBigUint64(72,true),code:d.getBigUint64(80,true),mapping:d.getBigUint64(88,true)};
    this.capacity=values.capacity;
    return values;
  }
  refresh() {
    const info=this.descriptor(),d=new DataView(this.engine.memory.buffer);
    if(this.identity!==null && this.identity!==info.identity)throw new Error('v86 backing identity changed');
    this.identity=info.identity;
    if(this.codeVersion!==null && this.codeVersion!==info.code)this.cpu.jit_clear_cache();
    this.codeVersion=info.code;
    if(this.mappingEpoch===info.mapping)return;
    if(info.backing!==this.ram+BACKING_BASE)throw new Error('v86 backing detached unexpectedly');
    for(const row of this.pages)d.setUint32(row.pte,0,true);
    const maps=new Uint32Array(this.engine.memory.buffer,info.maps,info.guestPages);
    const rights=new Uint8Array(this.engine.memory.buffer,info.permissions,info.guestPages);
    const privateSlots=info.privateSlots?new Uint32Array(this.engine.memory.buffer,info.privateSlots,info.guestPages):null;
    const rows=[];
    for(let page=0;page<info.guestPages;page++) {
      const mapped=maps[page];if(!mapped)continue;
      if(page>=(PRIVATE_VA>>>12) && page<(PRIVATE_VA>>>12)+8)
        throw new Error('Ring3 guest mapping collides with v86 private trap window');
      const permission=rights[page],physical=BACKING_BASE+(mapped-1)*PAGE,pte=this.pte(page);
      if(mapped>info.capacity)throw new Error('invalid Ring3 physical slot');
      const writable=(permission&2)!==0 && !privateSlots?.[page];
      d.setUint32(pte,physical|((permission&7)?5:0)|(writable?2:0),true);
      rows.push({page,pte,slot:mapped-1,permission,cow:!!privateSlots?.[page]});
    }
    this.pages=rows;this.maps=info.maps;this.rights=info.permissions;this.privateSlots=info.privateSlots;
    this.mappingEpoch=info.mapping;
    this.cpu.full_clear_tlb();
    this.cpu.jit_clear_cache();
  }
  isCopyOnWrite(address) {
    const page=address>>>12;
    return !!this.privateSlots && (new Uint8Array(this.engine.memory.buffer,this.rights,1<<20)[page]&2)!==0
      && new Uint32Array(this.engine.memory.buffer,this.privateSlots,1<<20)[page]!==0;
  }
  faultReason(address) {
    return new Uint32Array(this.engine.memory.buffer,this.maps,1<<20)[address>>>12] ? 2 : 1;
  }
  publishDirty() {
    const d=new DataView(this.engine.memory.buffer),dirty=new Set();
    for(const row of this.pages) {
      const value=d.getUint32(row.pte,true);
      if(value&64){dirty.add(row.slot);d.setUint32(row.pte,value&~64,true);}
    }
    if(!dirty.size)return 0;
    this.cpu.full_clear_tlb();
    const slots=[...dirty];
    for(let start=0;start<slots.length;start+=1024) {
      const part=slots.slice(start,start+1024);
      this.engine.request(new Uint8Array(new Uint32Array(part).buffer));
      this.engine.ok('component_cpu_dirty',...KEY,part.length);
    }
    // v86 already invalidates code written by the guest; only later host changes need a flush.
    this.codeVersion=this.descriptor().code;
    return slots.length;
  }
}
