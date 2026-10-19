import assert from 'node:assert/strict';

export const CODE=0x1000, DATA=0x3000, STACK=0x8000, TARGET=0x7000, FP=4236, ARENA=4796;
export const CWS=[0x007f,0x027f,0x037f];
const bytes=hex=>Buffer.from(hex,'hex');
const u32=n=>Number(BigInt.asUintN(32,BigInt(n)));
function flags(result,left,right,subtract,old) {
  const value=u32(result), sign=0x80000000, parity=(value&255).toString(2).replaceAll('0','').length%2===0;
  const carry=subtract?BigInt(left)<BigInt(right):BigInt(left)+BigInt(right)>0xffffffffn;
  const overflow=subtract?((left^right)&(left^value)&sign)!==0:((~(left^right))&(left^value)&sign)!==0;
  return ((old&~0x8d5)|(carry?1:0)|(parity?4:0)|(((left^right^value)&16)?16:0)|(value===0?64:0)|(value&sign?128:0)|(overflow?2048:0))>>>0;
}
const add=(a,b)=>{const e=Math.min(a.e,b.e);return{n:(a.n<<BigInt(a.e-e))+(b.n<<BigInt(b.e-e)),e};};
const multiply=(a,b)=>({n:a.n*b.n,e:a.e+b.e});
function dyadic32(word) {const exponent=(word>>>23)&255;assert.ok(exponent>0&&exponent<255);return{n:BigInt((word&0x7fffff)|0x800000)*(word>>>31?-1n:1n),e:exponent-150};}
function round(value,control) {
  const negative=value.n<0n,n=negative?-value.n:value.n,precision={0:24,1:64,2:53,3:64}[(control>>>8)&3];
  assert.ok(n>0n);let exponent=value.e+n.toString(2).length-1;
  const shift=Math.max(0,n.toString(2).length-precision),remainder=shift?n&((1n<<BigInt(shift))-1n):0n;
  let sig=shift?n>>BigInt(shift):n<<BigInt(precision-n.toString(2).length);
  const half=shift?1n<<BigInt(shift-1):0n,rc=(control>>>10)&3;
  const increment=rc===0?shift>0&&(remainder>half||(remainder===half&&(sig&1n)!==0n)):rc===1?negative&&remainder!==0n:rc===2?!negative&&remainder!==0n:false;
  if(increment)sig++;if(sig>>BigInt(precision)){sig>>=1n;exponent++;}
  const significand=sig<<BigInt(64-precision),raw=Buffer.alloc(10);raw.writeBigUInt64LE(significand);raw.writeUInt16LE(exponent+16383+(negative?0x8000:0),8);
  return{raw,value:{n:negative?-significand:significand,e:exponent-63},exceptions:remainder?32:0,increment};
}
function exact32(value) {
  const rounded=round(value,0x007f),sig=rounded.raw.readBigUInt64LE(),exponent=(rounded.raw.readUInt16LE(8)&0x7fff)-16383;
  assert.equal(rounded.exceptions,0,'authored stores are exact f32 dyadics');assert.ok(exponent>=-126&&exponent<=127);
  return((rounded.raw[9]&128?0x80000000:0)|((exponent+127)<<23)|Number((sig>>40n)&0x7fffffn))>>>0;
}

export function program(name) {
  const operations=[];let pc=CODE;
  const emit=(kind,hex,extra={})=>{const raw=bytes(hex);operations.push({kind,pc,length:raw.length,bytes:raw,...extra});pc+=raw.length;};
  if(name==='scalar') {emit('load','8b03');emit('add','83c001');emit('store','8907');emit('modify','83430c01');}
  else {
    emit('fld','d903',{opcode:0x103,offset:0});
    if(name==='fp') {emit('fadd','d84304',{opcode:0x43,offset:4});emit('fmul','d84b08',{opcode:0x4b,offset:8});}
    else {const width=Number(name.slice(5));assert.ok([2,8,9,32].includes(width));for(let i=0;i<width-2;i++)emit('fadd','d84304',{opcode:0x43,offset:4});}
    emit('fstp','d91f',{opcode:0x11f});emit('inc','40');
  }
  emit('dec','49');
  const branch=pc,displacement=CODE-(pc+2);assert.ok(displacement>=-128);emit('jnz',Buffer.from([0x75,displacement&255]).toString('hex'));
  const tail=pc;emit('jump','ffe2');
  const codePage=Buffer.alloc(4096);for(const op of operations)op.bytes.copy(codePage,op.pc-CODE);
  const dataPage=Buffer.alloc(4096);[0x3fc00000,0x40200000,0x40000000].forEach((n,i)=>dataPage.writeUInt32LE(n,i*4));
  return{name,operations,codePage,dataPage,stackPage:Buffer.alloc(4096),specs:[[CODE,branch+2-CODE],[tail,2]],tail};
}

