import assert from "node:assert/strict";
let input="";for await(const chunk of process.stdin)input+=chunk;const cases=JSON.parse(input),STATE=256,LOWER=STATE+64,UPPER=STATE+256,MXCSR=STATE+32;
function read(b,at,w,signed=false){let n=0n;for(let i=w-1;i>=0;i--)n=n*256n+BigInt(b[at+i]);return signed?BigInt.asIntN(w*8,n):n;}
function write(b,at,w,n){for(let i=0;i<w;i++)b[at+i]=Number(BigInt.asUintN(8,n>>BigInt(i*8)));}
const abs=n=>n<0n?-n:n,clamp=n=>n< -32768n?-32768n:n>32767n?32767n:n;
function oracle(c,a,b){const out=new Uint8Array(16),w=c.width;
 switch(c.kind){
 case "MultiplyUnsignedDword":case "MultiplySignedDword":for(let at=0;at<16;at+=8)write(out,at,8,read(a,at,4,c.kind==="MultiplySignedDword")*read(b,at,4,c.kind==="MultiplySignedDword"));break;
 case "MultiplyAddWords":for(let at=0;at<16;at+=4)write(out,at,4,read(a,at,2,true)*read(b,at,2,true)+read(a,at+2,2,true)*read(b,at+2,2,true));break;
 case "MultiplyAddUnsignedSignedBytes":for(let at=0;at<16;at+=2)write(out,at,2,clamp(read(a,at,1)*read(b,at,1,true)+read(a,at+1,1)*read(b,at+1,1,true)));break;
 case "SumAbsoluteByteDifferences":for(let at=0;at<16;at+=8){let sum=0n;for(let i=0;i<8;i++)sum+=abs(BigInt(a[at+i])-BigInt(b[at+i]));write(out,at,8,sum);}break;
 case "MultipleSumAbsoluteByteDifferences":for(let lane=0;lane<8;lane++){let sum=0n;for(let i=0;i<4;i++)sum+=abs(BigInt(a[(c.immediate>>2&1)*4+lane+i])-BigInt(b[(c.immediate&3)*4+i]));write(out,lane*2,2,sum);}break;
 case "MinimumPositionUnsignedWord":{let minimum=65536n,index=0;for(let lane=0;lane<8;lane++){const v=read(b,lane*2,2);if(v<minimum){minimum=v;index=lane;}}write(out,0,2,minimum);write(out,2,2,BigInt(index));break;}
 case "HorizontalAdd":case "HorizontalSubtract":case "HorizontalAddSignedSaturate":case "HorizontalSubtractSignedSaturate":{
  const half=8/w;for(let lane=0;lane<16/w;lane++){const input=lane<half?a:b,at=lane%half*w*2,x=read(input,at,w,true),y=read(input,at+w,w,true);let v=c.kind.includes("Subtract")?x-y:x+y;if(c.kind.endsWith("Saturate"))v=clamp(v);write(out,lane*w,w,v);}break;}
 default:for(let at=0;at<16;at+=w){let v;if(c.kind==="MultiplyLow")v=read(a,at,w)*read(b,at,w);else if(c.kind==="MultiplyHighUnsigned")v=read(a,at,w)*read(b,at,w)>>BigInt(w*8);else if(c.kind==="MultiplyHighSigned")v=read(a,at,w,true)*read(b,at,w,true)>>BigInt(w*8);else if(c.kind==="MultiplyHighRoundSigned")v=(read(a,at,w,true)*read(b,at,w,true)+16384n)>>15n;else throw Error(`unknown ${c.kind}`);write(out,at,w,v);}
 }
 return out;}
for(const [index,c]of cases.entries()){
 const label=`${index}:${c.kind}/${c.width}/${c.immediate}:alias${c.d}/${c.a}/${c.b}:address${c.address}`,memory=new WebAssembly.Memory({initial:1,maximum:1}),bytes=new Uint8Array(memory.buffer);
 for(let i=0;i<bytes.length;i++)bytes[i]=(i*37+(i>>>6)*53+19)&255;new DataView(memory.buffer).setUint32(MXCSR,c.invalid?0x10000:0xe5af,true);
 for(let r=0;r<8;r++)for(let i=0;i<16;i++)bytes[LOWER+r*16+i]=[0,255,128,127,1,254,129,85][(i+c.seed+r*3)%8];const length=c.b==="memory"?16:0;assert.equal(c.memoryBytes,length,label);if(length&&c.address<=65520)for(let i=0;i<16;i++)bytes[c.address+i]=[0,255,128,127,1,254,129,85][(i+c.seed+7)%8];
 const before=bytes.slice(),expected=before.slice(),status=c.invalid?4:length&&c.address>65520?3:0;if(!status){expected.set(oracle(c,before.slice(LOWER+c.a*16,LOWER+(c.a+1)*16),c.b==="memory"?before.slice(c.address,c.address+16):before.slice(LOWER+c.b*16,LOWER+(c.b+1)*16)),LOWER+c.d*16);expected.fill(0,UPPER+c.d*16,UPPER+(c.d+1)*16);}
 const module=new WebAssembly.Module(Buffer.from(c.module,"hex"));assert.deepEqual(WebAssembly.Module.imports(module),[{module:"env",name:"memory",kind:"memory"}],label);assert.deepEqual(WebAssembly.Module.exports(module),[{name:"run",kind:"function"}],label);const instance=new WebAssembly.Instance(module,{env:{memory}});assert.equal(instance.exports.run(STATE,c.address),status,`${label}: status`);assert.deepEqual(bytes,expected,`${label}: all64KiB state/output/refusal neutrality`);
}
console.log(`${cases.length} actual multiply modules passed`);
