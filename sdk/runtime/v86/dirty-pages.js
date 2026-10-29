import {DIRECTORY,TABLES} from './layout.js';

let rearmModule;
export function createDirtyRearm(memory,invlpg) {
  // rearm(pointer,count) calls the native helper for ((load32(pointer)-4096)<<10).
  // the loop imports the shared memory and actual wasm function without a js call per page.
  rearmModule??=new WebAssembly.Module(new Uint8Array([
    0x00,0x61,0x73,0x6d,0x01,0x00,0x00,0x00,0x01,0x0a,0x02,0x60,0x01,0x7f,0x00,0x60,
    0x02,0x7f,0x7f,0x00,0x02,0x1c,0x02,0x03,0x63,0x70,0x75,0x06,0x69,0x6e,0x76,0x6c,
    0x70,0x67,0x00,0x00,0x03,0x63,0x70,0x75,0x06,0x6d,0x65,0x6d,0x6f,0x72,0x79,0x02,
    0x00,0x00,0x03,0x02,0x01,0x01,0x07,0x09,0x01,0x05,0x72,0x65,0x61,0x72,0x6d,0x00,
    0x01,0x0a,0x34,0x01,0x32,0x01,0x01,0x7f,0x20,0x00,0x20,0x01,0x41,0x02,0x74,0x6a,
    0x21,0x02,0x02,0x40,0x03,0x40,0x20,0x00,0x20,0x02,0x4f,0x0d,0x01,0x20,0x00,0x28,
    0x02,0x00,0x41,0x80,0x20,0x6b,0x41,0x0a,0x74,0x10,0x00,0x20,0x00,0x41,0x04,0x6a,
    0x21,0x00,0x0c,0x00,0x0b,0x0b,0x0b,
  ]));
  return new WebAssembly.Instance(rearmModule,{cpu:{memory,invlpg}}).exports.rearm;
}

export class DirtyPages {
  constructor(memory,ram,flush) {
    this.memory=memory;this.ram=ram;this.flush=flush;this.groups=new Map();
  }
  add(row) {
    const index=row.page>>>10;
    let rows=this.groups.get(index);
    if(!rows)this.groups.set(index,rows=new Set());
    rows.add(row);
  }
  remove(row) {
    const index=row.page>>>10,rows=this.groups.get(index);
    rows?.delete(row);if(!rows?.size)this.groups.delete(index);
  }
  reset(rows) {
    this.groups.clear();for(const row of rows)this.add(row);
  }
  collect(partitioned) {
    const directory=new Uint32Array(this.memory.buffer,this.ram+DIRECTORY,1024);
    const ptes=new Uint32Array(this.memory.buffer,this.ram+TABLES,1<<20);
    const dirty=new Set();let scannedPtes=0,scannedDirectories=0,resetAccess=false;
    for(const [index,rows] of this.groups) {
      if(partitioned) {
        scannedDirectories++;
        if(!(directory[index]&32))continue;
        directory[index]&=~32;resetAccess=true;
      }
      for(const row of rows) {
        scannedPtes++;
        if(ptes[row.page]&64) {dirty.add(row.slot);ptes[row.page]&=~64;}
      }
    }
    // clearing a directory's accessed bit requires a new page walk, even after a read-only batch.
    if(resetAccess || dirty.size)this.flush();
    return {slots:[...dirty],scannedPtes,scannedDirectories};
  }
}
