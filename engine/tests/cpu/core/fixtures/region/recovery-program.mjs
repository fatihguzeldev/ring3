import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readGeneratedModule,inspectSharedCallbackSpine} from './structure.mjs';
import {inspectCompletedHandoffs} from './completed-structure.mjs';
import {inspectSharedRecovery} from './recovery-compact.mjs';

const ROOT=0x1000,TARGET=0x1800,FLAGS=0x403;
const registers=[0x13579bdf,2,0x2468ace0,0x3000,0x4800,0x89abcdef,TARGET,0x76543210];
const initialFp={status:0,tag:0xffff,opcode:0x5a5,fip:0x11223344,fdp:0x55667788,cs:0x33,ds:0x5b,
  registers:{0:'01000000000000c00040',1:'00000000000000000080',2:'0000000000000040ff3f',
    3:'00000000000000c0ff7f',4:'7856341200000080ff3f',5:'ffffffffffffffffffff',
    6:'efcdab8967452301807f',7:'ffffffffffffffffffff'}};
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
const data=page=>page.set(Buffer.from('0000c03f000080338877665544332211aabbccddeeff00aa','hex'));

// Independent dyadic encoding. PC53 sums N exact half-binary32-ULPs; PC24
// rounds each separate tie back to the even value1.5. No emitted value recipe
// or runtime numerical helper contributes to these expected bytes.
function result(count,single) {
  const increments=single?0n:BigInt(count),raw=Buffer.alloc(10),stored=Buffer.alloc(8);
  raw.writeBigUInt64LE(0xc000000000000000n+(increments<<39n));raw.writeUInt16LE(0x3fff,8);
  stored.writeBigUInt64LE(0x3ff8000000000000n+(increments<<28n));
  return{raw:raw.toString('hex'),stored:stored.toString('hex')};
}

