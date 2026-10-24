import assert from 'node:assert/strict';
import {readGeneratedModule} from './structure.mjs';
import {instrumentWitnessModule} from './witness.mjs';
import {inspectCompletedHandoffs} from './completed-structure.mjs';

const counters=new Map([['guest.lookup','lookup_entries'],['guest.select_frame','frame_commits']]);
const u=value=>{const result=[];do{const byte=value&127;value>>>=7;result.push(byte|(value?128:0));}while(value);return Uint8Array.from(result);};
const join=parts=>Uint8Array.from(Buffer.concat(parts.map(part=>Buffer.from(part))));
const section=(id,bytes)=>join([Uint8Array.of(id),u(bytes.length),bytes]);
class Reader {
  constructor(bytes){this.bytes=bytes;this.at=0;}
  byte(){assert(this.at<this.bytes.length);return this.bytes[this.at++];}
  unsigned(){let result=0;for(let shift=0;shift<35;shift+=7){const byte=this.byte();result+=(byte&127)*2**shift;if(byte<128)return result;}assert.fail('bounded u32');}
  take(length){assert(this.at+length<=this.bytes.length);const result=this.bytes.subarray(this.at,this.at+length);this.at+=length;return result;}
}

// Count real lookup/descriptor entries and the taken completed-adapter guard.
// A lookup distinguishes a direct RET edge from an in-body dynamic restart.
export function instrumentObservedModule(source) {
  const bytes=source,parsed=readGeneratedModule(bytes),completed=inspectCompletedHandoffs(parsed),r=new Reader(bytes);r.take(8);
  assert.deepEqual(completed.errors,[],'every completed handoff is positively authenticated before instrumentation');
  let defined=0;
  while(r.at<bytes.length){const id=r.byte(),raw=r.take(r.unsigned());if(id===6)defined=new Reader(raw).unsigned();}
  const imported=WebAssembly.Module.imports(new WebAssembly.Module(bytes)).filter(item=>item.kind==='global').length;
  const names=[...counters.values(),'last_add_result'],base=imported+defined;
  const globals=join([Uint8Array.from([...counters.values()].flatMap(()=>[0x7f,1,0x41,0,0x0b])),
    Uint8Array.of(0x7c,1,0x44,0,0,0,0,0,0,0,0,0x0b)]);
  const input=new Reader(bytes);input.take(8);const output=[bytes.subarray(0,8)],proof={functions:[],completed};
  let globalsEmitted=false;
  while(input.at<bytes.length){
    const id=input.byte(),raw=input.take(input.unsigned());let changed=raw;
    if(id>6&&!globalsEmitted){output.push(section(6,join([u(names.length),globals])));globalsEmitted=true;}
    if(id===6){const s=new Reader(raw),count=s.unsigned();changed=join([u(count+names.length),s.take(raw.length-s.at),globals]);globalsEmitted=true;}
    else if(id===7){
      const s=new Reader(raw),count=s.unsigned(),exports=names.map((name,i)=>{
        const encoded=new TextEncoder().encode(`test.observed_${name}`);
        return join([u(encoded.length),encoded,Uint8Array.of(3),u(base+i)]);
      });changed=join([u(count+names.length),s.take(raw.length-s.at),...exports]);
    }else if(id===10){
      const s=new Reader(raw),count=s.unsigned(),functions=[];assert.equal(count,parsed.functions.length);
      for(let i=0;i<count;i++){
        let body=s.take(s.unsigned());const name=parsed.names.get(parsed.imports.length+i),counter=counters.get(name);
        if(counter){
          const index=base+names.indexOf(counter),at=parsed.functions[i].localsEnd;
          const increment=join([Uint8Array.of(0x23),u(index),Uint8Array.of(0x41,1,0x6a,0x24),u(index)]);
          body=join([body.subarray(0,at),increment,body.subarray(at)]);
          proof.functions.push({name,index:parsed.imports.length+i,counter});
        }
        if(name==='guest.run'){
          // Keep the actual last arithmetic result on its original stack. This
          // distinguishes a carried-value bug from a later stale projection.
          const insertions=[];
          for(const op of parsed.functions[i].ops)if(op.op===0xa0){
            insertions.push({at:op.offset+1,bytes:join([Uint8Array.of(0x24),u(base+2),Uint8Array.of(0x23),u(base+2)])});
          }
          for(const adapter of completed.adapters)insertions.push({at:adapter.instrumentOffset,
            bytes:join([Uint8Array.of(0x23),u(base+1),Uint8Array.of(0x41,1,0x6a,0x24),u(base+1)])});
          const chunks=[];let from=0;
          for(const insertion of insertions.sort((a,b)=>a.at-b.at)){
            assert(insertion.at>=from,'all injections share the pristine operator offsets');
            chunks.push(body.subarray(from,insertion.at),insertion.bytes);from=insertion.at;
          }
          chunks.push(body.subarray(from));body=join(chunks);
        }
        functions.push(u(body.length),body);
      }
      changed=join([u(count),...functions]);
    }
    output.push(section(id,changed));
  }
  assert(globalsEmitted,'the counters occupy one actual global section');
  assert(proof.functions.some(fn=>fn.name==='guest.lookup'),'the owner contains the actual generated PC lookup');
  const observedBytes=join(output),standard=instrumentWitnessModule(observedBytes,readGeneratedModule(observedBytes));
  return{bytes:standard.bytes,proof:{...standard.proof,observed:proof}};
}

export function readObservedWitness(exports) {
  return Object.fromEntries([...counters.values()].map(name=>{
    const value=exports[`test.observed_${name}`];assert(value instanceof WebAssembly.Global);
    return[name,value.value];
  }));
}
