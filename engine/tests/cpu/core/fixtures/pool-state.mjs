// Inspect the private CPU ABI independently of emitter descriptors. Architectural
// payload/tag/status expectations remain literal in each calling fixture.
export function fpPoolState(memory, bank) {
  const d=new DataView(memory.buffer),base=bank+512;
  const word=at=>d.getUint32(base+at,true);
  const records=Array.from({length:8},(_,index)=>{
    const at=base+128+32*index;
    return {native:d.getFloat64(at,true),low:d.getBigUint64(at+8,true),
      high:d.getUint32(at+16,true),valid:d.getUint32(at+20,true),
      proofs:d.getUint32(at+24,true),control:d.getUint32(at+28,true)};
  });
  return {active:word(0),control:word(4),status:word(8),tag:word(12),top:word(16),
    fip:word(20),fdp:word(24),opcode:word(28),cs:word(32),ds:word(36),pending:word(40),
    left:d.getFloat64(base+48,true),right:d.getFloat64(base+56,true),
    result:d.getFloat64(base+64,true),changed:word(76),records};
}

export function linkedField(cache,memory,local) {
  const retained=cache.dispatch[`link.local${local}`];
  if(retained instanceof WebAssembly.Global)return retained.value;
  const bank=new DataView(memory.buffer).getUint32(cache.context+92,true),p=fpPoolState(memory,bank);
  const logicalMask=mask=>((mask>>>p.top)|(mask<<(8-p.top)))&255;
  if(local>=76&&local<84)return p.records[(p.top+local-76)&7].native;
  const valid=p.records.reduce((mask,record,index)=>mask|(Number(record.valid!==0)<<index),0);
  const values={84:p.active,85:p.active?logicalMask(valid):0,86:p.active?logicalMask(p.changed):0,
    87:p.control,88:(p.status&~0x3800)|(p.top<<11),89:p.tag,90:p.top,91:p.fip,92:p.fdp,
    93:p.opcode,94:p.cs,95:p.ds,100:p.pending,101:p.left,102:p.right,103:p.result};
  if(!(local in values))throw Error(`unknown linked field ${local}`);
  return values[local];
}
