const bytes=values=>Buffer.from([...values,...Array(16-values.length).fill(0)]);
const words=values=>{const result=Buffer.alloc(16);values.forEach((value,index)=>result.writeUInt16LE(value&0xffff,index*2));return result;};
const instruction=(opcode,control,modrm=0xc1)=>Buffer.from([0x66,0x0f,0x3a,opcode,modrm,control]).toString('hex');
function mask(bits,control){const result=Buffer.alloc(16),width=control&1?2:1;if(control&0x40){for(let lane=0;lane<16/width;lane++)if(bits&(1<<lane))result.fill(255,lane*width,(lane+1)*width);}else result.writeUInt16LE(bits);return result;}
function probe(name,opcode,control,left,right,eax,edx,bits,leftLength,rightLength,extra={}){
 const capacity=control&1?8:16,index=bits?(control&0x40?31-Math.clz32(bits):Math.log2(bits&-bits)):capacity;
 return{name,code:instruction(opcode,control),direct:true,a:left,b:right,registers:{0:eax,2:edx},mxcsr:0xdfe5,flags:0x402|Number(bits!==0)|(Number(rightLength<capacity)<<6)|(Number(leftLength<capacity)<<7)|((bits&1)<<11),expectedHelpers:0,...(opcode&1?{afterRegisters:{1:index}}:{out:mask(bits,control)}),...extra};
}

export const packedStringCases=[];
// these positive aggregation masks are literal results from the sdm tables;
// polarity and output formatting are applied independently of the rust model.
for(let format=0;format<4;format++){
 const word=format&1,left=word?words([0x8000,0x7fff,0x20,0x30]):bytes([0x80,0x7f,0x20,0x30]);
 const right=word?words([0x7fff,0x8000,0x20,0x30,0x25,0xffff]):bytes([0x7f,0x80,0x20,0x30,0x25,0xff]);
 const fullMask=word?0xff:0xffff,positive=[0xf,format&2?0x3f:0x1c,(fullMask&~0x3f)|0xc,0];
 for(let aggregation=0;aggregation<4;aggregation++)for(let polarity=0;polarity<4;polarity++){
  const control=format|(aggregation<<2)|(polarity<<4)|(((format^polarity)&1)<<6),opcode=0x60+((format+aggregation+polarity)&3);
  const bits=positive[aggregation]^(polarity===1?fullMask:polarity===3?0x3f:0);
  packedStringCases.push(probe(`sse42-string-control-${control.toString(16)}-form-${opcode.toString(16)}`,opcode,control,left,right,opcode<0x62?4:0x80000000,opcode<0x62?6:0xffffffff,bits,4,6));
 }
}

for(const opcode of[0x60,0x61,0x62,0x63])for(const word of[false,true])for(const selector of[0,0x40]){
 const left=word?words([0xffff]):bytes([0xff]),right=word?words([1,1,1,0xffff,1,1,1,0xffff]):Buffer.alloc(16,1);
 if(!word){right[3]=255;right[15]=255;}
 packedStringCases.push(probe(`sse42-string-output-${opcode.toString(16)}-${word?'word':'byte'}-${selector}`,opcode,Number(word)|selector,left,right,1,word?8:16,word?0x88:0x8008,1,word?8:16));
}
for(const length of[16,17,0x7fffffff,0x80000000,0xfffffff0]){
 packedStringCases.push(probe(`sse42-string-explicit-length-saturates-${length}`,0x61,0,Buffer.alloc(16,7),Buffer.alloc(16,7),length,length,0xffff,16,16));
}
const embeddedZero=bytes([0,0x2a]);
packedStringCases.push(
 probe('sse42-string-explicit-negative-length-keeps-zero-valid',0x61,0,embeddedZero,embeddedZero,0xfffffffe,2,3,2,2),
 probe('sse42-string-implicit-first-zero-ignores-explicit-registers',0x63,0,embeddedZero,embeddedZero,16,16,0,0,0),
 probe('sse42-string-ranges-ignores-unpaired-last-range-bound',0x61,4,bytes([10,20,30]),bytes([10,20,30,40]),3,4,3,3,4),
);
for(const word of[false,true]){
 const left=word?words([1,2,3,4]):bytes([1,2,3,4]),right=word?words([9,9,9,9,9,9,1,2]):Buffer.alloc(16,9);
 if(!word){right[14]=1;right[15]=2;}
 packedStringCases.push(probe(`sse42-string-ordered-partial-vector-boundary-${word?'word':'byte'}`,0x63,0xc|Number(word),left,right,0,0,word?0x40:0x4000,4,word?8:16));
}
for(const [control,bits]of[[0xc,0xffff],[0x1c,0],[0x2c,0xffff],[0x3c,0xfffe]]){
 packedStringCases.push(probe(`sse42-string-ordered-empty-left-polarity-${control.toString(16)}`,0x60,control,Buffer.alloc(16,1),Buffer.alloc(16,2),0,1,bits,0,1));
}
packedStringCases.push(probe('sse42-string-reserved-immediate-bit-has-no-effect',0x61,0x80,bytes([0xff]),Buffer.alloc(16,0xff),1,16,0xffff,1,16));

const aliasLeft=bytes([0x31]),aliasRight=Buffer.alloc(16,0x31),aliasMiddle=Buffer.alloc(16,0x77),tail=Buffer.alloc(80);
packedStringCases.push(probe('sse42-string-mask-snapshots-right-xmm0-before-publication',0x60,0,aliasLeft,aliasRight,1,16,0xffff,1,16,{
 code:instruction(0x60,0,0xd0),initialXmm:Buffer.concat([aliasRight,aliasMiddle,aliasLeft,tail]),afterXmm:Buffer.concat([mask(0xffff,0),aliasMiddle,aliasLeft,tail]),
}));
packedStringCases.push(probe('sse42-string-mask-self-alias-snapshots-xmm0',0x62,0,aliasLeft,aliasLeft,0,0,1,1,1,{code:instruction(0x62,0,0xc0)}));
packedStringCases.push(probe('sse42-string-memory-unaligned-full16-and-ecx-address-alias',0x61,0,bytes([0x31]),aliasRight,1,16,0xffff,1,16,{
 code:instruction(0x61,0,0x01),memory:aliasRight,address:0x5011,registers:{0:1,1:0x5011,2:16},readAlignment:1,expectedHelpers:1,
}));
packedStringCases.push(probe('sse42-string-zero-length-still-reads-full16',0x60,0,aliasLeft,aliasRight,0,0,0,0,0,{
 code:instruction(0x60,0,0x07),memory:aliasRight,address:0x5011,readAlignment:1,expectedHelpers:1,
}));
for(const [name,address,seedMemory,fault]of[
 ['unaligned-unmapped',0x6001,undefined,0x6001],
 ['split-source',0x5ff8,Buffer.alloc(8,0x31),0x6000],
]){
 packedStringCases.push({name:`sse42-string-${name}-preserves-entire-state-and-transfer`,code:instruction(0x60,0,0x07),direct:true,a:aliasLeft,b:aliasRight,registers:{0:0,2:0},address,seedMemory,reason:5,retired:0,detail:1,fault,access:1,length:16,readAlignment:1,stagedTransfer:Buffer.alloc(16,0x97),expectedHelpers:1});
}
