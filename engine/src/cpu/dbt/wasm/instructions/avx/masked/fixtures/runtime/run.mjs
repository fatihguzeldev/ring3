import assert from "node:assert/strict";
let input="";for await(const chunk of process.stdin)input+=chunk;
const cases=JSON.parse(input),STATE=256,LOWER=STATE+64,UPPER=STATE+256,MXCSR=STATE+32;
function vectorByte(register,byte){return (byte<16?LOWER:UPPER)+register*16+byte%16;}
for(const [index,c] of cases.entries()) {
 const label=`${index}:${c.kind}/${c.width}/${c.lane}:mask${c.maskBits.toString(16)}:regs${c.d}/${c.mask}/${c.source}:address${c.address}`,memory=new WebAssembly.Memory({initial:1,maximum:1}),bytes=new Uint8Array(memory.buffer);
 for(let i=0;i<bytes.length;i++)bytes[i]=(i*37+(i>>>6)*53+19)&255;
 new DataView(memory.buffer).setUint32(MXCSR,c.invalid?0x10000:0xe5af,true);
 for(let i=0;i<32;i++)bytes[vectorByte(c.mask,i)]=0x7f;
 for(let lane=0;lane<c.width/c.lane;lane++)bytes[vectorByte(c.mask,(lane+1)*c.lane-1)]=(c.maskBits&(1<<lane))?(lane%2?0xff:0x80):0x7f;
 const before=bytes.slice(),expected=before.slice();let status=c.invalid?4:0;
 const enabled=[];
 for(let lane=0;lane<c.width/c.lane;lane++)if(before[vectorByte(c.mask,(lane+1)*c.lane-1)]&128)enabled.push(lane);
 if(!status&&enabled.some(lane=>c.address+(lane+1)*c.lane>65536))status=3;
 if(!status){
  if(c.kind==="load")for(let i=0;i<32;i++)expected[vectorByte(c.d,i)]=0;
  for(const lane of enabled)for(let j=0;j<c.lane;j++){
   const byte=lane*c.lane+j;
   if(c.kind==="load")expected[vectorByte(c.d,byte)]=before[c.address+byte];
   else expected[c.address+byte]=before[vectorByte(c.source,byte)];
  }
 }
 const module=new WebAssembly.Module(Buffer.from(c.module,"hex"));assert.deepEqual(WebAssembly.Module.imports(module),[{module:"env",name:"memory",kind:"memory"}],label);assert.deepEqual(WebAssembly.Module.exports(module),[{name:"run",kind:"function"}],label);
 const instance=new WebAssembly.Instance(module,{env:{memory}});assert.equal(instance.exports.run(STATE,c.address),status,`${label}: local staging status`);assert.deepEqual(bytes,expected,`${label}: complete64KiB selectedload/store,upperzero,sourcecapture,disabledspan,faultneutrality`);
}
console.log(`${cases.length} actual masked memory modules passed`);
