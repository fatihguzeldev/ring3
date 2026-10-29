const RELAXED_TAG=0x7ffe;
const INTEGER_BIT=0x8000000000000000n;
const FRACTION_MASK=0xfffffffffffffn;

export function validateRelaxedFpuImport(data,fp) {
  const tags=data.getUint16(fp+20,true);
  for(let i=0;i<8;i++)if(((tags>>>(i*2))&3)!==3 && data.getUint16(fp+48+i*10,true)===RELAXED_TAG)
    throw new Error('v86 relaxed FPU cannot import a genuine tag-colliding exponent');
}

export function exportRelaxedFpuSlot(data,from,to) {
  const bits=data.getBigUint64(from,true),sign=Number(bits>>63n)<<15;
  const exponent=Number((bits>>52n)&0x7ffn),fraction=bits&FRACTION_MASK;
  let mantissa=0n,signExponent=sign;
  if(exponent===0) {
    if(fraction!==0n) {
      const high=Number(fraction>>32n);
      const shift=(high?Math.clz32(high):32+Math.clz32(Number(fraction)))-12;
      mantissa=fraction<<BigInt(12+shift);
      signExponent|=15360-shift;
    }
  } else {
    mantissa=INTEGER_BIT|(fraction<<11n);
    signExponent|=exponent===0x7ff?0x7fff:exponent+15360;
  }
  data.setBigUint64(to,mantissa,true);data.setUint16(to+8,signExponent,true);
}
