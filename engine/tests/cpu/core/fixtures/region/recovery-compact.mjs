import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {recoveryBlob,inspectRecoveryBlob} from '../../../../../../sdk/runtime/recovery.js';
import {readGeneratedModule} from './structure.mjs';

const ROOT=0x1000,TARGET=0x1800,FLAGS=0x403;
const registers=[0x7fffffc0,0x11223344,0x55667788,0x3000,0x4800,0x13579bdf,TARGET,0x2468ace0];
const initialFp={status:0,tag:0xffff,opcode:0x5a5,fip:0x11223344,fdp:0x55667788,cs:0x33,ds:0x5b,
  registers:{0:'01000000000000c00040',1:'00000000000000000080',2:'0000000000000040ff3f',
    3:'00000000000000c0ff7f',4:'7856341200000080ff3f',5:'ffffffffffffffffffff',
    6:'efcdab8967452301807f',7:'ffffffffffffffffffff'}};
const opaqueFp={status:0x1220,tag:0xff2f,opcode:0x5a5,fip:0x11223344,fdp:0x55667788,
  registers:{0:'01000000000000c00040',1:'00000000000000000080',2:'ffffffffffffffffffff'}};
const sha=bytes=>createHash('sha256').update(bytes).digest('hex');
const dword=value=>{const b=Buffer.alloc(4);b.writeUInt32LE(value>>>0);return b.toString('hex');};
const fpData=page=>page.set(Buffer.from('0000c03f000080338877665544332211aabbccddeeff00aa','hex'));
const scalarData=page=>{const v=new DataView(page.buffer);for(let i=0;i<1024;i++)v.setUint32(i*4,0x89abcd00+i,true);};

// Exact dyadic literals, independent of both the numerical engine and recovery
// representation. Each PC24 add ties back to1.5; PC53 retains all N increments.
function fpResult(count,single) {
  const n=single?0n:BigInt(count),raw=Buffer.alloc(10),stored=Buffer.alloc(8);
  raw.writeBigUInt64LE(0xc000000000000000n+(n<<39n));raw.writeUInt16LE(0x3fff,8);
  stored.writeBigUInt64LE(0x3ff8000000000000n+(n<<28n));
  return{raw:raw.toString('hex'),stored:stored.toString('hex')};
}

function literal(t,wanted) {
  const expected=t.bytes(t.context(),128),v=new DataView(expected.buffer),gpr=t.registers.slice(),fault=wanted.fault??[0,0,0];
  for(const[index,value]of Object.entries(wanted.regs??{}))gpr[Number(index)]=value;
  gpr.forEach((value,index)=>v.setUint32(index*4,value>>>0,true));
  for(const[offset,value]of[[32,wanted.pc],[36,wanted.flags??FLAGS],[40,wanted.retired],[44,wanted.remaining??0],
    [48,fault[0]],[52,fault[1]],[112,0],[116,fault[2]]])v.setUint32(offset,value>>>0,true);
  const run=t.check({...wanted,flags:wanted.flags??FLAGS});
  assert.deepEqual(t.bytes(t.context(),128),expected,'independent complete Context128 including lease/source/owner identity');
  return run;
}

// Decode the exact existing v2 immutable data transport. No guest getter or
// recovery function supplies expected values or decides which records are read.
function wordsOf(source) {
  const found=recoveryBlob(source);assert(found,'actual owner has one recovery blob');
  const metadata=inspectRecoveryBlob(found.bytes),d=new DataView(found.bytes.buffer,found.bytes.byteOffset,found.bytes.byteLength);
  const words=[];
  for(let index=0;index<metadata.words;index++){
    const bit=index*metadata.bits,word=Math.floor(bit/64),shift=bit%64;
    let packed=d.getBigUint64(metadata.packedOffset+word*8,true)>>BigInt(shift);
    if(shift+metadata.bits>64)packed|=d.getBigUint64(metadata.packedOffset+(word+1)*8,true)<<BigInt(64-shift);
    const key=Number(packed&((1n<<BigInt(metadata.bits))-1n));assert(key<metadata.dictionary);
    const entry=metadata.dictionaryOffset+key*(metadata.mode===0?8:metadata.mode===1?5:6);
    if(metadata.mode===0)words.push(d.getBigUint64(entry,true));
    else{const high=metadata.mode===1?d.getUint8(entry+4):d.getUint16(entry+4,true);assert(high<metadata.highCount);
      words.push(BigInt(d.getUint32(entry,true))|(BigInt(d.getUint32(metadata.highTableOffset+high*4,true))<<32n));}
  }
  return{metadata,words};
}

const constant=(node,value)=>node?.op===0x41&&(value===undefined||node.value===value);
const localGet=node=>node?.op===0x20;
const localSet=node=>node?.op===0x21;

