import assert from "node:assert/strict";
import {readFileSync,writeFileSync,mkdirSync} from "node:fs";
import {resolve} from "node:path";
import {createHash} from "node:crypto";
const [binaryPath,outputPath]=process.argv.slice(2);assert(binaryPath&&outputPath);
const binary=readFileSync(binaryPath),module=new WebAssembly.Module(binary),backend=new WebAssembly.Instance(module,{}).exports;
const {ring3_avx_probe_read:read,ring3_avx_probe_write:write,ring3_avx_probe_run:run}=backend;
for(const [fn,arity] of [[read,1],[write,2],[run,4]])assert.equal(fn.length,arity);
let calls=0;const groups={};
const hex=s=>{assert.equal(s.length,32);return Buffer.from(s,"hex");};
const seed=()=>Uint32Array.from({length:96},(_,i)=>(0xa5a50000^Math.imul(i,0x01010101))>>>0);
function lower(words,register,value){for(let lane=0;lane<4;lane++)words[12+register*4+lane]=value.readUInt32LE(lane*4);}
function operand(words,value){for(let lane=0;lane<4;lane++)words[76+lane]=value.readUInt32LE(lane*4);}
function state(){const words=seed();words[9]=0x244fd7;words[10]=0xe5af;return words;}
function regs(d,a,b,memory=false){return (d|(a<<8)|(b<<16)|(memory?1<<24:0))>>>0;}
function check(group,words,id,registers,immediate,length,result,status=0){
 const expected=Uint32Array.from(words),d=registers&255;
 if(!status){assert(result&&result.length===16);lower(expected,d,result);expected.fill(0,44+d*4,48+d*4);}
 for(let i=0;i<96;i++)assert.equal(write(i,words[i]),0);
 assert.equal(run(id,registers,immediate,length),status,`${group}/${id}/${registers.toString(16)}/${immediate}/${length}`);
 for(let i=0;i<96;i++)assert.equal(read(i)>>>0,expected[i],`${group}/${id}/word${i}`);
 calls++;groups[group]=(groups[group]??0)+1;
}
const nist=[
 [102,"00102030405060708090a0b0c0d0e0f0","d6aa74fdd2af72fadaa678f1d6ab76fe","89d810e8855ace682d1843d8cb128fe4"],
 [103,"bd6e7c3df2b5779e0b61216e8b10b689","13111d7fe3944a17f307a78b4d2b30c5","69c4e0d86a7b0430d8cdb78070b4c55a"],
 [104,"7ad5fda789ef4e272bca100b3d9ff59f","13aa29be9c8faff6f770f58000f7bf03","54d990a16ba09ab596bbf40ea111702f"],
 [105,"6353e08c0960e104cd70b751bacad0e7","000102030405060708090a0b0c0d0e0f","00112233445566778899aabbccddeeff"],
 [106,"00000000000000000000000000000000","549932d1f08557681093ed9cbe2c974e","13aa29be9c8faff6f770f58000f7bf03"],
];
for(const [id,a,b,result] of nist)for(let d=0;d<8;d++)for(const memory of [false,true]){
 const words=state();lower(words,1,hex(a));lower(words,2,hex(b));if(memory)operand(words,hex(b));check("NIST-rounds-IMC-register-memory",words,id,regs(d,id===106?0:1,memory?0:2,memory),0,memory?16:0,hex(result));
}
for(let id=102;id<=105;id++)for(let d=0;d<8;d++)for(let a=0;a<8;a++)for(let b=0;b<8;b++){
 const words=state();lower(words,a,Buffer.alloc(16));lower(words,b,Buffer.alloc(16));check("all-round-register-aliases",words,id,regs(d,a,b),0,0,Buffer.alloc(16,id<104?0x63:0x52));
}
for(let d=0;d<8;d++)for(let b=0;b<8;b++){
 const words=state();lower(words,b,Buffer.alloc(16));check("IMC-register-aliases",words,106,regs(d,0,b),0,0,Buffer.alloc(16));
}
const keySource=hex("000102030405060708090a0b0c0d0e0f"),keyBase=hex("f26b6fc56b6fc5f2fed7ab76d7ab76fe");
for(let immediate=0;immediate<256;immediate++)for(const [d,b,memory] of [[0,1,false],[1,1,false],[7,1,false],[0,0,true]]){
 const words=state(),result=Buffer.from(keyBase);result[4]^=immediate;result[12]^=immediate;if(memory)operand(words,keySource);else lower(words,b,keySource);check("all-key-constants",words,107,regs(d,0,b,memory),immediate,memory?16:0,result);
}
const left=hex("00000000000000800300000000000000"),right=hex("02000000000000000100000000000080");
const products={0:"00000000000000000100000000000000",1:"06000000000000000000000000000000",16:"00000000000000800000000000000040",17:"03000000000000800100000000000000"};
const squares={0:"00000000000000000000000000000040",1:"00000000000000800100000000000000",16:"00000000000000800100000000000000",17:"05000000000000000000000000000000"};
for(let immediate=0;immediate<256;immediate++)for(const [d,a,b,memory] of [[0,1,2,false],[1,1,2,false],[2,1,2,false],[7,1,1,false],[1,1,0,true]]){
 const words=state();lower(words,a,left);if(memory)operand(words,right);else if(a!==b)lower(words,b,right);check("all-carryless-selectors",words,108,regs(d,a,b,memory),immediate,memory?16:0,hex((!memory&&a===b?squares:products)[immediate&0x11]));
}
for(let id=102;id<=108;id++){
 const a=id===106||id===107?0:1;
 for(const length of [0,15,17,32])check("memory-length-refusal",state(),id,regs(0,a,0,true),0,length,null,1);
 check("register-length-refusal",state(),id,regs(0,a,2),0,16,null,1);
 {const words=state();words[10]=0x10000;check("host-state-refusal",words,id,regs(0,a,2),0,0,null,2);}
 check("memory-unused-register-refusal",state(),id,regs(0,a,2,true),0,16,null,1);
 check("register-range-refusal",state(),id,regs(8,a,2),0,0,null,1);
 check("register-reserved-bits-refusal",state(),id,(regs(0,a,2)|0x80000000)>>>0,0,0,null,1);
 check("oversized-operand-refusal",state(),id,regs(0,a,0,true),0,33,null,1);
 if(id<=106)check("immediate-refusal",state(),id,regs(0,a,2),1,0,null,1);
 else check("immediate-range-refusal",state(),id,regs(0,a,2),256,0,null,1);
}
for(const id of [106,107])check("unary-unused-first-refusal",state(),id,regs(0,1,2),0,0,null,1);
check("unknown-operation-refusal",state(),0xffffffff,regs(0,1,2),0,0,null,1);
assert.equal(calls,4576);
const receipt={calls,groups,engine_sha256:createHash("sha256").update(binary).digest("hex"),oracle:"literal NIST AES128 intermediate values, Sbox0/InvSbox0 round constants, hand polynomial products, every KEY/CLMUL immediate; no JS crypto execution",scope:"real Rust-Wasm private crypto kernels; exact384B staged state; no public VEX/guestpage/ABI activation",publicAVX:false};mkdirSync(resolve(outputPath),{recursive:true});writeFileSync(resolve(outputPath,"result.json"),JSON.stringify(receipt,null,2)+"\n");console.log(JSON.stringify(receipt));
