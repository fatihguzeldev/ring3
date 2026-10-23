import assert from 'node:assert/strict';

export const witnessCounters=['normalizer_calls','materializer_calls','raw80_stores','fp_metadata_writes',
  'body_entries','control_admissions','group_acquisitions','page_translations',
  'record_reads','record_writes','record_read_mask','record_write_mask','header_reads','header_writes',
  'wide_snapshot_calls','data_reads','data_writes','flags_calls','flag_kind_queries','flag_width_queries',
  'fp_environment_queries','fp_input_load_queries','fp_input_admission_queries','fp_control_queries',
  'fp_occupancy_queries','status_resolver_calls','fp_result_admissions','fp_pending_recipe_writes','fp_multiply_ops',
  'entry_filter_queries','entry_index_compares','entry_table_selections','entry_pending_queries','prefix_quota_queries',
  'canonical_calls','canonical_reads','canonical_writes','fp_generic_f32_packs','fp_single_precision_queries',
  'fp_value_precision_admissions','fp_operand_bounds',
  'executable_context_reads','private_context_reads','executable_authority_reads','private_authority_reads'];
const u=value=>{const out=[];do{const b=value&127;value>>>=7;out.push(b|(value?128:0));}while(value);return Uint8Array.from(out);};
const concat=parts=>Uint8Array.from(Buffer.concat(parts.map(value=>Buffer.from(value))));
const section=(id,data)=>concat([Uint8Array.of(id),u(data.length),data]);
class Reader {
  constructor(bytes){this.bytes=bytes;this.at=0;}
  byte(){assert(this.at<this.bytes.length);return this.bytes[this.at++];}
  unsigned(){let value=0;for(let s=0;s<35;s+=7){const b=this.byte();value+=(b&127)*2**s;if(b<128)return value;}assert.fail('bounded u32');}
  take(n){assert(this.at+n<=this.bytes.length);const value=this.bytes.subarray(this.at,this.at+n);this.at+=n;return value;}
}

export function readWitness(exports) {
  exports=exports.exports??exports;
  return Object.fromEntries(witnessCounters.map(name=>{
    assert(exports[`test.${name}`] instanceof WebAssembly.Global,`actual Wasm counter ${name}`);
    return[name,exports[`test.${name}`].value];
  }));
}

export function setWitnessBank(exports,pointer) {
  exports=exports.exports??exports;
  assert(Number.isInteger(pointer)&&pointer>=0&&pointer%8===0,
    'the witness uses the actual aligned private NumericBank, or zero for an integer-only fixture');
  assert(exports['test.bank_base'] instanceof WebAssembly.Global);
  exports['test.bank_base'].value=pointer;
}

export function resetWitnessMasks(exports) {
  exports=exports.exports??exports;
  for(const name of['record_read_mask','record_write_mask'])exports[`test.${name}`].value=0;
}

export function setWitnessData(exports,{pointer=0,length=0}={}) {
  exports=exports.exports??exports;
  assert(Number.isInteger(pointer)&&pointer>=0&&Number.isInteger(length)&&length>=0&&pointer+length<=0x100000000,
    'the payload witness receives an exact physical address range');
  for(const[name,value]of[['data_base',pointer],['data_length',length]]){
    assert(exports[`test.${name}`] instanceof WebAssembly.Global);
    exports[`test.${name}`].value=value;
  }
}

export function setWitnessAuthority(exports,{context=0,executable=0,executableLength=0,privateSlots=0,privateLength=0}={}) {
  exports=exports.exports??exports;
  for(const [pointer,length]of[[context,128],[executable,executableLength],[privateSlots,privateLength]])
    assert(Number.isInteger(pointer)&&pointer>=0&&Number.isInteger(length)&&length>=0&&pointer+length<=0x100000000,
      'authority witness uses actual bounded Context and table address ranges');
  for(const[name,value]of[['context_base',context],['executable_base',executable],
    ['executable_length',executable?executableLength:0],['private_base',privateSlots],['private_length',privateSlots?privateLength:0]]){
    assert(exports[`test.${name}`] instanceof WebAssembly.Global);exports[`test.${name}`].value=value;
  }
}

