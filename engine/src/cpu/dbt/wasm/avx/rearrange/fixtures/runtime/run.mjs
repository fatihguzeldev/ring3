import assert from "node:assert/strict";
let input="";for await(const chunk of process.stdin)input+=chunk;
const cases=JSON.parse(input),STATE=256,LOWER=STATE+64,UPPER=STATE+256,MXCSR=STATE+32;
function read(bytes,at,width,signed=false){let v=0n;for(let b=width-1;b>=0;b--)v=v*256n+BigInt(bytes[at+b]);return signed?BigInt.asIntN(width*8,v):v;}
function write(bytes,at,width,value){for(let b=0;b<width;b++)bytes[at+b]=Number(BigInt.asUintN(8,value>>BigInt(b*8)));}
function oracle(c,a,b,mask){
 const out=new Uint8Array(16),w=c.width,imm=c.immediate;
 const copy=(target,source,at,count)=>out.set(source.slice(at,at+count),target);
 if(c.kind.startsWith("SignExtend")||c.kind.startsWith("ZeroExtend")){
  const from=c.kind.endsWith("Bytes")?1:c.kind.endsWith("Words")?2:4;
  for(let lane=0;lane<16/w;lane++)write(out,lane*w,w,read(b,lane*from,from,c.kind.startsWith("Sign")));
 }else switch(c.kind){
 case "UnpackLow":case "UnpackHigh":{
  const start=c.kind==="UnpackHigh"?8:0;
  for(let lane=0;lane<8/w;lane++){copy(lane*w*2,a,start+lane*w,w);copy(lane*w*2+w,b,start+lane*w,w);}break;
 }
 case "PackSigned":case "PackUnsigned":{
  const signed=c.kind==="PackSigned",lo=signed?-(1n<<BigInt(w*8-1)):0n,hi=signed?(1n<<BigInt(w*8-1))-1n:(1n<<BigInt(w*8))-1n;
  for(let lane=0;lane<16/w;lane++){let v=read(lane<8/w?a:b,lane%(8/w)*w*2,w*2,true);write(out,lane*w,w,v<lo?lo:v>hi?hi:v);}break;
 }
 case "Dwords":for(let lane=0;lane<4;lane++)copy(lane*4,b,(imm>>(lane*2)&3)*4,4);break;
 case "HighWords":case "LowWords":{
  out.set(b);const start=c.kind==="HighWords"?8:0;
  for(let lane=0;lane<4;lane++)copy(start+lane*2,b,start+(imm>>(lane*2)&3)*2,2);break;
 }
 case "Bytes":for(let i=0;i<16;i++)out[i]=b[i]&128?0:a[b[i]&15];break;
 case "AlignRight":{const concat=Uint8Array.from([...b,...a,...new Uint8Array(256)]);out.set(concat.slice(imm,imm+16));break;}
 case "BlendImmediate":case "BlendVariable":
  for(let lane=0;lane<16/w;lane++)copy(lane*w,(c.kind==="BlendImmediate"?(imm>>lane&1):(mask[(lane+1)*w-1]>>7))?b:a,lane*w,w);break;
 default:throw Error(`unhandled ${c.kind}`);
 }
 return out;
}
for(const [index,c] of cases.entries()){
 const label=`${index}:${c.kind}/${c.width}/${c.immediate}:alias${c.d}/${c.a}/${c.b}/${c.mask}:address${c.address}`;
 const memory=new WebAssembly.Memory({initial:1,maximum:1}),bytes=new Uint8Array(memory.buffer);
 for(let i=0;i<bytes.length;i++)bytes[i]=(i*37+(i>>>6)*53+19)&255;
 new DataView(memory.buffer).setUint32(MXCSR,c.invalid?0x10000:0xe5af,true);
 for(let r=0;r<8;r++)for(let i=0;i<16;i++)bytes[LOWER+r*16+i]=[0,255,128,127,1,254,129,85][(i+c.seed+r*3)%8];
 const from=c.kind.endsWith("Bytes")?1:c.kind.endsWith("Words")?2:4;
 const width=c.kind.startsWith("SignExtend")||c.kind.startsWith("ZeroExtend")?16/c.width*from:16;
 const length=c.b==="memory"?width:0;
 assert.equal(c.memoryBytes,length,`${label}: exact source span`);
 if(length&&c.address<=65536-length)for(let i=0;i<length;i++)bytes[c.address+i]=[0,255,128,127,1,254,129,85][(i+c.seed+7)%8];
 const before=bytes.slice(),expected=before.slice(),status=c.invalid?4:length&&c.address>65536-length?3:0;
 if(!status){
  const a=before.slice(LOWER+c.a*16,LOWER+(c.a+1)*16),b=c.b==="memory"?before.slice(c.address,c.address+length):before.slice(LOWER+c.b*16,LOWER+(c.b+1)*16),mask=before.slice(LOWER+c.mask*16,LOWER+(c.mask+1)*16);
  expected.set(oracle(c,a,b,mask),LOWER+c.d*16);expected.fill(0,UPPER+c.d*16,UPPER+(c.d+1)*16);
 }
 const module=new WebAssembly.Module(Buffer.from(c.module,"hex"));
 assert.deepEqual(WebAssembly.Module.imports(module),[{module:"env",name:"memory",kind:"memory"}],label);
 assert.deepEqual(WebAssembly.Module.exports(module),[{name:"run",kind:"function"}],label);
 const instance=new WebAssembly.Instance(module,{env:{memory}});
 assert.equal(instance.exports.run(STATE,c.address),status,`${label}: status`);
 assert.deepEqual(bytes,expected,`${label}: full64KiB state/output/refusal neutrality`);
}
console.log(`${cases.length} actual rearrange modules passed`);
