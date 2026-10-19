import assert from 'node:assert/strict';

let input = '';
for await (const chunk of process.stdin) input += chunk;
const fixture = JSON.parse(input);
const initial = Buffer.from(fixture.arena, 'hex');
const PC = 0x1000, TARGET = 0x2000, PAYLOAD = 0xcafebabe;
const positions = [0, 1, 2, 3, 5, 7, 8, 9];
const lengths = [1, 1, 1, 2, 2, 1, 1, 2];
const registers = [0x12345678, 0x11223344, TARGET, 0x8000, 0x9000, 0x55667788, 0x10203040, 0x50607080];
let controls = 0;

function expected(name, {entry=0, budget=64, flags=0x8d7, shadow=0, cancel=0, mode='normal', limit=0xfffff}={}) {
  let pc=PC+entry, retired=0, calls=0, detail=0, reason=0, base=0, default32=true;
  const regs=[...registers];
  const stop=(value, more=0)=>({pc,retired,calls,reason:value,detail:more,regs,flags:flags>>>0,shadow,cancel});
  for(let remaining=budget;;) {
    if(cancel)return stop(2);
    if(remaining===0)return stop(1);
    if(base!==0||!default32)return stop(pc>limit?11:3);
    const index=positions.indexOf(pc-PC);
    if(index<0)return stop(3);
    if(pc+lengths[index]-1>limit)return stop(11);
    const singleStep=(flags&0x100)!==0;
    flags&=~0x10000; shadow=0;
    if(index===3) {
      calls++;
      if(mode.includes('limit'))limit=PC+4;
      if(mode.includes('base'))base=0x8000;
      if(mode.includes('default16'))default32=false;
      if(mode.includes('cancel'))cancel=1;
      if(name==='read'&&(base!==0||!default32))return stop(pc>limit?11:3);
      if(mode.includes('transport'))return stop(base!==0||!default32?(pc>limit?11:3):7,base!==0||!default32?0:3);
      if(mode.includes('malformed'))return stop(7,2);
      if(mode==='fault')return stop(name==='read'?5:11,name==='read'?1:0);
      if(name==='read')regs[0]=PAYLOAD;
    }
    if(index===4)regs[1]=regs[0];
    if(index===7) {
      if(TARGET>limit)return stop(11);
      pc=TARGET;
    } else pc=PC+positions[index]+lengths[index];
    retired++; remaining--;
    if(singleStep)return stop(16);
  }
}

for(const item of fixture.modules) {
  const module=new WebAssembly.Module(Buffer.from(item.bytes,'hex'));
  const memory=new WebAssembly.Memory({initial:1,maximum:1});
  const data=new DataView(memory.buffer), bytes=new Uint8Array(memory.buffer);
  let options, calls, finishes;
  const set=(offset,value)=>data.setUint32(offset,value>>>0,true);
  const get=offset=>data.getUint32(offset,true);
  function helper(flags) {
    calls++;
    const mode=options.mode??'normal';
    if(mode.includes('limit'))set(fixture.segments+32,(get(fixture.segments+32)&~0x010fffff)|(PC+4));
    if(mode.includes('default16'))set(fixture.segments+32,get(fixture.segments+32)&~0x02000000);
    if(mode.includes('base')) {
      data.setUint16(fixture.segments+4,3,true);
      set(fixture.bases+20,0x8000);
    }
    if(mode.includes('cancel'))set(96,1);
    const words=[0x484d3352,item.name==='read'?0x1000a:0x10007,40,0,0,item.name==='read'?PAYLOAD:flags,0,0,0,item.name==='read'?4:0];
    if(mode==='fault') {
      words[4]=item.name==='read'?1:3; words[5]=0;
      if(item.name==='read') {words[6]=1;words[7]=0x8000;words[8]=1;}
    }
    if(mode.includes('malformed'))words[0]=0;
    words.forEach((word,index)=>set(100+index*4,word));
    return mode.includes('transport')?7:0;
  }
  const bindings={};
  for(const imp of WebAssembly.Module.imports(module)) {
    if(imp.module==='env')continue;
    if(imp.name==='guard_detect'||imp.name==='guard_detect_resident')bindings[imp.name]=()=>1n;
    else if(imp.name==='finish_exception_detection')bindings[imp.name]=(...args)=>{finishes.push(args);return 0;};
    else if(imp.name==='cpu_read32')bindings[imp.name]=()=>helper(0);
    else if(imp.name==='cpu_read32_view')bindings[imp.name]=()=>0;
    else if(imp.name==='x87_op'||imp.name==='x87_resident_op')bindings[imp.name]=(...args)=>helper(args.at(-4));
    else throw new Error(`unexpected import ${imp.module}.${imp.name}`);
  }
  const {run}=new WebAssembly.Instance(module,{env:{memory},ring3:bindings}).exports;
  function check(label, opts={}) {
    options=opts;calls=0;finishes=[];
    bytes.fill(0);bytes.set(initial);
    set(4,0x10003);set(12,opts.shadow??0);
    registers.forEach((value,index)=>set(16+index*4,value));
    set(48,PC+(opts.entry??0));set(52,opts.flags??0x8d7);set(96,opts.cancel??0);
    if(opts.limit!==undefined)set(fixture.segments+32,(get(fixture.segments+32)&~0x010fffff)|opts.limit);
    const fpBefore=bytes.slice(4236,4236+128);
    const oracle=expected(item.name,opts);
    assert.equal(run(0,56,opts.budget??64,96),0,`${label}: transport`);
    assert.deepEqual(Array.from({length:8},(_,index)=>get(16+index*4)),oracle.regs,`${label}: registers`);
    assert.deepEqual([get(48),get(52),get(12),get(72),get(76),get(80),get(96),calls],[oracle.pc,oracle.flags,oracle.shadow,oracle.reason,oracle.retired,oracle.detail,oracle.cancel,oracle.calls],`${label}: exact stop state`);
    assert.deepEqual(bytes.slice(4236,4236+128),fpBefore,`${label}: host-owned FP record preserved`);
    if(oracle.reason===5)assert.deepEqual([get(84),get(88),get(92)],[0x8000,1,4],`${label}: access fault`);
    assert.equal(finishes.length,Number(oracle.reason===3||oracle.reason===5||(oracle.reason>=10&&oracle.reason<=24)),`${label}: detector boundary`);
    controls++;
  }
  const prefix=`${item.name}/${item.resident}/${item.tier}`;
  for(const entry of positions)for(const budget of [1,2,64])check(`${prefix}/entry${entry}/budget${budget}`,{entry,budget});
  for(const entry of [4,6,10])check(`${prefix}/interior${entry}`,{entry});
  for(const opts of [
    {budget:0},{cancel:1},{flags:0x102},{flags:0x10002},{shadow:1},{flags:0x10002,shadow:1,budget:1},
    {limit:PC+1},{mode:'limit'},{mode:'base'},{mode:'default16'},{mode:'cancel'},
    {mode:'base-cancel'},{mode:'malformed'},{mode:'transport'},{mode:'fault'},
    {mode:'base-transport'},{mode:'base-malformed'},
    {entry:3,mode:'limit',budget:1},{entry:3,mode:'base',budget:1},
  ])check(`${prefix}/${JSON.stringify(opts)}`,opts);
}
assert.equal(controls,8*46);
console.log('mixed context barrier controls passed');
