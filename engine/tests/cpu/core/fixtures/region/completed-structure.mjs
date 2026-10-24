// Authenticate the emitted completed FP handoff before attaching any witness.
export function inspectCompletedHandoffs(module) {
  const functionIndex=[...module.names].find(([,name])=>name==='guest.run')?.[0];
  const run=module.functions[functionIndex-module.imports.length],adapters=[],errors=[];
  if(!run)return{functionIndex,adapters,errors:['missing actual guest.run']};
  const ops=run.ops;
  const at=(i,op,local)=>ops[i]?.op===op&&(local===undefined||ops[i].local===local);
  const value=(i,op,wanted)=>at(i,op)&&ops[i].value===wanted;
  const signature=index=>module.types[index<module.imports.length?module.imports[index].type:
    module.functionTypes[index-module.imports.length]];
  const same=(a,b)=>a?.length===b.length&&a.every((v,i)=>v===b[i]);
  const writer=(name,parameters)=>{
    const index=module.imports.findIndex(item=>item.module==='ring3'&&item.name===name);
    if(index<0)return undefined;
    const type=signature(index);
    return same(type?.parameters,parameters)&&same(type?.results,[0x7f])?index:undefined;
  };
  const native=writer('support.commit_native',[0x7f,0x7f,0x7c,0x7f,0x7f]);
  const raw=writer('support.commit_raw',[0x7f,0x7f,0x7e,0x7f]);
  const isValue=i=>at(i,0x20)&&ops[i].local>=179;
  const physical=i=>at(i,0x20,152)&&at(i+1,0x20,160)&&value(i+2,0x41,ops[i+2]?.value)&&
    Number.isInteger(ops[i+2]?.value)&&ops[i+2].value>=0&&ops[i+2].value<8&&at(i+3,0x6a)&&
    value(i+4,0x41,7)&&at(i+5,0x71);
  for(let i=4;i<ops.length;i++) {
    if(!(at(i-4,0x20,84)&&at(i-3,0x20,126)&&at(i-2,0x45)&&at(i-1,0x71)&&at(i,4)))continue;
    const frame=ops[i].frame,end=frame?.end,fail=[];
    const require=(ok,why)=>{if(!ok)fail.push(why);};
    require(frame?.type===-64&&frame.otherwise===undefined&&Number.isInteger(end),'empty active-view guard without else');
    if(!Number.isInteger(end)){errors.push({guardOffset:ops[i-4].offset,reasons:fail});continue;}
    const body=ops.slice(i+1,end),stores=body.filter(node=>node.op>=0x36&&node.op<=0x3e);
    require(!body.some(node=>[0,12,13,14,15,17,18,19,20,21].includes(node.op)),
      'projection has no trap, exit, indirect call or branch out');
    require(native!==undefined&&raw!==undefined,'actual typed native/raw writer imports');
    const calls=body.filter(node=>node.op===16),writerCalls=[];
    for(const call of calls) {
      const n=ops.indexOf(call),isNative=call.call===native,isRaw=call.call===raw,start=n-(isNative?9:8);
      const argumentsMatch=physical(start)&&isValue(start+6)&&(isNative?
        (isValue(start+7)||at(start+7,0x41)&&ops[start+7].value>=0&&ops[start+7].value<=15)&&isValue(start+8):
        isValue(start+7));
      require((isNative||isRaw)&&argumentsMatch&&at(n+1,0x1a),
        'only exact same-pool payload writers; no getter or descriptor call');
      writerCalls.push({offset:call.offset,index:call.call,kind:isNative?'native':isRaw?'raw':'unexpected'});
    }
    const expectedOffsets=[4,8,12,16,20,24,28,32,36,40,48,56,64,76];
    require(same(stores.map(node=>node.memoryOffset).sort((a,b)=>a-b),expectedOffsets),
      'exact private pool header, changed mask and pending triple stores');
    require(stores.every(node=>node.frames.at(-1)===frame),
      'every header and changed-mask store executes on the taken adapter guard');
    for(const store of stores) {
      const n=ops.indexOf(store),offset=store.memoryOffset;
      if(offset===76) {
        const pattern=[
          [0x20,152],[0x20,152],[0x28,76],[0x20,158],[0x20,160],[0x74],
          [0x20,158],[0x41,8],[0x20,160],[0x6b],[0x76],[0x72],
          [0x41,255],[0x71],[0x72],
        ];
        require(store.op===0x36&&store.align===2&&pattern.every(([op,arg],j)=>{
          const p=ops[n-pattern.length+j];return p?.op===op&&(arg===undefined||
            (op===0x20?p.local:op===0x28?p.memoryOffset:p.value)===arg);
        }),'changed mask merges only rotated completed dirty bits into same pool');
      } else if(offset===8) {
        require(store.op===0x36&&store.align===2&&at(n-4,0x20,152)&&isValue(n-3)&&
          value(n-2,0x41,~0x3800)&&at(n-1,0x71),'status stores preserve separately published TOP');
      } else if([48,56,64].includes(offset)) {
        require(store.op===0x39&&store.align===3&&at(n-2,0x20,152)&&isValue(n-1),
          'pending arithmetic triples use actual staged F64 values and same pool');
      } else {
        const source=ops[n-1]?.local;
        const allowed=offset===12?source===159:offset===16?source===160:
          isValue(n-1)||offset===20&&source===163||offset===28&&source===164||
          offset===32&&source===165||offset===36&&source===166;
        require(store.op===0x36&&store.align===2&&at(n-2,0x20,152)&&at(n-1,0x20)&&allowed,
          'header fields use staged SSA values or exact metadata override locals');
      }
    }
    const tail=end-18,clears=[84,85,86,153,154,109,178];
    require(clears.every((local,n)=>value(tail+2*n,0x41,0)&&at(tail+2*n+1,0x21,local))&&
      value(tail+14,0x41,0)&&at(tail+15,0x21)&&ops[tail+15].local>=179&&
      value(tail+16,0x42,0n)&&at(tail+17,0x21,177),
      'exact active, VALID, DIRTY, loaded, admission, selection, token and marker release');
    if(fail.length)errors.push({guardOffset:ops[i-4].offset,reasons:fail});
    else adapters.push({functionIndex,guardOffset:ops[i-4].offset,instrumentOffset:ops[i+1].offset,
      endOffset:ops[end].offset,releaseOffset:ops[tail+1].offset,validResetOffset:ops[tail+3].offset,
      marker:ops[tail+15].local,writerCalls,poolStoreOffsets:expectedOffsets});
  }
  return{functionIndex,adapters,errors};
}