export function expected(p,initial,spec={}) {
  const cw=spec.cw??0x037f,registers=[5,spec.iterations??3,TARGET,spec.ebx??DATA,STACK+0x800,0,0,spec.edi??DATA+0x100];
  let pc=CODE,eflags=spec.flags??2,shadow=spec.shadow??0,retired=0,reason=0,detail=0,address=0,access=0,length=0,top=0,value=null;
  const fp=Buffer.from(initial.subarray(FP,FP+128)),memory=new Map([[CODE,Buffer.from(p.codePage)],[DATA,Buffer.from(p.dataPage)],[STACK,Buffer.from(p.stackPage)]]);
  fp.writeUInt16LE(cw,16);fp.writeUInt16LE(spec.pending??0,18);
  if(spec.precision) {memory.get(DATA).writeUInt32LE(0x3f800000,0);memory.get(DATA).writeUInt32LE(0x33800000,4);}
  const fault=(at,write)=>{for(let i=0;i<4;i++)if(!memory.has((at+i)&~4095)){reason=23;detail=write?6:4;address=at+i;access=write?2:1;length=4;return true;}return false;};
  const read=at=>memory.get(at&~4095).readUInt32LE(at&4095);
  const pointer=(op,at)=>{fp.writeUInt16LE(op.opcode,22);fp.writeUInt32LE(op.pc,24);fp.writeUInt32LE(at,28);fp.writeUInt16LE(0x1b,32);fp.writeUInt16LE(0x23,34);};
  while(!reason) {
    if(spec.cancel){reason=2;break;}if(retired===(spec.budget??1000)){reason=1;break;}
    const op=p.operations.find(op=>op.pc===pc);if(!op){reason=3;break;}
    const next=pc+op.length,tf=(eflags&0x100)!==0;eflags&=~0x10000;shadow=0;
    if(['fld','fadd','fmul','fstp'].includes(op.kind)) {
      if(fp.readUInt16LE(18)&0x3f&~cw){fp.writeUInt16LE(fp.readUInt16LE(18)|0x8080,18);reason=12;detail=fp.readUInt16LE(18);break;}
      const at=op.kind==='fstp'?registers[7]:registers[3]+op.offset;
      if(fault(at,op.kind==='fstp'))break;
      pointer(op,at);
      if(op.kind==='fld') {value=dyadic32(read(at));top=7;round(value,0x037f).raw.copy(fp,110);fp.writeUInt16LE(0x3fff,20);fp.writeUInt16LE((fp.readUInt16LE(18)&~0x3800)|0x3800,18);}
      else if(op.kind==='fstp') {assert.ok(value);memory.get(at&~4095).writeUInt32LE(exact32(value),at&4095);top=0;fp.writeUInt16LE(0xffff,20);fp.writeUInt16LE(fp.readUInt16LE(18)&~0x3800,18);}
      else {
        assert.ok(value);const operand=dyadic32(read(at)),result=round(op.kind==='fadd'?add(value,operand):multiply(value,operand),cw);
        const status=(fp.readUInt16LE(18)&~0x200)|(result.increment?0x200:0)|result.exceptions;
        if(result.exceptions&~cw)fp.writeUInt16LE(status|0x8080,18);
        else {value=result.value;result.raw.copy(fp,110);fp.writeUInt16LE(status,18);}
      }
    } else if(op.kind==='load') {if(fault(registers[3],false))break;registers[0]=read(registers[3]);}
    else if(op.kind==='add') {const old=registers[0];registers[0]=u32(old+1);eflags=flags(registers[0],old,1,false,eflags);}
    else if(op.kind==='store') {if(fault(registers[7],true))break;memory.get(registers[7]&~4095).writeUInt32LE(registers[0],registers[7]&4095);}
    else if(op.kind==='modify') {const at=registers[3]+12;if(fault(at,true))break;const old=read(at),value=u32(old+1);memory.get(at&~4095).writeUInt32LE(value,at&4095);eflags=flags(value,old,1,false,eflags);}
    else if(op.kind==='inc') {const old=registers[0],carry=eflags&1;registers[0]=u32(old+1);eflags=(flags(registers[0],old,1,false,eflags)&~1)|carry;}
    else if(op.kind==='dec') {const old=registers[1],carry=eflags&1;registers[1]=u32(old-1);eflags=(flags(registers[1],old,1,true,eflags)&~1)|carry;}
    retired++;pc=op.kind==='jnz'&&registers[1]!==0?CODE:op.kind==='jump'?registers[2]:next;
    if(['store','fstp'].includes(op.kind)&&registers[7]===CODE){reason=6;break;}
    if(tf){reason=16;break;}assert.ok(retired<=1000,'bounded authored model');
  }
  const state=Buffer.alloc(56);state.write('R3ST');state.writeUInt16LE(3,4);state.writeUInt16LE(1,6);state.writeUInt32LE(56,8);state.writeUInt32LE(shadow,12);
  registers.forEach((n,i)=>state.writeUInt32LE(n,16+4*i));state.writeUInt32LE(pc,48);state.writeUInt32LE(eflags,52);
  return{state,fp,registers,pc,eflags,reason,retired,detail,address,access,length,memory,top};
}

export function validate(p,initial,spec,row) {
  const wanted=expected(p,initial,spec),arena=Buffer.from(row.arena,'hex');
  assert.deepEqual(arena.subarray(0,56),wanted.state,'literal complete State32');
  assert.deepEqual(arena.subarray(FP,FP+128),wanted.fp,'literal full FP80 bank/TOP/CW/SW/pointers');
  assert.deepEqual([row.reason,row.retired,row.pc],[wanted.reason,wanted.retired,wanted.pc],'literal finite prefix');
  assert.deepEqual(row.outer_fields,[wanted.reason,wanted.retired,wanted.detail,wanted.address,wanted.access,wanted.length],'literal canonical Exit payload');
  for(const [address,bytes] of wanted.memory)assert.equal(row.memory.find(page=>page.address===address).hex,bytes.toString('hex'),'entire authored mapped page');
  assert.deepEqual(arena.subarray(4364,4588),initial.subarray(4364,4588),'SIMD/segment banks unchanged');
  assert.deepEqual(arena.subarray(4612),initial.subarray(4612),'upper SIMD/segment companion unchanged');
  return wanted;
}
