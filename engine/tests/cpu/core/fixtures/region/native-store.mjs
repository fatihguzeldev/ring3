import assert from 'node:assert/strict';

function retained(t,wanted,format,helpers=0) {
  const run=t.check(wanted);
  assert.equal(run.activations.length,1,'one actual body executes the exact native-store prefix');
  assert.equal(t.result.modules.length,1,'no replacement body hides a store admission failure');
  const adapter=typeof t.api.core_precise==='function';
  assert.equal(run.helperCalls,adapter?0:helpers,'failed fast admission deopts before the current numerical effect');
  if(adapter)assert.equal(run.preciseCalls,helpers,'the actual precise executor owns each rejected store suffix');
  const sites=t.inspect().nativeFloatStores;
  assert.deepEqual(sites.map(site=>site.format),format?[format]:[],
    format?'the actual body stores the admitted native value with a floating-point Wasm store':
      'a value without a Native24 proof retains the exact integer packing path');
}

export function runNativeStoreCases(fixture) {
  for(const c of[
    {name:'positive-zero',memory:'00000000',raw:'00000000000000000000'},
    {name:'negative-zero',memory:'00000080',raw:'00000000000000000080'},
    {name:'three-and-a-half',memory:'00006040',raw:'00000000000000e00040'},
  ])fixture(`proven-native-f32-store-preserves-${c.name}`,{
    segments:[[0x1000,'d903 d95b08 ffe6']],registers:[0,2,0,0x3000,0x4800,0,0x1100,0],
    data:page=>page.set(Buffer.from(c.memory,'hex')),
  },t=>{
    const before=t.version();t.writeExpected(0x3008,c.memory);
    retained(t,{pc:0x1100,retired:3,budget:3,
      fp:t.fp({opcode:0x15b,fip:0x1002,fdp:0x3008,registers:{7:c.raw}})},'f32');
    assert.equal(t.version(),before+1n,'the exact native store commits one transaction');
  });

  // 1 + 2^-25 is exact in f64 but not in f32. The same literal separates an
  // admitted native f64 store from a still-required correctly rounded f32 pack.
  for(const c of[
    {name:'f64',store:'dd5b08',memory:'000000080000f03f',opcode:0x55b,status:0,native:'f64'},
    {name:'f32-needs-rounding',store:'d95b08',memory:'0000803f',opcode:0x15b,status:0x20},
  ])fixture(`proven-native-store-${c.name}-after-exact-arithmetic`,{
    segments:[[0x1000,`d903 d84304 ${c.store} ffe6`]],
    registers:[0,2,0,0x3000,0x4800,0,0x1100,0],
    data:page=>page.set(Buffer.from('0000803f00000033','hex')),
  },t=>{
    const before=t.version();t.writeExpected(0x3008,c.memory);
    retained(t,{pc:0x1100,retired:4,budget:4,
      fp:t.fp({status:c.status,opcode:c.opcode,fip:0x1005,fdp:0x3008,
        registers:{7:'0000000040000080ff3f'}})},c.native);
    assert.equal(t.version(),before+1n,'both store representations preserve the exact transaction count');
  });

  fixture('packed-f32-store-fault-preserves-the-prior-pc24-rounding-direction',{
    segments:[[0x1000,'d903 d84304 d91f 0f0b']],control:0x7f,missing:[0x5000],
    registers:[0,2,0,0x3000,0x4800,0,0,0x5000],
    data:page=>page.set(Buffer.from('0100404000000034','hex')),
  },t=>{
    // At 3 the binary32 ULP is 2^-22. The odd midpoint
    // (3+2^-22)+2^-23 rounds upward to the even 3+2^-21.
    const before=t.version();
    retained(t,{pc:0x1005,retired:2,budget:3,work:2,remaining:1,stop:3,
      fault:[0x5000,0x10204,0x40e],fp:t.fp({status:0x3a20,tag:0x3fff,
        opcode:0x43,fip:0x1002,fdp:0x3004,registers:{7:'00000000000200c00040'}})},undefined,1);
    assert.equal(t.version(),before,'a rejected packed store preserves memory and the transaction sequence');
  });

  fixture('proven-native-f32-store-fault-preserves-another-values-pending-c1-and-popped-ghost',{
    segments:[[0x1000,'d903 d9c9 d84304 ddd8 d91f 0f0b']],missing:[0x5000],
    registers:[0,2,0,0x3000,0x4800,0,0,0x5000],
    initialFp:{status:0x20,tag:0xfffc,registers:{0:'0008000000000080ff3f'}},
    data:page=>page.set(Buffer.from('0000604000000025','hex')),
  },t=>{
    // (1+2^-52)+2^-53 is the odd binary64 midpoint, so ties-to-even gives
    // 1+2^-51 with C1=1. Sticky PE makes this an actual deferred status recipe.
    // The register pop exposes the earlier exact binary32 target and preserves
    // the rounded empty physical ghost without deriving Native24 from arithmetic.
    const before=t.version();
    retained(t,{pc:0x1009,retired:4,budget:5,work:4,remaining:1,stop:3,
      fault:[0x5000,0x10204,0x40e],fp:t.fp({status:0x220,tag:0xfffc,
        opcode:0x5d8,fip:0x1007,fdp:0x3004,
        registers:{0:'00000000000000e00040',7:'0010000000000080ff3f'}})},'f32',1);
    assert.equal(t.version(),before,'a rejected native store cannot clear C1, pop its target, or commit memory');
  });
}