// The retained alternative is positively identified by the original request
// normalization and exact dense I32 constant/selector leaf evaluator. Absence
// of the VM alone never certifies a recognized legacy representation.
function legacyFormat(run,getter,words) {
  const ops=run.ops,eq=(n,op,local)=>n?.op===op&&(local===undefined||n.local===local),
    get=(n,local)=>eq(n,0x20,local),set=(n,local)=>eq(n,0x21,local),wrappers=[];
  for(let i=0;i+18<ops.length;i++){
    const p=ops.slice(i,i+19),l=p[0]?.local;
    if(!get(p[0],l)||!eq(p[1],4)||!get(p[2],l)||!constant(p[3],0)||!eq(p[4],0x4a)||!get(p[5],84)||
      !eq(p[6],0x72)||!eq(p[7],4)||!get(p[8],l)||!get(p[9],l)||!constant(p[10],-1)||!eq(p[11],0x6c)||
      !get(p[12],l)||!constant(p[13],0)||!eq(p[14],0x4a)||!eq(p[15],0x1b)||!constant(p[16],1)||
      !eq(p[17],0x6b)||!set(p[18],l+1))continue;
    const outer=p[1].frame,leaves=[];
    for(let j=i+19;j+36<outer.end;j++){
      const q=ops.slice(j,j+37),tag=q[0]?.local;
      if(!get(q[0],tag)||!constant(q[1],0)||!eq(q[2],0x4a)||!eq(q[3],4)||!get(q[4],tag)||
        !constant(q[5],1)||!eq(q[6],0x6b)||!constant(q[7])||q[7].value<0||!eq(q[8],0x6a)||
        !eq(q[9],0x10)||q[9].call!==getter||!eq(q[10],0x22)||!eq(q[11],0x42)||q[11].value!==32n||
        !eq(q[12],0x88)||!eq(q[13],0xa7)||!eq(q[14],0x22,l+15)||!eq(q[15],4)||
        !get(q[16],l+15)||!constant(q[17],2)||!eq(q[18],0x46)||!eq(q[19],4)||q[19].frame.type!==-1||
        !get(q[20],96)||!get(q[21],q[10].local)||!eq(q[22],0xa7)||!constant(q[23],1)||!eq(q[24],0x74)||
        !eq(q[25],0x6a)||!eq(q[26],0x2f)||q[26].memoryOffset!==100||!eq(q[27],5)||
        !get(q[28],q[10].local)||!eq(q[29],0xa7)||!eq(q[30],11)||!localSet(q[31])||
        !get(q[32],tag)||!constant(q[33],-1)||!eq(q[34],0x6c)||!set(q[35],tag)||!eq(q[36],11))continue;
      assert(q[0].frames.includes(outer));leaves.push({base:q[7].value,color:q[31].local,tag,offset:q[0].offset});
    }
    if(!leaves.length)continue;
    assert(leaves.every(leaf=>leaf.base===leaves[0].base),'one exact dense legacy leaf map');
    const base=leaves[0].base;assert(base<words.length);
    const entries=words.slice(base);let eligible=0;
    for(const word of entries){const kind=Number(word>>32n);assert(kind<=2,'finite legacy constant/selector map');
      if(kind===0)assert.equal(word,0n);else{eligible++;if(kind===2)assert(Number(word&0xffffffffn)<6);}}
    assert(eligible>0);
    const sites=[...new Set(ops.flatMap((n,j)=>set(n,l)&&constant(ops[j-1])&&ops[j-1].value!==0?
      [Math.abs(ops[j-1].value)-1]:[]))].sort((a,b)=>a-b);
    const stride=sites.length>1?sites[1]-sites[0]:0;assert(stride===6||stride===7,'source-backed legacy header generation');
    sites.forEach((site,j)=>assert.equal(site,stride*j,'all legacy descriptor selectors accounted'));
    assert(sites.length*stride<=base);
    wrappers.push({requestLocal:l,headerWords:stride,sites:sites.length,leafBase:base,
      leafReferences:entries.length,eligibleLeaves:eligible,leafColors:leaves.length,
      commonOffset:p[0].offset,commonEnd:ops[outer.end].offset});
  }
  assert(wrappers.length<=1,'one positively authenticated legacy recovery executor');
  return wrappers[0]??null;
}

