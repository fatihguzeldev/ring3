import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {coldFixture,coldLiteral} from '../../../../support/core_wasm/cold_observed.mjs';

const unhex=value=>Uint8Array.from(Buffer.from(value.replaceAll(' ',''),'hex'));
const hash=value=>createHash('sha256').update(value).digest('hex');
const THREE='00000000000000c00040',TWO='00000000000000800040';
const FULL80='01000000000000c00040',ZERO='00000000000000000000';

function image(initial,{control,status,tag,opcode=0x5a5,fip=0x11223344,fdp=0x55667788,cs=0x33,ds=0x5b,registers}) {
  const result=initial.slice(),d=new DataView(result.buffer,result.byteOffset,result.byteLength);
  for(const[offset,value]of[[16,control],[18,status],[20,tag],[22,opcode],[32,cs],[34,ds]])d.setUint16(offset,value,true);
  d.setUint32(24,fip,true);d.setUint32(28,fdp,true);
  for(const[index,value]of Object.entries(registers))result.set(unhex(value),40+Number(index)*10);
  return result;
}

const ghosts={2:'00000000000000000080',3:'0000000000000040ff3f',
  4:'00000000000000c0ff7f',5:FULL80,6:'000000000000008000c0',7:ZERO};

export function runFamilyEntryCases(artifact,report) {
  const code=new Uint8Array(4096);
  code.set(unhex('d8c1 eb0c'));
  code.set(unhex('83c001 49 75ea 90 0f0b'),0x10);
  coldFixture(artifact,report,'cold-built-family-reuses-offset-zero-and-interior-ports-with-contradictory-state',
    {code,registers:[0,128,0x11111111,0x3000,0x4800,0x22222222,0x33333333,0x44444444]},world=>{
      const initial=world.snapshot().fp;
      const training=image(initial,{control:0x27f,status:0,tag:0xfff0,
        registers:{0:THREE,1:TWO,...ghosts}});
      const trainingRegisters=[0,128,0x11111111,0x3000,0x4800,0x22222222,0x33333333,0x44444444];
      world.reset({registers:trainingRegisters,flags:2,pc:0x1000,fp:training});
      let before=world.snapshot(),run=world.execute(641,0x1000);
      const trained=image(training,{control:0x27f,status:0,tag:0xfff0,opcode:0xc1,fip:0x1000,cs:0x1b,
        registers:{0:'00000000000080810740'}});
      coldLiteral(world,before,run,{pc:0x1017,retired:641,budget:641,
        registers:[128,0,...trainingRegisters.slice(2)],flags:0x46,fp:trained,writes:[],versionWrites:0,
        identity:world.bigint(world.context,120)});
      world.result.trainingSemanticComplete=true;
      assert.equal(world.cache.admissions,1,'completed cold instructions produce one hot family without a baseline owner');
      assert.equal(world.cache.units.size,1);
      const family=[...world.cache.units.values()][0];
      const source=world.bytes(world.raw.core_code_ptr()>>>0,world.raw.core_code_len()>>>0);
      const retained={id:family.id,slot:family.slot,run:family.run,link:family.link,publish:family.publish,
        sourceHash:hash(source),sourceBytes:source.length,admissions:world.cache.admissions,
        generatedBytes:world.cache.generatedModuleBytes};
      world.result.family={id:String(family.id),slot:family.slot,sha256:retained.sourceHash,bytes:source.length};

      const registers=[0x7fffffff,3,0xdeadbeef,0x3500,0x4f00,0x24681357,0xabcdef01,0x87654321];
      // Both occupied tag classes contradict their payload classification.
      // The arithmetic destination is reclassified; the unmodified source is not.
      const contradictory=image(initial,{control:0x37f,status:0x1220,tag:0xff2f,
        registers:{0:'00000000000000000080',1:'0000000000000040ff3f',2:FULL80,3:ZERO,
          4:'00000000000000c0ff7f',5:THREE,6:TWO,7:'000000000000008000c0'}});
      const cases=[
        {name:'secondary-block-zero-quota',entry:0x1010,budget:0,pc:0x1010,registers,flags:0x403,fp:contradictory,helpers:0},
        {name:'secondary-block-add-overflow',entry:0x1010,budget:1,pc:0x1013,
          registers:[0x80000000,...registers.slice(1)],flags:0xc96,fp:contradictory,helpers:0},
        {name:'secondary-interior-dec',entry:0x1013,budget:1,pc:0x1014,
          registers:[registers[0],2,...registers.slice(2)],flags:0x403,fp:contradictory,helpers:0},
        {name:'primary-interior-jump',entry:0x1002,budget:1,pc:0x1010,
          registers,flags:0x403,fp:contradictory,helpers:0},
        {name:'primary-full80-arithmetic',entry:0x1000,budget:1,pc:0x1002,
          registers,flags:0x403,helpers:1,
          fp:image(contradictory,{control:0x37f,status:0x1020,tag:0xff0f,opcode:0xc1,fip:0x1000,cs:0x1b,registers:{}})},
      ];
      for(const test of cases){
        world.reset({registers,flags:0x403,pc:test.entry,fp:contradictory});
        before=world.snapshot();
        const helperStart=world.helperCalls.length;
        run=world.execute(test.budget,test.entry);
        coldLiteral(world,before,run,{pc:test.pc,retired:test.budget,budget:test.budget,
          registers:test.registers,flags:test.flags,fp:test.fp,writes:[],versionWrites:0});
        world.result.runs.at(-1).entryCase=test.name;
        world.result.runs.at(-1).semanticComplete=true;
        assert.equal(world.helperCalls.length-helperStart,test.helpers);
        if(test.helpers){
          const callback=world.helperCalls.at(-1);
          assert.equal(callback.ordinal,0,'the callback uses the family native dictionary');
          assert.equal(BigInt(callback.identity),retained.id,'the actual callback is bound to the selected family');
        }
        assert.equal(run.id,retained.id);
        assert.equal(world.cache.units.get(retained.id),family);
        assert.equal(family.slot,retained.slot);
        assert.equal(family.run,retained.run);assert.equal(family.link,retained.link);assert.equal(family.publish,retained.publish);
        assert.equal(world.cache.table.get(retained.slot),retained.link);
        assert.equal(world.cache.publishers.get(retained.slot),retained.publish);
        const current=world.bytes(world.raw.core_code_ptr()>>>0,world.raw.core_code_len()>>>0);
        assert.equal(hash(current),retained.sourceHash);assert.equal(current.length,retained.sourceBytes);
        assert.equal(world.cache.admissions,retained.admissions);
        assert.equal(world.cache.generatedModuleBytes,retained.generatedBytes);
        assert.equal(world.cache.units.size,1);
      }
      world.result.semanticComplete=true;
    });
}
