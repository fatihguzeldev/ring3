import assert from "node:assert/strict";
let input="";for await(const chunk of process.stdin)input+=chunk;
const cases=JSON.parse(input),STATE=256,LOWER=STATE+64,UPPER=STATE+256,MXCSR=STATE+32;
function read(b,at,w,signed=false){let n=0n;for(let i=w-1;i>=0;i--)n=n*256n+BigInt(b[at+i]);return signed?BigInt.asIntN(w*8,n):n;}
function write(b,at,w,n){for(let i=0;i<w;i++)b[at+i]=Number(BigInt.asUintN(8,n>>BigInt(i*8)));}
for(const [index,c] of cases.entries()){
 const label=`${index}:${c.kind}/${c.width}:count${c.immediate??c.count}:alias${c.d}/${c.a}/${c.b}:address${c.address}`;
 const memory=new WebAssembly.Memory({initial:1,maximum:1}),bytes=new Uint8Array(memory.buffer);
 for(let i=0;i<bytes.length;i++)bytes[i]=(i*37+(i>>>6)*53+19)&255;
 new DataView(memory.buffer).setUint32(MXCSR,c.invalid?0x10000:0xe5af,true);
 for(let r=0;r<8;r++)for(let i=0;i<16;i++)bytes[LOWER+r*16+i]=[0,255,128,127,1,254,129,85][(i+c.seed+r*3)%8];
 const memoryCount=c.immediate===null&&c.b==="memory",length=memoryCount?16:0;assert.equal(c.memoryBytes,length,`${label}: full16 count authority`);
 if(c.immediate===null){const at=memoryCount?c.address:LOWER+c.b*16;if(at<=65520){write(bytes,at,8,BigInt(c.count));bytes.fill(0xff,at+8,at+16);}}
 const before=bytes.slice(),expected=before.slice(),status=c.invalid?4:length&&c.address>65520?3:0;
 if(!status){const source=LOWER+c.a*16,count=c.immediate!==null?BigInt(c.immediate):read(before,memoryCount?c.address:LOWER+c.b*16,8),out=new Uint8Array(16);
  if(c.kind==="ShiftBytesLeft"||c.kind==="ShiftBytesRight"){
   const n=Number(count);for(let i=0;i<16;i++){const at=c.kind==="ShiftBytesLeft"?i-n:i+n;out[i]=at>=0&&at<16?before[source+at]:0;}
  }else{for(let lane=0;lane<16/c.width;lane++){const signed=c.kind==="ShiftRightSigned",v=read(before,source+lane*c.width,c.width,signed),bits=BigInt(c.width*8);let value;
   if(count>=bits)value=signed?(v<0n?-1n:0n):0n;else value=c.kind==="ShiftLeft"?v<<count:v>>count;write(out,lane*c.width,c.width,value);
  }}expected.set(out,LOWER+c.d*16);expected.fill(0,UPPER+c.d*16,UPPER+(c.d+1)*16);
 }
 const module=new WebAssembly.Module(Buffer.from(c.module,"hex"));assert.deepEqual(WebAssembly.Module.imports(module),[{module:"env",name:"memory",kind:"memory"}],label);assert.deepEqual(WebAssembly.Module.exports(module),[{name:"run",kind:"function"}],label);
 const instance=new WebAssembly.Instance(module,{env:{memory}});assert.equal(instance.exports.run(STATE,c.address),status,`${label}: status`);assert.deepEqual(bytes,expected,`${label}: all64KiB output/state/refusal neutrality`);
}
console.log(`${cases.length} actual shift modules passed`);
