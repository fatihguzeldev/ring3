import {KEY} from '../engine.js';
import {PAGE,BACKING_BASE,DIRECTORY,TABLES,GDT,PRIVATE_VA} from './layout.js';
import {CodeValidity} from './code-validity.js';
import {DirtyPages} from './dirty-pages.js';

export class PageTables {
  constructor(engine,cpu,options={}) {
    this.engine=engine;this.cpu=cpu;this.ram=cpu.mem8.byteOffset;this.pages=[];
    this.ownsPin=false;
    this.mappingEpoch=null;this.codeVersion=null;this.identity=null;
    this.policy={selectiveCode:options.selectiveCode??true,incrementalSync:options.incrementalSync??true};
    this.codeValidity=new CodeValidity(engine.memory);
    this.rowsByPage=new Map();this.dirty=new DirtyPages(engine.memory,this.ram,()=>cpu.full_clear_tlb());
    this.lastMappingScanCount=0;this.lastMappingFullRebuild=false;
    this.lastDirtyScanCount=0;this.lastDirtyDirectoryCount=0;
    const info=this.descriptor();
    this.nativeDirtySupported=(info.mappingChangeFlags&2)!==0;
    this.nativeDirty=options.nativeDirty??this.nativeDirtySupported;
    if(typeof this.nativeDirty!=='boolean' || (this.nativeDirty && !this.nativeDirtySupported))
      throw new Error('unsupported v86 native dirty collector');
    const length=this.capacity*PAGE;
    if(BACKING_BASE+length>cpu.mem8.length)throw new Error('v86 physical RAM is too small for Ring3 backing');
    engine.ok('component_cpu_attach',...KEY,this.ram+BACKING_BASE,length);
    this.ownsPin=true;
    try {
      const d=new DataView(engine.memory.buffer);
      new Uint8Array(engine.memory.buffer,this.ram+DIRECTORY,PAGE*1025).fill(0);
      for(let index=0;index<1024;index++)d.setUint32(this.ram+DIRECTORY+index*4,(TABLES+index*PAGE)|7,true);
      for(let index=0;index<8;index++) {
        const page=(PRIVATE_VA>>>12)+index;
        d.setUint32(this.pte(page),(GDT+index*PAGE)|3,true);
      }
      d.setUint32(580,0x80010033,true);d.setUint32(592,DIRECTORY,true);d.setUint32(596,0x600,true);
      this.refresh();
    } catch(error) {
      this.rollback(error);throw error;
    }
  }
  release() {
    if(!this.ownsPin)return;
    this.engine.ok('component_cpu_release',...KEY);this.ownsPin=false;
  }
  rollback(error) {
    try {this.release();}catch(cleanup){error.cleanupFailure=cleanup.message;}
  }
  pte(page) {return this.ram+TABLES+page*4;}
  configureDirty(nativeDirty,owner) {
    if(owner?.paused!==true || owner?.open!==false)
      throw new Error('v86 dirty collector changes require a closed paused owner');
    if(typeof nativeDirty!=='boolean' || (nativeDirty && !this.nativeDirtySupported))
      throw new Error('unsupported v86 native dirty collector');
    if(this.nativeDirty===nativeDirty)return nativeDirty;
    this.publishDirty();this.nativeDirty=nativeDirty;
    return nativeDirty;
  }
  configure(policy) {
    if(typeof policy?.selectiveCode!=='boolean' || typeof policy?.incrementalSync!=='boolean')
      throw new Error('v86 memory policy requires two boolean switches');
    if(this.policy.selectiveCode===policy.selectiveCode && this.policy.incrementalSync===policy.incrementalSync)
      return {...this.policy};
    this.publishDirty();
    this.cpu.full_clear_tlb();this.cpu.jit_clear_cache();
    this.policy={...policy};this.mappingEpoch=null;this.codeVersion=null;
    this.codeValidity=new CodeValidity(this.engine.memory);this.refresh();
    return {...this.policy};
  }
  descriptor() {
    const h=this.engine;h.ok('component_cpu_view',...KEY);
    const d=h.resultView(128);
    if(d.getUint32(0,true)!==0x56433352 || d.getUint32(8,true)!==128)
      throw new Error('invalid external CPU memory descriptor');
    const values={backing:d.getUint32(16,true),length:d.getUint32(20,true),maps:d.getUint32(24,true),
      permissions:d.getUint32(28,true),guestPages:d.getUint32(32,true),capacity:d.getUint32(36,true),
      mapped:d.getUint32(40,true),mappingVersions:d.getUint32(44,true),physicalVersions:d.getUint32(48,true),
      privateSlots:d.getUint32(52,true),identity:d.getBigUint64(64,true),
      transaction:d.getBigUint64(72,true),code:d.getBigUint64(80,true),mapping:d.getBigUint64(88,true),
      mappingChanges:d.getUint32(96,true),mappingChangeCount:d.getUint32(100,true),
      mappingChangeCapacity:d.getUint32(104,true),mappingChangeFlags:d.getUint32(108,true)};
    this.capacity=values.capacity;
    return values;
  }
  refresh() {
    const info=this.descriptor(),d=new DataView(this.engine.memory.buffer);
    this.lastMappingScanCount=0;this.lastMappingFullRebuild=false;
    if(this.identity!==null && this.identity!==info.identity)throw new Error('v86 backing identity changed');
    this.identity=info.identity;
    this.physicalVersions=info.physicalVersions;this.guestPages=info.guestPages;
    if(!this.policy.selectiveCode && this.codeVersion!==null && this.codeVersion!==info.code)this.cpu.jit_clear_cache();
    this.codeVersion=info.code;
    if(this.mappingEpoch===info.mapping) {
      if(this.policy.selectiveCode && this.codeValidity.refresh(info,this.pages).invalidate)this.cpu.jit_clear_cache();
      return;
    }
    if(info.backing!==this.ram+BACKING_BASE)throw new Error('v86 backing detached unexpectedly');
    const incremental=this.policy.incrementalSync && this.mappingEpoch!==null && (info.mappingChangeFlags&1)!==0;
    if((info.mappingChangeFlags&1) && (info.mappingChangeCapacity!==info.guestPages ||
        info.mappingChangeCount>info.mappingChangeCapacity))throw new Error('invalid v86 mapping change journal');
    if(!incremental) {
      for(const row of this.pages)d.setUint32(row.pte,0,true);
      this.pages=[];this.rowsByPage.clear();this.dirty.reset([]);this.lastMappingFullRebuild=true;
    }
    const maps=new Uint32Array(this.engine.memory.buffer,info.maps,info.guestPages);
    const rights=new Uint8Array(this.engine.memory.buffer,info.permissions,info.guestPages);
    const privateSlots=info.privateSlots?new Uint32Array(this.engine.memory.buffer,info.privateSlots,info.guestPages):null;
    const changes=incremental?new Uint32Array(this.engine.memory.buffer,info.mappingChanges,info.mappingChangeCount):null;
    const count=incremental?changes.length:info.guestPages;
    for(let index=0;index<count;index++) {
      const page=changes?changes[index]:index;
      if(page>=info.guestPages)throw new Error('invalid v86 mapping change page');
      const mapped=maps[page],old=incremental?this.rowsByPage.get(page):null;
      if(!mapped) {
        if(old) {
          d.setUint32(old.pte,0,true);this.dirty.remove(old);this.rowsByPage.delete(page);
          const last=this.pages.pop();
          if(last!==old){this.pages[old.index]=last;last.index=old.index;}
        }
        continue;
      }
      if(page>=(PRIVATE_VA>>>12) && page<(PRIVATE_VA>>>12)+8)
        throw new Error('Ring3 guest mapping collides with v86 private trap window');
      const permission=rights[page],physical=BACKING_BASE+(mapped-1)*PAGE,pte=this.pte(page);
      if(mapped>info.capacity)throw new Error('invalid Ring3 physical slot');
      const writable=(permission&2)!==0 && !privateSlots?.[page];
      d.setUint32(pte,physical|((permission&7)?5:0)|(writable?2:0),true);
      if(old)Object.assign(old,{slot:mapped-1,permission,cow:!!privateSlots?.[page]});
      else {
        const row={page,pte,slot:mapped-1,permission,cow:!!privateSlots?.[page],index:this.pages.length};
        this.pages.push(row);
        if(this.policy.incrementalSync){this.rowsByPage.set(page,row);this.dirty.add(row);}
      }
    }
    this.lastMappingScanCount=count;
    this.maps=info.maps;this.rights=info.permissions;this.privateSlots=info.privateSlots;
    this.mappingEpoch=info.mapping;
    this.cpu.full_clear_tlb();
    if(!this.policy.selectiveCode || this.codeValidity.refresh(info,this.pages).invalidate)this.cpu.jit_clear_cache();
    if((info.mappingChangeFlags&1) && info.mappingChangeCount)
      this.engine.ok('component_cpu_ack_mappings',...KEY,Number(info.mapping&0xffffffffn),Number(info.mapping>>32n),info.mappingChangeCount);
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
    if(this.nativeDirty && this.policy.incrementalSync)return this.publishDirtyNative();
    let result;
    if(this.policy.incrementalSync)result=this.dirty.collect(true);
    else {
      const d=new DataView(this.engine.memory.buffer),dirty=new Set();
      for(const row of this.pages) {
        const value=d.getUint32(row.pte,true);
        if(value&64){dirty.add(row.slot);d.setUint32(row.pte,value&~64,true);}
      }
      if(dirty.size)this.cpu.full_clear_tlb();
      result={slots:[...dirty],scannedPtes:this.pages.length,scannedDirectories:0};
    }
    const {slots,scannedPtes,scannedDirectories}=result;
    this.lastDirtyScanCount=scannedPtes;this.lastDirtyDirectoryCount=scannedDirectories;
    if(!slots.length)return 0;
    for(let start=0;start<slots.length;start+=1024) {
      const part=slots.slice(start,start+1024);
      this.engine.request(new Uint8Array(new Uint32Array(part).buffer));
      this.engine.ok('component_cpu_dirty',...KEY,part.length);
    }
    // v86 already invalidates code written by the guest; only later host changes need a flush.
    const info=this.descriptor();this.codeVersion=info.code;
    if(this.policy.selectiveCode)this.codeValidity.acknowledgeGuestWrites(info,slots);
    return slots.length;
  }
  publishDirtyNative() {
    const h=this.engine,epoch=this.mappingEpoch;
    h.ok('component_cpu_collect_dirty',...KEY,Number(epoch&0xffffffffn),Number(epoch>>32n),this.ram+DIRECTORY);
    let flushed=false;
    try {
      const d=h.resultView(80),buffer=h.memory.buffer;
      if(d.getUint32(0,true)!==0x43443352 || d.getUint16(4,true)!==1 || d.getUint16(6,true)!==1 ||
          d.getUint32(8,true)!==80 || d.getUint32(12,true)!==0)
        throw new Error('invalid v86 native dirty result header');
      const pointer=d.getUint32(16,true),count=d.getUint32(20,true),resetAccess=d.getUint32(24,true),
        flushRequired=d.getUint32(28,true),scannedPtes=d.getUint32(32,true),scannedDirectories=d.getUint32(36,true);
      const info={mapping:d.getBigUint64(40,true),transaction:d.getBigUint64(48,true),code:d.getBigUint64(56,true),
        identity:d.getBigUint64(64,true),capacity:d.getUint32(72,true),physicalVersions:d.getUint32(76,true)};
      if(info.identity!==this.identity || info.mapping!==epoch || info.capacity!==this.capacity ||
          info.physicalVersions!==this.physicalVersions || count>info.capacity || (count && !pointer) || pointer%4 ||
          pointer>buffer.byteLength || count>(buffer.byteLength-pointer)/4 || resetAccess>1 || flushRequired>1 ||
          ((resetAccess || count) && !flushRequired) || scannedPtes>this.guestPages || scannedDirectories>1024)
        throw new Error('invalid v86 native dirty result bounds or identity');
      const slots=new Uint32Array(buffer,pointer,count);
      if(flushRequired){this.cpu.full_clear_tlb();flushed=true;}
      this.lastDirtyScanCount=scannedPtes;this.lastDirtyDirectoryCount=scannedDirectories;
      if(count) {
        if(this.policy.selectiveCode)this.codeValidity.acknowledgeGuestWrites(info,slots);
        this.codeVersion=info.code;
      }
      return count;
    } catch(error) {
      // a successful native call may already have reset page-table access or dirty bits.
      if(!flushed)this.cpu.full_clear_tlb();
      throw error;
    }
  }
}
