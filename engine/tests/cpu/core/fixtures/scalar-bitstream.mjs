import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync,writeFileSync,openSync,closeSync} from 'node:fs';
import {readEngine} from '../../../support/wasm/engine.mjs';
import {coldFixture,coldLiteral} from '../../../support/core_wasm/cold_observed.mjs';
import {record,view} from '../../../../../sdk/runtime/engine.js';

const [enginePath,reportPath]=process.argv.slice(2);
assert(enginePath&&reportPath,'usage: scalar-bitstream.mjs engine.wasm fresh-report.json');
const artifact=readEngine(enginePath),output=openSync(reportPath,'wx');closeSync(output);
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
const hex=bytes=>Buffer.from(bytes).toString('hex');
const unhex=text=>Uint8Array.from(Buffer.from(text.replaceAll(' ',''),'hex'));
const ROOT=0x1000,TARGET=0x1800,WORK=85;
const initialRegisters=[0xa5a55a5a,0xdeadbeef,0x1234ab00,0x76543210,0x4800,0x3100,0x3000,0x3200];
const finalRegisters=[0xf,0,0x1234abed,0,0x4800,0x3100,0x91a58e20,0x3208];
const decoded='98e899ef9aee9bed',checksum='208ea591';

// Eight low-first nibbles select ASCII table bytes. The byte operation keeps
// EDX's upper 24 bits; SHR supplies CF to the following ADC checksum update.
// This is an authored scalar decoder, not a captured game implementation.
const source=new Uint8Array(4096);
source.set(unhex('8b1e 8d7604 b908000000 89d8 83e00f 8a540500 30f2 c1eb04 11d6 8817 8d7f01 49 75e8 8937 e9d7070000'));
source.set(unhex('0f0b'),TARGET-ROOT);
const data=d=>{
  d.setUint32(0,0xf0e1d2c3,true);
  new Uint8Array(d.buffer,d.byteOffset+0x100,16).set(Buffer.from('0123456789ABCDEF','ascii'));
};
const writes=[...unhex(decoded)].map((byte,index)=>({address:0x3200+index,bytes:Uint8Array.of(byte)}));
writes.push({address:0x3208,bytes:checksum});
const sources=['./scalar-bitstream.mjs','../../../support/core_wasm/cold_observed.mjs',
  '../../../support/wasm/engine.mjs','../../../../../sdk/runtime/engine.js','../../../../../sdk/runtime/cache.js'];
const report={schema:1,complete:false,engineSha256:artifact.sha256,
  scope:'bounded real-emitted scalar bitstream decoder versus an independent exact cold execution; no timing or game claim',
  sources:Object.fromEntries(sources.map(path=>[path,hash(readFileSync(new URL(path,import.meta.url)))])),cases:[]};
const save=()=>writeFileSync(reportPath,JSON.stringify(report,null,2));
const architectural=[];
save();

try {
  for(const compiled of [false,true]) {
    const name=compiled?'scalar-bitstream-actual-emitted-owner':'scalar-bitstream-exact-cold-reference';
    coldFixture(artifact,report,name,{code:source,registers:initialRegisters,flags:0x403,data,
      referencePrepared:compiled,budget:WORK},world=>{
      let owner;
      if(compiled) {
        owner=world.cache.entry(ROOT);
        assert(owner&&owner.id>0n,'the compiled case selects a real retained emitted owner');
      }
      const before=world.snapshot();
      const run=compiled?world.execute(WORK,ROOT):world.executeNative(WORK);
      coldLiteral(world,before,run,{pc:TARGET,retired:WORK,budget:WORK,registers:finalRegisters,
        flags:0x446,writes,versionWrites:9,fp:world.initialFp,identity:owner?.id??0n});
      assert.equal(world.helperCalls.length,0,'this scalar program executes no numeric callback');
      const wantedWork=record('R3WK',24);view(wantedWork).setUint32(16,WORK,true);
      const wantedExit=record('R3EX',40),e=view(wantedExit);
      e.setUint16(4,15,true);e.setUint32(16,1,true);e.setUint32(20,WORK,true);
      assert.deepEqual(world.bytes(world.engine.base+4588,24),wantedWork,'complete literal work packet');
      assert.deepEqual(world.bytes(world.engine.base+56,40),wantedExit,'complete literal quota exit packet');
      const after=world.snapshot();
      const pageDeltas=Array.from({length:after.pages.length/16},(_,index)=>{
        const at=index*16+8;
        return String(new DataView(after.pages.buffer).getBigUint64(at,true)
          -new DataView(before.pages.buffer).getBigUint64(at,true));
      });
      const image={state:hex(after.state),fp:hex(after.fp),simd:hex(after.simd),ymm:hex(after.ymm),
        exit:hex(world.bytes(world.engine.base+56,40)),work:hex(world.bytes(world.engine.base+4588,24)),
        ram:hex(after.ram),pageContentVersionDeltas:pageDeltas,
        revisionDelta:String(after.revision-before.revision),
        // Literal Context128 is checked above against each engine's own exact
        // pointer/identity authority. Compare only architectural words here.
        scalar:Array.from({length:14},(_,index)=>world.word(world.context,index*4)),
        active:world.word(world.context,112),fault:world.word(world.context,116)};
      architectural.push(image);
      world.result.semanticComplete=true;
      world.result.decoder={instructions:WORK,tableReads:8,streamReads:1,writes:9,
        output:hex(world.bytes(world.physical(0x3200),12)),expectedOutput:decoded+checksum,
        codeSha256:hash(source),architecturalSha256:hash(Buffer.from(JSON.stringify(image)))};
      assert.equal(world.result.decoder.output,decoded+checksum,'independent literal decoded bytes and checksum');
      if(compiled) {
        assert.equal(run.kind,'compiled');assert.equal(run.id,owner.id);
        assert.equal(world.cache.preciseRuns,0,'the full admitted decoder executes the emitted path');
        assert.equal(run.regions,1,'the internal decoder loop stays in one actual emitted body');
        assert.equal(world.cache.admissions,1);
        assert.equal(world.result.modules.filter(module=>module.kind==='guest').length,1);
      } else {
        assert.equal(run.kind,'cold-native');assert.equal(world.prepares,0);
        assert.equal(world.cache.admissions,0);assert.equal(world.cache.generatedModuleBytes,0);
        assert.equal(world.raw.core_code_len()>>>0,0);
        assert.equal(world.result.modules.filter(module=>module.kind==='guest').length,0);
      }
    });
    save();
  }
  assert.deepEqual(architectural[1],architectural[0],
    'emitted and exact native executions agree on complete architectural packets, backing, faults, and write-stamp deltas');
  report.differential={complete:true,architecturalSha256:hash(Buffer.from(JSON.stringify(architectural[0])))};
  report.complete=true;save();
  console.log(JSON.stringify({complete:true,cases:report.cases.length,engineSha256:artifact.sha256}));
} catch(error) {report.failure={message:String(error),stack:error.stack};save();throw error;}
