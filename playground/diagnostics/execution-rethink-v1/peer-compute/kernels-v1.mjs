import { Asm, mem, reg, EAX, ECX, EDX, EBX } from '../reference/bottleship/tools/guestbench/lib/asm.mjs';

export const CODE=0x1000, DATA=0x3000, STACK=0x4000, TERMINAL=0x1f00;
export const KINDS=['integer','call-ret','fp'];
export const CWS=[0x007f,0x027f,0x037f];
export const hex=bytes=>Array.from(bytes,b=>b.toString(16).padStart(2,'0')).join('');
const put32=(bytes,offset,value)=>new DataView(bytes.buffer,bytes.byteOffset,bytes.byteLength).setUint32(offset,value>>>0,true);

export function makeKernel(kind,N,cw=0x007f) {
  if(!KINDS.includes(kind)||!Number.isInteger(N)||N<1||N>0x1000000||!CWS.includes(cw)) throw new Error('unsupported kernel arguments');
  const a=new Asm(CODE),blockSpecs=[];
  a.label('loop');
  if(kind==='call-ret') {
    a.callRel('leaf'); blockSpecs.push([CODE,a.pos]);
  } else if(kind==='fp') {
    a.fldM32(mem({base:EBX})).farithM32('add',mem({base:EBX,disp:4})).farithM32('mul',mem({base:EBX,disp:8})).fstpM32(mem({base:EBX,disp:0x20}));
    a.aluImm('add',reg(EAX),1);
  } else {
    a.aluImm('add',reg(EAX),3).alu('xor',EDX,reg(EAX)).aluImm('add',mem({base:EBX,disp:0x0c}),1);
  }
  const continuation=kind==='call-ret'?a.addr:CODE;
  a.dec(reg(ECX)).jcc('nz','loop');
  blockSpecs.push([continuation,a.addr-continuation]);
  const suffix=a.addr;
  a.movTo(mem({base:EBX,disp:0x40}),EAX).movTo(mem({base:EBX,disp:0x44}),EDX).movTo(mem({base:EBX,disp:0x48}),ECX);
  a.db(0x9c).db(0x8f).modrm(0,mem({base:EBX,disp:0x4c})).ret();
  blockSpecs.push([suffix,a.addr-suffix]);
  if(kind==='call-ret') {
    const leaf=a.addr; a.label('leaf');
    a.aluImm('add',reg(EAX),3).alu('xor',EDX,reg(EAX)).aluImm('add',mem({base:EBX,disp:0x0c}),1).ret();
    blockSpecs.push([leaf,a.addr-leaf]);
  }
  const kernelBytes=a.link(),code=new Uint8Array(4096),data=new Uint8Array(4096),stack=new Uint8Array(4096);
  code.set(kernelBytes); code.set([0x0f,0x0b],TERMINAL-CODE);
  const dv=new DataView(data.buffer); dv.setFloat32(0,1.5,true);dv.setFloat32(4,2.5,true);dv.setFloat32(8,2,true);dv.setUint16(0xf0,cw,true);
  put32(stack,0xffc,TERMINAL);
  let eax=5,edx=7;
  if(kind==='fp') eax+=N;
  else for(let i=0;i<N;i++) {eax=(eax+3)>>>0;edx=(edx^eax)>>>0;}
  const expectedData=data.slice(),expectedStack=stack.slice(),expectedFlags=0x46;
  if(kind==='fp') new DataView(expectedData.buffer).setFloat32(0x20,8,true);else put32(expectedData,0x0c,N);
  put32(expectedData,0x40,eax);put32(expectedData,0x44,edx);put32(expectedData,0x48,0);put32(expectedData,0x4c,expectedFlags);put32(expectedStack,0xff8,expectedFlags);
  const regs=[5,N,7,DATA,STACK+0xffc,0,0,0],expectedRegs=[eax,0,edx,DATA,STACK+0x1000,0,0,0];
  return {kind,N,cw,code,data,stack,kernelBytes,blockSpecs,regs,entry:CODE,terminal:TERMINAL,
    retired:(kind==='integer'?5:7)*N+6,expectedData,expectedStack,expectedRegs,expectedFlags,
    fpLast:kind==='fp'?{fip:CODE+8,fdp:DATA+0x20,fop:0x015b,top:0,tags:0xffff,status:0,st7Significand:'8000000000000000',st7Exponent:0x4002}:null};
}
