export function runBits(check) {
  const base = 0xcd7;
  const regs = (a = 0x12345678, index = 3, address = 0x3000) => [a,index,7,address,0x4800,0,0x8000,0];
  for (const width of [16,32]) {
    for (const [kind,input,index] of [['bsf',0,0],['bsf',0x8008,3],['bsf',0x80000000,31],
      ['bsr',0,0],['bsr',0x8008,15],['bsr',0x80000000,31]]) {
      const source = width === 16 ? input & 65535 : input >>> 0;
      const initial = regs(0x12345678,input), expected = [...initial];
      if (source) expected[0] = width === 16 ? (0x12340000 | index) >>> 0 : index;
      check(`${kind}${width}`,`${kind}${width}-${input}`,8,{regs:initial,flags:base},
        {reason:4,pc:width === 16 ? 0x1004 : 0x1003,retired:1,regs:expected,flags:(base & ~64) | (source ? 0 : 64)});
    }
  }
  for (const [input,result,flags] of [[0,32,0xc97],[1,0,0xcd6],[0x1000,12,0xc96],[0x80000000,31,0xc96]]) {
    const initial=regs(0x12345678,input),expected=[...initial];expected[0]=result;
    check('tzcnt',`tzcnt-${input}`,8,{regs:initial,flags:base},
      {reason:4,pc:0x1004,retired:1,regs:expected,flags});
  }
  for (const [name,index,expectedValue,carry] of [['bt32',31,0x80000001,1],['bt32',32,0x80000001,1],
    ['bts32',30,0xc0000001,0],['bts32',-1,0x80000001,1],['btr16',16,0x80000000,1],['btr16',15,0x80000001,0]]) {
    const initial=regs(0x80000001,index),expected=[...initial];expected[0]=expectedValue;
    check(name,`${name}-${index}`,8,{regs:initial,flags:base},
      {reason:4,pc:name === 'btr16' ? 0x1004 : 0x1003,retired:1,regs:expected,flags:(base&~1)|carry});
  }
  for (const [name,index,address,target,width,value,carry] of [
    ['btmem',-1,0x3004,0x3000,4,0x80000000,1],
    ['btmem',32,0x3000,0x3004,4,1,1],
    ['bt16mem',0xffff,0x3002,0x3000,2,0x8000,1],
    ['bt16mem',16,0x3000,0x3002,2,0,0],
    ['btsmem',-1,0x3004,0x3000,4,1,0],
    ['lockbtsmem',32,0x3000,0x3004,4,1,1],
  ]) {
    const initial=regs(5,index,address),write=name.includes('bts');
    const result=value | (1 << (index & (width*8-1)));
    check(name,`${name}-signed-index${index}`,8,{regs:initial,flags:base,initial:[[target,width,value]]},
      {reason:4,pc:name==='bt16mem'||name==='lockbtsmem'?0x1004:0x1003,retired:1,
        flags:(base&~1)|carry,...(write?{writes:[[target,width,result>>>0]]}:{})});
  }
  check('btmem','bt-memory-unmapped-preserves-flags',8,{regs:regs(5,32,0x5ffc),flags:base},
    {reason:3,pc:0x1000,retired:0,reads:1,fault:[0x6000,0x10104]});
  check('btsmem','bts-write-denied-preserves-flags',8,{regs:regs(5,0),flags:base,readOnly:true},
    {reason:3,pc:0x1000,retired:0,reads:1,fault:[0x3000,0x20204]});
  check('bsfmem','bsf-memory-unmapped-preserves-destination',8,{regs:regs(0x12345678,3,0x8000),flags:base},
    {reason:3,pc:0x1000,retired:0,reads:1,fault:[0x8000,0x10104]});
}