function recipeTable(words,base,sites) {
  const lo=value=>Number(value&0xffffffffn),hi=value=>Number(value>>32n);
  const closures=sites.map(site=>({site,base:lo(words[site+6]),count:hi(words[site+6])}));
  const nonempty=closures.filter(c=>c.count);assert(nonempty.length>0,'real recovery closures');
  for(const closure of closures)assert(closure.count?closure.base>base:closure.base===0);
  const end=Math.min(...nonempty.map(c=>c.base));assert((end-base)%4===0);
  const count=(end-base)/4;assert(count>0&&end<=words.length);
  const records=[];
  for(let id=0;id<count;id++){
    const [header,arg,dep01,dep23]=words.slice(base+4*id,base+4*id+4),low=lo(header),
      kind=low&255,type=(low>>>8)&255,color=hi(header),deps=[lo(dep01),hi(dep01),lo(dep23),hi(dep23)];
    assert(kind<=14&&type<=2&&(low>>>16)===0,'finite exact source kind/type');
    for(const dependency of deps)assert(dependency===0||dependency<=id,'dependencies strictly precede their user');
    const requireDeps=types=>{assert.deepEqual(deps.map(Boolean),[...types.map(()=>true),...Array(4-types.length).fill(false)]);
      types.forEach((expected,i)=>assert.equal(records[deps[i]-1].type,expected));};
    if(kind===0||kind===2)requireDeps([]);
    else if(kind===1){assert.equal(type,0);assert(arg>=2n&&arg<=9n);requireDeps([]);}
    else if(kind>=3&&kind<=7){assert.equal(type,0);assert.equal(arg,0n);requireDeps([0,0]);}
    else if(kind===8){assert.equal(type,0);assert.equal(hi(arg),0);requireDeps([0]);}
    else if(kind===9){
      const field=lo(arg),argument=hi(arg),types=[2,1,0,0,0,0,0,0,0,2,2,2,0,0,0,0,0,0];
      assert(field<=17);assert.equal(type,types[field]);
      if(field<=4)assert(argument<8);else if(field===17)assert(argument<6);else assert.equal(argument,0);
      if(deps[0]){assert(field<=4);requireDeps([0]);}else requireDeps([]);
    }else if(kind===10){assert.equal(type,0);assert.equal(arg,0n);requireDeps([0]);}
    else if(kind===11){assert.equal(type,0);assert(arg<8n);requireDeps([0,0,0]);}
    else if(kind===12){assert.equal(type,0);assert.equal(arg,0n);requireDeps([2]);}
    else if(kind===13){assert.equal(type,0);assert.equal(arg,0n);requireDeps([1,0]);}
    else if(kind===14){assert.equal(type,0);assert.equal(arg,0n);requireDeps([0,2,1,0]);}
    records.push({kind,type,color,deps});
  }
  for(const closure of closures){
    assert(closure.base+Math.ceil(closure.count/2)<=words.length);
    const markers=Array.from({length:closure.count},(_,i)=>{
      const word=words[closure.base+(i>>>1)];return i&1?hi(word):lo(word);
    });
    if(closure.count&1)assert.equal(hi(words[closure.base+(closure.count>>>1)]),0,'odd packed closure has neutral padding');
    const colors=new Set(),needed=new Set(markers);
    markers.forEach((marker,i)=>{
      assert(marker>0&&marker<=count&&(!i||markers[i-1]<marker),'exact sorted bounded RefId+1 closure');
      const record=records[marker-1];assert(!colors.has(record.color),'one whole closure never aliases live colors');colors.add(record.color);
      for(const dependency of record.deps)if(dependency)assert(needed.has(dependency),'closure contains every selected reference dependency');
    });
  }
  return{base,count,end,kinds:[...new Set(records.map(r=>r.kind))].sort((a,b)=>a-b),
    sites:sites.length,closures:closures.map(({site,base,count})=>({site,base,count})),
    maxClosure:Math.max(...closures.map(c=>c.count)),records};
}


