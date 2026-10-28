const unchanged=Object.freeze({invalidate:false,reason:'unchanged'});

export class CodeValidity {
  constructor(memory) {
    this.memory=memory;this.identity=null;this.code=null;this.mapping=null;
    this.executableEpoch=undefined;
    this.pages=new Map();this.content=new Map();
  }
  refresh(info,rows) {
    const precise=info.executableEpoch!==undefined;
    if(this.identity===info.identity && (precise?this.executableEpoch===info.executableEpoch:
        this.code===info.code && this.mapping===info.mapping)) {
      this.code=info.code;this.mapping=info.mapping;
      return unchanged;
    }
    const data=new DataView(this.memory.buffer),pages=new Map(),content=new Map();
    for(const row of rows) {
      if(!(row.permission&4))continue;
      if(!Number.isInteger(row.page) || row.page<0 || row.page>=info.guestPages ||
          !Number.isInteger(row.slot) || row.slot<0 || row.slot>=info.capacity || pages.has(row.page))
        throw new Error('invalid executable mapping in v86 code validity');
      pages.set(row.page,{slot:row.slot,permission:row.permission,
        version:data.getBigUint64(info.mappingVersions+row.page*8,true)});
      if(!content.has(row.slot))content.set(row.slot,data.getBigUint64(info.physicalVersions+row.slot*16+8,true));
    }
    let reason='unchanged';
    if(this.identity!==null) {
      if(this.identity!==info.identity)reason='address-space';
      else if(this.pages.size!==pages.size)reason='executable-mapping';
      else {
        for(const [page,current] of pages) {
          const previous=this.pages.get(page);
          if(!previous || previous.slot!==current.slot || previous.permission!==current.permission ||
              previous.version!==current.version) {reason='executable-mapping';break;}
        }
        if(reason==='unchanged')for(const [slot,version] of content) {
          if(this.content.get(slot)!==version){reason='host-code-write';break;}
        }
      }
    }
    this.identity=info.identity;this.code=info.code;this.mapping=info.mapping;
    this.executableEpoch=info.executableEpoch;
    this.pages=pages;this.content=content;
    return reason==='unchanged'?unchanged:{invalidate:true,reason};
  }
  acknowledgeGuestWrites(info,slots) {
    if(this.identity!==info.identity)throw new Error('v86 guest write acknowledgement changed address space');
    const stamped=info.beforeCode!==undefined;
    const known=!stamped || (info.beforeCode===this.code && info.mapping===this.mapping);
    const data=stamped?null:new DataView(this.memory.buffer),updates=[];
    for(const slot of slots) {
      if(!Number.isInteger(slot) || slot<0 || slot>=info.capacity)
        throw new Error('invalid v86 guest dirty slot');
      if(known && this.content.has(slot))updates.push([slot,stamped?info.transaction:data.getBigUint64(info.physicalVersions+slot*16+8,true)]);
    }
    if(!known)return;
    for(const [slot,version] of updates)this.content.set(slot,version);
    if(stamped) {
      this.code=info.code;
      if(this.executableEpoch!==undefined && info.code!==info.beforeCode)this.executableEpoch=info.code;
    }
    // legacy publication has no proven global pre-write stamp.
  }
}