function format(bytes) {
  const module=readGeneratedModule(bytes),runIndex=[...module.names].find(([,name])=>name==='guest.run')?.[0];
  assert(Number.isInteger(runIndex)&&runIndex>=module.imports.length,'actual guest.run is defined');
  const run=module.functions[runIndex-module.imports.length];
  let sharedAdapter,sharedError;
  try{sharedAdapter=inspectSharedRecovery(bytes);}catch(error){sharedError=String(error?.message??error);}
  const dataFunctions=[...module.names].filter(([,name])=>name==='guest.recovery_data'||/^guest\.recovery_data\.\d+$/.test(name));
  const main=dataFunctions.filter(([,name])=>name==='guest.recovery_data');
  const dataIndices=new Set(dataFunctions.map(([index])=>index));
  const bases=module.globalImports.flatMap((item,index)=>item.module==='ring3'&&item.name==='recovery.base'?[{...item,index}]:[]);
  const sections=WebAssembly.Module.customSections(new WebAssembly.Module(bytes),'ring3.recovery');
  const blob=sections.length===1&&sections[0].byteLength>=40?new Uint8Array(sections[0]):undefined;
  let metadata;
  if(blob){
    const d=new DataView(blob.buffer,blob.byteOffset,blob.byteLength);
    metadata={bytes:blob.byteLength,magic:Buffer.from(blob.subarray(0,4)).toString(),version:d.getUint32(4,true),
      wordCount:d.getUint32(8,true),bits:d.getUint32(12,true),packedCount:d.getUint32(16,true),
      dictionaryCount:d.getUint32(20,true),packedOffset:d.getUint32(24,true),dictionaryOffset:d.getUint32(28,true),
      owner:String(d.getBigUint64(32,true)),
      mode:d.getUint32(4,true)===2&&blob.byteLength>=48?d.getUint32(40,true):undefined,
      highCount:d.getUint32(4,true)===2&&blob.byteLength>=48?d.getUint32(44,true):undefined};
  }
  const at=(index,op,local)=>run.ops[index]?.op===op&&(local===undefined||run.ops[index].local===local);
  const zero=(index,local)=>at(index,0x41)&&run.ops[index].value===0&&at(index+1,0x21,local);
  const tokenZero=index=>at(index,0x42)&&run.ops[index].value===0n&&at(index+1,0x21,177);
  const callbackSpine=inspectSharedCallbackSpine(bytes);
  const completedHandoffs=inspectCompletedHandoffs(module);
  const completedResets=new Set(completedHandoffs.adapters.map(adapter=>adapter.validResetOffset));
  const projectionStarts=[],resetOrigins={blockEntry:[],inlineCommit:[],environment:[],sharedService:[],shared:[],completedHandoff:[],unclassified:[]};
  for(let i=0;i<run.ops.length;i++)if(at(i,0x21,85)&&at(i-1,0x41)&&run.ops[i-1].value===0){
    const offset=run.ops[i].offset;projectionStarts.push(offset);
    const viewReset=zero(i-3,84)&&zero(i+1,86)&&zero(i+3,153)&&zero(i+5,154)&&zero(i+7,178);
    if(viewReset&&zero(i+9,126)&&tokenZero(i+11)&&at(i+13,0x41)&&run.ops[i+13].value===-1&&at(i+14,0x21,109))
      resetOrigins.blockEntry.push(offset);
    else if(viewReset&&zero(i+9,109)&&tokenZero(i+11)&&at(i-6,0x20,152)&&at(i-5,0x20,103)&&
      at(i-4,0x39)&&run.ops[i-4].memoryOffset===64)
      resetOrigins.inlineCommit.push(offset);
    else if(viewReset&&zero(i+9,100)&&tokenZero(i+11)&&callbackSpine.shared&&
      callbackSpine.viewResetOffset===offset)resetOrigins.sharedService.push(offset);
    else if(zero(i+1,86)&&zero(i+3,153)&&zero(i+5,154)&&tokenZero(i+7)&&zero(i+9,178)&&
      at(i+11,0x41)&&run.ops[i+11].value===1&&at(i+12,0x21,84))resetOrigins.environment.push(offset);
    else if(at(i-3,16)&&dataIndices.has(run.ops[i-3].call)&&at(i-2,0x21,177)&&
      at(i+1,0x20)&&at(i+2,0x21,158))resetOrigins.shared.push(offset);
    else if(completedResets.has(offset))resetOrigins.completedHandoff.push(offset);
    else resetOrigins.unclassified.push(offset);
  }
  const getterCalls=run.ops.filter(node=>node.op===16&&dataIndices.has(node.call)).map(node=>node.offset);
  const functions=dataFunctions.map(([index,name])=>{
    const type=module.types[module.functionTypes[index-module.imports.length]],fn=module.functions[index-module.imports.length];
    const ops=fn?.ops??[],base=bases[0]?.index,guard=ops[3]?.frame;
    const memoryRead=node=>[0x28,0x29,0x2d,0x2f].includes(node.op);
    const forbidden=ops.filter(node=>node.op===3||node.op===14||node.op>=16&&node.op<=21||node.op===0x24||
      node.op>=0x25&&node.op<=0x40&&!memoryRead(node)||[0xfc,0xfd].includes(node.op)||
      node.op===0x23&&node.global!==base);
    const loads=ops.filter(memoryRead),globals=ops.filter(node=>node.op===0x23);
    const prefix=ops.slice(0,4).map(node=>node.op),suffix=ops.slice(-4).map(node=>node.op);
    const crossing=loads[1]?.frames.find(frame=>{
      const p=ops.slice(frame.open-5,frame.open);
      return frame!==guard&&frame.kind===4&&frame.type===-2&&p.length===5&&
        p[0].op===0x20&&p[0].local===2&&p[1].op===0x41&&p[1].value===metadata?.bits&&
        p[2].op===0x6a&&p[3].op===0x41&&p[3].value===64&&p[4].op===0x4b;
    });
    const bounded=JSON.stringify(prefix)===JSON.stringify([0x20,0x41,0x49,4])&&ops[0].local===0&&
      ops[1].value===metadata?.wordCount&&guard?.type===-2&&
      loads.every(node=>node.frames.includes(guard))&&crossing!==undefined&&
      loads.filter((_,index)=>index!==1).every(node=>!node.frames.includes(crossing))&&
      JSON.stringify(suffix)===JSON.stringify([5,0x42,11,11])&&ops.at(-3)?.value===0n;
    return{index,name,type,bytes:fn?.raw.length,forbidden:forbidden.map(node=>({op:node.op,offset:node.offset})),bounded,
      loads:loads.map(node=>({op:node.op,align:node.align,offset:node.memoryOffset})),baseReads:globals.map(node=>node.global),
      exported:[...module.exports.values()].includes(index)};
  });
  return{bytes:bytes.length,bodyBytes:run.raw.length,operators:run.ops.length,
    mainGetters:main.length,projectionStarts,resetOrigins,getterCalls,functions,callbackSpine,completedHandoffs,sharedAdapter,sharedError,
    metadataSections:sections.length,metadata,bases};
}

function contextLiteral(t,wanted) {
  const bytes=t.bytes(t.context(),128),view=new DataView(bytes.buffer),fault=wanted.fault??[0,0,0];
  for(const[offset,value]of[[32,wanted.pc],[36,FLAGS],[40,wanted.retired],[44,wanted.remaining??0],
    [48,fault[0]],[52,fault[1]],[112,0],[116,fault[2]]])view.setUint32(offset,value>>>0,true);
  const run=t.check({...wanted,flags:FLAGS});
  assert.deepEqual(t.bytes(t.context(),128),bytes,'literal complete Context128, including owner/source authority');
  return run;
}