// The shared form is identified by its actual scratch transport, held ticket,
// bounded helper call loop, selected closure capture and every finite output.
// Importing the complete support ABI alone never identifies this form.
export function inspectSharedRecovery(source) {
  const module=readGeneratedModule(source),scratch=module.globalImports.flatMap((item,index)=>
    item.module==='ring3'&&item.name==='recovery.scratch'?[{...item,index}]:[]),
    imported=module.imports.findIndex(item=>item.module==='ring3'&&item.name==='support.recover'),
    runIndex=[...module.names].find(([,name])=>name==='guest.run')?.[0],
    getter=[...module.names].find(([,name])=>name==='guest.recovery_data')?.[0];
  const run=module.functions[runIndex-module.imports.length],ops=run?.ops??[],
    helperCalls=ops.filter(node=>node.op===16&&node.call===imported);
  if(!scratch.length){assert.equal(helperCalls.length,0,'a legacy encoding does not call the shared recovery helper');return null;}
  assert.deepEqual(scratch,[{module:'ring3',name:'recovery.scratch',valueType:0x7f,mutable:0,index:27}]);
  assert(imported>=0&&Number.isInteger(getter)&&getter>=module.imports.length);
  assert.deepEqual(module.types[module.imports[imported].type],{parameters:[0x7f,0x7f,0x7f,0x7f],results:[0x7f]});
  assert.equal(helperCalls.length,1,'one actual shared recovery callsite');
  const eq=(n,op,local)=>n?.op===op&&(local===undefined||n.local===local),
    get=(n,local)=>eq(n,0x20,local),set=(n,local)=>eq(n,0x21,local),
    find=(from,to,predicate)=>ops.slice(from,to).findIndex((_,j)=>predicate(ops.slice(from+j))),
    global=(n,index)=>n?.op===0x23&&n.global===index;
  const wrappers=[];
  for(let i=0;i+10<ops.length;i++){
    const q=ops.slice(i,i+10),request=q[0]?.local;
    if(!get(q[0],request)||!eq(q[1],4)||!get(q[2],request)||!constant(q[3],0)||!eq(q[4],0x4a)||
      !get(q[5],84)||!eq(q[6],0x72)||!eq(q[7],4)||!global(q[8],27)||!localSet(q[9]))continue;
    const outer=q[1].frame,guard=q[7].frame,packet=q[9].local,body=ops.slice(guard.open+1,guard.end),
      call=helperCalls[0],callIndex=ops.indexOf(call);
    if(!call.frames.includes(guard))continue;
    const store32=(offset)=>body.flatMap((n,j)=>n.op===0x36&&n.memoryOffset===offset&&
      get(body[j-2],packet)&&constant(body[j-1])&&n.frames.at(-1)===guard?[{value:body[j-1].value,offset:n.offset}]:[]),
      fixed=offset=>{const rows=store32(offset);assert.equal(rows.length,1,`one immutable packet field at${offset}`);return rows[0].value;};
    const colors=fixed(68),base=fixed(96),count=fixed(100),siteWords=fixed(104),expressionBase=fixed(108);
    assert(colors>0&&colors<=8192&&base>=siteWords&&count>0&&siteWords>0&&siteWords%7===0&&expressionBase>=179);
    const ownerStores=body.filter((n,j)=>n.op===0x37&&n.memoryOffset===56&&get(body[j-2],packet)&&
      body[j-1]?.op===0x42&&n.frames.at(-1)===guard);
    assert.equal(ownerStores.length,1);const owner=body[body.indexOf(ownerStores[0])-1].value;
    for(const[offset,header,wide]of[[64,76,false],[72,88,true],[80,104,true]])assert(find(guard.open+1,callIndex,q=>
      get(q[0],packet)&&global(q[1],23)&&eq(q[2],wide?0x29:0x28)&&q[2].memoryOffset===header&&
      eq(q[3],wide?0x37:0x36)&&q[3].memoryOffset===offset&&q[3].frames.at(-1)===guard)>=0,
      `packet${offset} copies actual held header field${header}`);
    assert(find(guard.open+1,callIndex,q=>get(q[0],packet)&&global(q[1],26)&&eq(q[2],0x36)&&q[2].memoryOffset===88&&
      q[2].frames.at(-1)===guard)>=0);
    assert(find(guard.open+1,callIndex,q=>get(q[0],packet)&&get(q[1],request)&&eq(q[2],0x36)&&q[2].memoryOffset===92&&
      q[2].frames.at(-1)===guard)>=0);
    for(const[offset,local]of[[112,84],[116,86],[120,109],[124,126],[128,96],[132,152],[136,131],[140,12],[144,13],[148,10]])
      assert(find(guard.open+1,callIndex,q=>get(q[0],packet)&&get(q[1],local)&&eq(q[2],0x36)&&
        q[2].memoryOffset===offset&&q[2].frames.at(-1)===guard)>=0,`actual caller state field${offset}`);
    const args=ops.slice(callIndex-4,callIndex);
    assert(get(args[0],0)&&global(args[1],26)&&get(args[2],request)&&get(args[3],packet),'actual helper consumes ctx/blob/signedsite/descriptor');
    const result=ops.slice(callIndex+1,callIndex+8),status=result[0]?.local;
    assert(eq(result[0],0x22)&&eq(result[1],0x45)&&eq(result[2],13)&&result[2].targets[0]===1&&
      get(result[3],status)&&constant(result[4],2)&&eq(result[5],0x4b)&&eq(result[6],4),'DONE exits and all unknown helper statuses reject');
    const loop=call.frames.findLast(frame=>frame.kind===3);assert(loop);
    const protocolRejectReturns=[],reject=frame=>{const body=ops.slice(frame.open+1,frame.end);
      assert(frame.otherwise===undefined&&body.length===2&&constant(body[0],8)&&eq(body[1],15),
        'protocol rejection returns exact Infrastructure before any architectural output');
      protocolRejectReturns.push(body[1].offset);};
    const idle=find(guard.open+1,loop.open,q=>get(q[0],packet)&&eq(q[1],0x28)&&q[1].memoryOffset===48&&eq(q[2],4));
    assert(idle>=0,'packet must be idle before entering helper');reject(ops[guard.open+1+idle+2].frame);
    reject(result[6].frame);
    const counter=find(loop.open+1,callIndex,q=>get(q[0])&&constant(q[1],1)&&eq(q[2],0x6a)&&eq(q[3],0x22,q[0].local)&&
      constant(q[4],3)&&eq(q[5],0x4b)&&eq(q[6],4));assert(counter>=0,'at most three calls encode exactly two capture rounds');
    reject(ops[loop.open+1+counter+6].frame);
    const capture=find(callIndex+1,loop.end,q=>get(q[0],request)&&get(q[1],request)&&constant(q[2],-1)&&eq(q[3],0x6c)&&
      get(q[4],request)&&constant(q[5],0)&&eq(q[6],0x4a)&&eq(q[7],0x1b)&&constant(q[8],5)&&eq(q[9],0x6a)&&
      eq(q[10],16)&&q[10].call===getter);assert(capture>=0,'capture reads exact absolute-site Header6 closure');
    const captureOps=ops.slice(callIndex+1,loop.end),packetStores=captureOps.filter(n=>n.op===0x37||n.op===0x36);
    assert(packetStores.some(n=>n.op===0x37&&n.memoryOffset===0)&&packetStores.some(n=>n.op===0x36&&n.memoryOffset===8),
      'capture writes raw64 bits and exact signed RefId marker');
    assert(captureOps.some(n=>n.op===0x28&&n.memoryOffset===12)&&captureOps.some(n=>n.op===0x28&&n.memoryOffset===52),
      'capture gates actual cell/request epochs');
    assert(captureOps.some((n,j)=>constant(n,base)&&eq(captureOps[j-1],0x74)&&eq(captureOps[j+1],0x6a)),
      'the capture decoder consumes this owner recipe base');
    const targets=[2,3,4,5,6,7,8,9,11,20,25,21,22,23,24,87,88,90,91,92,93,94,95,100,101,102,103,
      76,77,78,79,80,81,82,83,110,111,112,113,114,115,116,117,118,119,120,121,122,123,124,125,85,109,158,177,10,13,12,131];
    const outputs=[];
    targets.forEach((local,role)=>{
      const start=find(loop.end+1,guard.end,q=>get(q[0],packet)&&eq(q[1],0x29)&&q[1].memoryOffset===152&&
        eq(q[2],0x42)&&q[2].value===(1n<<BigInt(role))&&eq(q[3],0x83)&&eq(q[4],0x50)&&eq(q[5],0x45)&&eq(q[6],4)&&
        get(q[7],packet)&&eq(q[8],0x29)&&q[8].memoryOffset===160+8*role);
      assert(start>=0,`fixed output role${role} is controlled by its actual written-mask bit`);
      const at=loop.end+1+start,frame=ops[at+6].frame,body=ops.slice(frame.open+1,frame.end),
        type=local>=76&&local<84||local>=101&&local<104?2:local>=110&&local<118||local===177?1:0,
        wanted=type===0?[0x20,0x29,0xa7,0x21]:type===1?[0x20,0x29,0x21]:[0x20,0x29,0xbf,0x21];
      assert.deepEqual(body.map(n=>n.op),wanted);assert.equal(body.at(-1).local,local);
      assert(frame.open>loop.end&&ops[frame.open].frames.at(-1)===guard,'all selected sources finish before any finite local output');
      outputs.push({role,local,type,offset:ops[frame.open].offset,setOffset:body.at(-1).offset});
    });
    assert(find(loop.end+1,guard.end,q=>get(q[0],packet)&&constant(q[1],0)&&eq(q[2],0x36)&&q[2].memoryOffset===48&&
      get(q[3],packet)&&eq(q[4],0x42)&&q[4].value===0n&&eq(q[5],0x37)&&q[5].memoryOffset===152)>=0,
      'output adapter releases state/mask before any existing service continuation');
    const sites=[...new Set(ops.flatMap((node,j)=>set(node,request)&&constant(ops[j-1])&&ops[j-1].value!==0?
      [Math.abs(ops[j-1].value)-1]:[]))].sort((a,b)=>a-b);
    assert.equal(siteWords,7*sites.length);sites.forEach((site,j)=>assert.equal(site,7*j));
    const {metadata,words}=wordsOf(source),table=recipeTable(words,base,sites);
    assert.equal(owner,metadata.identity);assert.equal(table.count,count);
    assert(table.records.every(record=>record.color<colors),'every serialized reference fits the bounded typed color transport');
    wrappers.push({requestLocal:request,packetLocal:packet,scratchGlobal:27,baseGlobal:26,helper:imported,
      callOffset:call.offset,guardOffset:ops[guard.open].offset,endOffset:ops[guard.end].offset,
      recipeBase:base,recipeCount:count,colors,siteWords,sites:sites.length,recipeKinds:table.kinds,
      maxClosure:table.maxClosure,outputs,protocolRejectReturns});
  }
  assert.equal(wrappers.length,1,'one positively authenticated shared capture/output adapter');
  return wrappers[0];
}

