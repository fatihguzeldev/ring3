import {DIRECTORY,TABLES} from './layout.js';

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
