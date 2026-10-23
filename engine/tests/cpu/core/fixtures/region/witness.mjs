import assert from 'node:assert/strict';

export const witnessCounters=['normalizer_calls','materializer_calls','raw80_stores','fp_metadata_writes',
  'body_entries','control_admissions','group_acquisitions','page_translations',
  'record_reads','record_writes','record_read_mask','record_write_mask','header_reads','header_writes'];
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

// Count actual effective memory addresses, rather than a marker or a private
// helper's name. Scratch locals preserve the original address/value stack and
// all original indices; the appended globals hold diagnostic state only.
export function instrumentWitnessModule(source,parsed,isSupport=false) {
  const original=new WebAssembly.Module(source),imports=WebAssembly.Module.imports(original);
  const scan=new Reader(source);scan.take(8);let definedGlobals=0;
  while(scan.at<source.length){const id=scan.byte(),raw=scan.take(scan.unsigned());if(id===6)definedGlobals=new Reader(raw).unsigned();}
  const globalBase=imports.filter(item=>item.kind==='global').length+definedGlobals;
  const names=[...witnessCounters.map(name=>`test.${name}`),'test.bank_base'];
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
  const proof={addressBasis:'actual effective address minus diagnostic Context92 pointer',
    canonical:[0,100],header:[512,640],records:[640,896],recordBytes:32,memorySites:0,normalizers:[]};
  const instrumentBody=(raw,fn,bodyIndex)=>{
    const functionIndex=parsed.imports.length+bodyIndex,name=parsed.names.get(functionIndex);
    const type=parsed.types[parsed.functionTypes[bodyIndex]],params=type.parameters.length;
    const addr=params+fn.locals.reduce((sum,group)=>sum+group.count,0);
    const values={i32:addr+1,i64:addr+2,f32:addr+3,f64:addr+4};
    const counter=isSupport?new Map([['support.normalize_kernel','normalizer_calls'],
      ['support.materialize_numeric','materializer_calls'],['support.acquire_group','group_acquisitions'],
      ['support.translate_page','page_translations']]).get(name):name==='guest.run'?'body_entries':undefined;
    if(name==='support.normalize_kernel'){
      const loops=fn.ops.filter(node=>node.op===3).length;
      assert.equal(type.parameters.length,32);assert.equal(type.results.length,30);assert.equal(loops,1);
      proof.normalizers.push({functionIndex,name,loops,parameters:type.parameters,results:type.results});
    }
    const at=new Reader(raw);const groups=at.unsigned(),declarations=raw.subarray(at.at,fn.localsEnd);
    const chunks=[u(groups+4),declarations,Uint8Array.of(2,0x7f,1,0x7e,1,0x7d,1,0x7c)];
    if(counter)chunks.push(increment(counter));
    let from=fn.localsEnd;
    for(const[i,node]of fn.ops.entries()){
      assert(!(node.op===0xfc&&[10,11].includes(node.ext)),'authored pool witness rejects uncounted bulk memory copies');
      assert(!(node.op===0xfd&&node.ext===11),'authored pool witness rejects uncounted vector stores');
      const before=[];
      if(!isSupport&&[0x21,0x22].includes(node.op)&&node.local===91)before.push(increment('fp_metadata_writes'));
      if(!isSupport&&node.op===0x20&&node.local===87&&fn.ops[i+1]?.op===0x41&&
        fn.ops[i+1].value===0xf3f&&fn.ops[i+2]?.op===0x71)before.push(increment('control_admissions'));
      if(node.op>=0x28&&node.op<=0x3e){
        proof.memorySites++;
        const store=node.op>=0x36;
        const value=store?node.op===0x37||node.op>=0x3c?values.i64:node.op===0x38?values.f32:
          node.op===0x39?values.f64:values.i32:undefined;
        if(store)before.push(setLocal(value));
        before.push(concat([Uint8Array.of(0x22),u(addr)]));
        before.push(local(addr),constant(node.memoryOffset),Uint8Array.of(0x6a),global(bankGlobal),Uint8Array.of(0x6b),setLocal(addr));
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