// Authenticate the actual source-backed VM packet. Its loops walk Header6's
// selected closure; unrelated color selectors or arbitrary loops cannot pass.
function compactFormat(source) {
  const module=readGeneratedModule(source),named=name=>[...module.names].find(([,value])=>value===name)?.[0],
    index=named('guest.run'),getter=named('guest.recovery_data');
  assert(Number.isInteger(index)&&index>=module.imports.length);
  assert(Number.isInteger(getter)&&getter>=module.imports.length);
  const run=module.functions[index-module.imports.length],{metadata,words}=wordsOf(source),candidates=[],shared=inspectSharedRecovery(source);
  for(let i=0;!shared&&i+7<run.ops.length;i++){
    const p=run.ops.slice(i,i+8);
    if(p[0].op!==0x22||!constant(p[1],1)||p[2].op!==0x6b||!constant(p[3],2)||p[4].op!==0x74||
      !constant(p[5])||p[5].value<0||p[6].op!==0x6a||!localSet(p[7]))continue;
    const loop=p[0].frames.findLast(frame=>frame.kind===3);if(!loop)continue;
    const body=run.ops.slice(loop.open+1,loop.end),base=p[5].value,record=p[7].local,l=record-17;
    if(p[0].local!==l+22)continue;
    const eq=(n,op,local)=>n?.op===op&&(local===undefined||n.local===local),
      get=(n,local)=>eq(n,0x20,local),set=(n,local)=>eq(n,0x21,local),
      find=(ops,predicate)=>ops.findIndex((_,j)=>predicate(ops.slice(j)));
    // Header6 -> packed closure base/count -> reverse cursor/count and mode0.
    const pre=run.ops.slice(Math.max(0,loop.open-40),loop.open);
    const setup=find(pre,q=>get(q[0],l+1)&&constant(q[1],6)&&eq(q[2],0x6a)&&eq(q[3],0x10)&&q[3].call===getter&&
      eq(q[4],0x22)&&eq(q[5],0xa7)&&set(q[6],l+27)&&get(q[7],q[4].local)&&eq(q[8],0x42)&&q[8].value===32n&&
      eq(q[9],0x88)&&eq(q[10],0xa7)&&eq(q[11],0x22,l+24)&&set(q[12],l+16)&&constant(q[13],0)&&set(q[14],l+18));
    assert(setup>=0,'exact descriptor closure setup before the shared loop');
    assert(find(body,q=>get(q[0],l+18)&&eq(q[1],0x45)&&get(q[2],l+16)&&eq(q[3],0x45)&&eq(q[4],0x71)&&eq(q[5],4))>=0,
      'mode changes from reverse to forward only at cursor0');
    assert(find(body,q=>get(q[0],l+18)&&get(q[1],l+16)&&get(q[2],l+24)&&eq(q[3],0x4f)&&eq(q[4],0x71)&&eq(q[5],13))>=0,
      'forward traversal exits at exact descriptor closure count');
    assert(find(body,q=>get(q[0],l+27)&&get(q[1],l+16)&&constant(q[2],1)&&eq(q[3],0x76)&&eq(q[4],0x6a)&&
      eq(q[5],0x10)&&q[5].call===getter&&get(q[6],l+16)&&constant(q[7],1)&&eq(q[8],0x71)&&constant(q[9],5)&&eq(q[10],0x74))>=0,
      'cursor reads exact packed two-marker closure words');
    const header=find(body,q=>get(q[0],record)&&constant(q[1],0)&&eq(q[2],0x6a)&&eq(q[3],0x10)&&q[3].call===getter&&
      eq(q[4],0x22)&&eq(q[5],0xa7)&&constant(q[6],255)&&eq(q[7],0x71)&&set(q[8],l+19));
    assert(header>=0,'record kind is decoded from actual word0');
    const needed=find(body,q=>get(q[0],l+19)&&eq(q[1],0x45)&&eq(q[2],0x45)&&get(q[3],l+13)&&get(q[4],l+22)&&
      eq(q[5],0x46)&&eq(q[6],0x71)&&eq(q[7],4));
    assert(needed>=0,'only non-Available records with exact positive NeedTag evaluate');
    const neededFrame=body[needed+7].frame;
    const dep=find(body,q=>get(q[0],record)&&constant(q[1],2)&&eq(q[2],0x6a)&&get(q[3],l+21)&&constant(q[4],1)&&
      eq(q[5],0x76)&&eq(q[6],0x6a)&&eq(q[7],0x10)&&q[7].call===getter);
    assert(dep>=0&&body[dep].frames.includes(neededFrame),'requested record reads its four packed dependency markers');
    assert(find(body,q=>get(q[0],l+21)&&constant(q[1],5)&&eq(q[2],0x4f)&&eq(q[3],13))>=0,
      'one finite dependency loop has exactly four inputs plus completion');
    const cases=[];
    for(let j=0;j+3<body.length;j++)if(get(body[j],l+19)&&constant(body[j+1])&&eq(body[j+2],0x46)&&eq(body[j+3],4)){
      assert(body[j].frames.includes(neededFrame));cases.push(body[j+1].value);
    }
    const sites=[...new Set(run.ops.flatMap((n,j)=>set(n,l)&&constant(run.ops[j-1])&&run.ops[j-1].value!==0?
      [Math.abs(run.ops[j-1].value)-1]:[]))].sort((a,b)=>a-b);
    assert(sites.length>0);sites.forEach((site,j)=>assert.equal(site,7*j,'all seven-word descriptor selectors accounted'));
    const table=recipeTable(words,base,sites),kinds=table.kinds.filter(kind=>kind!==0);
    assert.deepEqual(cases.sort((a,b)=>a-b),kinds,'each used finite source kind emitted once, never once per RefId');
    candidates.push({base,recordLocal:record,loopOffset:run.ops[loop.open].offset,loopEnd:run.ops[loop.end].offset,
      kindCases:cases,recipeCount:table.count,sites:table.sites,maxClosure:table.maxClosure,
      closures:table.closures,recipeKinds:table.kinds});
  }
  const legacy=candidates.length||shared?null:legacyFormat(run,getter,words);
  return{encodedBytes:source.length,bodyBytes:run.raw.length,operators:run.ops.length,legacy,shared,
    metadata:{version:metadata.version,mode:metadata.mode,owner:String(metadata.identity),words:metadata.words,
      bytes:metadata.byteLength},vmCandidates:candidates,words};
}

