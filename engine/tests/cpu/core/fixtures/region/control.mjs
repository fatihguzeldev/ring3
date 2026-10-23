import assert from 'node:assert/strict';

function assemble(build) {
  const bytes=[],labels=new Map(),fixups=[];
  const emit=text=>bytes.push(...Buffer.from(text.replaceAll(' ',''),'hex'));
  const relative=(opcode,label)=>{
    emit(opcode);fixups.push({offset:bytes.length,label});bytes.push(0);
  };
  build({emit,label:name=>{assert(!labels.has(name));labels.set(name,0x1000+bytes.length);},
    jump:name=>relative('eb',name),zero:name=>relative('74',name),
    nonzero:name=>relative('75',name),loop:name=>relative('e2',name),
    loopEqual:name=>relative('e1',name),loopWord:name=>relative('67e2',name)});
  for(const{offset,label}of fixups) {
    assert(labels.has(label),`defined branch label ${label}`);
    const displacement=labels.get(label)-(0x1000+offset+1);
    assert(displacement>=-128&&displacement<=127,'authored short branch fits rel8');
    bytes[offset]=displacement&255;
  }
  return {segments:[[0x1000,Buffer.from(bytes).toString('hex')]],
    pc:name=>{assert(labels.has(name));return labels.get(name);}};
}

function singleActivation(t,wanted) {
  const observed=t.check({...wanted,fp:t.startingFp});
  assert.equal(observed.activations.length,1,'the original compiled unit executes every selected block');
  assert.equal(t.result.modules.length,1,'control flow does not silently recompile the selected secondary entry');
}

function selectSecondary(t,pc) {
  new DataView(t.engine.memory.buffer).setUint32(t.context()+32,pc,true);
  t.result.secondaryEntry={pc,retainedPrimary:t.result.modules[0].pc};
}

export function runControlCases(fixture) {
  const secondary=assemble(a=>{
    a.jump('outer');
    a.label('outer');a.emit('8d4010 894310 b901000000');
    a.label('inner');a.emit('8d4020 894314');a.jump('body');
    a.label('body');a.emit('8d4001');a.loop('inner');
    a.emit('85d2');a.nonzero('outer');
    a.label('done');a.emit('0f0b');
  });
  fixture('secondary-entry-inside-nested-loops-skips-both-headers',{
    segments:secondary.segments,registers:[5,1,0,0x3000,0x4800,0,0,0],
  },t=>{
    t.result.codeSegments=secondary.segments;selectSecondary(t,secondary.pc('body'));
    singleActivation(t,{pc:secondary.pc('done'),retired:4,budget:4,flags:0x46,ecx:0,regs:{0:6}});
  });

  const forward=assemble(a=>{
    a.emit('85d2');a.zero('body');
    a.label('header');a.emit('8d4010 894310');
    a.label('body');a.emit('8d4001');a.loop('header');
    a.label('done');a.emit('0f0b');
  });
  fixture('forward-edge-enters-cycle-body-before-its-first-header',{
    segments:forward.segments,registers:[0,2,0,0x3000,0x4800,0,0,0],
  },t=>{
    t.result.codeSegments=forward.segments;t.writeExpected(0x3010,'11000000');
    singleActivation(t,{pc:forward.pc('done'),retired:8,budget:8,flags:0x46,ecx:0,regs:{0:18}});
  });

  const nested=assemble(a=>{
    a.label('outer');a.emit('8d4010 4a');a.zero('done');a.emit('b902000000');
    a.label('inner');a.emit('8d4001 85c9');a.zero('outer');a.loop('inner');a.jump('inner');
    a.label('done');a.emit('0f0b');
  });
  fixture('inner-loop-exits-directly-to-an-enclosing-loop-header',{
    segments:nested.segments,registers:[0,0,2,0x3000,0x4800,0,0,0],
  },t=>{
    t.result.codeSegments=nested.segments;
    singleActivation(t,{pc:nested.pc('done'),retired:19,budget:19,flags:0x46,ecx:0,regs:{0:35,2:0}});
  });

  const irreducible=assemble(a=>{
    a.emit('85d2');a.zero('right');
    a.label('left');a.emit('8d4001');a.loop('right');a.jump('done');
    a.label('right');a.emit('8d4002');a.loop('left');a.jump('done');
    a.label('done');a.emit('0f0b');
  });
  for(const{edx,eax,flags,name}of[
    {edx:0,eax:5,flags:0x46,name:'taken-entry'},
    {edx:1,eax:4,flags:2,name:'fallthrough-entry'},
  ])fixture(`irreducible-two-entry-cycle-${name}`,{
    segments:irreducible.segments,registers:[0,3,edx,0x3000,0x4800,0,0,0],
  },t=>{
    t.result.codeSegments=irreducible.segments;
    singleActivation(t,{pc:irreducible.pc('done'),retired:9,budget:9,flags,ecx:0,regs:{0:eax}});
  });

  for(const{eax,pc,flags,name}of[
    {eax:0,pc:0x6000,flags:0x46,name:'taken'},
    {eax:1,pc:0x2000,flags:2,name:'fallthrough'},
  ])fixture(`conditional-two-external-targets-${name}`,{
    segments:[[0x1000,'e9f30f0000'],[0x1ff8,'85c0 0f8400400000']],
    registers:[eax,2,0,0x3000,0x4800,0,0,0],
  },t=>{
    t.result.externalTargets=[0x6000,0x2000];
    singleActivation(t,{pc,retired:3,budget:3,flags});
  });

  for(const{kind,ecx,finalEcx,budget,flags}of[
    {kind:'loop',ecx:3,finalEcx:0,budget:6,flags:2},
    {kind:'loopEqual',ecx:3,finalEcx:0,budget:7,flags:0x46},
    {kind:'loopWord',ecx:0x12340003,finalEcx:0x12340000,budget:6,flags:2},
  ]) {
    const counted=assemble(a=>{
      if(kind==='loopEqual')a.emit('85c0');
      a.label('body');a.emit('8d5201');a[kind]('body');
      a.label('done');a.emit('0f0b');
    });
    fixture(`counted-${kind}-changes-its-selected-counter-once`,{
      segments:counted.segments,registers:[0,ecx,0,0x3000,0x4800,0,0,0],
    },t=>{
      t.result.codeSegments=counted.segments;
      singleActivation(t,{pc:counted.pc('done'),retired:budget,budget,flags,ecx:finalEcx,regs:{2:3}});
    });
  }
}
