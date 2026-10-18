import assert from 'node:assert/strict';
import {readFileSync,writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';

const [inputPath,outputPath]=process.argv.slice(2);
assert.ok(inputPath&&outputPath,'expected native-generated control JSON and ignored result path');
const fixture=JSON.parse(readFileSync(inputPath,'utf8'));
const bytes=Buffer.from(fixture.module,'hex');
const memory=new WebAssembly.Memory({initial:1,maximum:1});
const view=new DataView(memory.buffer);
const {exports}=new WebAssembly.Instance(new WebAssembly.Module(bytes),{env:{memory}});
[0x484d3352,0x1000a,40,0,0,0xfedcba98,0,0,0,4].forEach((x,i)=>view.setUint32(100+i*4,x,true));
view.setUint16(fixture.segments+4,1,true);
const iterations=Number(process.env.ITERATIONS??100000);
assert.ok(Number.isSafeInteger(iterations)&&iterations>0&&iterations<=10000000);
for(const fn of [exports.inline_loop,exports.outlined_loop])assert.equal(fn(10000),0xf7);
const samples=[];
for(let pair=0;pair<4;pair++)for(const kind of pair%2?['outlined','inline','inline','outlined']:['inline','outlined','outlined','inline']) {
  const start=performance.now();
  assert.equal(exports[`${kind}_loop`](iterations),0xf7);
  samples.push({kind,iterations,milliseconds:performance.now()-start});
}
assert.equal(exports.slowCalls.value,0,'all timed successes use the inline predicate');
const totals=Object.fromEntries(['inline','outlined'].map(kind=>[kind,samples.filter(x=>x.kind===kind).reduce((s,x)=>s+x.milliseconds,0)]));
const result={createdAt:new Date().toISOString(),node:process.version,v8:process.versions.v8,moduleSha256:createHash('sha256').update(bytes).digest('hex'),iterations,samples,totals,outlinedOverInline:totals.outlined/totals.inline,claim:'authored validator-only warm loop, identical Wasm module/instance/RAM; no actual memory bus, paging, owner guard, code context or game speed claim'};
writeFileSync(outputPath,JSON.stringify(result,null,2)+'\n');
console.log(JSON.stringify(result));