function retain(t,body,checks,kind) {
  const source=t.bytes(t.api.core_code_ptr()>>>0,t.api.core_code_len()>>>0),
    owner=new DataView(t.engine.memory.buffer).getBigUint64(t.context()+120,true),revision=t.version();
  const result=body({source,owner,revision});
  assert.equal(new DataView(t.engine.memory.buffer).getBigUint64(t.context()+120,true),owner);
  assert.deepEqual(t.bytes(t.api.core_code_ptr()>>>0,t.api.core_code_len()>>>0),source,'same immutable actual owner after execution');
  assert.equal(t.result.modules.length,1);
  t.result.semanticComplete=true;t.result.retained={id:String(owner),sha256:sha(source),bytes:source.length};
  let proof;
  try{proof=compactFormat(source);t.result.compactRecovery={...proof,words:undefined};}
  catch(error){t.result.compactRecovery={error:String(error?.message??error)};}
  checks.push(()=>{
    assert(proof,'current module and immutable recovery metadata decode completely');
    assert.equal(proof.metadata.owner,String(owner));assert.equal(proof.metadata.version,2);
    assert.equal(proof.vmCandidates.length+Number(!!proof.legacy)+Number(!!proof.shared),1,'one positively authenticated selected recovery representation');
    if(kind==='costly')assert.equal(proof.vmCandidates.length+Number(!!proof.shared),1,
      'the costly CFG source class uses the smaller finite SourceOp VM, not repeated per-reference code');
    if(kind==='fp128')assert(source.length<115789,'the unchanged accepted128-add format cap remains115789 bytes');
    if(result)result();
  });
}