export function runRecoveryProgramCases(fixture) {
  const checks=[],proofs=[];
  for(const count of[1,8,32,128])for(const single of[true,false]) {
    const storePc=ROOT+2+3*count,total=count+3,expected=result(count,single);
    fixture(`recovery-program-${count}-distinct-fp-states-pc${single?24:53}`,{
      segments:[[ROOT,`d903 ${'d84304 '.repeat(count)}dd5b08 ffe6`]],registers,
      initialFlags:FLAGS,initialFp,control:single?0x7f:0x27f,stateWitness:true,data,
    },t=>{
      const encoded=t.bytes(t.api.core_code_ptr()>>>0,t.api.core_code_len()>>>0),revision=t.version();
      const identity=new DataView(t.engine.memory.buffer).getBigUint64(t.context()+120,true);
      t.writeExpected(0x3008,expected.stored);
      const run=contextLiteral(t,{pc:TARGET,retired:total,budget:total,
        fp:t.fp({status:single?0x20:0,tag:0xffff,opcode:0x55b,fip:storePc,fdp:0x3008,
          registers:{7:expected.raw}})});
      assert.equal(t.version(),revision+1n,'one final8-byte write, never an effect replay');
      assert.deepEqual(t.bytes(t.api.core_code_ptr()>>>0,t.api.core_code_len()>>>0),encoded);
      assert.equal(new DataView(t.engine.memory.buffer).getBigUint64(t.context()+120,true),identity);
      const witness=t.stateWitness(),proof=format(encoded);
      t.result.semanticComplete=true;
      t.result.recoveryProgram={count,precision:single?24:53,owner:String(identity),sha256:hash(encoded),...proof};
      t.result.valueWitness=witness;proofs.push({count,single,proof});
      checks.push(()=>{
        assert.equal(run.activations.length,1);assert.equal(run.helperCalls,0);assert.equal(run.preciseCalls,0);
        assert.equal(t.result.modules.length,1,'one retained owner supplies the full numerical result');
        assert.equal(witness.fp_add_ops,count,'every original rounded arithmetic operation executes');
        assert.equal(witness.fp_single_rounds,single?count:0);
        assert.equal(proof.mainGetters,1,'one actual immutable recovery descriptor getter');
        assert(proof.getterCalls.length>0,'the actual guest body consumes the descriptor function');
        assert.equal(proof.bases.length,1,'one immutable owner-bound recovery base replaces the code dispatcher');
        assert.deepEqual(proof.bases[0],{module:'ring3',name:'recovery.base',valueType:0x7f,mutable:0,index:26});
        assert.equal(proof.metadataSections,1,'the charged encoded owner contains exactly one portable metadata blob');
        const metadata=proof.metadata;assert(metadata);
        assert.equal(metadata.magic,'R3RD');assert.equal(metadata.version,2);assert.equal(metadata.owner,String(identity));
        assert(metadata.wordCount>0&&metadata.dictionaryCount>0&&metadata.dictionaryCount<=metadata.wordCount);
        assert.equal(metadata.bits,Math.max(1,Math.ceil(Math.log2(metadata.dictionaryCount))));
        assert.equal(metadata.packedCount,Math.ceil(metadata.wordCount*metadata.bits/64));
        assert.equal(metadata.packedOffset,48);assert.equal(metadata.dictionaryOffset,48+8*metadata.packedCount);
        assert([0,1,2].includes(metadata.mode),'one compile-time dictionary representation is selected');
        const entryBytes=metadata.mode===0?8:4+metadata.mode;
        const highOffset=metadata.dictionaryOffset+entryBytes*metadata.dictionaryCount;
        assert.equal(metadata.bytes,highOffset+4*metadata.highCount);
        if(metadata.mode===0)assert.equal(metadata.highCount,0,'raw dictionaries have no high-word table');
        else {
          assert(metadata.highCount>0&&metadata.highCount<=(metadata.mode===1?256:65536));
          if(metadata.mode===2)assert(metadata.highCount>256,'u16 indices are needed only beyond the u8 domain');
          assert(entryBytes*metadata.dictionaryCount+4*metadata.highCount<8*metadata.dictionaryCount,
            'split dictionaries are used only when they reduce immutable storage');
        }
        assert.equal(proof.functions.length,1,'metadata access uses one bounded getter, without chunk functions');
        for(const fn of proof.functions){
          assert.deepEqual(fn.type,{parameters:[0x7f],results:[0x7e]},`${fn.name}: private i32→i64 metadata read`);
          assert.equal(fn.forbidden.length,0,`${fn.name}: descriptors have no loops, dispatch tables, stores or callbacks`);
          assert(fn.bounded,`${fn.name}: the unsigned word bound dominates every load and OOB returns zero`);
          const packed=[{op:0x29,align:0,offset:48},{op:0x29,align:0,offset:48}];
          const dictionary=metadata.mode===0?[{op:0x29,align:0,offset:metadata.dictionaryOffset}]:
            [{op:0x28,align:0,offset:metadata.dictionaryOffset},
              {op:metadata.mode===1?0x2d:0x2f,align:0,offset:metadata.dictionaryOffset+4},
              {op:0x28,align:0,offset:highOffset}];
          assert.deepEqual(fn.loads,[...packed,...dictionary],
            'the bounded getter has exactly three raw or five split load sites, including the conditional packed crossing');
          assert.deepEqual(fn.baseReads,Array(metadata.mode===0?3:5).fill(26));
          assert.equal(fn.exported,false,'metadata is private to the guarded body');
        }
        assert.equal(proof.sharedError,undefined,'shared recovery representation is positively authenticated');
        if(proof.sharedAdapter){
          assert.equal(proof.resetOrigins.shared.length,0,'the shared form removes the old owner-local projection executor');
          assert.equal(proof.sharedAdapter.outputs.filter(output=>output.local===85).length,1,
            'one actual written-mask role applies the shared VALID projection after both selected capture phases');
        }else assert.equal(proof.resetOrigins.shared.length,1,'one actual descriptor-driven owner-local FP projection executor');
        assert.equal(proof.resetOrigins.blockEntry.length,1,'one unchanged block-entry view reset');
        assert.equal(proof.resetOrigins.inlineCommit.length,1,
          'the one selective pool commit clears its view after writing the pending-result header');
        assert.equal(proof.resetOrigins.environment.length,0,
          'typed block ingress does not add a separate legacy environment-import reset');
        assert.deepEqual(proof.callbackSpine.errors,[],
          'the actual shared callback packet proves full transfer, FP materialization policy, exclusive callbacks, authority reload and quota-qualified reentry');
        assert.equal(proof.resetOrigins.sharedService.length,1,
          'one positively verified SERVICE>=2 packet drops the local view for all emitted callback kinds');
        assert.deepEqual(proof.completedHandoffs.errors,[],
          'completed publication has the actual pool guard, sparse typed stores, pending header and neutral legacy tail');
        assert.equal(proof.completedHandoffs.adapters.length,1,
          'the authored complete block has one direct typed handoff');
        assert.equal(proof.resetOrigins.completedHandoff.length,1,
          'the positively verified direct handoff releases one local FP view');
        assert.equal(proof.resetOrigins.unclassified.length,0,'every VALID clear has an exact emitted purpose');
        if(count===128)assert(encoded.length<115789,
          `the accepted128-add format must be smaller than the retained Step24 body: ${encoded.length}`);
      });
    });
  }
  // Preserve a genuine late ordered fault with an occupied native value. The
  // failed FSTP never pops or overwrites the last arithmetic metadata/payload.
  for(const single of[true,false])fixture(`recovery-program-late-store-fault-pc${single?24:53}`,{
    segments:[[ROOT,`d903 ${'d84304 '.repeat(128)}dd1f ffe6`]],
    registers:[...registers.slice(0,7),0x5000],initialFlags:FLAGS,initialFp,
    control:single?0x7f:0x27f,stateWitness:true,data,missing:[0x5000],
  },t=>{
    const revision=t.version(),expected=result(128,single);
    const run=contextLiteral(t,{pc:0x1182,retired:129,budget:131,work:129,remaining:2,stop:3,
      fault:[0x5000,0x10208,0x80e],fp:t.fp({status:single?0x3820:0x3800,tag:0x3fff,
        opcode:0x43,fip:0x117f,fdp:0x3004,registers:{7:expected.raw}})});
    assert.equal(t.version(),revision,'faulting store performs no payload/version write');
    t.result.semanticComplete=true;
    checks.push(()=>{assert.equal(run.helperCalls,0);assert.equal(run.preciseCalls,1);});
  });
  // Historical binaries must finish every literal state oracle before the
  // format-only rejection; all N and both precision modes stay in the receipt.
  for(const check of checks)check();
  for(const single of[true,false]){
    const rows=proofs.filter(row=>row.single===single);
    const purposes=proof=>Object.fromEntries(Object.entries(proof.resetOrigins).map(([key,value])=>
      [key,value.length+(key==='shared'?Number(!!proof.sharedAdapter):0)]));
    assert(rows.every(row=>JSON.stringify(purposes(row.proof))===JSON.stringify(purposes(rows[0].proof))),
      'increasing distinct recovery prefixes does not duplicate the projection executor');
  }
}