// Count actual effective memory addresses, rather than a marker or a private
// helper's name. Scratch locals preserve the original address/value stack and
// all original indices; the appended globals hold diagnostic state only.
export function instrumentWitnessModule(source,parsed,isSupport=false) {
  const original=new WebAssembly.Module(source),imports=WebAssembly.Module.imports(original);
  const scan=new Reader(source);scan.take(8);let definedGlobals=0;
  while(scan.at<source.length){const id=scan.byte(),raw=scan.take(scan.unsigned());if(id===6)definedGlobals=new Reader(raw).unsigned();}
  const globalBase=imports.filter(item=>item.kind==='global').length+definedGlobals;
  const names=[...witnessCounters.map(name=>`test.${name}`),'test.bank_base','test.data_base','test.data_length',
    'test.context_base','test.executable_base','test.executable_length','test.private_base','test.private_length'];
  assert(!WebAssembly.Module.exports(original).some(item=>names.includes(item.name)),'fresh witness exports');
  const index=name=>globalBase+witnessCounters.indexOf(name);
  const global=number=>concat([Uint8Array.of(0x23),u(number)]);
  const setGlobal=number=>concat([Uint8Array.of(0x24),u(number)]);
  const local=number=>concat([Uint8Array.of(0x20),u(number)]);
  const setLocal=number=>concat([Uint8Array.of(0x21),u(number)]);
  const increment=name=>concat([global(index(name)),Uint8Array.of(0x41,1,0x6a),setGlobal(index(name))]);
  const constant=value=>{assert(Number.isInteger(value)&&value>=0&&value<=0xffffffff);const a=[];let v=BigInt(value|0),more;
    do{const b=Number(v&127n);v>>=7n;more=!(v===0n&&!(b&64)||v===-1n&&Boolean(b&64));a.push(b|(more?128:0));}while(more);
    return Uint8Array.from([0x41,...a]);};
  const bankGlobal=globalBase+witnessCounters.length;
  const dataGlobal=bankGlobal+1,dataLengthGlobal=bankGlobal+2;
  const contextGlobal=bankGlobal+3,executableGlobal=bankGlobal+4,executableLengthGlobal=bankGlobal+5;
  const privateGlobal=bankGlobal+6,privateLengthGlobal=bankGlobal+7;
  const canonicalImport=parsed.imports.findIndex(item=>item.module==='ring3'&&item.name==='numeric');
  const readImport=parsed.imports.findIndex(item=>item.module==='ring3'&&item.name==='read');
  const writeImport=parsed.imports.findIndex(item=>item.module==='ring3'&&item.name==='write');
  const proof={addressBasis:'actual effective address minus diagnostic Context92 pointer',
    canonical:[0,100],header:[512,640],records:[640,896],recordBytes:32,memorySites:0,normalizers:[],wideSnapshots:[]};
  const instrumentBody=(raw,fn,bodyIndex)=>{
    const functionIndex=parsed.imports.length+bodyIndex,name=parsed.names.get(functionIndex);
    const type=parsed.types[parsed.functionTypes[bodyIndex]],params=type.parameters.length;
    const addr=params+fn.locals.reduce((sum,group)=>sum+group.count,0);
    const values={i32:addr+1,i64:addr+2,f32:addr+3,f64:addr+4,v128:addr+5};
    const counter=isSupport?new Map([['support.normalize_kernel','normalizer_calls'],
      ['support.materialize_numeric','materializer_calls'],['support.acquire_group','group_acquisitions'],
      ['support.translate_page','page_translations'],['support.flags','flags_calls'],
      ['support.resolve_status','status_resolver_calls']]).get(name):name==='guest.run'?'body_entries':undefined;
    if(name==='support.normalize_kernel'){
      const loops=fn.ops.filter(node=>node.op===3).length;
      assert.equal(type.parameters.length,32);assert.equal(type.results.length,30);assert.equal(loops,1);
      proof.normalizers.push({functionIndex,name,loops,parameters:type.parameters,results:type.results});
    }
    const at=new Reader(raw);const groups=at.unsigned(),declarations=raw.subarray(at.at,fn.localsEnd);
    const chunks=[u(groups+5),declarations,Uint8Array.of(2,0x7f,1,0x7e,1,0x7d,1,0x7c,1,0x7b)];
    if(counter)chunks.push(increment(counter));
    // Count entry to the actual typed function in either module, independently
    // of its name. Selective memory writes cannot hide a 40-value call snapshot.
    if(params>=40){
      proof.wideSnapshots.push({functionIndex,name,parameters:type.parameters,results:type.results});
      chunks.push(increment('wide_snapshot_calls'));
    }
    const transported=new Set();
    for(const[i,node]of fn.ops.entries())if(node.op===16&&parsed.names.get(node.call)==='guest.publish'){
      const callType=parsed.types[node.call<parsed.imports.length?parsed.imports[node.call].type:
        parsed.functionTypes[node.call-parsed.imports.length]],count=callType.parameters.length;
      const args=fn.ops.slice(i-count,i);
      assert.equal(args.length,count);assert(args.every(op=>op.op===0x20),'actual boundary transports a contiguous local argument list');
      for(let n=i-count;n<i;n++)transported.add(n);
    }
    let from=fn.localsEnd;
    for(const[i,node]of fn.ops.entries()){
      assert(!(node.op===0xfc&&[10,11].includes(node.ext)),'authored pool witness rejects uncounted bulk memory copies');
      const before=[];
      if(!isSupport&&[0x21,0x22].includes(node.op)&&node.local===91)before.push(increment('fp_metadata_writes'));
      if(!isSupport&&node.op===0x20&&node.local===87&&fn.ops[i+1]?.op===0x41&&
        fn.ops[i+1].value===0xf3f&&fn.ops[i+2]?.op===0x71)before.push(increment('control_admissions'));
      if(name==='guest.run'){
        const following=fn.ops.slice(i+1,i+5);
        if(node.op===16&&node.call===canonicalImport)before.push(increment('canonical_calls'));
        if(node.op===16&&node.call===readImport)before.push(increment('canonical_reads'));
        if(node.op===16&&node.call===writeImport)before.push(increment('canonical_writes'));
        if(node.op===0x20&&node.local===46&&following[0]?.op===0x42&&following[0].value===40n&&
          following[1]?.op===0x88&&following[2]?.op===0x21&&following[2].local===47)
          before.push(increment('fp_generic_f32_packs'));
        const precision=fn.ops.slice(i+1,i+10);
        if(node.op===0x20&&precision[0]?.op===0xbd&&precision[1]?.op===0x42&&
          precision[1].value===0x1fffffffn&&precision[2]?.op===0x83&&precision[3]?.op===0x50&&
          precision[4]?.op===0x45&&precision[5]?.op===0x20&&precision[5].local===54&&
          precision[6]?.op===0x45&&precision[7]?.op===0x71&&precision[8]?.op===13)
          before.push(increment('fp_single_precision_queries'));
        if(node.op===0x20&&precision[0]?.op===0xbd&&precision[1]?.op===0x42&&
          precision[1].value===0x1fffffffn&&precision[2]?.op===0x83&&precision[3]?.op===0x50&&
          precision[4]?.op===0x45&&precision[5]?.op===13)
          before.push(increment('fp_value_precision_admissions'));
        const bound=fn.ops.slice(i+1,i+21);
        if(node.op===0x20&&[48,49].includes(node.local)&&bound[0]?.op===0xbd&&bound[1]?.op===0x42&&
          bound[1].value===52n&&bound[2]?.op===0x88&&bound[3]?.op===0xa7&&bound[4]?.op===0x41&&
          bound[4].value===2047&&bound[5]?.op===0x71&&bound[6]?.op===0x41&&bound[6].value===1023&&
          bound[7]?.op===0x6b&&bound[8]?.op===0x22&&bound[8].local===44&&bound[9]?.op===0x41&&
          bound[9].value===-900&&bound[10]?.op===0x48&&bound[11]?.op===0x20&&bound[11].local===44&&
          bound[12]?.op===0x41&&bound[12].value===900&&bound[13]?.op===0x4a&&bound[14]?.op===0x72&&
          bound[15]?.op===0x20&&bound[15].local===node.local&&bound[16]?.op===0x44&&
          bound[17]?.op===0x62&&bound[18]?.op===0x71&&bound[19]?.op===13)
          before.push(increment('fp_operand_bounds'));
        // These count executed routing operators, rather than emitted label or
        // descriptor counts. INDEX tables are distinct from PREFIX_BASE's
        // instruction-port tables and the physical-payload cell router.
        if(node.op===0x20&&node.local===98){
          before.push(increment('entry_pending_queries'));
          const filter=fn.ops.slice(i+1,i+7);
          if(filter[0]?.op===0x45&&filter[1]?.op===0x20&&filter[1].local===26&&
            filter[2]?.op===0x41&&filter[2].value>=0&&filter[3]?.op===0x46&&
            filter[4]?.op===0x72&&filter[5]?.op===4)
            before.push(increment('entry_filter_queries'));
        }
        if(node.op===0x20&&node.local===26&&following[0]?.op===0x41&&following[0].value>=0&&
          following[1]?.op===0x46)before.push(increment('entry_index_compares'));
        if(node.op===14){
          const direct=fn.ops[i-1]?.op===0x20&&fn.ops[i-1].local===26;
          const relative=fn.ops[i-3]?.op===0x20&&fn.ops[i-3].local===26&&
            fn.ops[i-2]?.op===0x41&&fn.ops[i-2].value>=0&&fn.ops[i-1]?.op===0x6b;
          if(direct||relative)before.push(increment('entry_table_selections'));
        }
        if(node.op===0x20&&node.local===151&&following[0]?.op===0x45&&following[1]?.op===4)
          before.push(increment('prefix_quota_queries'));
        if(node.op===0xa2)before.push(increment('fp_multiply_ops'));
        if(node.op===0x20&&!transported.has(i)&&[20,25].includes(node.local))
          before.push(increment(node.local===20?'flag_kind_queries':'flag_width_queries'));
        if(node.op===0x20&&node.local===84&&following[0]?.op===0x45&&following[1]?.op===4)
          before.push(increment('fp_environment_queries'));
        if(node.op===0x20&&[153,154].includes(node.local)&&following[0]?.op===0x41&&
          following[1]?.op===0x71&&following[2]?.op===0x45&&following[3]?.op===4){
          const bit=following[0].value;
          if(bit>0&&bit<=128&&(bit&(bit-1))===0)
            before.push(increment(node.local===153?'fp_input_load_queries':'fp_input_admission_queries'));
          else if(node.local===154&&bit===256)before.push(increment('fp_control_queries'));
        }
        if(node.op===0x20&&node.local===89&&following[0]?.op===0x20&&following[0].local===90&&
          following[1]?.op===0x41&&following[1].value>=0&&following[1].value<8&&following[2]?.op===0x6a)
          before.push(increment('fp_occupancy_queries'));
        if(node.op===0x20&&node.local===66&&following[0]?.op===0xbd&&following[1]?.op===0x42&&
          following[1].value===52n&&following[2]?.op===0x88&&following[3]?.op===0xa7)
          before.push(increment('fp_result_admissions'));
        if(node.op===0x21&&node.local===100&&fn.ops[i-1]?.op===0x41&&
          fn.ops[i-1].value>=1&&fn.ops[i-1].value<=3)before.push(increment('fp_pending_recipe_writes'));
      }
      if(node.op>=0x28&&node.op<=0x3e||node.op===0xfd&&node.ext<=11){
        proof.memorySites++;
        const vector=node.op===0xfd,store=vector?node.ext===11:node.op>=0x36;
        const value=store?vector?values.v128:node.op===0x37||node.op>=0x3c?values.i64:node.op===0x38?values.f32:
          node.op===0x39?values.f64:values.i32:undefined;
        if(store)before.push(setLocal(value));
        before.push(concat([Uint8Array.of(0x22),u(addr)]));
        before.push(local(addr),constant(node.memoryOffset),Uint8Array.of(0x6a),setLocal(addr));
        before.push(global(dataLengthGlobal),Uint8Array.of(0x04,0x40),local(addr),global(dataGlobal),Uint8Array.of(0x4f),
          local(addr),global(dataGlobal),Uint8Array.of(0x6b),global(dataLengthGlobal),Uint8Array.of(0x49,0x71,0x04,0x40),
          increment(store?'data_writes':'data_reads'),Uint8Array.of(0x0b,0x0b));
        if(!store){
          before.push(global(contextGlobal),Uint8Array.of(0x04,0x40));
          for(const[offset,counter]of[[72,'executable_context_reads'],[76,'private_context_reads']])
            before.push(local(addr),global(contextGlobal),constant(offset),Uint8Array.of(0x6a,0x46,0x04,0x40),
              increment(counter),Uint8Array.of(0x0b));
          before.push(Uint8Array.of(0x0b));
          for(const[base,length,counter]of[[executableGlobal,executableLengthGlobal,'executable_authority_reads'],
            [privateGlobal,privateLengthGlobal,'private_authority_reads']])
            before.push(global(length),Uint8Array.of(0x04,0x40),local(addr),global(base),Uint8Array.of(0x4f),
              local(addr),global(base),Uint8Array.of(0x6b),global(length),Uint8Array.of(0x49,0x71,0x04,0x40),
              increment(counter),Uint8Array.of(0x0b,0x0b));
        }
        before.push(local(addr),global(bankGlobal),Uint8Array.of(0x6b),setLocal(addr));
        before.push(global(bankGlobal),Uint8Array.of(0x04,0x40));
        const range=(low,high,body)=>concat([local(addr),constant(low),Uint8Array.of(0x4f),
          local(addr),constant(high),Uint8Array.of(0x49,0x71,0x04,0x40),...body,Uint8Array.of(0x0b)]);
        before.push(range(512,640,[increment(store?'header_writes':'header_reads')]));
        const mask=index(store?'record_write_mask':'record_read_mask');
        before.push(range(640,896,[increment(store?'record_writes':'record_reads'),global(mask),constant(1),
          local(addr),constant(640),Uint8Array.of(0x6b),constant(5),Uint8Array.of(0x76,0x74,0x72),setGlobal(mask)]));
        if(store)before.push(range(0,100,[increment('raw80_stores')]));
        before.push(Uint8Array.of(0x0b));
        if(store)before.push(local(value));
      }
      if(before.length){chunks.push(raw.subarray(from,node.offset),...before);from=node.offset;}
    }
    chunks.push(raw.subarray(from));return concat(chunks);
  };
  const input=new Reader(source);input.take(8);const output=[source.subarray(0,8)];let exported=false,globalsAdded=false;
  const diagnosticGlobals=Uint8Array.from(names.flatMap(()=>[0x7f,1,0x41,0,0x0b]));
  while(input.at<source.length){
    const id=input.byte(),raw=input.take(input.unsigned());let data=raw;
    if(id===6){
      const r=new Reader(raw),count=r.unsigned();
      data=concat([u(count+names.length),r.take(raw.length-r.at),diagnosticGlobals]);globalsAdded=true;
    }else if(id===7){
      if(!globalsAdded)output.push(section(6,concat([u(names.length),diagnosticGlobals])));
      const r=new Reader(raw),count=r.unsigned(),entries=names.map((name,i)=>{
        const text=new TextEncoder().encode(name);return concat([u(text.length),text,Uint8Array.of(3),u(globalBase+i)]);
      });data=concat([u(count+names.length),r.take(raw.length-r.at),...entries]);exported=true;
    }else if(id===10){
      const r=new Reader(raw),count=r.unsigned(),bodies=[];assert.equal(count,parsed.functions.length);
      for(let i=0;i<count;i++){const body=instrumentBody(r.take(r.unsigned()),parsed.functions[i],i);bodies.push(u(body.length),body);}
      assert.equal(r.at,raw.length);data=concat([u(count),...bodies]);
    }
    output.push(section(id,data));
  }
  assert(exported);return{bytes:concat(output),proof};
}
