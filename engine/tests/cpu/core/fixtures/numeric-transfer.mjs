import {coreImports} from './link.mjs';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync,writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {runActualPrefix} from './actual-prefix.mjs';
import {runActualTransfer} from './numeric-transfer-actual.mjs';

const directory=process.argv[2],BUDGET=32;
const hash=value=>createHash('sha256').update(value).digest('hex');
const initial=[0xa0a0a0a0,0xb1b1b1b1,0xc2c2c2c2,0xd3d3d3d3,0x4400,0xe5e5e5e5,0xf6f6f6f6,0x87878787];
const dirty=[0,4,0x22222222,0x3000,0x4800,0x55555555,0x66666666,0x77777777];
const programs=new Map(),records=[],report=join(directory,'numeric-transfer-result.json');
for(const line of readFileSync(join(directory,'numeric-transfer.tsv'),'utf8').trim().split('\n')) {
  const [name,hex,id,instructions]=line.split('\t'),source=readFileSync(join(directory,`${name}.wasm`));
  const module=new WebAssembly.Module(source);
  assert.deepEqual(coreImports(module),[
    {module:'ring3',name:'memory',kind:'memory'},{module:'ring3',name:'read',kind:'function'},
    {module:'ring3',name:'write',kind:'function'},{module:'ring3',name:'numeric',kind:'function'},
  ]);
  programs.set(name,{id:BigInt(id),code:Buffer.from(hex,'hex'),instructions:Number(instructions),sourceSha256:hash(source)});
}

function check(name,inputs,flags,outputs={},mode='success') {
  if(name==='status') {
    const program=programs.get(name),regs=[...dirty];regs[0]=0x1234;regs[7]=0x77777778;
    runActualPrefix(program,name,'status/nonwaiting-actual-observer',BUDGET,
      {regs:initial,flags:0x402,retired:37,fpStatus:0x1234},
      {reason:4,pc:0x1000+program.code.length-2,retired:11,regs,flags:0x406},
      {onRow:row=>{row.scope='actual nonwaiting x87 status observer; no canonical bridge is claimed';records.push(row);
        writeFileSync(report,JSON.stringify({complete:false,records}));},after:({row})=>assert.equal(row.numericCalls,0)});
    return;
  }
  runActualTransfer(programs.get(name),name,{initial,dirty,inputs,inputFlags:flags,publishesPc:outputs.pc,
    mode,budget:BUDGET,onRow:row=>{records.push(row);writeFileSync(report,JSON.stringify({complete:false,
      scope:'real native Context and source with explicitly mocked canonical transfer results',records}));}});
  writeFileSync(report,JSON.stringify({complete:false,records}));
}

check('register',[],undefined);
check('indexed',[1,3],undefined);
check('status',[0],undefined);
check('move-below',[],0x403);
check('move-below-equal',[],0x443);
check('move-unordered',[],0x406);
check('compare-flags',[],0x457);
check('initialize',[],undefined);
check('processor',[0,1,2,3,4,5,6,7],0x457,{pc:true});
check('indexed',[1,3],undefined,{},'fault');
check('indexed',[1,3],undefined,{},'smc');
writeFileSync(report,JSON.stringify({complete:true,scope:'authored generated bridge transfer; no numerical or performance claim',records}));
console.log(`${records.length} generated numerical transfer controls passed`);
