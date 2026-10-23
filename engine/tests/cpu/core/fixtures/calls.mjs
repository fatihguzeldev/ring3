import assert from 'node:assert/strict';
import {readGeneratedModule} from './region/structure.mjs';

export function instrumentCallCounter(source,name,exportName) {
  const parsed=readGeneratedModule(source),imports=WebAssembly.Module.imports(new WebAssembly.Module(source));
  const functions=imports.filter(item=>item.kind==='function'),target=functions.findIndex(item=>item.module==='ring3'&&item.name===name);
  assert(target>=0,'the counter observes an actual direct Wasm support import');
  const global=imports.filter(item=>item.kind==='global').length;
  const u=value=>{const result=[];do{const b=value&127;value>>>=7;result.push(b|(value?128:0));}while(value);return Uint8Array.from(result);};
  const concat=parts=>Uint8Array.from(Buffer.concat(parts.map(x=>Buffer.from(x))));
  const section=(id,data)=>concat([Uint8Array.of(id),u(data.length),data]);
  const take=(r,n)=>{assert(r.at+n<=r.bytes.length);const b=r.bytes.subarray(r.at,r.at+n);r.at+=n;return b;};
  const read=r=>{let value=0;for(let s=0;s<35;s+=7){const b=take(r,1)[0];value+=(b&127)*2**s;if(b<128)return value;}assert.fail('bounded u32');};
  const input={bytes:source,at:8},output=[source.subarray(0,8)];let added=false,sites=0;
  const increment=concat([Uint8Array.of(0x23),u(global),Uint8Array.of(0x41,1,0x6a,0x24),u(global)]);
  while(input.at<source.length) {
    const id=take(input,1)[0],raw=take(input,read(input));let data=raw;
    assert.notEqual(id,6,'the immutable owner has no defined production globals');
    if(id===7) {
      output.push(section(6,Uint8Array.of(1,0x7f,1,0x41,0,0x0b)));
      const r={bytes:raw,at:0},count=read(r),text=new TextEncoder().encode(exportName);
      data=concat([u(count+1),take(r,raw.length-r.at),u(text.length),text,Uint8Array.of(3),u(global)]);added=true;
    }else if(id===10) {
      const r={bytes:raw,at:0},count=read(r),bodies=[];
      assert.equal(count,parsed.functions.length,'all actual owner functions are decoded');
      for(let i=0;i<count;i++) {
        const bytes=take(r,read(r)),fn=parsed.functions[i],parts=[];let start=0;
        for(const op of fn.ops.filter(op=>op.op===16&&op.call===target)) {
          parts.push(bytes.subarray(start,op.offset),increment);start=op.offset;sites++;
        }
        parts.push(bytes.subarray(start));const body=concat(parts);bodies.push(u(body.length),body);
      }
      assert.equal(r.at,raw.length);data=concat([u(count),...bodies]);
    }
    output.push(section(id,data));
  }
  assert(added&&sites>0,'actual direct support call sites carry the counter');
  return concat(output);
}
