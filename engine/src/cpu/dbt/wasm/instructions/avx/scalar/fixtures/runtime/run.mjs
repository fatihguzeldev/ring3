import assert from "node:assert/strict";
let input="";for await(const chunk of process.stdin)input+=chunk;const cases=JSON.parse(input),STATE=256,LOWER=STATE+64,UPPER=STATE+256,MXCSR=STATE+32;
function read(b,at,w){let n=0;for(let i=w-1;i>=0;i--)n=n*256+b[at+i];return n>>>0;}
function write(b,at,w,n){for(let i=0;i<w;i++)b[at+i]=(n>>>(i*8))&255;}
for(const[index,c]of cases.entries()){
 const label=`${index}:${c.form}/${c.width}/${c.immediate}:vector${c.d}/${c.a}:gpr${c.scalar}:memory${c.memory}/${c.address}`,memory=new WebAssembly.Memory({initial:1,maximum:1}),bytes=new Uint8Array(memory.buffer);
 for(let i=0;i<bytes.length;i++)bytes[i]=(i*37+(i>>>6)*53+19)&255;new DataView(memory.buffer).setUint32(MXCSR,c.invalid?0x10000:0xe5af,true);
 for(let r=0;r<8;r++)for(let i=0;i<16;i++)bytes[LOWER+r*16+i]=[0,255,128,127,1,254,129,85][(i+r*3)%8];
 const length=c.memory?c.width:0;assert.equal(c.memoryBytes,length,`${label}: exact scalar span`);assert.equal(c.write,c.memory&&c.form==="extract",`${label}: read/write role`);assert.ok(!(c.form==="legacyWord"&&c.memory),`${label}: opcodeC5 registeronly`);
 if(c.memory&&c.form==="insert"&&c.address<=65536-c.width)for(let i=0;i<c.width;i++)bytes[c.address+i]=[0xef,0xbe,0xad,0xde][i];
 const before=bytes.slice(),expected=before.slice(),status=c.invalid?4:length&&c.address>65536-length?3:0;
 if(!status){if(c.form==="insert"){
   expected.set(before.slice(LOWER+c.a*16,LOWER+(c.a+1)*16),LOWER+c.d*16);
   expected.set(before.slice(c.memory?c.address:STATE+c.scalar*4,(c.memory?c.address:STATE+c.scalar*4)+c.width),LOWER+c.d*16+(c.immediate%(16/c.width))*c.width);expected.fill(0,UPPER+c.d*16,UPPER+(c.d+1)*16);
  }else{let value=0;if(c.form==="mask"){for(let lane=0;lane<16;lane++)value|=(before[LOWER+c.a*16+lane]>>7)<<lane;}else value=read(before,LOWER+c.a*16+(c.immediate%(16/c.width))*c.width,c.width);write(expected,c.memory?c.address:STATE+c.scalar*4,c.memory?c.width:4,value);}}
 const module=new WebAssembly.Module(Buffer.from(c.module,"hex"));assert.deepEqual(WebAssembly.Module.imports(module),[{module:"env",name:"memory",kind:"memory"}],label);assert.deepEqual(WebAssembly.Module.exports(module),[{name:"run",kind:"function"}],label);const instance=new WebAssembly.Instance(module,{env:{memory}});assert.equal(instance.exports.run(STATE,c.address),status,`${label}: status`);assert.deepEqual(bytes,expected,`${label}: full64KiB vector/GPR/flags/tail/refusal neutrality`);
}
console.log(`${cases.length} actual scalar modules passed`);
