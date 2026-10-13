import assert from 'node:assert/strict';
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import {resolve} from 'node:path';
import {createHash} from 'node:crypto';
const [binaryPath,outputPath] = process.argv.slice(2);
assert(binaryPath && outputPath);
const binary = readFileSync(binaryPath), backend = new WebAssembly.Instance(new WebAssembly.Module(binary),{}).exports;
const {ring3_avx_probe_read:read,ring3_avx_probe_write:write,ring3_avx_probe_run:run}=backend;
for(const [fn,arity] of [[read,1],[write,2],[run,4]]) assert.equal(fn.length,arity);
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
let calls=0;const groups={};
const seed=()=>Uint32Array.from({length:96},(_,i)=>(0xa5a50000 ^ Math.imul(i,0x01010101))>>>0);
function source(words,register){const bytes=Buffer.alloc(32);for(let lane=0;lane<4;lane++){bytes.writeUInt32LE(words[12+register*4+lane],lane*4);bytes.writeUInt32LE(words[44+register*4+lane],16+lane*4);}return bytes;}
function setSource(words,register,value){for(let lane=0;lane<4;lane++){words[12+register*4+lane]=value.readUInt32LE(lane*4);words[44+register*4+lane]=value.readUInt32LE(16+lane*4);}}
function oracle(words,operation,registers){const bytes=operation%2===0?16:32,kind=Math.floor((operation-96)/2),first=source(words,(registers>>>8)&255);let second;if(registers&(1<<24)){second=Buffer.alloc(32);for(let i=0;i<8;i++)second.writeUInt32LE(words[76+i],i*4);}else second=source(words,(registers>>>16)&255);let and=false,andNot=false;const start=kind===0?0:kind===1?31:63,step=kind===0?1:kind===1?32:64;for(let bit=start;bit<bytes*8;bit+=step){const mask=1<<(bit%8),a=!!(first[bit>>>3]&mask),b=!!(second[bit>>>3]&mask);and ||= a&&b;andNot ||= !a&&b;}return ((words[9]&~0x8d5)|(!and?0x40:0)|(!andNot?1:0))>>>0;}
function check(group,words,operation,registers,length,status=0,immediate=0){const expected=Uint32Array.from(words);if(status===0)expected[9]=oracle(words,operation,registers);for(let i=0;i<96;i++)assert.equal(write(i,words[i]),0);assert.equal(run(operation,registers,immediate,length),status,`${group}/${operation}/${length}`);for(let i=0;i<96;i++)assert.equal(read(i)>>>0,expected[i],`${group}/${operation}/word${i}`);calls++;groups[group]=(groups[group]??0)+1;}
for(let operation=96;operation<=101;operation++){
 const length=operation%2===0?16:32;
 for(let bit=0;bit<256;bit++)for(let mode=0;mode<3;mode++){
  const words=seed();words[9]=0x244fd7;words[10]=0x8040;const one=Buffer.alloc(32);one[bit>>>3]=1<<(bit%8);setSource(words,0,mode===1?Buffer.alloc(32):one);setSource(words,1,mode===2?Buffer.alloc(32):one);check('all-bit-directions',words,operation,1<<16,0);
 }
 for(let a=0;a<8;a++)for(let b=0;b<8;b++){const words=seed();words[9]=0xcd7;words[10]=0x1f80;check('register-aliases',words,operation,(a<<8)|(b<<16),0);}
 for(let a=0;a<8;a++){const words=seed();words[9]=0xcd7;words[10]=0x1f80;check('memory',words,operation,(a<<8)|(1<<24),length);}
 for(const badLength of [0,length-1,length+1]){const words=seed();words[10]=0x1f80;check('operand-refusal',words,operation,1<<24,badLength,1);}
 {const words=seed();words[10]=0x10000;check('state-refusal',words,operation,1<<16,0,2);}
 {const words=seed();words[10]=0x1f80;check('immediate-refusal',words,operation,1<<16,0,1,1);}
}
const receipt={calls,groups,engine_sha256:hash(binary),scope:'real Rust-Wasm private predicate kernels; exact384-byte state comparison; no public decode/guest page/ABI activation',oracle:'independent bit-index truth reductions; measured VTESTPS159/160/223/224 distinction retained separately',publicAVX:false};mkdirSync(resolve(outputPath),{recursive:true});writeFileSync(resolve(outputPath,'result.json'),JSON.stringify(receipt,null,2)+'\n');console.log(JSON.stringify(receipt));