export function runRecoveryCompactCases(fixture) {
  const checks=[];
  // Growing distinct numerical recovery states exercise one compact recipe
  // representation. All untouched raw80 ghosts stay architecturally exact.
  for(const count of[16,128])for(const single of[true,false])fixture(
    `recovery-compact-${count}-fp-states-pc${single?24:53}`,{
      segments:[[ROOT,`d903 ${'d84304 '.repeat(count)}dd5b08 ffe6`]],registers,
      initialFlags:FLAGS,initialFp,control:single?0x7f:0x27f,stateWitness:true,data:fpData,
    },t=>retain(t,({revision})=>{
      const expected=fpResult(count,single),total=count+3,storePc=ROOT+2+3*count;
      t.writeExpected(0x3008,expected.stored);
      const run=literal(t,{pc:TARGET,retired:total,budget:total,
        fp:t.fp({status:single?0x20:0,tag:0xffff,opcode:0x55b,fip:storePc,fdp:0x3008,registers:{7:expected.raw}})});
      assert.equal(t.version(),revision+1n,'one final store, no recovery effect replay');
      const witness=t.stateWitness();t.result.valueWitness=witness;
      return()=>{assert.equal(run.activations.length,1);assert.equal(run.helperCalls,0);assert.equal(run.preciseCalls,0);
        assert.equal(witness.fp_add_ops,count,'all original arithmetic rounding points still execute');
        assert.equal(witness.fp_single_rounds,single?count:0);};
    },checks,count===128?'fp128':'fp16'));

  for(const single of[true,false])fixture(`recovery-compact-late-fp-store-fault-pc${single?24:53}`,{
    segments:[[ROOT,`d903 ${'d84304 '.repeat(128)}dd1f ffe6`]],registers:[...registers.slice(0,7),0x5000],
    initialFlags:FLAGS,initialFp,control:single?0x7f:0x27f,stateWitness:true,data:fpData,missing:[0x5000],
  },t=>retain(t,({revision})=>{
    const expected=fpResult(128,single),run=literal(t,{pc:0x1182,retired:129,budget:131,work:129,remaining:2,stop:3,
      fault:[0x5000,0x10208,0x80e],fp:t.fp({status:single?0x3820:0x3800,tag:0x3fff,
        opcode:0x43,fip:0x117f,fdp:0x3004,registers:{7:expected.raw}})});
    assert.equal(t.version(),revision,'failed store preserves the occupied stack cell and every RAM/page stamp');
    const witness=t.stateWitness();t.result.valueWitness=witness;
    return()=>{assert.equal(run.helperCalls,0);assert.equal(run.preciseCalls,1);assert.equal(witness.fp_add_ops,128);};
  },checks,'fp-fault'));

  const count=32,total=2*count+3,
    scalarCode='83f801 ba00000000 '+Array.from({length:count},(_,i)=>`8d4001 8b8b${dword(i*4)}`).join(' ')+' ffe6';
  // FLAGS still owns the original EAX while every LEA changes EAX. Recovery
  // must stage that lazy old-input recipe before publishing the changed GPRs.
  for(const fault of[false,true])fixture(`recovery-compact-scalar-old-flags-${fault?'late-read-fault':'ordered-reads'}`,{
    segments:[[ROOT,scalarCode]],registers:fault?[...registers.slice(0,3),0x3f84,...registers.slice(4)]:registers,
    initialFlags:FLAGS,initialFp:opaqueFp,data:scalarData,compileInstructions:512,
    ...(fault?{missing:[0x4000]}:{}),
  },t=>retain(t,({revision})=>{
    const completed=fault?65:total,run=literal(t,{pc:fault?0x1122:TARGET,retired:completed,budget:total,
      work:completed,remaining:fault?2:0,...(fault?{stop:3,fault:[0x4000,0x10104,14]}:{}),
      regs:{0:0x7fffffe0,1:fault?0x89abd0ff:0x89abcd1f,2:0},flags:0x412,fp:t.startingFp});
    assert.equal(t.version(),revision,'all ordered reads leave source/RAM/page stamps unchanged');
    return()=>{assert.equal(run.helperCalls,0);assert.equal(run.preciseCalls,0);};
  },checks,'scalar'));

  // Independently authored CFG diamonds retain both source paths. Nonzero EAX
  // executes TEST/JZ/FADD/JMP for each diamond; the unexecuted FSUB path still
  // creates real joined FP sources in the compiled CFG. No game bytes are used.
  for(const count of[16,32])for(const single of[true,false])fixture(
    `recovery-compact-${count}-cfg-diamonds-pc${single?24:53}`,{
      segments:[[ROOT,`d903 ${'85c0 7405 d84304 eb03 d86304 '.repeat(count)}dd5b08 ffe6`]],registers,
      initialFlags:FLAGS,initialFp,control:single?0x7f:0x27f,stateWitness:true,data:fpData,
      compileInstructions:2048,compileBlocks:512,
    },t=>retain(t,({revision})=>{
      const expected=fpResult(count,single),total=4*count+3,storePc=ROOT+2+12*count;
      t.writeExpected(0x3008,expected.stored);
      const run=literal(t,{pc:TARGET,retired:total,budget:total,flags:0x406,
        fp:t.fp({status:single?0x20:0,tag:0xffff,opcode:0x55b,fip:storePc,fdp:0x3008,registers:{7:expected.raw}})});
      assert.equal(t.version(),revision+1n,'only the final store writes RAM or page stamps');
      const witness=t.stateWitness();t.result.valueWitness=witness;
      return()=>{assert.equal(run.activations.length,1);assert.equal(run.helperCalls,0);assert.equal(run.preciseCalls,0);
        assert.equal(witness.fp_add_ops,count,'only the authored nonzero branch executes each distinct arithmetic point');
        assert.equal(witness.fp_single_rounds,single?count:0);};
    },checks,'costly'));

  // The OLD binary reaches every architectural oracle before rejecting the
  // representation. No expected guest state is supplied by a decoded recipe.
  for(const check of checks)check();
}
